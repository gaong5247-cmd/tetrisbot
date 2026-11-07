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
    let initial = Snapshot::from_observation(&observed, true).unwrap();
    client.start(&initial).unwrap();
    for turn in 0..30 {
        let state = Snapshot::from_observation(&observed, true)
            .unwrap_or_else(|e| panic!("turn {turn} input observation: {e}; {observed:?}"));
        assert_eq!(state.board, lab.board, "turn {turn} before move");
        let moves = client.suggest().unwrap();
        let route = moves
            .into_iter()
            .find_map(|m| plan(&state, m, &MovementProfile::default()).ok())
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
