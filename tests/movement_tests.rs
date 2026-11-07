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
    assert_eq!(profile.step_ms(), 94);
    let mut no_vision = profile.clone();
    no_vision.feedback_ms = 0;
    assert_eq!(no_vision.step_ms(), 64);
    let mut impossible = profile;
    impossible.gravity_ms = 60;
    assert!(impossible.validate().is_err(), "a slow visual loop cannot be modeled as faster than gravity");
}


#[test]
fn instant_hard_drop_uses_no_ghost_position_and_no_soft_drop_on_open_board() {
    let snapshot = state(Piece::S);
    let target = Placement {
        location: snapshot.board.dropped(snapshot.active),
        spin: Spin::None,
    };
    let path = plan(&snapshot, target, &MovementProfile::default()).unwrap();
    assert_eq!(path.actions, vec![Action::HardDrop]);
    assert_eq!(path.expected.rows.iter().map(|r|r.count_ones()).sum::<u32>(),4);

    // Changing which ghost marker the UI draws never enters the planner.
    // The actual target is Cold Clear 2's grounded placement, not a pixel.
    for piece in PIECES {
        for rotation in ROTATIONS {
            let snapshot = state(piece);
            let seed = Location { rotation, x: 4, ..snapshot.active };
            if snapshot.board.collides(seed) { continue; }
            let desired = Placement {
                location: snapshot.board.dropped(seed),
                spin: Spin::None,
            };
            let path = plan(&snapshot, desired, &MovementProfile::default()).unwrap();
            assert_eq!(path.actions.last(), Some(&Action::HardDrop));
            assert!(!path.actions.contains(&Action::Down),
                "open-board {:?}/{:?} should rotate and hard drop without waiting",
                piece,rotation);
        }
    }
}


#[test]
fn repeated_horizontal_inputs_use_fewer_screenshots_without_skipping_t_spins() {
    let right = Action::Right;
    let drop = Action::HardDrop;
    let cw = Action::Cw;
    let down = Action::Down;
    let no_spin = [right, right, right, drop];
    let checks = |actions: &[Action], spin: Spin, stride: u8| {
        (0..actions.len()).map(|i| should_verify_after_action(actions, i, spin, stride))
            .collect::<Vec<_>>()
    };
    assert_eq!(checks(&no_spin, Spin::None, 1), [true, true, true, false]);
    assert_eq!(checks(&no_spin, Spin::None, 2), [false, true, false, false]);
    assert_eq!(checks(&no_spin, Spin::None, 3), [false, false, false, false]);
    assert_eq!(checks(&no_spin, Spin::Full, 3), [true, true, true, false]);
    assert_eq!(checks(&[right, right, cw, drop], Spin::None, 2),
        [false, true, true, false]);
    assert_eq!(checks(&[down, drop], Spin::None, 3), [true, false],
        "soft drop needs separate contact verification");
    assert_eq!(checks(&[Action::Left, drop], Spin::None, 1), [true, false]);
    assert_eq!(checks(&[Action::Left, drop], Spin::None, 2), [false, false],
        "pre-drop landing verification covers the final horizontal move");

    let mut invalid = MovementProfile::default();
    invalid.horizontal_verify_stride = 0;
    assert!(invalid.validate().is_err());
    invalid.horizontal_verify_stride = 4;
    assert!(invalid.validate().is_err());
    invalid.horizontal_verify_stride = 3;
    invalid.validate().unwrap();
}


#[test]
fn no_t_spin_rejects_spin_only_engine_moves_and_uses_normal_drop() {
    let snapshot = state(Piece::T);
    let landing = snapshot.board.dropped(snapshot.active);
    let only_spins = [
        Placement { location: landing, spin: Spin::Mini },
        Placement { location: landing, spin: Spin::Full },
    ];
    // No ordinary suggestions: do NOT execute the mini/full spin just to
    // satisfy CC2; compute a reachable ordinary piece placement instead.
    let planned = plan_no_t_spin(&snapshot, &only_spins, &MovementProfile::default()).unwrap();
    assert_eq!(planned.placement.spin, Spin::None);
    assert_eq!(planned.placement.location.piece, Piece::T);
    assert!(!planned.uses_hold);
    assert!(!planned.actions.contains(&Action::Hold));
    assert_eq!(planned.actions.last(), Some(&Action::HardDrop));
    let recomputed = plan(&snapshot, planned.placement, &MovementProfile::default()).unwrap();
    assert_eq!(planned.expected, recomputed.expected);
}

#[test]
fn no_t_spin_preserves_reachable_cc2_normal_recommendation() {
    let snapshot = state(Piece::T);
    let mut preferred = snapshot.board.dropped(snapshot.active);
    preferred.x = 3;
    let preferred = snapshot.board.dropped(preferred);
    let normal = Placement { location: preferred, spin: Spin::None };
    let candidates = [
        Placement {location: snapshot.board.dropped(snapshot.active), spin: Spin::Full },
        normal,
    ];
    let planned = plan_no_t_spin(&snapshot, &candidates, &MovementProfile::default()).unwrap();
    assert_eq!(planned.placement.spin, Spin::None);
    assert!(planned.placement.location.same_cells(normal.location));
    assert_eq!(planned.actions.last(), Some(&Action::HardDrop));
}

#[test]
fn no_t_spin_fallback_always_selects_current_piece_without_hold() {
    for piece in PIECES {
        let snapshot = state(piece);
        let planned = plan_no_t_spin(&snapshot, &[], &MovementProfile::default())
            .unwrap_or_else(|err| panic!("{piece:?} fallback failed: {err}"));
        assert_eq!(planned.placement.location.piece, piece);
        assert_eq!(planned.placement.spin, Spin::None);
        assert!(!planned.uses_hold);
        assert!(!planned.actions.contains(&Action::Hold));
        assert_eq!(planned.actions.last(), Some(&Action::HardDrop));
    }
}
