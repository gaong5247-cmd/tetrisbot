use detection::*;
use game_state::*;
#[test]
fn lock_loss_move_and_recovery() {
    let cfg = DetectionConfig {
        stable_frames: 3,
        loss_grace_frames: 2,
        ..Default::default()
    };
    let mut t = Tracker::new(cfg);
    let g = Grid {
        x: 100.,
        y: 50.,
        cw: 24.,
        ch: 24.,
        score: 0.95,
        shape_evidence: true,
    };
    assert_eq!(t.state, State::Searching);
    assert!(!t.may_input());
    t.update(&[g]);
    assert_eq!(t.state, State::Candidate);
    t.update(&[g]);
    assert_eq!(t.state, State::Validating);
    t.update(&[g]);
    assert!(t.may_input());
    let generation = t.generation;
    t.update(&[]);
    assert_eq!(t.state, State::Locked);
    assert!(!t.may_input());
    assert_eq!(t.generation, generation);
    t.update(&[]);
    assert_eq!(t.state, State::Locked);
    assert!(!t.may_input());
    t.update(&[g]);
    assert!(t.may_input(), "valid frame should restore tracking immediately");
    assert_eq!(t.generation, generation);
    for _ in 0..3 { t.update(&[]); }
    assert_eq!(t.state, State::Lost);
    assert!(!t.may_input());
    t.update(&[]);
    assert_eq!(t.state, State::Recovering);
    let moved = Grid { x: 500., ..g };
    t.update(&[moved]);
    assert!(!t.may_input());
    t.update(&[moved]);
    t.update(&[moved]);
    assert!(t.may_input());
    t.reset();
    assert_eq!(t.state, State::Searching);
}
#[test]
fn unknown_hold_queue_and_cells_block_execution() {
    let mut o = Observation {
        confidence: 1.,
        active: Some(Location::spawn(Piece::T)),
        ..Default::default()
    };
    assert!(Snapshot::from_observation(&o, true).is_err());
    o.hold = Known::Value(None);
    o.next = vec![Piece::I, Piece::O, Piece::J];
    assert!(Snapshot::from_observation(&o, true).is_err());
    o.cells = [[Cell {
        kind: CellKind::Empty,
        confidence: 1.,
    }; 10]; 20];
    assert!(Snapshot::from_observation(&o, true).is_ok());
    assert!(Snapshot::from_observation(&o, false).is_err());
}
#[test]
fn cancellation_is_shared() {
    let c = input::Cancellation::default();
    let other = c.clone();
    c.stop();
    assert!(other.stopped());
    c.reset();
    assert!(!other.stopped());
}
#[test]
fn tracking_cancel_is_not_reported_as_f10_and_preserves_first_cause() {
    use input::{Cancellation, StopReason};
    let c = Cancellation::default();
    c.stop_with(StopReason::TrackingLost);
    c.stop();
    assert_eq!(c.reason(), Some(StopReason::TrackingLost));
    assert!(!c.reason().unwrap().to_string().contains("emergency stop"));
    // Check is side-effect-free for an already canceled keyboard; no OS keys sent.
    let keyboard = input::Keyboard::new(c.clone(), 0);
    assert!(
        keyboard
            .check()
            .unwrap_err()
            .to_string()
            .contains("board tracking lost")
    );
    c.reset();
    c.stop_with(StopReason::EmergencyStop);
    assert!(c.reason().unwrap().to_string().contains("emergency stop"));
}
#[test]
fn attached_t_spin_keeps_grid_lock_and_temporal_piece_tracking() {
    let mut lab = gui::lab::Lab::default();
    lab.board.rows[0] = 1023 ^ (1 << 4);
    lab.board.rows[1] = 1023 ^ ((1 << 3) | (1 << 4) | (1 << 5));
    lab.board.rows[2] = 1 << 3;
    let (_, grid) = gui::lab::fixture(1280, 720, (300, 90), 24, 0, true);
    let mut tracker = Tracker::new(DetectionConfig {
        stable_frames: 2,
        loss_grace_frames: 2,
        ..Default::default()
    });
    let frame = lab.frame(1280, 720, (300, 90), 24, 0, true);
    let initial = detection::score(&frame, grid, &Default::default());
    tracker.update(&[initial]);
    tracker.update(&[initial]);
    assert!(tracker.may_input());
    let generation = tracker.generation;
    let mut vision = vision::Vision::default();
    vision.observe(&frame, grid);
    lab.action(movement::Action::Cw);
    let mut saw_attached = false;
    for y in (1..=18).rev() {
        lab.active.y = y;
        let frame = lab.frame(1280, 720, (300, 90), 24, 0, true);
        let score = detection::score(&frame, grid, &Default::default());
        saw_attached |= !score.shape_evidence;
        assert!(tracker.observe_frame(&frame), "tracking at y={y}");
        assert!(tracker.may_input());
        assert_eq!(
            tracker.generation, generation,
            "contact must not revoke baseline"
        );
        let observed = vision.observe(&frame, grid);
        assert!(
            observed.confidence >= 0.9,
            "vision y={y}: {:?}",
            observed.reasons
        );
        assert_eq!(
            Board::from_visible(&observed.cells, true).unwrap(),
            lab.board
        );
    }
    lab.action(movement::Action::Cw);
    let frame = lab.frame(1280, 720, (300, 90), 24, 0, true);
    assert!(tracker.observe_frame(&frame));
    let observed = vision.observe(&frame, grid);
    assert!(observed.confidence >= 0.9);
    assert!(
        saw_attached,
        "fixture must exercise non-isolated active cells"
    );
    assert_eq!(observed.active.unwrap().rotation, Rotation::South);
    let old_grid = tracker.grid.unwrap();
    let moved = lab.frame(1280, 720, (600, 90), 24, 0, true);
    for i in 0..3 {
        assert!(!tracker.observe_frame(&moved), "moved grid frame {i}");
        assert!(!tracker.may_input(), "unconfirmed grid must never permit input");
    }
    assert_eq!(tracker.state, State::Lost);
    assert_eq!(old_grid.x, grid.x);
}

