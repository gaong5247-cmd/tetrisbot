use game_state::*;
use std::{path::PathBuf, time::Duration};
use tbp_client::*;
fn engine() -> PathBuf {
    std::env::var_os("CC2_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../engines/cold-clear-2.exe")
        })
}
#[test]
fn actual_cold_clear_lifecycle_and_restarts() {
    let path = engine();
    assert!(
        path.is_file(),
        "actual CC2 executable required at {} or CC2_EXE",
        path.display()
    );
    for hold in [None, Some(Piece::I)] {
        let mut c = Client::spawn(&path, Duration::from_secs(5)).unwrap();
        assert!(c.info.contains("Cold Clear 2"));
        let s = Snapshot {
            board: Board::default(),
            active: Location::spawn(Piece::T),
            hold,
            next: vec![Piece::L, Piece::J, Piece::S, Piece::O, Piece::Z],
            combo: 0,
            back_to_back: false,
        };
        c.start(&s).unwrap();
        let moves = c.suggest().unwrap();
        assert!(!moves.is_empty());
        let best = moves[0];
        assert!(s.needs_hold(best.location.piece).is_ok());
        assert!(!s.board.collides(best.location));
        assert_eq!(s.board.dropped(best.location), best.location);
        let path = movement::plan(&s, best, &movement::MovementProfile::default()).unwrap();
        assert_eq!(path.actions.last(), Some(&movement::Action::HardDrop));
        c.play(best).unwrap();
        c.new_piece(Piece::I).unwrap();
        let moves2 = c.suggest().unwrap();
        assert!(!moves2.is_empty());
        c.stop().unwrap();
        c.resync(&s).unwrap();
        assert!(!c.suggest().unwrap().is_empty());
    }
}
#[test]
fn exact_wire_fields_and_all_spin_values() {
    for spin in [Spin::None, Spin::Mini, Spin::Full] {
        let p = Placement {
            location: Location {
                piece: Piece::I,
                rotation: Rotation::East,
                x: 3,
                y: 5,
            },
            spin,
        };
        let v = serde_json::to_value(p).unwrap();
        assert_eq!(v["location"]["type"], "I");
        assert_eq!(v["location"]["orientation"], "east");
        assert!(v["location"].get("piece").is_none());
        assert_eq!(serde_json::from_value::<Placement>(v).unwrap(), p);
    }
}
#[test]
fn actual_process_death_and_timeout_fail_closed() {
    let mut c = Client::spawn(engine(), Duration::from_millis(200)).unwrap();
    c.send(&serde_json::json!({"type":"quit"})).unwrap();
    assert!(c.receive().is_err());
}


#[test]
fn actual_cold_clear_search_with_no_next_and_unknown_hold() {
    // Only a fresh-game board and the current piece are available.
    // No future pieces are fabricated. The reserve is an internal search-only
    // duplicate; the game does NOT receive a Hold key.
    let mut observed = Observation::default();
    observed.confidence = 1.0;
    observed.active = Some(Location::spawn(Piece::S));
    observed.cells = [[Cell { kind: CellKind::Empty, confidence: 1.0 }; 10]; 20];
    observed.next.clear();
    observed.hold = Known::Unknown;
    let limited = PlanningSnapshot::from_observation(&observed, true).unwrap();
    assert!(limited.restricted_no_hold);
    let mut c = Client::spawn(engine(), Duration::from_secs(7)).unwrap();
    c.start_with_mode(&limited.snapshot, true).unwrap();
    let moves = c.suggest().expect("CC2 should speculate beyond unknown NEXT");
    let safe = moves.iter().copied()
        .filter(|m| m.location.piece == Piece::S)
        .find_map(|m| movement::plan(
            &limited.snapshot, m, &movement::MovementProfile::default()
        ).ok());
    assert!(safe.is_some(), "No HOLD-free current-piece CC2 move available");
    c.stop().unwrap();

    // Repeated turns reinitialize the engine instead of advancing synthetic
    // future-piece and reserve states.
    let mut observed2 = observed;
    observed2.active = Some(Location::spawn(Piece::T));
    let limited2 = PlanningSnapshot::from_observation(&observed2, true).unwrap();
    c.resync_with_mode(&limited2.snapshot, true).unwrap();
    let moves2 = c.suggest().unwrap();
    assert!(moves2.iter().any(|m| m.location.piece == Piece::T));
}


#[test]
fn external_engine_death_before_any_input_is_restarted_once() {
    // A child that exits while between moves must not permanently disable
    // autoplay; retrying before SendInput is safe. Do not retry a hard drop.
    let mut observed = Observation::default();
    observed.confidence = 1.0;
    observed.active = Some(Location::spawn(Piece::T));
    observed.cells = [[Cell {kind: CellKind::Empty,confidence:1.0};10];20];
    let snapshot = PlanningSnapshot::from_observation(&observed,true).unwrap().snapshot;
    let path = engine();
    let timeout = Duration::from_secs(5);
    let mut client = Some(Client::spawn(&path, timeout).unwrap());
    client.as_mut().unwrap()
        .send(&serde_json::json!({"type":"quit"})).unwrap();
    // Wait for EOF before asking the worker to recover the crashed process.
    assert!(client.as_ref().unwrap().receive().is_err());
    let (moves, restarted) = gui::runtime::suggest_current_with_restart(
        &mut client, &path, timeout, &snapshot
    ).unwrap();
    assert!(restarted);
    assert!(moves.iter().any(|m| m.location.piece==Piece::T));
}
