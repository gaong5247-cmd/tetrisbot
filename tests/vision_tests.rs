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
    assert!(Snapshot::from_observation(&fresh, true).is_ok());
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
fn palette_independent_occupancy_and_hud() {
    for skin in 0..=1 {
        let (f, g) = fixture(1280, 720, (300, 90), 24, skin, true);
        let mut vision = vision::Vision::default();
        let o = vision.observe(&f, g);
        eprintln!("{o:?}");
        assert_eq!(o.active.unwrap().piece, Piece::T);
        assert_eq!(o.hold, Known::Value(Some(Piece::I)));
        assert_eq!(o.next.len(), 5);
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
