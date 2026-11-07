use game_state::*;
use gui::lab::{Lab, fixture};
#[test]
fn lower_active_evidence_survives_worker_handoff() {
    let mut lab = Lab::default();
    let (_, grid) = fixture(1280, 720, (300, 90), 24, 0, true);
    let mut tracker = vision::Vision::default();
    let mut observed = tracker.observe(&lab.frame(1280, 720, (300, 90), 24, 0, true), grid);
    for y in (5..18).rev() {
        lab.active.y = y;
        observed = tracker.observe(&lab.frame(1280, 720, (300, 90), 24, 0, true), grid);
    }
    assert!(observed.active.unwrap().y < 12);
    let mut worker = vision::Vision::default();
    worker.seed(&observed);
    lab.active.y -= 1;
    let fresh = worker.observe(&lab.frame(1280, 720, (300, 90), 24, 0, true), grid);
    assert_eq!(fresh.active.unwrap().piece, lab.active.piece);
    assert!(PlanningSnapshot::from_observation(&fresh, true).is_ok());
}
#[test]
fn all_shapes_all_rotations() {
    for piece in PIECES {
        for rotation in ROTATIONS {
            let p = Location {
                piece,
                rotation,
                x: 5,
                y: 12,
            };
            let matches = match_shape(&p.cells());
            assert!(matches.iter().any(|m| m.piece == piece && m.same_cells(p)));
        }
    }
}
#[test]
fn obscured_and_invalid_silhouettes() {
    assert!(match_shape(&[(0, 0), (1, 0), (2, 0)]).is_empty());
    assert!(match_shape(&[(0, 0), (1, 0), (2, 0), (7, 9)]).is_empty());
    assert!(match_shape(&[(0, 0), (0, 0), (1, 0), (2, 0)]).is_empty());
}
#[test]
fn palette_independent_occupancy_ignores_next_and_hold() {
    for skin in 0..=1 {
        let (f, g) = fixture(1280, 720, (300, 90), 24, skin, true);
        let mut vision = vision::Vision::default();
        let o = vision.observe(&f, g);
        eprintln!("{o:?}");
        assert_eq!(o.active.unwrap().piece, Piece::T);
        // Local Lab visually renders HOLD and five NEXT pieces; the vision
        // engine must ignore them entirely and never classify their HUD.
        assert_eq!(o.hold, Known::Unknown);
        assert!(o.next.is_empty());
        assert_eq!(
            o.cells
                .iter()
                .flatten()
                .filter(|c| c.kind == CellKind::Active)
                .count(),
            4
        );
        assert!(
            Board::from_visible(&o.cells, true)
                .unwrap()
                .rows
                .iter()
                .all(|&r| r == 0)
        );
        assert!(o.confidence >= 0.9);
    }
}
#[test]
fn ghost_active_overlap_is_not_locked() {
    let mut lab = Lab::default();
    lab.active = lab.board.dropped(lab.active);
    let f = lab.frame(1280, 720, (300, 90), 24, 0, true);
    let (_, g) = fixture(1280, 720, (300, 90), 24, 0, true);
    let mut vision = vision::Vision::default();
    let mut high = Lab::default();
    high.active.y = 12;
    vision.observe(&high.frame(1280, 720, (300, 90), 24, 0, true), g);
    let o = vision.observe(&f, g);
    assert!(o.active.is_some());
    assert_eq!(
        Board::from_visible(&o.cells, true).unwrap(),
        Board::default()
    );
}
#[test]
fn hidden_rows_are_not_invented() {
    let o = Observation::default();
    assert!(Board::from_visible(&o.cells, false).is_err());
}
#[test]
fn newly_spawned_piece_supersedes_old_locked_shape() {
    let mut lab = Lab::default();
    let (_, g) = fixture(1280, 720, (300, 90), 24, 0, true);
    let mut v = vision::Vision::default();
    v.observe(&lab.frame(1280, 720, (300, 90), 24, 0, true), g);
    lab.active = lab.board.dropped(lab.active);
    v.observe(&lab.frame(1280, 720, (300, 90), 24, 0, true), g);
    lab.action(movement::Action::HardDrop);
    let o = v.observe(&lab.frame(1280, 720, (300, 90), 24, 0, true), g);
    assert_eq!(o.active.unwrap().piece, lab.active.piece);
    assert_eq!(Board::from_visible(&o.cells, true).unwrap(), lab.board);
}
#[test]
fn active_touching_stack_is_subtracted_temporally() {
    let mut lab = Lab::default();
    lab.board.rows[0] = 0b1111111110;
    let (_, g) = fixture(1280, 720, (300, 90), 24, 0, true);
    let mut v = vision::Vision::default();
    lab.active.y = 3;
    v.observe(&Lab::default().frame(1280, 720, (300, 90), 24, 0, true), g);
    let _ = v.observe(&lab.frame(1280, 720, (300, 90), 24, 0, true), g);
    lab.active.y = 2;
    let o = v.observe(&lab.frame(1280, 720, (300, 90), 24, 0, true), g);
    assert_eq!(o.active.unwrap().piece, Piece::T);
    assert_eq!(Board::from_visible(&o.cells, true).unwrap(), lab.board);
}
#[test]
fn row_conversion_bottom_up() {
    let mut rows = [[Cell {
        kind: CellKind::Empty,
        confidence: 1.,
    }; 10]; 20];
    rows[19][2].kind = CellKind::Locked;
    rows[0][8].kind = CellKind::Locked;
    rows[3][4].kind = CellKind::Active;
    rows[18][4].kind = CellKind::Ghost;
    let b = Board::from_visible(&rows, true).unwrap();
    assert_eq!(b.rows[0], 1 << 2);
    assert_eq!(b.rows[19], 1 << 8);
    assert!(b.rows[20..].iter().all(|&r| r == 0));
    let json = b.tbp_rows();
    assert_eq!(json.len(), 40);
    assert_eq!(json[0][2], Some('G'));
    assert_eq!(json[19][8], Some('G'));
    assert_eq!(json[16][4], None);
}
#[test]
fn line_clear_animation_is_uncertain() {
    let (mut f, g) = fixture(1280, 720, (300, 90), 24, 0, true);
    for x in 0..10 {
        let (cx, cy) = g.center(x, 19);
        f.fill(
            capture::Rect {
                x: cx - 4,
                y: cy - 4,
                width: 9,
                height: 9,
            },
            [24, 25, 29, 255],
        );
    }
    let o = vision::Vision::default().observe(&f, g);
    assert!(o.confidence < 0.9 || o.cells[19].iter().any(|c| c.kind == CellKind::Uncertain));
}


