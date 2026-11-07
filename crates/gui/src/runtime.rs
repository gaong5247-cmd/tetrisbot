use crate::Config;
use anyhow::{Context, Result, bail};
use capture::{Frame, Rect};
use crossbeam_channel::{Receiver, Sender, bounded};
use detection::{Grid, State, Tracker, scan};
use game_state::*;
use input::{Cancellation, Keyboard, StopReason};
use movement::{Action, plan};
use std::path::Path;
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
/// The solver has not emitted any key yet. A stale/failed Cold Clear 2
/// child can be replaced once without risking a double hard-drop.
fn suggest_current_with_restart(
    client: &mut Option<tbp_client::Client>,
    engine_path: &Path,
    timeout: Duration,
    snapshot: &Snapshot,
) -> Result<(Vec<Placement>, bool)> {
    let mut restarted = false;
    for attempt in 0..2 {
        if client.is_none() {
            *client = Some(tbp_client::Client::spawn(engine_path, timeout)?);
        }
        let result = client.as_mut().unwrap()
            .resync_with_mode(snapshot, true)
            .and_then(|_| client.as_mut().unwrap().suggest());
        match result {
            Ok(moves) => return Ok((moves, restarted)),
            Err(error) if attempt == 0 => {
                // Drop kills/reaps the old process. No inputs were sent, so
                // restarting is safe; the caller still verifies the live board.
                *client = None;
                restarted = true;
                let _ = error;
            }
            Err(error) => {
                *client = None;
                return Err(error).context("Cold Clear 2 failed after one clean process restart");
            }
        }
    }
    bail!("Cold Clear 2 search exhausted its retry budget")
}

