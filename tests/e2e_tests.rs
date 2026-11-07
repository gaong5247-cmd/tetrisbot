//! Real Cold Clear 2 + procedurally rendered Local Lab pixels.
//! This validates current-piece-only gameplay; Windows SendInput is not covered.
use game_state::*;
use gui::lab::Lab;
use movement::{Action, MovementProfile, plan};
use std::{path::PathBuf, time::Duration};

#[test]
fn repeated_rendered_turns_use_only_current_piece_without_next_or_hold() {
    let mut lab = Lab::default();
    let (_, grid) = gui::lab::fixture(1280, 720, (300, 90), 24, 0, true);
    let mut vision = vision::Vision::default();
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../engines/cold-clear-2.exe");
    let mut client = tbp_client::Client::spawn(engine, Duration::from_secs(7)).unwrap();
    let mut profile = MovementProfile::default();
    profile.feedback_ms = 0; // deterministic Lab advances on explicit input only

    for turn in 0..12 {
        let frame = lab.frame(1280, 720, (300, 90), 24, turn % 2, true);
        let observed = vision.observe(&frame, grid);
        // Even though the rendered game has visible NEXT/HOLD panels,
        // the robot never attempts to read or send those values to CC2.
        assert!(observed.next.is_empty(), "NEXT must not be recognized");
        assert_eq!(observed.hold, Known::Unknown, "HOLD must not be recognized");
        let state = PlanningSnapshot::from_observation(&observed, true)
            .unwrap_or_else(|err| panic!("turn {turn}: {err}; {observed:?}")).snapshot;
        assert_eq!(state.board, lab.board);
        assert_eq!(state.queue(), vec![state.active.piece]);
        assert_eq!(state.hold, Some(state.active.piece));

        client.resync_with_mode(&state, true).unwrap();
        let choices = client.suggest().unwrap();
        let route = choices.into_iter()
            .filter(|m| m.location.piece == state.active.piece)
            .find_map(|m| plan(&state, m, &profile).ok())
            .unwrap_or_else(|| panic!("turn {turn}: no reachable current-piece suggestion"));
        assert!(!route.uses_hold);
        assert!(!route.actions.contains(&Action::Hold));

        for &action in &route.actions {
            if action == Action::HardDrop {
                let fresh = vision.observe(
                    &lab.frame(1280, 720, (300, 90), 24, turn % 2, true), grid);
                let board = Board::from_visible(&fresh.cells, true).unwrap();
                assert_eq!(board, state.board);
                assert!(
                    board.dropped(fresh.active.expect("visible active")).same_cells(route.placement.location),
                    "turn {turn}: must visually confirm exact landing before dropping"
                );
            }
            lab.action(action);
            let _ = vision.observe(
                &lab.frame(1280, 720, (300, 90), 24, turn % 2, true), grid);
        }
        assert!(!lab.over, "game over at turn {turn}");
        assert_eq!(lab.board, route.expected, "expected post-lock stack at turn {turn}");
        let after = vision.observe(
            &lab.frame(1280, 720, (300, 90), 24, turn % 2, true), grid);
        let actual = PlanningSnapshot::from_observation(&after, true).unwrap().snapshot;
        assert_eq!(actual.board, lab.board, "pixel-based post-lock board at turn {turn}");
        assert_eq!(actual.active.piece, lab.active.piece);
        assert!(actual.next.is_empty());
    }
    assert_eq!(lab.placed, 12);
}
