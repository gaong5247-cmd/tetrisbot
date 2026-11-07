//! Deterministic rendered-pixel/real-engine loop. The desktop SendInput run is
//! separately measured in VALIDATION.md; this test does not claim OS input coverage.
use game_state::*;
use gui::lab::Lab;
use movement::{Action, MovementProfile, plan};
use std::{path::PathBuf, time::Duration};

#[test]
fn thirty_rendered_turns_with_actual_engine_and_visual_verification() {
    let mut lab = Lab::default();
    let frame = lab.frame(1280, 720, (300, 90), 24, 0, true);
    let grid = detection::scan(&frame, &Default::default())[0];
    let mut vision = vision::Vision::default();
    let mut observed = vision.observe(&frame, grid);
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../engines/cold-clear-2.exe");
    let mut client = tbp_client::Client::spawn(engine, Duration::from_secs(5)).unwrap();
    // The synthetic Lab advances only on explicit actions: no wall-clock capture
    // delay or gravity tick occurs between frames in this deterministic test.
    let mut deterministic_profile = MovementProfile::default();
    deterministic_profile.feedback_ms = 0;
    let initial = Snapshot::from_observation(&observed, true).unwrap();
    client.start(&initial).unwrap();
    for turn in 0..30 {
        let state = Snapshot::from_observation(&observed, true)
            .unwrap_or_else(|e| panic!("turn {turn} input observation: {e}; {observed:?}"));
        assert_eq!(state.board, lab.board, "turn {turn} before move");
        let moves = client.suggest().unwrap();
        let route = moves
            .into_iter()
            .find_map(|m| plan(&state, m, &deterministic_profile).ok())
            .expect("reachable recommendation");
        for &action in &route.actions {
            if action == Action::HardDrop {
                let b = Board::from_visible(&observed.cells, true)
                    .unwrap_or_else(|e| panic!("turn {turn} pre-drop {e}; {observed:?}"));
                assert_eq!(b, state.board, "turn {turn} pre-drop stack");
                assert!(
                    b.dropped(observed.active.expect("visible active"))
                        .same_cells(route.placement.location),
                    "turn {turn} landing differs"
                );
            }
            lab.action(action);
            observed = vision.observe(&lab.frame(1280, 720, (300, 90), 24, turn % 2, true), grid);
        }
        let next = Snapshot::from_observation(&observed, true)
            .unwrap_or_else(|e| panic!("turn {turn} verification {e}; {observed:?}"));
        assert_eq!(next.board, route.expected, "turn {turn} expected board");
        assert_eq!(next.board, lab.board, "turn {turn} visual board");
        assert_eq!(next.active.piece, lab.active.piece);
        client.play(route.placement).unwrap();
        client.new_piece(*next.next.last().unwrap()).unwrap();
    }
    assert_eq!(lab.placed, 30);
}


#[test]
fn repeated_cold_clear_turns_without_visible_next_or_hold() {
    // Even if a game's HUD never exposes NEXT/HOLD, the currently visible
    // piece can still be placed using the real CC2 process. This is a
    // deterministic pixel-rendered Lab test, not a SendInput desktop test.
    let mut lab = Lab::default();
    let (_, grid) = gui::lab::fixture(1280, 720, (300, 90), 24, 0, true);
    let mut vision = vision::Vision::default();
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../engines/cold-clear-2.exe");
    let mut client = tbp_client::Client::spawn(engine, Duration::from_secs(7)).unwrap();
    let mut profile = MovementProfile::default();
    profile.feedback_ms = 0; // deterministic Lab has no real-time capture delay
    for turn in 0..8 {
        let frame = lab.frame(1280, 720, (300, 90), 24, turn % 2, true);
        let mut observed = vision.observe(&frame, grid);
        observed.next.clear(); // simulate inaccessible external-game HUD
        observed.hold = Known::Unknown;
        let planning = PlanningSnapshot::from_observation(&observed, true).unwrap();
        assert!(planning.restricted_no_hold);
        let state = planning.snapshot;
        client.resync_with_mode(&state, true).unwrap();
        let choices = client.suggest().unwrap();
        let route = choices.into_iter()
            .filter(|m| m.location.piece == state.active.piece)
            .find_map(|m| movement::plan(&state, m, &profile).ok())
            .unwrap_or_else(||panic!("no safe CC2 current-piece move on turn {turn}"));
        assert!(!route.uses_hold);
        assert!(!route.actions.contains(&Action::Hold));
        for &action in &route.actions {
            lab.action(action);
            let _ = vision.observe(&lab.frame(1280, 720, (300, 90), 24, turn % 2, true), grid);
        }
        assert!(!lab.over);
        assert_eq!(lab.board, route.expected, "visual game board after turn {turn}");
    }
    assert_eq!(lab.placed, 8);
}
