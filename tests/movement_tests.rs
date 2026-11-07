use game_state::*;
use movement::*;
fn state(p: Piece) -> Snapshot {
    Snapshot {
        board: Board::default(),
        active: Location::spawn(p),
        hold: Some(Piece::I),
        next: vec![Piece::L, Piece::J, Piece::S, Piece::O],
        combo: 0,
        back_to_back: false,
    }
}
#[test]
fn reaches_t_spin_double_without_horizontal_shuffling() {
    let mut s = state(Piece::T);
    s.board.rows[0] = 1023 ^ (1 << 4);
    s.board.rows[1] = 1023 ^ ((1 << 3) | (1 << 4) | (1 << 5));
    s.board.rows[2] = 1 << 3;
    let target = Placement {
        location: Location {
            piece: Piece::T,
            rotation: Rotation::South,
            x: 4,
            y: 1,
        },
        spin: Spin::Full,
    };
    let route = plan(&s, target, &MovementProfile::default()).expect("reachable T-spin double");
    eprintln!(
        "T-spin route: {:?}, nodes={}",
        route.actions, route.expanded
    );
    assert!(
        !route
            .actions
            .iter()
            .any(|a| matches!(a, Action::Left | Action::Right)),
        "straight slot must not require sideways shuffling"
    );
    assert_eq!(
        route
            .expected
            .rows
            .iter()
            .map(|r| r.count_ones())
            .sum::<u32>(),
        1
    );
}
#[test]
fn plans_all_pieces_to_legal_floor_positions() {
    let profile = MovementProfile::default();
    for piece in PIECES {
        for rotation in ROTATIONS {
            for x in 0..10 {
                let s = state(piece);
                let p = Location {
                    piece,
                    rotation,
                    x,
                    y: 18,
                };
                if s.board.collides(p) {
                    continue;
                }
                let target = Placement {
                    location: s.board.dropped(p),
                    spin: Spin::None,
                };
                let path = plan(&s, target, &profile).unwrap();
                assert_eq!(path.actions.last(), Some(&Action::HardDrop));
                assert!(
                    path.expected
                        .rows
                        .iter()
                        .map(|r| r.count_ones())
                        .sum::<u32>()
                        == 4
                );
            }
        }
    }
}
#[test]
fn srs_i_wall_kick_and_o_true_center() {
    let b = Board::default();
    let p = Location {
        piece: Piece::I,
        rotation: Rotation::East,
        x: 0,
        y: 10,
    };
    assert!(!b.collides(p));
    let (rot, _) = rotate(&b, p, true).unwrap();
    assert!(!b.collides(rot));
    assert!(rot.x > 0);
    let o = Location {
        piece: Piece::O,
        rotation: Rotation::North,
        x: 0,
        y: 0,
    };
    let (cw, _) = rotate(&b, o, true).unwrap();
    assert!(o.same_cells(cw));
}
#[test]
fn target_collision_or_floating_rejected() {
    let s = state(Piece::T);
    let t = Placement {
        location: Location { y: 10, ..s.active },
        spin: Spin::None,
    };
    assert!(plan(&s, t, &MovementProfile::default()).is_err());
}
#[test]
fn hold_inferred_from_queue_and_reserve() {
    let s = state(Piece::T);
    assert!(!s.needs_hold(Piece::T).unwrap());
    assert!(s.needs_hold(Piece::I).unwrap());
    assert!(s.needs_hold(Piece::Z).is_err());
    let mut empty = s.clone();
    empty.hold = None;
    assert!(empty.needs_hold(Piece::L).unwrap());
    assert_eq!(
        empty.queue(),
        vec![Piece::T, Piece::L, Piece::J, Piece::S, Piece::O]
    );
}
#[test]
fn timed_profile_rejects_auto_repeat_and_unknown_rules() {
    let mut p = MovementProfile::default();
    p.pulse_ms = p.das_ms;
    assert!(p.validate().is_err());
    p = MovementProfile::default();
    p.guideline_srs = false;
    assert!(p.validate().is_err());
}
#[test]
fn blocked_spawn_and_spin_do_not_succeed_by_faking_path() {
    let mut s = state(Piece::T);
    s.board.rows[18] = 1023;
    let target = Placement {
        location: Location { y: 0, ..s.active },
        spin: Spin::Full,
    };
    assert!(plan(&s, target, &MovementProfile::default()).is_err());
}

#[test]
fn visual_feedback_time_counts_toward_t_spin_gravity_budget() {
    let profile = MovementProfile::default();
    assert_eq!(profile.step_ms(), 70);
    let mut no_vision = profile.clone();
    no_vision.feedback_ms = 0;
    assert_eq!(no_vision.step_ms(), 40);
    let mut impossible = profile;
    impossible.gravity_ms = 60;
    assert!(impossible.validate().is_err(), "a slow visual loop cannot be modeled as faster than gravity");
}