#[test]
fn reversible_t_spin_recovery_requires_same_board_queue_and_hold() {
    use gui::runtime::same_reversible_state;
    let before = Snapshot {
        board: Board::default(),
        active: Location::spawn(Piece::T),
        hold: Some(Piece::I),
        next: vec![Piece::L, Piece::J, Piece::S],
        combo: 2,
        back_to_back: true,
    };
    let mut after = before.clone();
    // Reversible Down/Rotate inputs can change location, not the queue.
    after.active.y -= 1;
    after.active.rotation = Rotation::East;
    assert!(same_reversible_state(&before, &after));
    after.board.rows[0] |= 1 << 4;
    assert!(!same_reversible_state(&before, &after), "unexpected lock is irreversible");
    after.board = before.board;
    after.next.remove(0);
    assert!(!same_reversible_state(&before, &after), "spawned piece changed queue");
    after.next = before.next.clone();
    after.hold = None;
    assert!(!same_reversible_state(&before, &after), "HOLD changed");
}


#[test]
fn any_foreground_tetris_window_allowed_with_external_profile() {
    use gui::runtime::validate_input_target;
    let config = gui::Config::default();
    assert!(config.allow_external);
    for title in ["Cold Clear (FPS: 59)", "TETR.IO", "Jstris", "Tetris.com", gui::lab::TITLE] {
        assert!(validate_input_target(title, config.allow_external).is_ok(), "{title}");
    }
    assert!(validate_input_target("Cold Clear (FPS: 59)", false).is_err());
    assert!(validate_input_target(gui::lab::TITLE, false).is_ok());
    assert!(validate_input_target("SilhouetteTetris Debugger", true).is_err());
}

#[test]
fn input_profiles_accept_common_keys_and_reject_unsafe_or_duplicate_bindings() {
    use input::{KeyBindings, virtual_key};
    assert_eq!(virtual_key("ArrowLeft").unwrap(), 0x25);
    assert_eq!(virtual_key("Space").unwrap(), 0x20);
    assert_eq!(virtual_key("Up").unwrap(), 0x26);
    assert_eq!(virtual_key("Shift").unwrap(), 0x10);
    assert_eq!(virtual_key("F10").unwrap(), 0x79);
    assert!(virtual_key("UnrecognizedKeyName").is_err());
    let mut keys = KeyBindings::default();
    keys.validate().unwrap();
    keys.cw = "Up".into();
    keys.hold = "LeftShift".into();
    keys.validate().unwrap();
    keys.ccw = "F10".into();
    assert!(keys.validate().is_err(), "reserved emergency key may never be mapped");
    keys.ccw = "Left".into();
    assert!(keys.validate().is_err(), "two actions should not share a key");
}

#[test]
fn shipped_external_profile_loads_and_keymap_is_resolved() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../config/default.toml");
    let config = gui::Config::load(&path).unwrap();
    assert!(config.allow_external, "the bundled profile must permit external games");
    assert_eq!(config.controls.key(movement::Action::HardDrop).unwrap(), 0x20);
    assert_eq!(config.controls.key(movement::Action::Cw).unwrap(), 0x58);
}