/// A timeout should explain *which* observation failed; never assume that
/// SendInput succeeded just because the OS accepted the event.
#[derive(Default)]
pub struct LockDiagnostics {
    frames: u32,
    passed: u32,
    confidence: f32,
    active: Option<Location>,
    uncertain_cells: usize,
    board_difference_cells: Option<u32>,
    classification: &'static str,
}
impl LockDiagnostics {
    pub fn observe(&mut self, observation: &Observation, expected: &Board) -> bool {
        self.frames += 1;
        self.confidence = observation.confidence;
        self.active = observation.active;
        self.uncertain_cells = observation.cells.iter().flatten()
            .filter(|c| c.kind == CellKind::Uncertain).count();
        self.board_difference_cells = Board::from_visible(&observation.cells, true)
            .ok()
            .map(|board| board.rows.iter().zip(expected.rows.iter())
                .map(|(a,b)| (a^b).count_ones()).sum());
        self.classification = if self.uncertain_cells > 0 {
            "uncertain_board_or_animation"
        } else if self.board_difference_cells.is_some_and(|n| n > 0) {
            "board_mismatch_or_hard_drop_not_received"
        } else if observation.active.is_none() {
            "next_active_not_visible"
        } else if observation.confidence < 0.9 {
            "low_vision_confidence"
        } else {
            "verified"
        };
        let verified = self.classification == "verified";
        if verified { self.passed += 1; } else { self.passed = 0; }
        self.passed >= 2
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "classification": self.classification,
            "frames_observed": self.frames,
            "consecutive_verified_frames": self.passed,
            "confidence": self.confidence,
            "active": self.active,
            "uncertain_cells": self.uncertain_cells,
            "board_difference_cells": self.board_difference_cells,
        })
    }
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
                            let generation = tracker.generation;
                            tracker.apply_scan(&candidates);
                            if tracker.generation != generation {
                                vision.reset();
                            }
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
                        if failures > dcfg.loss_grace_frames && tracker.state != State::Locked {
                            // Clear ROI only after sustained confirmed loss, not
                            // merely because a fast game animated two frames.
                            s.roi = None;
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
   let mut client:Option<tbp_client::Client>=None;
   while alive.load(Ordering::Relaxed) {
    let job=match jrx.recv_timeout(Duration::from_millis(100)){Ok(j)=>j,Err(_)=>continue};
    if matches!(job,Job::Reset) {if let Some(c)=client.as_mut(){let _=c.stop();}continue}
    let Job::Turn {frame,grid,observation,baseline,execute}=job else {unreachable!()};
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
    let mut lock_diagnostics = LockDiagnostics::default();
    let outcome=(||->Result<()> {
     let initial=PlanningSnapshot::from_observation(&observation,baseline)?;
     let snapshot=initial.snapshot;
     // CC2 always receives only the current piece; the simulated reserve is
     // never a real HOLD action. No future-piece HUD data is observed.
     let _=etx.send(Event::Status("Cold Clear 2 (current piece only; no NEXT/HOLD scan)".into()));
     // Searching occurs before SendInput. If the child fails or stalls,
     // restart it once rather than aborting the entire autoplay session.
     phase="engine suggestion";
     recovery_snapshot=Some(snapshot.clone());
     let start=Instant::now();
     let (moves, restarted)=suggest_current_with_restart(
         &mut client, &cfg.engine_path,
         Duration::from_millis(cfg.engine_timeout_ms), &snapshot)?;
     let elapsed=start.elapsed().as_secs_f64()*1000.;
     if restarted {let _=etx.send(Event::Status("Cold Clear 2 search recovered by restarting engine".into()));}
     let _=etx.send(Event::Suggestion(moves.clone(),elapsed));
     if !execute{let _=etx.send(Event::PreviewDone);return Ok(())}
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
     let fresh=Arc::new(capture::capture(frame.rect,frame.sequence+1)?);
     let current=vision.observe(&fresh,grid);
     let current_snapshot=PlanningSnapshot::from_observation(&current,baseline)?.snapshot;
     if current_snapshot.board!=snapshot.board
         || current_snapshot.active.piece!=snapshot.active.piece
     {bail!("state changed during engine search")}
     // Ignore every suggestion for any piece other than the observed active.
     let chosen=moves.iter()
        .filter(|m| m.location.piece == current_snapshot.active.piece)
        .find_map(|&m| plan(&current_snapshot,m,&cfg.movement).ok())
        .context("no reachable current-piece recommendation from Cold Clear 2")?;
     if chosen.uses_hold || chosen.actions.contains(&Action::Hold) {
      bail!("HOLD actions are forbidden in current-piece-only mode")
     }
     phase="execute path";actions=chosen.actions.clone();placement=Some(chosen.placement);
     let mut trusted=current.clone();
     let start=Instant::now();
     for &a in &chosen.actions {
      last_action=Some(a);
      keyboard.check()?;
      if a==Action::HardDrop {
       // Last reversible checkpoint: observe exact active landing before committing.
       let f=capture::capture(frame.rect,frame.sequence+2)?;let o=vision.observe(&f,grid);
       let b=Board::from_visible(&o.cells,baseline).with_context(||format!("pre-drop uncertain at {:?}; active {:?}",o.cells.iter().enumerate().flat_map(|(y,row)|row.iter().enumerate().filter_map(move |(x,c)|(c.kind==CellKind::Uncertain).then_some((x,y)))).collect::<Vec<_>>(),o.active))?;let active=o.active.context("lost active piece before hard drop")?;
       if o.confidence<0.9||b!=snapshot.board||!b.dropped(active).same_cells(chosen.placement.location) {bail!("pre-drop visual checkpoint disagrees with plan")}
       trusted=o;
      }
      if a == Action::HardDrop {hard_drop_sent=true;}
      keyboard.pulse(a,&cfg.movement)?;
      completed_actions+=1;
      if a!=Action::HardDrop {
       // A single ghost/stack-contact animation frame is not proof that a
       // T-spin failed. Reobserve briefly, but do not send the next key until
       // the active piece and unchanged locked stack are both verified.
       let deadline=Instant::now()+Duration::from_millis(220);
       loop {
        keyboard.check()?;
        let step_frame=capture::capture(frame.rect,frame.sequence+2)?;
        let step_observation=vision.observe(&step_frame,grid);
        let verified=step_observation.confidence>=0.9
            && step_observation.active.is_some()
            && Board::from_visible(&step_observation.cells,baseline).is_ok_and(|b| b==snapshot.board);
        // For horizontal moves the active piece's x must actually change.
        // Merely seeing an active tetromino again does NOT prove that the game
        // accepted SendInput; otherwise bot keeps moving on a stale model.
        let movement_accepted=match (trusted.active,step_observation.active,a) {
            (Some(before),Some(after),Action::Left) => after.piece==before.piece && after.x==before.x-1,
            (Some(before),Some(after),Action::Right) => after.piece==before.piece && after.x==before.x+1,
            (Some(before),Some(after),Action::Down) => after.piece==before.piece && after.y<before.y,
            _=>true,
        };
        if verified && movement_accepted {
            trusted=step_observation;
            break;
        }
        // Do not contaminate temporal tracking with an untrusted frame.
        vision.seed(&trusted);
        if Instant::now()>=deadline {
            if matches!(a,Action::Left|Action::Right|Action::Down) {
                phase="input delivery";
                bail!("game did not confirm {a:?} movement within 220ms; check keyboard binding, Scan Code/Virtual Key mode and game focus");
            }
            bail!("temporary visual desync after {a:?}; safe path recovery requested");
        }
        std::thread::sleep(Duration::from_millis(8));
       }
      }
     }
     phase="verify lock";
     let input_ms=start.elapsed().as_secs_f64()*1000.;
     let verify_start=Instant::now();
     let mut verifier=vision::Vision::default();
     verifier.seed(&trusted); // keep pre-drop history through spawn/line-clear animation
     let mut verified=None;
     while verify_start.elapsed()<Duration::from_millis(cfg.verification_timeout_ms) {
      keyboard.check()?;
      let f=capture::capture(frame.rect,frame.sequence+3)?;
      let o=verifier.observe(&f,grid);
      if lock_diagnostics.observe(&o,&chosen.expected) {
       verified=Some(o);
       break;
      }
      // A transient animation is not the next verified piece. Preserve the
      // pre-drop reference so the next actual spawn can be distinguished from
      // the four cells that just locked.
      verifier.seed(&trusted);
      std::thread::sleep(Duration::from_millis(16));
     }
     let observation=verified.with_context(||format!(
         "post-lock board/current-piece verification timed out: {} ({} frames; board_delta={:?}; confidence={:.2}; active={:?}; uncertain={})",
         lock_diagnostics.classification, lock_diagnostics.frames,
         lock_diagnostics.board_difference_cells, lock_diagnostics.confidence,
         lock_diagnostics.active, lock_diagnostics.uncertain_cells
     ))?;
     // The real game state is reconstructed from the next captured piece.
     client.as_mut().context("engine disappeared before stop")?.stop()?;
     let _=etx.send(Event::Completed {observation,input_ms,verify_ms:verify_start.elapsed().as_secs_f64()*1000.});Ok(())
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
     let report=serde_json::json!({
        "error":error,"phase":phase,"cancel_reason":cancel.reason().map(|r|r.to_string()),
        "target_window":bound_target,"foreground_window":input::foreground(),
        "placement":placement,"planned_actions":actions,
        "completed_actions":completed_actions,"last_action":last_action,
        "post_lock":lock_diagnostics.report(),
        "configured_controls": {
            "input_method":cfg.controls.input_method,
            "left":cfg.controls.left,
            "right":cfg.controls.right,
            "cw":cfg.controls.cw,
            "ccw":cfg.controls.ccw,
            "hard_drop":cfg.controls.hard_drop,
            "pulse_ms":cfg.movement.pulse_ms,
            "hard_drop_ms":cfg.movement.hard_drop_ms,
        }
     });
     if std::fs::create_dir_all("logs").is_ok(){if let Ok(json)=serde_json::to_string_pretty(&report){let _=std::fs::write("logs/last-error.json",json);}}
     cancel.stop();if let Some(c)=client.as_mut(){if c.stop().is_err(){client=None;}}let _=etx.send(Event::Failed(error));
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
