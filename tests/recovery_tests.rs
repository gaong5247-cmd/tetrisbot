use detection::*;
use game_state::*;
#[test]
fn lock_loss_move_and_recovery() {
    let cfg = DetectionConfig {
        stable_frames: 3,
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
    t.update(&[]);
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
    assert!(
        !tracker.observe_frame(&moved),
        "actual grid movement must still pause"
    );
    assert_eq!(tracker.state, State::Lost);
    assert_eq!(old_grid.x, grid.x);
}