#[test]
fn current_piece_only_mode_discards_next_and_hold_even_if_provided() {
    let empty = Cell { kind: CellKind::Empty, confidence: 1. };
    let mut o = Observation {
        cells: [[empty; 10]; 20],
        active: Some(Location::spawn(Piece::T)),
        confidence: 0.99,
        next: vec![],
        hold: Known::Unknown,
        ..Default::default()
    };
    let limited = PlanningSnapshot::from_observation(&o, true).unwrap();
    assert!(limited.restricted_no_hold);
    assert_eq!(limited.snapshot.queue(), vec![Piece::T]);
    assert_eq!(limited.snapshot.hold, Some(Piece::T)); // internal synthetic reserve
    assert_eq!(limited.snapshot.next.len(), 0);
    assert!(!limited.snapshot.needs_hold(Piece::T).unwrap());
    assert!(limited.snapshot.needs_hold(Piece::I).is_err());

    // Even populated HUD fields must be completely discarded.
    o.next = vec![Piece::O, Piece::L, Piece::J, Piece::Z];
    o.hold = Known::Value(Some(Piece::I));
    let still = PlanningSnapshot::from_observation(&o, true).unwrap();
    assert!(still.restricted_no_hold);
    assert_eq!(still.snapshot.queue(), vec![Piece::T]);
    assert_eq!(still.snapshot.hold, Some(Piece::T));
    assert!(still.snapshot.next.is_empty());
    assert_eq!(
        PlanningSnapshot::with_mode(&o, true, false).unwrap().snapshot.queue(),
        vec![Piece::T],
        "full-preview path must never be reachable through the vision planner"
    );

    // Current-piece-only is not permission to invent unobserved stacks.
    o.cells[19][4].kind = CellKind::Uncertain;
    assert!(PlanningSnapshot::from_observation(&o, true).is_err());
    o.cells[19][4] = empty;
    assert!(PlanningSnapshot::from_observation(&o, false).is_err());
    o.confidence = 0.35;
    assert!(PlanningSnapshot::from_observation(&o, true).is_err());
}

#[test]
fn stale_fullscreen_scan_does_not_invalidate_locked_board() {
    let mut t = Tracker::new(DetectionConfig {
        stable_frames: 2,
        loss_grace_frames: 6,
        ..Default::default()
    });
    let g = Grid {
        x: 110., y: 35., cw: 24., ch: 24.,
        score: 0.94, shape_evidence: true,
    };
    t.update(&[g]);
    t.update(&[g]);
    let generation = t.generation;
    assert!(t.may_input());
    // Scanner result was captured before the playfield moved into ROI.
    t.apply_scan(&[]);
    t.apply_scan(&[Grid { x: 600., ..g }]);
    assert_eq!(t.generation, generation);
    assert_eq!(t.grid.unwrap().x, g.x);
    assert!(t.may_input());

    let moderate = Grid { score: 0.6, shape_evidence: false, ..g };
    t.update(&[moderate]);
    assert!(t.may_input(), "verified grid should survive animation contrast");
    for _ in 0..6 {
        t.update(&[]);
        assert!(!t.may_input(), "no inputs while temporarily unconfirmed");
        assert_eq!(t.generation, generation);
    }
    t.update(&[g]);
    assert_eq!(t.generation, generation);
    assert!(t.may_input());
    for _ in 0..7 { t.update(&[]); }
    assert_eq!(t.state, State::Lost);
    assert!(!t.may_input());
}


#[test]
fn keyboard_scancode_and_vk_profiles_can_be_selected() {
    use input::{InputMethod, KeyBindings};
    let mut controls = KeyBindings::default();
    assert_eq!(controls.input_method, InputMethod::ScanCode);
    controls.validate().unwrap();
    assert_eq!(
        serde_json::to_value(&controls).unwrap()["input_method"],
        "scan_code"
    );
    controls.input_method = InputMethod::VirtualKey;
    controls.validate().unwrap();
    let json = serde_json::to_value(&controls).unwrap();
    assert_eq!(json["input_method"], "virtual_key");
    let decoded: KeyBindings = serde_json::from_value(json).unwrap();
    assert_eq!(decoded.input_method, InputMethod::VirtualKey);
    assert_eq!(decoded.key(movement::Action::HardDrop).unwrap(), 0x20);
}

#[test]
fn post_lock_diagnostics_identify_ignored_drop_vs_missing_spawn() {
    use gui::runtime::LockDiagnostics;
    let mut o = Observation::default();
    o.cells = [[Cell {kind: CellKind::Empty, confidence: 1.0}; 10];20];
    o.confidence = 0.99;
    o.active = Some(Location::spawn(Piece::T));
    let mut expected = Board::default();
    expected.rows[0] = 1 << 5;
    let mut diag = LockDiagnostics::default();
    assert!(!diag.observe(&o, &expected));
    assert_eq!(diag.report()["classification"], "board_mismatch_or_hard_drop_not_received");
    assert_eq!(diag.report()["board_difference_cells"], 1);
    o.cells[19][5].kind = CellKind::Locked;
    o.active = None;
    o.confidence = 0.4;
    assert!(!diag.observe(&o, &expected));
    assert_eq!(diag.report()["classification"], "next_active_not_visible");
    o.active = Some(Location::spawn(Piece::I));
    o.confidence = 0.99;
    assert!(!diag.observe(&o, &expected));
    assert_eq!(diag.report()["classification"], "verified");
    assert!(diag.observe(&o, &expected), "two stable verified frames required");
    o.cells[2][4].kind = CellKind::Uncertain;
    assert!(!diag.observe(&o, &expected));
    assert_eq!(diag.report()["classification"], "uncertain_board_or_animation");
}

#[test]
fn harddrop_pulse_crosses_multiple_60fps_frame_boundaries() {
    let p = movement::MovementProfile::default();
    assert!(p.hard_drop_ms >= 50);
    assert!(p.hard_drop_ms > p.pulse_ms);
    p.validate().unwrap();
    let mut bad = p;
    bad.hard_drop_ms = 1;
    assert!(bad.validate().is_err());
}