/// The game may draw ghost cells as bright gray blocks with dark centers,
/// unlike the thin outlined ghosts in Local Lab. They are UI hints, not terrain.
#[test]
fn filled_gray_hollow_ghost_does_not_block_immediate_drop() {
    use capture::Rect;
    for skin in 0..=1 {
        for gray in [90u8, 170u8, 230u8] {
            let mut lab = Lab::default();
            lab.active = Location {
                piece: Piece::S,
                rotation: Rotation::North,
                x: 4,
                y: 12,
            };
            let (_, g) = fixture(1280, 720, (300, 90), 24, skin, true);
            let mut frame = lab.frame(1280, 720, (300, 90), 24, skin, true);
            // The majority of the 9-point inner samples are bright, with a
            // small dark center: a naive cell classifier sees a solid block.
            let landing = lab.board.dropped(lab.active);
            for (x, y) in landing.cells() {
                let left = g.x as i32 + x as i32 * 24;
                let top = g.y as i32 + (19 - y) as i32 * 24;
                frame.fill(
                    Rect { x: left + 2, y: top + 2, width: 20, height: 20 },
                    [gray, gray, gray, 255],
                );
                frame.fill(
                    Rect { x: left + 9, y: top + 9, width: 6, height: 6 },
                    [7, 9, 13, 255],
                );
            }
            let observation = vision::Vision::default().observe(&frame, g);
            assert_eq!(observation.active.unwrap().piece, Piece::S);
            assert_eq!(
                Board::from_visible(&observation.cells, true).unwrap(),
                lab.board,
                "skin={skin}, gray={gray}"
            );
            let count = observation.cells.iter().flatten()
                .filter(|cell| cell.kind == CellKind::Ghost).count();
            assert_eq!(count, 4, "gray ghost must be separate from locked cells");
            assert!(observation.confidence >= 0.9);
        }
    }
}

#[test]
fn actual_solid_locked_blocks_remain_occupied_next_to_ghost() {
    let mut lab = Lab::default();
    lab.board.rows[0] = (1 << 0) | (1 << 1);
    lab.active = Location {
        piece: Piece::S,
        rotation: Rotation::North,
        x: 4,
        y: 12,
    };
    let (_, g) = fixture(1280, 720, (300, 90), 24, 0, true);
    let frame = lab.frame(1280, 720, (300, 90), 24, 0, true);
    let observed = vision::Vision::default().observe(&frame, g);
    assert_eq!(Board::from_visible(&observed.cells, true).unwrap(), lab.board);
    assert_eq!(observed.cells[19][0].kind, CellKind::Locked);
    assert_eq!(observed.cells[19][1].kind, CellKind::Locked);
}

#[test]
fn hud_pixels_do_not_affect_current_piece_or_locked_board() {
    use capture::Rect;
    let (mut frame, grid) = fixture(1280, 720, (300, 90), 24, 0, true);
    let mut tracker = vision::Vision::default();
    let original = tracker.observe(&frame, grid);
    let b = Board::from_visible(&original.cells, true).unwrap();

    // Paint the side HUD aggressively; board-only vision must remain identical.
    for box_at in [
        Rect { x: 165, y: 98, width: 120, height: 380 },
        Rect { x: 560, y: 92, width: 180, height: 500 },
    ] {
        frame.fill(box_at, [210, 208, 206, 255]);
    }
    let changed = tracker.observe(&frame, grid);
    assert_eq!(changed.active.unwrap().piece, original.active.unwrap().piece);
    assert_eq!(Board::from_visible(&changed.cells, true).unwrap(), b);
    assert!(changed.next.is_empty());
    assert_eq!(changed.hold, Known::Unknown);
    assert!(PlanningSnapshot::from_observation(&changed, true).unwrap().restricted_no_hold);
}
