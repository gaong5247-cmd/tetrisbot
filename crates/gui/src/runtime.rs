use crate::Config;
use anyhow::{Context, Result, bail};
use capture::{Frame, Rect};
use crossbeam_channel::{Receiver, Sender, bounded};
use detection::{Grid, State, Tracker, scan};
use game_state::*;
use input::{Cancellation, Keyboard, StopReason};
use movement::{Action, plan};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Clone)]
pub struct View {
    pub frame: Arc<Frame>,
    pub candidates: Vec<Grid>,
    pub grid: Option<Grid>,
    pub state: State,
    pub observation: Observation,
    pub generation: u64,
    pub capture_ms: f64,
    pub vision_ms: f64,
}
pub enum Event {
    Status(String),
    Suggestion(Vec<Placement>, f64),
    PreviewDone,
    /// A reversible path was interrupted before hard drop. The GUI may
    /// resume automatically only after re-observing this same board and queue.
    Recovering { reason: String, snapshot: Snapshot },
    Completed {
        observation: Observation,
        input_ms: f64,
        verify_ms: f64,
    },
    Failed(String),
}
pub enum Job {
    Turn {
        frame: Arc<Frame>,
        grid: Grid,
        observation: Observation,
        baseline: bool,
        empty_hold: bool,
        execute: bool,
    },
    Reset,
}
struct Shared {
    roi: Option<Rect>,
    monitor: usize,
    epoch: u64,
}
/// Input can be retried only if no placement has locked and the game's
/// stack, current piece/queue identity and HOLD are unchanged.
/// A reversible rotation or movement may change active position.
pub fn same_reversible_state(before: &Snapshot, after: &Snapshot) -> bool {
    before.board == after.board
        && before.queue() == after.queue()
        && before.hold == after.hold
}
/// Accept any foreground Tetris game when the external-games option is enabled.
/// Keep the debugger itself excluded so a board screenshot in the GUI can never
/// receive input by mistake. All other vision, focus, and path checks still apply.
pub fn validate_input_target(title: &str, allow_external: bool) -> Result<()> {
    if title == "SilhouetteTetris Debugger" {
        bail!("debugger window is not a playable target; move it away from the game board");
    }
    if !allow_external && title != crate::lab::TITLE {
        bail!("external Tetris input disabled; set allow_external = true in the game profile");
    }
    Ok(())
}
pub struct Runtime {
    pub views: Receiver<Result<View>>,
    pub jobs: Sender<Job>,
    pub events: Receiver<Event>,
    pub cancel: Cancellation,
    pub alive: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
    threads: Vec<std::thread::JoinHandle<()>>,
}
fn latest<T>(tx: &Sender<T>, discard: &Receiver<T>, mut value: T) {
    loop {
        match tx.try_send(value) {
            Ok(()) => break,
            Err(crossbeam_channel::TrySendError::Full(v)) => {
                value = v;
                let _ = discard.try_recv();
            }
            Err(_) => break,
        }
    }
}
impl Runtime {
    pub fn start(cfg: Config) -> Self {
        let cancel = Cancellation::default();
        let alive = Arc::new(AtomicBool::new(true));
        let shared = Arc::new(Mutex::new(Shared {
            roi: None,
            monitor: cfg.monitor,
            epoch: 0,
        }));
        let (ftx, frx) = bounded::<Result<(Arc<Frame>, f64, u64)>>(1);
        let discard = frx.clone();
        let (vtx, views) = bounded(1);
        let vdiscard = views.clone();
        let (stx, srx) = bounded::<(Arc<Frame>, u64)>(1);
        let sdiscard = srx.clone();
        let (dtx, drx) = bounded::<(Vec<Grid>, u64)>(1);
        let ddiscard = drx.clone();
        let (jobs, jrx) = bounded::<Job>(1);
        let (etx, events) = crossbeam_channel::unbounded();
        let mut threads = vec![];
        {
            let alive = alive.clone();
            let shared = shared.clone();
            let hz = cfg.capture_hz.clamp(1, 240);
            threads.push(std::thread::spawn(move || {
                let mut sequence = 0;
                while alive.load(Ordering::Relaxed) {
                    let start = Instant::now();
                    let (roi, monitor, epoch) = {
                        let s = shared.lock().unwrap();
                        (s.roi, s.monitor, s.epoch)
                    };
                    let result = (|| {
                        let monitors = capture::monitors()?;
                        let screen = monitors
                            .get(monitor)
                            .context("selected monitor disconnected")?
                            .rect;
                        let rect = roi.and_then(|r| r.intersection(screen)).unwrap_or(screen);
                        let frame = Arc::new(capture::capture(rect, sequence)?);
                        Ok((frame, start.elapsed().as_secs_f64() * 1000., epoch))
                    })();
                    latest(&ftx, &discard, result);
                    sequence += 1;
                    let period = Duration::from_secs_f64(1. / hz as f64);
                    if let Some(left) = period.checked_sub(start.elapsed()) {
                        std::thread::sleep(left);
                    }
                }
            }));
        }
        {
            let alive = alive.clone();
            let dcfg = cfg.detection.clone();
            threads.push(std::thread::spawn(move || {
                while alive.load(Ordering::Relaxed) {
                    if let Ok((f, e)) = srx.recv_timeout(Duration::from_millis(100)) {
                        latest(&dtx, &ddiscard, (scan(&f, &dcfg), e));
                    }
                }
            }));
        }
        {
            let alive = alive.clone();
            let shared = shared.clone();
            let dcfg = cfg.detection.clone();
            let cancel = cancel.clone();
            threads.push(std::thread::spawn(move || {
                let mut tracker = Tracker::new(dcfg.clone());
                let mut vision = vision::Vision::default();
                let mut last_scan = Instant::now() - Duration::from_secs(10);
                let mut epoch = 0;
                let mut candidates = vec![];
                let mut failures = 0;
                while alive.load(Ordering::Relaxed) {
                    let received = match frx.recv_timeout(Duration::from_millis(100)) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let (frame, capture_ms, e) = match received {
                        Ok(v) => v,
                        Err(err) => {
                            cancel.stop_with(StopReason::CaptureFailed);
                            tracker.reset();
                            latest(&vtx, &vdiscard, Err(err));
                            continue;
                        }
                    };
                    if e != epoch {
                        epoch = e;
                        tracker.reset();
                        vision.reset();
                        candidates.clear();
                    }
                    let start = Instant::now();
                    if let Ok((found, scan_epoch)) = drx.try_recv() {
                        if scan_epoch == epoch {
                            candidates = found;
                            tracker.update(&candidates);
                            vision.reset();
                        }
                    }
                    if tracker.grid.is_some() {
                        if tracker.observe_frame(&frame) {
                            failures = 0;
                        } else {
                            failures += 1;
                            // Preserve the last trustworthy silhouette during a brief
                            // detection dropout at T-spin contact. Reset only on real loss.
                            if tracker.state != State::Locked {
                                vision.reset();
                            }
                        }
                    }
                    if !tracker.may_input()
                        && last_scan.elapsed() >= Duration::from_millis(dcfg.rescan_ms)
                    {
                        latest(&stx, &sdiscard, (frame.clone(), epoch));
                        last_scan = Instant::now();
                    }
                    {
                        let mut s = shared.lock().unwrap();
                        if failures > 2 {
                            s.roi = None;
                            tracker.grid = None;
                        } else if tracker.may_input() {
                            if let Ok(m) = capture::monitors() {
                                if let Some(mon) = m.get(s.monitor) {
                                    s.roi = tracker.grid.map(|g| g.roi(mon.rect));
                                }
                            }
                        }
                    }
                    let mut observation = if tracker.grid.is_some() && failures == 0 {
                        vision.observe(&frame, tracker.grid.unwrap())
                    } else {
                        Observation::default()
                    };
                    if tracker.grid.is_some_and(|g| g.score < 0.79) && !vision.motion_seen {
                        observation.confidence = observation.confidence.min(0.6);
                        observation
                            .reasons
                            .push("Sparse grid needs observed tetromino motion".into());
                    }
                    latest(
                        &vtx,
                        &vdiscard,
                        Ok(View {
                            frame,
                            candidates: candidates.clone(),
                            grid: tracker.grid,
                            state: tracker.state,
                            observation,
                            generation: tracker.generation,
                            capture_ms,
                            vision_ms: start.elapsed().as_secs_f64() * 1000.,
                        }),
                    );
                }
            }));
        }
        {
            let alive = alive.clone();
            let cancel = cancel.clone();
            threads.push(std::thread::spawn(move || {
                while alive.load(Ordering::Relaxed) {
                    if input::f10() {
                        cancel.stop_with(StopReason::EmergencyStop);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }));
        }
        {
            let alive = alive.clone();
            let cancel = cancel.clone();
            threads.push(std::thread::spawn(move || {
   let mut client:Option<tbp_client::Client>=None;let mut synchronized:Option<Snapshot>=None;
   while alive.load(Ordering::Relaxed) {
    let job=match jrx.recv_timeout(Duration::from_millis(100)){Ok(j)=>j,Err(_)=>continue};
    if matches!(job,Job::Reset) {if let Some(c)=client.as_mut(){let _=c.stop();}synchronized=None;continue}
    let Job::Turn {frame,grid,mut observation,baseline,empty_hold,execute}=job else {unreachable!()};
    let mut phase="snapshot";
    let mut bound_target=0isize;
    let mut actions=Vec::<Action>::new();
    let mut completed_actions=0usize;
    let mut last_action=None;
    let mut placement=None;
    let mut recovery_snapshot: Option<Snapshot> = None;
    // Once the hard-drop key could have been delivered, a failure is no
    // longer reversible and must never be automatically retried.
    let mut hard_drop_sent=false;
    let outcome=(||->Result<()> {
     if empty_hold&&observation.hold==Known::Unknown {observation.hold=Known::Value(None);}
     let snapshot=Snapshot::from_observation(&observation,baseline)?;
     if client.is_none(){let c=tbp_client::Client::spawn(&cfg.engine_path,Duration::from_millis(cfg.engine_timeout_ms))?;let _=etx.send(Event::Status(c.info.clone()));client=Some(c);}
     let c=client.as_mut().unwrap();
     let same=synchronized.as_ref().is_some_and(|s|s.board==snapshot.board&&s.queue()==snapshot.queue()&&s.hold==snapshot.hold);
     let mut snapshot=snapshot;
     if same {if let Some(s)=&synchronized {snapshot.combo=s.combo;snapshot.back_to_back=s.back_to_back;}} else {c.resync(&snapshot)?;}
     recovery_snapshot=Some(snapshot.clone());
     let start=Instant::now();let moves=c.suggest()?;let elapsed=start.elapsed().as_secs_f64()*1000.;let _=etx.send(Event::Suggestion(moves.clone(),elapsed));
     synchronized=Some(snapshot.clone());if !execute{let _=etx.send(Event::PreviewDone);return Ok(())}
     phase="bind target";
     if let Some(reason)=cancel.reason() {bail!("AI canceled: {reason}")}
     // Bind to the desktop window containing the observed board, never a guessed game coordinate.
     let rect=grid.rect();let target=input::window_at(rect.x+rect.width as i32/2,rect.y+rect.height as i32/2);
     bound_target=target;
     let title=input::title(target);
     validate_input_target(&title,cfg.allow_external)?;
     // Key bindings are selected by the user for this game, not hard-coded
     // to Local Lab. Foreground and cancellation are still checked per key.
     let mut keyboard=Keyboard::with_bindings(cancel.clone(),target,cfg.controls.clone())?;
     keyboard.check()?;
     // Recapture after engine search: active gravity may have advanced since the request.
     let mut vision=vision::Vision::default();vision.seed(&observation);vision.observe(&frame,grid);
     let fresh=Arc::new(capture::capture(frame.rect,frame.sequence+1)?);let mut current=vision.observe(&fresh,grid);if empty_hold&&current.hold==Known::Unknown {current.hold=Known::Value(None);}
     let current_snapshot=Snapshot::from_observation(&current,baseline)?;
     if current_snapshot.board!=snapshot.board||current_snapshot.queue()!=snapshot.queue()||current_snapshot.hold!=snapshot.hold {bail!("state changed during engine search")}
     let chosen=moves.iter().find_map(|&m|plan(&current_snapshot,m,&cfg.movement).ok()).context("no legal reachable recommendation")?;
     phase="execute path";actions=chosen.actions.clone();placement=Some(chosen.placement);
     let mut trusted=current.clone();
     let start=Instant::now();
     for &a in &chosen.actions {
      last_action=Some(a);
      keyboard.check()?;
      if a==Action::HardDrop {
       // Last reversible checkpoint: observe exact active landing before committing.
       let f=capture::capture(frame.rect,frame.sequence+2)?;let mut o=vision.observe(&f,grid);
       if empty_hold&&o.hold==Known::Unknown {o.hold=Known::Value(None);}
       let b=Board::from_visible(&o.cells,baseline).with_context(||format!("pre-drop uncertain at {:?}; active {:?}",o.cells.iter().enumerate().flat_map(|(y,row)|row.iter().enumerate().filter_map(move |(x,c)|(c.kind==CellKind::Uncertain).then_some((x,y)))).collect::<Vec<_>>(),o.active))?;let active=o.active.context("lost active piece before hard drop")?;
       if o.confidence<0.9||b!=snapshot.board||!b.dropped(active).same_cells(chosen.placement.location) {bail!("pre-drop visual checkpoint disagrees with plan")}
      }
      if a == Action::HardDrop {hard_drop_sent=true;}
      keyboard.pulse(a,&cfg.movement)?;
      completed_actions+=1;
      if a!=Action::HardDrop {
       // A single ghost/stack-contact animation frame is not proof that a
       // T-spin failed. Reobserve briefly, but do not send the next key until
       // the active piece and unchanged locked stack are both verified.
       let deadline=Instant::now()+Duration::from_millis(120);
       loop {
        keyboard.check()?;
        let step_frame=capture::capture(frame.rect,frame.sequence+2)?;
        let step_observation=vision.observe(&step_frame,grid);
        let verified=step_observation.confidence>=0.9
            && step_observation.active.is_some()
            && Board::from_visible(&step_observation.cells,baseline).is_ok_and(|b| b==snapshot.board);
        if verified {
            trusted=step_observation;
            break;
        }
        // Do not contaminate temporal tracking with an untrusted frame.
        vision.seed(&trusted);
        if Instant::now()>=deadline {
            bail!("temporary visual desync after {a:?}; safe path recovery requested");
        }
        std::thread::sleep(Duration::from_millis(8));
       }
      }
     }
     phase="verify lock";
     let input_ms=start.elapsed().as_secs_f64()*1000.;let verify_start=Instant::now();let mut verifier=vision::Vision::default();let mut verified=None;let mut matches=0;
     while verify_start.elapsed()<Duration::from_millis(cfg.verification_timeout_ms) {
      keyboard.check()?;let f=capture::capture(frame.rect,frame.sequence+3)?;let mut o=verifier.observe(&f,grid);
      if snapshot.hold.is_none()&&!chosen.uses_hold&&o.hold==Known::Unknown {o.hold=Known::Value(None);}
      let consumed=if snapshot.hold.is_none()&&chosen.uses_hold {2}else{1};
      let expected_hold=if chosen.uses_hold {Some(snapshot.active.piece)}else{snapshot.hold};
      let expected_active=snapshot.next[consumed-1];
      let queue_ok=o.active.is_some_and(|a|a.piece==expected_active)&&o.next.len()>=snapshot.next.len()&&o.next.iter().take(snapshot.next.len()-consumed).eq(snapshot.next.iter().skip(consumed))&&o.hold==Known::Value(expected_hold);
      if o.confidence>=0.9&&queue_ok&&Board::from_visible(&o.cells,baseline).is_ok_and(|b|b==chosen.expected) {matches+=1;if matches>=2 {verified=Some(o);break}}else{matches=0;}
      std::thread::sleep(Duration::from_millis(16));
     }
     let observation=verified.context("post-lock board/NEXT/HOLD verification timed out")?;
     c.play(chosen.placement)?;
     let consumed=if snapshot.hold.is_none()&&chosen.uses_hold {2}else{1};
     for &p in observation.next.iter().skip(snapshot.next.len()-consumed) {c.new_piece(p)?;}
     let mut next=Snapshot::from_observation(&observation,baseline)?;
     let cleared=(snapshot.board.rows.iter().map(|r|r.count_ones()).sum::<u32>()+4-chosen.expected.rows.iter().map(|r|r.count_ones()).sum::<u32>())/10;
     next.combo=if cleared>0 {snapshot.combo+1}else{0};next.back_to_back=if cleared>0 {cleared==4||chosen.placement.spin!=Spin::None}else{snapshot.back_to_back};
     synchronized=Some(next);let _=etx.send(Event::Completed {observation,input_ms,verify_ms:verify_start.elapsed().as_secs_f64()*1000.});Ok(())
    })();
    if let Err(e)=outcome {
     let error=format!("{e:#}");
     // Retrying is allowed only before any potentially irreversible hard drop.
     // A fresh, high-confidence observation must match this exact board,
     // queue and HOLD before the GUI can schedule another turn.
     if phase=="execute path" && !hard_drop_sent && cancel.reason().is_none() {
      if let Some(snapshot)=recovery_snapshot {
       let report=serde_json::json!({
        "reason":&error,"phase":phase,"placement":placement,
        "planned_actions":actions,"completed_actions":completed_actions,
        "last_action":last_action,"target_window":bound_target,
        "foreground_window":input::foreground(),
       });
       if std::fs::create_dir_all("logs").is_ok() {
        if let Ok(json)=serde_json::to_string_pretty(&report) {
         let _=std::fs::write("logs/last-recovery.json",json);
        }
       }
       let _=etx.send(Event::Recovering {reason:error,snapshot});
       continue;
      }
     }
     let report=serde_json::json!({"error":error,"phase":phase,"cancel_reason":cancel.reason().map(|r|r.to_string()),"target_window":bound_target,"foreground_window":input::foreground(),"placement":placement,"planned_actions":actions,"completed_actions":completed_actions,"last_action":last_action});
     if std::fs::create_dir_all("logs").is_ok(){if let Ok(json)=serde_json::to_string_pretty(&report){let _=std::fs::write("logs/last-error.json",json);}}
     cancel.stop();if let Some(c)=client.as_mut(){if c.stop().is_err(){client=None;}}synchronized=None;let _=etx.send(Event::Failed(error));
    }
   }
  }));
        }
        Self {
            views,
            jobs,
            events,
            cancel,
            alive,
            shared,
            threads,
        }
    }
    pub fn rescan(&self, monitor: usize) {
        self.cancel.stop();
        let mut s = self.shared.lock().unwrap();
        s.roi = None;
        s.monitor = monitor;
        s.epoch += 1;
        let _ = self.jobs.try_send(Job::Reset);
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.cancel.stop_with(StopReason::Shutdown);
        self.alive.store(false, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}
