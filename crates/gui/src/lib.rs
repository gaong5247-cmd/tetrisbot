pub mod lab;
pub mod runtime;
use anyhow::{Context, Result, bail};
use detection::{DetectionConfig, State};
use diagnostics::Timings;
use eframe::egui::{self, Color32, Stroke, StrokeKind};
use game_state::*;
use movement::MovementProfile;
use runtime::{Event, Job, Runtime, View};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub monitor: usize,
    pub capture_hz: u32,
    pub engine_path: PathBuf,
    pub engine_timeout_ms: u64,
    pub verification_timeout_ms: u64,
    pub allow_external: bool,
    /// Only in explicitly agreed multiplayer matches: accept verified
    /// bottom-injected garbage and recalculate the current piece's path.
    pub multiplayer_attack_mode: bool,
    /// Avoid Mini/Full T-spins; use safe regular placements instead.
    pub no_t_spin: bool,
    pub controls: input::KeyBindings,
    pub detection: DetectionConfig,
    pub movement: MovementProfile,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            monitor: 0,
            capture_hz: 60,
            engine_path: PathBuf::from("engines/cold-clear-2.exe"),
            engine_timeout_ms: 4500,
            verification_timeout_ms: 3500,
            allow_external: true,
            multiplayer_attack_mode: false,
            no_t_spin: false,
            controls: input::KeyBindings::default(),
            detection: DetectionConfig::default(),
            movement: MovementProfile::default(),
        }
    }
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let mut c: Self = toml::from_str(
            &std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?,
        )?;
        c.movement.validate()?;
        c.controls.validate()?;
        if c.detection.stable_frames < 2
            || !(0.65..=1.).contains(&c.detection.lock_score)
            || !(0.3..=c.detection.lock_score).contains(&c.detection.retain_score)
            || c.detection.loss_grace_frames > 90
            || c.engine_timeout_ms > 10000
            || c.engine_timeout_ms == 0
            || c.verification_timeout_ms > 10000
            || c.capture_hz == 0
        {
            bail!("invalid safety thresholds")
        }
        if c.engine_path.is_relative() {
            c.engine_path = path
                .parent()
                .unwrap_or(Path::new("."))
                .parent()
                .unwrap_or(Path::new("."))
                .join(&c.engine_path);
        }
        Ok(c)
    }
}
#[derive(PartialEq, Clone, Copy, Debug)]
enum Mode {
    Original,
    Grayscale,
    Edges,
    Heatmap,
    Occupancy,
    Silhouettes,
    Reconstruction,
}
pub struct Debugger {
    cfg: Config,
    runtime: Runtime,
    view: Option<View>,
    texture: Option<egui::TextureHandle>,
    status: String,
    error: String,
    preview: bool,
    armed: bool,
    busy: bool,
    baseline: bool,
    debug: bool,
    always_on_top: bool,
    /// Changed game controls are inactive until the worker is restarted.
    profile_pending: bool,
    /// Expected unchanged stack/queue after a reversible, unfinished move.
    recovery: Option<(Snapshot, Instant)>,
    recovery_attempts: u8,
    mode: Mode,
    moves: Vec<Placement>,
    last_key: String,
    /// New placements require matching observations on separate frames.
    stable_board: vision::StableBoardGate,
    engine: String,
    capture: Timings,
    vision: Timings,
    search: Timings,
    input: Timings,
    verify: Timings,
    started: Instant,
    completed: u64,
    frames: u64,
    last_generation: u64,
}
impl Debugger {
    pub fn new(cfg: Config) -> Self {
        Self {
            runtime: Runtime::start(cfg.clone()),
            cfg,
            view: None,
            texture: None,
            status: "Capture started; inputs disabled".into(),
            error: String::new(),
            preview: true,
            armed: false,
            busy: false,
            baseline: false,
            debug: true,
            always_on_top: true,
            profile_pending: false,
            recovery: None,
            recovery_attempts: 0,
            mode: Mode::Original,
            moves: vec![],
            last_key: String::new(),
            stable_board: vision::StableBoardGate::default(),
            engine: "Not connected".into(),
            capture: Timings::default(),
            vision: Timings::default(),
            search: Timings::default(),
            input: Timings::default(),
            verify: Timings::default(),
            started: Instant::now(),
            completed: 0,
            frames: 0,
            last_generation: 0,
        }
    }
    fn stop(&mut self, reason: &str) {
        self.armed = false;
        self.recovery = None;
        if reason.starts_with("Emergency stop") {
            self.runtime
                .cancel
                .stop_with(input::StopReason::EmergencyStop);
        } else {
            self.runtime.cancel.stop();
        }
        let _ = self.runtime.jobs.try_send(Job::Reset);
        self.status = reason.into();
    }
    fn apply_game_profile(&mut self) {
        if let Err(e) = self.cfg.controls.validate().and_then(|_| self.cfg.movement.validate()) {
            self.error = format!("Invalid game / speed profile: {e}");
            return;
        }
        // Do not hot-swap key mappings while the input worker is armed.
        self.stop("Game profile applied; confirm fresh game state before Start AI");
        self.runtime = Runtime::start(self.cfg.clone());
        self.last_generation = 0;
        self.last_key.clear();
        self.stable_board.reset();
        self.busy = false;
        self.armed = false;
        self.baseline = false;
        self.recovery_attempts = 0;
        self.profile_pending = false;
        self.view = None;
        self.texture = None;
        self.error.clear();
    }
    fn update_workers(&mut self) {
        while let Ok(v) = self.runtime.views.try_recv() {
            match v {
                Ok(v) => {
                    self.frames += 1;
                    self.capture.record(v.capture_ms);
                    self.vision.record(v.vision_ms);
                    if v.generation != self.last_generation {
                        if self.last_generation != 0 && self.baseline {
                            self.baseline = false;
                            self.armed = false;
                            self.runtime
                                .cancel
                                .stop_with(input::StopReason::TrackingLost);
                            self.status =
                                "Board tracking changed; confirm a fresh baseline before resuming"
                                    .into();
                        }
                        self.last_generation = v.generation;
                        self.last_key.clear();
                        self.stable_board.reset();
                        if self.busy {
                            self.runtime
                                .cancel
                                .stop_with(input::StopReason::TrackingLost);
                        }
                    }
                    if self.busy && v.state != State::Locked {
                        self.runtime
                            .cancel
                            .stop_with(input::StopReason::TrackingLost);
                    }
                    // Only a real new camera frame advances this gate.
                    if v.state == State::Locked {
                        self.stable_board.observe(&v.observation,self.baseline);
                    } else {
                        self.stable_board.reset();
                    }
                    self.view = Some(v);
                }
                Err(e) => {
                    self.error = format!("{e:#}");
                    self.stop("Capture failed; inputs paused");
                    self.stable_board.reset();
                    self.view = None;
                }
            }
        }
        while let Ok(e) = self.runtime.events.try_recv() {
            match e {
                Event::Status(s) => self.engine = s,
                Event::Suggestion(m, t) => {
                    self.moves = m;
                    self.search.record(t);
                }
                Event::PreviewDone => self.busy = false,
                Event::Recovering { reason, snapshot } => {
                    self.busy = false;
                    self.last_key.clear();
                    if self.armed {
                        self.recovery_attempts = self.recovery_attempts.saturating_add(1);
                        if self.recovery_attempts > 3 {
                            self.error = format!("Reversible path failed repeatedly: {reason}");
                            self.stop("Automatic retries exhausted; confirm the board before Start AI");
                        } else {
                            self.recovery = Some((snapshot, Instant::now()));
                            self.status = format!(
                                "Rechecking board before safe retry ({}/3): {reason}",
                                self.recovery_attempts
                            );
                        }
                    }
                }
                Event::Completed {
                    observation: _,
                    input_ms,
                    verify_ms,
                    garbage_lines,
                } => {
                    self.completed += 1;
                    self.recovery_attempts = 0;
                    self.input.record(input_ms);
                    self.verify.record(verify_ms);
                    self.busy = false;
                    self.status = if garbage_lines>0 {
                        format!("Verified move {} + {} incoming garbage lines; replanning", self.completed, garbage_lines)
                    } else {
                        format!("Verified move {}", self.completed)
                    };
                    self.last_key.clear();
                    self.stable_board.reset(); // next turn needs fresh visual confirmation
                }
                Event::Failed(e) => {
                    self.busy = false;
                    self.error = e;
                    self.stop("Paused after failed safety check; re-establish state and Start AI");
                }
            }
        }
        if self.armed && input::f10() {
            self.stop("Emergency stop (F10)");
        }
        if self.busy || !self.preview || !self.baseline {
            return;
        }
        if self.recovery.as_ref().is_some_and(|(_, since)| since.elapsed() > Duration::from_secs(3)) {
            self.error = "Could not restore a verified T-spin/path state within 3 seconds".into();
            self.stop("Recovery timed out; Start AI again only after confirming the board");
            return;
        }
        let Some(v) = self.view.as_ref() else { return };
        if v.state != State::Locked
            || v.observation.confidence < 0.9
            || v.frame.at.elapsed() > Duration::from_millis(250)
        {
            if self.armed {
                self.status = format!(
                    "AI waiting: board={} | confidence={:.2} | frame_age={}ms | active={:?}",
                    v.state, v.observation.confidence,
                    v.frame.at.elapsed().as_millis(), v.observation.active
                );
            }
            return;
        }
        if !self.stable_board.confirmed() {
            if self.armed {
                self.status = format!(
                    "Waiting for stable board: {}/2 matching camera frames",
                    self.stable_board.count().min(2)
                );
            }
            return;
        }
        let Some(g) = v.grid else { return };
        let o = v.observation.clone();
        let Ok(observed_snapshot) = PlanningSnapshot::from_observation(&o, self.baseline) else {
            return;
        };
        if let Some((expected, since)) = &self.recovery {
            if since.elapsed() < Duration::from_millis(120) {
                return;
            }
            if !runtime::compatible_multiplayer_recovery(
                expected, &observed_snapshot.snapshot, self.cfg.multiplayer_attack_mode
            ) {
                self.error = "Board or current piece changed during an unfinished path".into();
                self.stop("Unsafe to auto-resume: game state diverged");
                return;
            }
        }
        let key = format!(
            "{:?}/{:?}",
            Board::from_visible(&o.cells, true).ok(),
            o.active.map(|a| a.piece)
        );
        if key == self.last_key {
            return;
        }
        if self.armed {
            let r = g.rect();
            let target = input::window_at(r.x + r.width as i32 / 2, r.y + r.height as i32 / 2);
            if target != input::foreground() {
                self.status = format!(
                    "AI waiting for game focus: target={:?} (hwnd={}) / foreground={:?}",
                    input::title(target), target, input::title(input::foreground())
                );
                return;
            }
            if self.runtime.cancel.stopped() {
                self.armed = false;
                return;
            }
        }
        let job = Job::Turn {
            frame: v.frame.clone(),
            grid: g,
            observation: o,
            baseline: self.baseline,
            execute: self.armed,
        };
        if self.runtime.jobs.try_send(job).is_ok() {
            self.busy = true;
            self.last_key = key;
            self.recovery = None;
        }
    }
    fn capture_image(&mut self, ctx: &egui::Context) {
        let Some(v) = &self.view else { return };
        let f = &v.frame;
        let mut bytes = f.rgba.clone();
        if matches!(self.mode, Mode::Grayscale | Mode::Edges | Mode::Heatmap) {
            let gray = f.gray();
            let w = f.rect.width as usize;
            for i in 0..gray.len() {
                let n = match self.mode {
                    Mode::Grayscale => gray[i],
                    Mode::Edges => {
                        if i > w && i % w > 0 {
                            gray[i]
                                .abs_diff(gray[i - 1])
                                .saturating_add(gray[i].abs_diff(gray[i - w]))
                        } else {
                            0
                        }
                    }
                    _ => gray[i],
                };
                bytes[4 * i..4 * i + 3].fill(n);
            }
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [f.rect.width as usize, f.rect.height as usize],
            &bytes,
        );
        if let Some(t) = &mut self.texture {
            t.set(image, egui::TextureOptions::LINEAR);
        } else {
            self.texture = Some(ctx.load_texture("capture", image, egui::TextureOptions::LINEAR));
        }
    }
    fn live(&self, ui: &mut egui::Ui) {
        ui.heading("Live Capture");
        let (Some(v), Some(t)) = (&self.view, &self.texture) else {
            ui.label("Waiting for desktop pixels...");
            return;
        };
        let size = egui::vec2(v.frame.rect.width as f32, v.frame.rect.height as f32);
        let scale = (ui.available_width() / size.x).min(510. / size.y).min(1.);
        let response = ui.image((t.id(), size * scale));
        let painter = ui.painter_at(response.rect);
        let to_pos = |x: f32, y: f32| {
            response.rect.min
                + egui::vec2(x - v.frame.rect.x as f32, y - v.frame.rect.y as f32) * scale
        };
        for g in &v.candidates {
            let r = egui::Rect::from_min_max(
                to_pos(g.x, g.y),
                to_pos(g.x + 10. * g.cw, g.y + 20. * g.ch),
            );
            painter.rect_stroke(
                r,
                0.,
                Stroke::new(1_f32, Color32::YELLOW),
                StrokeKind::Inside,
            );
        }
        if let Some(g) = v.grid {
            let color = if v.state == State::Locked {
                Color32::from_rgb(70, 235, 180)
            } else {
                Color32::YELLOW
            };
            for x in 0..=10 {
                painter.line_segment(
                    [
                        to_pos(g.x + x as f32 * g.cw, g.y),
                        to_pos(g.x + x as f32 * g.cw, g.y + 20. * g.ch),
                    ],
                    Stroke::new(0.6_f32, color),
                );
            }
            for y in 0..=20 {
                painter.line_segment(
                    [
                        to_pos(g.x, g.y + y as f32 * g.ch),
                        to_pos(g.x + 10. * g.cw, g.y + y as f32 * g.ch),
                    ],
                    Stroke::new(0.6_f32, color),
                );
            }
            if self.debug {
                for y in 0..20 {
                    for x in 0..10 {
                        let (a, b) = g.center(x, y);
                        painter.circle_filled(to_pos(a as f32, b as f32), 1.3, color);
                    }
                }
            }
            ui.monospace(format!(
                "Desktop origin ({:.1}, {:.1})  cell {:.2} x {:.2}  score {:.3}",
                g.x, g.y, g.cw, g.ch, g.score
            ));
        }
        if self.mode == Mode::Heatmap {
            for g in &v.candidates {
                let r = egui::Rect::from_min_max(
                    to_pos(g.x, g.y),
                    to_pos(g.x + 10. * g.cw, g.y + 20. * g.ch),
                );
                painter.rect_filled(
                    r,
                    0.,
                    Color32::from_rgba_unmultiplied(255, 100, 0, (g.score * 75.) as u8),
                );
            }
        }
    }
    fn board(&self, ui: &mut egui::Ui, mode: Mode) {
        ui.heading(match mode {
            Mode::Occupancy => "Occupancy Matrix",
            Mode::Silhouettes => "Piece Silhouettes",
            _ => "Reconstructed Board",
        });
        let cell = 21.;
        let (r, _) =
            ui.allocate_exact_size(egui::vec2(10. * cell, 20. * cell), egui::Sense::hover());
        let painter = ui.painter_at(r);
        let obs = self.view.as_ref().map(|v| &v.observation);
        for y in 0..20 {
            for x in 0..10 {
                let c = obs.map(|o| o.cells[y][x]).unwrap_or_default();
                let kind = if mode == Mode::Silhouettes && c.kind == CellKind::Locked {
                    CellKind::Empty
                } else {
                    c.kind
                };
                let col = if mode == Mode::Occupancy
                    && matches!(kind, CellKind::Locked | CellKind::Active)
                {
                    Color32::WHITE
                } else {
                    match kind {
                        CellKind::Empty => Color32::from_rgb(17, 24, 35),
                        CellKind::Locked => Color32::from_rgb(163, 179, 201),
                        CellKind::Active => Color32::from_rgb(62, 224, 178),
                        CellKind::Ghost => Color32::from_rgb(60, 93, 109),
                        CellKind::Uncertain => Color32::from_rgb(173, 101, 44),
                    }
                };
                let rect = egui::Rect::from_min_size(
                    r.min + egui::vec2(x as f32 * cell, y as f32 * cell),
                    egui::vec2(cell - 1., cell - 1.),
                );
                painter.rect_filled(rect, 1., col);
            }
        }
        if let Some(m) = self.moves.first().filter(|_| mode == Mode::Reconstruction) {
            for (x, y) in m.location.cells() {
                if (0..20).contains(&y) {
                    let rect = egui::Rect::from_min_size(
                        r.min + egui::vec2(x as f32 * cell, (19 - y) as f32 * cell),
                        egui::vec2(cell - 1., cell - 1.),
                    );
                    painter.rect_stroke(
                        rect,
                        1.,
                        Stroke::new(2_f32, Color32::YELLOW),
                        StrokeKind::Inside,
                    );
                }
            }
        }
        ui.label("Green active | gray locked | blue ghost | amber unknown | yellow AI");
    }
}
impl eframe::App for Debugger {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.update_workers();
        if self.preview {
            self.capture_image(ctx);
        }
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(format!("SilhouetteTetris {}", env!("CARGO_PKG_VERSION")));
                ui.separator();
                ui.label(format!("CC2: {}", self.engine));
                ui.label(format!(
                    "Capture {} | {}",
                    if self.view.is_some() { "LIVE" } else { "WAIT" },
                    self.view
                        .as_ref()
                        .map(|v| v.state.to_string())
                        .unwrap_or_default()
                ));
                ui.label(format!(
                    "{:.1} FPS / {:.2} PPS",
                    self.frames as f64 / self.started.elapsed().as_secs_f64(),
                    self.completed as f64 / self.started.elapsed().as_secs_f64()
                ));
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("Start egui").clicked() {
                    self.preview = true;
                }
                if ui.button("Auto Detect Board").clicked() || ui.button("Rescan").clicked() {
                    self.stop("Searching");
                    self.runtime.rescan(self.cfg.monitor);
                    self.last_key.clear();
                }
                if ui.button("Start Vision").clicked() {
                    self.preview = true;
                }
                if ui
                    .add_enabled(self.baseline && !self.busy && !self.profile_pending, egui::Button::new("Start AI"))
                    .clicked()
                {
                    self.runtime.cancel.reset();
                    self.recovery = None;
                    self.recovery_attempts = 0;
                    self.armed = true;
                    self.preview = true;
                    self.last_key.clear();
                    self.stable_board.reset();
                    self.started = Instant::now();
                    self.frames = 0;
                    self.completed = 0;
                    self.error.clear();
                    self.status = "AI armed; focus the target game".into();
                }
                if ui.button("Pause").clicked() {
                    self.stop("Paused");
                }
                if ui.button("Stop").clicked() {
                    self.stop("Stopped");
                    self.preview = false;
                }
                if ui
                    .add(egui::Button::new("Emergency Stop").fill(Color32::from_rgb(135, 35, 45)))
                    .clicked()
                {
                    self.stop("Emergency stop; all owned keys released");
                }
                if ui.button("Save Configuration").clicked() {
                    if let Err(e) = self.cfg.controls.validate() {
                        self.error = format!("Cannot save invalid key bindings: {e}");
                    } else {
                        let save = std::fs::create_dir_all("config").and_then(|_| {
                            std::fs::write(
                                "config/local.toml",
                                toml::to_string_pretty(&self.cfg).unwrap(),
                            )
                        });
                        match save {
                            Ok(()) => self.status = "Saved config/local.toml".into(),
                            Err(e) => self.error = e.to_string(),
                        }
                    }
                }
                ui.checkbox(&mut self.debug, "Debug Mode");
                if ui.checkbox(&mut self.always_on_top, "Always on Top").changed() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                        if self.always_on_top {
                            egui::WindowLevel::AlwaysOnTop
                        } else {
                            egui::WindowLevel::Normal
                        },
                    ));
                }
            });
            let mut changed_profile = false;
            let mut apply_profile = false;
            ui.horizontal_wrapped(|ui| {
                changed_profile |= ui.checkbox(&mut self.cfg.allow_external, "Allow external Tetris games").changed();
                changed_profile |= ui.checkbox(
                    &mut self.cfg.multiplayer_attack_mode,
                    "Multiplayer Attack (bottom garbage rises)"
                ).changed();
                if ui.button("Apply Game Profile").clicked() {
                    apply_profile = true;
                }
                if self.profile_pending {
                    ui.colored_label(Color32::YELLOW, "Pending changes: click Apply before Start AI");
                }
            });
            egui::CollapsingHeader::new("Game Keyboard Bindings")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label("Set these to the keys used by the detected game (not its block colors).");
                    ui.horizontal(|ui| {
                        ui.label("Keyboard input:");
                        changed_profile |= ui
                            .selectable_value(&mut self.cfg.controls.input_method, input::InputMethod::ScanCode, "Scan Code")
                            .changed();
                        changed_profile |= ui
                            .selectable_value(&mut self.cfg.controls.input_method, input::InputMethod::VirtualKey, "Virtual Key")
                            .changed();
                    });
                    ui.small("Scan Code is the default. If movement is ignored, try Virtual Key then Apply Game Profile.");
                    egui::Grid::new("game-keymap").num_columns(2).show(ui, |ui| {
                        for (label, value) in [
                            ("Left", &mut self.cfg.controls.left),
                            ("Right", &mut self.cfg.controls.right),
                            ("Rotate CW", &mut self.cfg.controls.cw),
                            ("Rotate CCW", &mut self.cfg.controls.ccw),
                            ("Soft Drop", &mut self.cfg.controls.down),
                            ("Hard Drop", &mut self.cfg.controls.hard_drop),
                        ] {
                            ui.label(label);
                            changed_profile |= ui.text_edit_singleline(value).changed();
                            ui.end_row();
                        }
                    });
                    ui.small("Examples: Left, Right, Up, Down, X, Z, Space, Shift, C. F10 stays reserved for emergency stop.");
                });
            egui::CollapsingHeader::new("Placement Speed & Safety")
                .default_open(false)
                .show(ui, |ui| {
                    changed_profile |= ui.checkbox(
                        &mut self.cfg.no_t_spin,
                        "NO T-SPIN (ordinary placements only)"
                    ).changed();
                    ui.small("Mini/Full T-spin recommendations are excluded. If CC2 only suggests spins, calculate a reachable non-spin placement instead.");
                    ui.label("Horizontal screenshot frequency:");
                    changed_profile |= ui.add(
                        egui::Slider::new(&mut self.cfg.movement.horizontal_verify_stride, 1..=3)
                            .text("horizontal taps/check")
                    ).changed();
                    ui.small("1 = every tap (conservative), 2 = balanced default, 3 = faster. Rotations and soft drops stay checked; the hard-drop landing is ALWAYS checked.");
                    ui.small("Changes require Apply Game Profile and a fresh-game baseline. If a game misses keys, use 1.");
                });
            if changed_profile {
                self.profile_pending = true;
                if self.armed || self.busy {
                    self.stop("Placement speed changed; Apply Game Profile before resuming AI");
                }
            }
            if apply_profile {
                self.apply_game_profile();
            }
            ui.horizontal(|ui| {
                let before = self.baseline;
                ui.checkbox(
                    &mut self.baseline,
                    "Confirmed fresh guideline game: hidden rows empty, combo=0, B2B=false",
                );
                if before && !self.baseline {
                    self.stop("Baseline revoked");
                }
            });
            ui.horizontal(|ui| {
                ui.label("Monitor");
                if let Ok(ms) = capture::monitors() {
                    for (i, m) in ms.iter().enumerate() {
                        if ui
                            .selectable_label(self.cfg.monitor == i, &m.name)
                            .clicked()
                        {
                            self.cfg.monitor = i;
                            self.stop("Monitor changed");
                            self.runtime.rescan(i);
                            self.baseline = false;
                        }
                    }
                }
                ui.label("View:");
                for mode in [
                    Mode::Original,
                    Mode::Grayscale,
                    Mode::Edges,
                    Mode::Heatmap,
                    Mode::Occupancy,
                    Mode::Silhouettes,
                    Mode::Reconstruction,
                ] {
                    ui.selectable_value(&mut self.mode, mode, format!("{mode:?}"));
                }
            });
        });
        egui::SidePanel::right("stats").min_width(245.).show(ctx,|ui|{
   ui.heading("State & Diagnostics");ui.colored_label(if self.armed {Color32::LIGHT_GREEN}else{Color32::GRAY},if self.armed{"AI ARMED"}else{"INPUT DISABLED"});
   if let Some(v)=&self.view {let o=&v.observation;ui.label(format!("Current piece: {:?}",o.active));ui.label(format!("Cell confidence: {:.1}%",o.confidence*100.));ui.colored_label(Color32::LIGHT_BLUE,"Current-piece-only CC2 mode | NEXT/HOLD scanning removed");for reason in &o.reasons{ui.colored_label(Color32::YELLOW,reason);} }
   ui.separator();ui.label(format!("Suggested: {:?}",self.moves.first()));ui.label(format!("Last verified move: {}",self.completed));
   for (name,t) in [("Capture",&self.capture),("Vision",&self.vision),("Search",&self.search),("Input",&self.input),("Verify",&self.verify)] {let r=t.report();ui.label(format!("{name}: {:.2} ms / p95 {:.2}",r.mean_ms,r.p95_ms));}
   ui.separator();ui.label(&self.status);if !self.error.is_empty(){ui.colored_label(Color32::LIGHT_RED,format!("Last verification error: {}",self.error));}
   ui.separator();ui.small(format!("External Tetris windows: {} | F10 emergency stop | Game keyboard profile must match",if self.cfg.allow_external {"ENABLED"}else{"DISABLED"}));
  });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(2, |cols| {
                if matches!(
                    self.mode,
                    Mode::Occupancy | Mode::Silhouettes | Mode::Reconstruction
                ) {
                    self.board(&mut cols[0], self.mode);
                } else {
                    self.live(&mut cols[0]);
                }
                self.board(&mut cols[1], Mode::Reconstruction);
            });
        });
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}
pub fn run(cfg: Config) -> Result<()> {
    capture::dpi_aware();
    eframe::run_native(
        "SilhouetteTetris Debugger",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1300., 820.])
                .with_position([600., 20.])
                .with_always_on_top(),
            ..Default::default()
        },
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(Debugger::new(cfg)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
/// Read-only headless capture by default. Input requires --allow-input and --fresh-game.
pub fn headless(cfg: Config, seconds: u64, allow_input: bool, fresh: bool) -> Result<()> {
    if allow_input && !fresh {
        bail!("--allow-input also requires --fresh-game")
    }
    let multiplayer_attack_mode = cfg.multiplayer_attack_mode;
    let rt = Runtime::start(cfg);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut busy = false;
    let mut recovery_guard: Option<(Snapshot, Instant)> = None;
    let mut recovery_attempts = 0u8;
    let mut completed = 0;
    let mut capture = Timings::default();
    let mut vision = Timings::default();
    let mut search = Timings::default();
    let mut inputs = Timings::default();
    let mut verify = Timings::default();
    let mut frames = 0;
    let mut locked = 0;
    let mut good = 0;
    let mut last = String::new();
    let mut failures = vec![];
    let mut last_observation = String::new();
    while Instant::now() < deadline {
        while let Ok(e) = rt.events.try_recv() {
            match e {
                Event::Completed {
                    input_ms,
                    verify_ms,
                    ..
                } => {
                    completed += 1;
                    recovery_guard = None;
                    recovery_attempts = 0;
                    inputs.record(input_ms);
                    verify.record(verify_ms);
                    busy = false;
                    last.clear();
                }
                Event::Suggestion(_, ms) => {
                    search.record(ms);
                }
                Event::PreviewDone => busy = false,
                Event::Recovering { reason, snapshot } => {
                    busy = false;
                    last.clear();
                    recovery_attempts = recovery_attempts.saturating_add(1);
                    if recovery_attempts > 3 {
                        failures.push(format!("headless recovery budget exhausted: {reason}"));
                    } else {
                        recovery_guard = Some((snapshot, Instant::now()));
                    }
                }
                Event::Failed(e) => {
                    failures.push(e);
                    busy = false;
                }
                Event::Status(s) => eprintln!("{s}"),
            }
        }
        let Ok(v) = rt.views.recv_timeout(Duration::from_millis(100)) else {
            continue;
        };
        let v = v?;
        frames += 1;
        capture.record(v.capture_ms);
        vision.record(v.vision_ms);
        if v.state == State::Locked {
            locked += 1;
        }
        if v.observation.confidence >= 0.9 && v.observation.active.is_some() {
            good += 1;
        }
        last_observation = format!(
            "state={} active={:?} hold={:?} next={:?} confidence={} reasons={:?} uncertain={:?}",
            v.state,
            v.observation.active,
            v.observation.hold,
            v.observation.next,
            v.observation.confidence,
            v.observation.reasons,
            v.observation
                .cells
                .iter()
                .enumerate()
                .flat_map(|(y, row)| row
                    .iter()
                    .enumerate()
                    .filter_map(move |(x, c)| (c.kind == CellKind::Uncertain).then_some((x, y))))
                .collect::<Vec<_>>()
        );
        if !allow_input || busy || !failures.is_empty() || v.state != State::Locked {
            continue;
        }
        let Ok(observed) = PlanningSnapshot::from_observation(&v.observation, fresh) else {
            continue;
        };
        if let Some((expected, since)) = &recovery_guard {
            if since.elapsed() > Duration::from_secs(3) {
                failures.push("headless reversible path could not revalidate within 3s".into());
                break;
            }
            if since.elapsed() < Duration::from_millis(120) {
                continue;
            }
            if !runtime::compatible_multiplayer_recovery(
                expected, &observed.snapshot, multiplayer_attack_mode
            ) {
                failures.push("headless board/current piece diverged before retry".into());
                break;
            }
            recovery_guard = None;
        }
        let g = v.grid.unwrap();
        let r = g.rect();
        if input::window_at(r.x + r.width as i32 / 2, r.y + r.height as i32 / 2)
            != input::foreground()
        {
            continue;
        }
        let key = format!(
            "{:?}/{:?}",
            Board::from_visible(&v.observation.cells, true).ok(),
            v.observation.active.map(|a| a.piece)
        );
        if key == last {
            continue;
        }
        // F10 is latched for this headless session. Only a new explicit
        // invocation may re-arm it; a new piece must never clear the stop.
        if rt.cancel.stopped() {
            failures.push("headless session canceled (F10/capture stop)".into());
            break;
        }
        rt.jobs.send(Job::Turn {
            frame: v.frame,
            grid: g,
            observation: v.observation,
            baseline: fresh,
            execute: true,
        })?;
        busy = true;
        last = key;
    }
    let report = serde_json::json!({"seconds":seconds,"frames":frames,"locked_frames":locked,"confident_frames":good,"verified_moves":completed,"pps":completed as f64/seconds as f64,"capture":capture.report(),"vision":vision.report(),"search":search.report(),"input":inputs.report(),"verification":verify.report(),"failures":failures,"last_observation":last_observation});
    std::fs::create_dir_all("logs")?;
    std::fs::write(
        "logs/headless-report.json",
        serde_json::to_string_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if !failures.is_empty() {
        bail!("headless execution failed; see logs/headless-report.json")
    }
    Ok(())
}
