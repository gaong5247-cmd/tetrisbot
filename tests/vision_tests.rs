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
fn uniformly_filled_neutral_gray_ghost_is_not_locked_terrain() {
    use capture::{Frame, Rect};
    use detection::Grid;
    let grid=Grid{x:60.,y:35.,cw:24.,ch:24.,score:1.,shape_evidence:true};
    let mut f=Frame::new(Rect{x:0,y:0,width:360,height:550},[7,9,13,255]);
    let mut paint=|x:i8,y:i8,col:[u8;4]| {
        f.fill(Rect{
            x:grid.x as i32+x as i32*24+1,
            y:grid.y as i32+(19-y as i32)*24+1,
            width:22,height:22
        },col);
    };
    let active=Location{piece:Piece::T,rotation:Rotation::North,x:5,y:12};
    for (x,y) in Board::default().dropped(active).cells() {
        paint(x,y,[160,160,160,255]);
    }
    for (x,y) in active.cells() {paint(x,y,[183,58,151,255]);}
    paint(0,0,[110,110,110,255]); // a genuinely locked block
    let o=vision::Vision::default().observe(&f,grid);
    assert!(o.active.unwrap().same_cells(active));
    assert_eq!(o.cells.iter().flatten().filter(|c|c.kind==CellKind::Ghost).count(),4);
    let board=Board::from_visible(&o.cells,true).unwrap();
    assert_eq!(board.rows[0],1);
    assert!(board.rows[1..].iter().all(|&r|r==0));
}

#[test]
fn faint_neutral_gray_ghost_does_not_create_uncertain_cells() {
    use capture::{Frame, Rect};
    use detection::Grid;
    let grid=Grid{x:60.,y:35.,cw:24.,ch:24.,score:1.,shape_evidence:true};
    let mut f=Frame::new(Rect{x:0,y:0,width:360,height:550},[7,9,13,255]);
    let active=Location{piece:Piece::T,rotation:Rotation::North,x:5,y:12};
    for (x,y) in Board::default().dropped(active).cells() {
        f.fill(Rect{x:60+x as i32*24+1,y:35+(19-y as i32)*24+1,
            width:22,height:22},[28,28,28,255]);
    }
    for (x,y) in active.cells() {
        f.fill(Rect{x:60+x as i32*24+1,y:35+(19-y as i32)*24+1,
            width:22,height:22},[183,58,151,255]);
    }
    let o=vision::Vision::default().observe(&f,grid);
    assert!(o.active.unwrap().same_cells(active));
    assert_eq!(o.cells.iter().flatten().filter(|c|c.kind==CellKind::Ghost).count(),4);
    assert!(o.confidence>=0.9);
    assert_eq!(Board::from_visible(&o.cells,true).unwrap(),Board::default());
}

#[test]
fn previously_locked_gray_cells_are_not_turned_into_ghosts() {
    use capture::{Frame, Rect};
    use detection::Grid;
    let grid=Grid{x:60.,y:35.,cw:24.,ch:24.,score:1.,shape_evidence:true};
    let mut f=Frame::new(Rect{x:0,y:0,width:360,height:550},[7,9,13,255]);
    let active=Location{piece:Piece::T,rotation:Rotation::North,x:5,y:12};
    let locked=Board::default().dropped(active);
    let mut expected=Board::default();
    let mut prior=Observation {
        cells:[[Cell{kind:CellKind::Empty,confidence:1.};10];20],
        active:Some(active),confidence:0.99,..Default::default()
    };
    for (x,y) in locked.cells() {
        expected.rows[y as usize]|=1<<x;
        prior.cells[(19-y) as usize][x as usize].kind=CellKind::Locked;
        f.fill(Rect{x:60+x as i32*24+1,y:35+(19-y as i32)*24+1,
            width:22,height:22},[160,160,160,255]);
    }
    for (x,y) in active.cells() {
        f.fill(Rect{x:60+x as i32*24+1,y:35+(19-y as i32)*24+1,
            width:22,height:22},[183,58,151,255]);
    }
    let mut v=vision::Vision::default();
    v.seed(&prior);
    let o=v.observe(&f,grid);
    assert_eq!(Board::from_visible(&o.cells,true).unwrap(),expected);
}

#[test]
fn transient_blank_frame_does_not_poison_previous_active_piece() {
    let mut lab=Lab::default();
    let (_,g)=fixture(1280,720,(300,90),24,0,true);
    let mut v=vision::Vision::default();
    let initial=v.observe(&lab.frame(1280,720,(300,90),24,0,true),g);
    assert!(initial.confidence>=0.9);
    let blank=capture::Frame::new(g.rect(),[7,9,13,255]);
    assert!(v.observe(&blank,g).confidence<0.9);
    lab.active.x+=1;
    let recovered=v.observe(&lab.frame(1280,720,(300,90),24,0,true),g);
    assert!(recovered.active.unwrap().same_cells(lab.active));
    assert!(v.motion_seen,"motion evidence should survive a one-frame dropout");
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


/// Production per-action verification now captures only the actual 10x20
/// grid instead of the surrounding HUD; pixel coordinates remain absolute.
#[test]
fn board_only_cropped_frame_preserves_current_piece_and_stack() {
    let (full, grid) = fixture(1280, 720, (300, 90), 24, 0, true);
    let rect = grid.rect();
    let mut crop = capture::Frame::new(rect, [0, 0, 0, 255]);
    for dy in 0..rect.height as usize {
        let src_y = rect.y as usize + dy;
        let src_x = rect.x as usize;
        let src_start = (src_y * full.rect.width as usize + src_x) * 4;
        let dst_start = dy * rect.width as usize * 4;
        let len = rect.width as usize * 4;
        crop.rgba[dst_start..dst_start + len]
            .copy_from_slice(&full.rgba[src_start..src_start + len]);
    }
    let whole = vision::Vision::default().observe(&full, grid);
    let cropped = vision::Vision::default().observe(&crop, grid);
    assert_eq!(whole.active.unwrap().piece, cropped.active.unwrap().piece);
    assert_eq!(whole.active.unwrap().cells(), cropped.active.unwrap().cells());
    assert_eq!(whole.confidence, cropped.confidence);
    assert_eq!(Board::from_visible(&whole.cells, true).unwrap(),
               Board::from_visible(&cropped.cells, true).unwrap());
    assert_eq!(rect.width, 240);
    assert_eq!(rect.height, 480);
}


/// Recreates a translucent solid-purple ghost like the reported game:
/// active T RGB(175,41,138), ghost RGB(87,20,69), cyan locked floor.
/// The two T silhouettes touch and look like one eight-cell binary blob.
#[test]
fn touching_dim_translucent_ghost_is_not_treated_as_locked_or_active() {
    use capture::{Frame,Rect};
    use detection::Grid;
    let grid=Grid {x:80.,y:40.,cw:24.,ch:24.,score:1.,shape_evidence:true};
    let mut frame=Frame::new(Rect{x:0,y:0,width:400,height:570},[0,0,0,255]);
    let mut paint=|x:i8,y:i8,color:[u8;4]| {
        frame.fill(Rect{
            x:grid.x as i32+x as i32*24,
            y:grid.y as i32+(19-y as i32)*24,
            width:24,height:24
        },color);
    };
    let bright=Location {piece:Piece::T,rotation:Rotation::North,x:1,y:2};
    let ghost=Location {y:0,..bright};
    for (x,y) in bright.cells() {paint(x,y,[175,41,138,255]);}
    for (x,y) in ghost.cells() {paint(x,y,[87,20,69,255]);}
    for x in 3..7 {paint(x,0,[15,155,215,255]);}
    let observation=vision::Vision::default().observe(&frame,grid);
    assert_eq!(observation.active.unwrap().piece,Piece::T);
    assert!(observation.active.unwrap().same_cells(bright));
    let ghost_count=observation.cells.iter().flatten()
        .filter(|c|c.kind==CellKind::Ghost).count();
    assert_eq!(ghost_count,4,"dimmed ghost T must be kept out of locked terrain");
    let board=Board::from_visible(&observation.cells,true).unwrap();
    assert_eq!(board.rows[0],(3..7).fold(0,|v,x|v|(1<<x)));
    assert!(board.rows[1..].iter().all(|&row|row==0));
    assert!(observation.confidence>=0.9);
}

#[test]
fn rising_multiplayer_garbage_does_not_change_playfield_background() {
    use capture::{Frame,Rect};
    use detection::Grid;
    let grid=Grid {x:80.,y:40.,cw:24.,ch:24.,score:1.,shape_evidence:true};
    let frame_rect=Rect{x:0,y:0,width:400,height:570};
    let mut frame=Frame::new(frame_rect,[0,0,0,255]);
    let mut stable=vision::Vision::default();
    let _=stable.observe(&frame,grid); // acquire the empty black background

    // 17 of 20 rows are filled with rising garbage, leaving an open column.
    // The old median-of-all-200-cells selected gray garbage as the background.
    let garbage=[115,115,115,255];
    for y in 0..17 {
        for x in 0..10 {
            if x==4 {continue}
            frame.fill(Rect{
                x:grid.x as i32+x*24,
                y:grid.y as i32+(19-y)*24,
                width:24,height:24
            },garbage);
        }
    }
    let active=Location {piece:Piece::T,rotation:Rotation::North,x:4,y:18};
    for (x,y) in active.cells() {
        frame.fill(Rect{
            x:grid.x as i32+x as i32*24,
            y:grid.y as i32+(19-y as i32)*24,
            width:24,height:24
        },[194,55,122,255]);
    }
    let observation=stable.observe(&frame,grid);
    let board=Board::from_visible(&observation.cells,true).unwrap();
    for y in 0..17 {assert_eq!(board.rows[y],1023^(1<<4),"garbage row {y}");}
    assert!(board.rows[17..].iter().all(|&row|row==0));
    assert_eq!(observation.active.unwrap().piece,Piece::T);
    assert!(observation.confidence>=0.9);

    // Also handle first capture during a high-stack attack without a history.
    let first=vision::Vision::default().observe(&frame,grid);
    let first_board=Board::from_visible(&first.cells,true).unwrap();
    assert_eq!(first_board,board);
}


#[test]
fn almost_topped_out_19_garbage_rows_keep_real_holes_and_active_i() {
    use capture::{Frame,Rect};
    use detection::Grid;
    let grid=Grid{x:70.,y:36.,cw:24.,ch:24.,score:1.,shape_evidence:true};
    let mut frame=Frame::new(Rect{x:0,y:0,width:380,height:570},[0,0,0,255]);
    for y in 0..19i32 {
        for x in 0..10i32 {
            if x==4 {continue}
            frame.fill(Rect{
                x:grid.x as i32+x*24,
                y:grid.y as i32+(19-y)*24,
                width:24,height:24
            },[120,120,120,255]);
        }
    }
    // The I block is touching the top of the 19-row garbage stack; the
    // previous binary blob classifier merged active+garbage and lost it.
    let active=Location{piece:Piece::I,rotation:Rotation::North,x:4,y:19};
    for (x,y) in active.cells() {
        frame.fill(Rect {
            x:grid.x as i32+x as i32*24,
            y:grid.y as i32+(19-y as i32)*24,
            width:24,height:24
        },[15,155,215,255]);
    }
    let o=vision::Vision::default().observe(&frame,grid);
    assert_eq!(o.active.unwrap().piece,Piece::I);
    assert!(o.active.unwrap().same_cells(active));
    let b=Board::from_visible(&o.cells,true).unwrap();
    assert!(b.rows[19..].iter().all(|&row|row==0));
    for y in 0..19 {assert_eq!(b.rows[y],1023^(1<<4));}
    assert!(o.confidence>=0.9);
}


#[test]
fn stable_board_gate_requires_distinct_observations_and_resets_on_change() {
    let mut gate=vision::StableBoardGate::default();
    let empty=Cell { kind:CellKind::Empty,confidence:1.0 };
    let mut o=Observation {
        cells:[[empty;10];20],active:Some(Location::spawn(Piece::T)),
        confidence:0.99,..Default::default()
    };
    assert!(!gate.observe(&o,true));
    assert_eq!(gate.count(),1);
    assert!(gate.observe(&o,true));
    assert!(gate.confirmed());
    o.cells[19][4].kind=CellKind::Locked;
    assert!(!gate.observe(&o,true));
    assert_eq!(gate.count(),1);
    o.cells[19][4].kind=CellKind::Uncertain;
    assert!(!gate.observe(&o,true));
    assert_eq!(gate.count(),0);
    o.cells[19][4]=empty;
    o.active=Some(Location::spawn(Piece::I));
    assert!(!gate.observe(&o,true));
    assert!(gate.observe(&o,true));
    gate.reset();
    assert!(!gate.confirmed());
}

#[test]
fn initial_dense_garbage_background_uses_real_hole_not_gray_majority() {
    use capture::{Frame,Rect};
    use detection::Grid;
    let grid=Grid{x:50.,y:35.,cw:24.,ch:24.,score:1.,shape_evidence:true};
    let mut f=Frame::new(Rect{x:0,y:0,width:340,height:560},[7,9,13,255]);
    // All 20 rows nearly full: the top-row and full-board medians are
    // garbage gray, but 20 real black hole cells remain in column 4.
    for y in 0..20i32 {
        for x in 0..10i32 {
            if x==4 {continue}
            f.fill(Rect {
                x:grid.x as i32+x*24+2,
                y:grid.y as i32+y*24+2,
                width:20,height:20
            },[117,117,117,255]);
        }
    }
    let o=vision::Vision::default().observe(&f,grid);
    for y in 0..20 {
        assert_eq!(o.cells[y][4].kind,CellKind::Empty,"real hole row {y}");
        for x in 0..10 {
            if x!=4 {assert_eq!(o.cells[y][x].kind,CellKind::Locked,
                "filled garbage row {y} col {x}");}
        }
    }
    assert_eq!(o.active,None,"a packed, top-out-like board has no invented active piece");
}

#[test]
fn faint_solid_same_hue_ghost_is_separated_from_real_locked_terrain() {
    use capture::{Frame,Rect};
    use detection::Grid;
    let grid=Grid{x:60.,y:35.,cw:24.,ch:24.,score:1.,shape_evidence:true};
    let mut f=Frame::new(Rect{x:0,y:0,width:360,height:550},[7,9,13,255]);
    let mut paint=|x:i8,y:i8,col:[u8;4]| {
        f.fill(Rect{
            x:grid.x as i32+x as i32*24+1,
            y:grid.y as i32+(19-y as i32)*24+1,
            width:22,height:22
        },col);
    };
    let active=Location {piece:Piece::T,rotation:Rotation::North,x:5,y:12};
    let landing=Board::default().dropped(active);
    for (x,y) in landing.cells() {
        paint(x,y,[27,12,23,255]); // too faint for solid-cell threshold
    }
    for (x,y) in active.cells() {
        paint(x,y,[183,58,151,255]);
    }
    // A genuinely locked blue block outside the landing survives.
    paint(1,0,[36,96,182,255]);
    let o=vision::Vision::default().observe(&f,grid);
    assert_eq!(o.active.unwrap().piece,Piece::T);
    assert_eq!(o.active.unwrap().cells(),active.cells());
    let ghost=o.cells.iter().flatten().filter(|c|c.kind==CellKind::Ghost).count();
    assert_eq!(ghost,4);
    let board=Board::from_visible(&o.cells,true).unwrap();
    assert_eq!(board.rows[0],1<<1);
    assert!(board.rows[1..].iter().all(|&r|r==0));
}

#[test]
fn majority_cell_votes_ignore_two_highlight_pixels() {
    use capture::{Frame,Rect};
    use detection::Grid;
    let grid=Grid{x:60.,y:40.,cw:24.,ch:24.,score:1.,shape_evidence:true};
    let mut f=Frame::new(Rect{x:0,y:0,width:360,height:550},[7,9,13,255]);
    let cx=(grid.x+4.5*grid.cw).round() as i32;
    let cy=(grid.y+19.5*grid.ch).round() as i32;
    f.fill(Rect{x:cx-10,y:cy-10,width:20,height:20},[120,141,176,255]);
    // Exactly two of the nine interior sample positions are overpainted.
    for (dx,dy) in [(-0.18,-0.18),(0.18,0.18)] {
        f.fill(Rect{
            x:(cx as f32+dx*24.).round() as i32,
            y:(cy as f32+dy*24.).round() as i32,
            width:1,height:1
        },[7,9,13,255]);
    }
    let o=vision::Vision::default().observe(&f,grid);
    assert_eq!(o.cells[19][4].kind,CellKind::Locked);
}

#[test]
fn jstris_translucent_ghost_touching_similar_locked_terrain_is_not_part_of_stack() {
    use capture::{Frame,Rect};
    use detection::Grid;
    let g=Grid{x:60.,y:35.,cw:24.,ch:24.,score:1.,shape_evidence:true};
    let background=[7u8,9,13];
    let piece=[220u8,60,148];
    let active=Location{piece:Piece::T,rotation:Rotation::North,x:5,y:12};
    let landing=Board::default().dropped(active);
    for alpha in [0.30f32, 0.50, 0.72, 0.88] {
        let mut f=Frame::new(Rect{x:0,y:0,width:360,height:550},
            [background[0],background[1],background[2],255]);
        let ghost: [u8;3]=std::array::from_fn(|i|
            (background[i] as f32*(1.-alpha)+piece[i] as f32*alpha).round() as u8);
        let mut paint=|x:i8,y:i8,color:[u8;3]| {
            f.fill(Rect{x:60+x as i32*24+1,y:35+(19-y as i32)*24+1,
                width:22,height:22},[color[0],color[1],color[2],255]);
        };
        for (x,y) in landing.cells() {paint(x,y,ghost);}
        // Same-color real terrain next to the ghost makes a 5-cell component.
        paint(7,0,ghost);
        for (x,y) in active.cells() {paint(x,y,piece);}
        let o=vision::Vision::default().observe(&f,g);
        assert!(o.active.is_some_and(|p|p.same_cells(active)),
            "active silhouette lost at opacity {alpha}");
        assert_eq!(o.cells.iter().flatten()
            .filter(|c|c.kind==CellKind::Ghost).count(),4,
            "connected translucent ghost missed at opacity {alpha}");
        let board=Board::from_visible(&o.cells,true).unwrap();
        assert_eq!(board.rows[0],1<<7,"real neighbor erased at opacity {alpha}");
        assert!(board.rows[1..].iter().all(|&r|r==0));
    }
}

#[test]
fn jstris_darker_previous_locked_landing_is_not_erased_as_ghost() {
    use capture::{Frame,Rect};
    use detection::Grid;
    let g=Grid{x:60.,y:35.,cw:24.,ch:24.,score:1.,shape_evidence:true};
    let mut f=Frame::new(Rect{x:0,y:0,width:360,height:550},[7,9,13,255]);
    let active=Location{piece:Piece::T,rotation:Rotation::North,x:5,y:12};
    let landing=Board::default().dropped(active);
    let mut previous=Observation {
        cells:[[Cell{kind:CellKind::Empty,confidence:0.97};10];20],
        active:Some(active),confidence:0.97,..Default::default()
    };
    for (x,y) in landing.cells() {
        let row=(19-y) as usize;
        previous.cells[row][x as usize].kind=CellKind::Locked;
        f.fill(Rect{x:60+x as i32*24+1,y:35+(19-y as i32)*24+1,
            width:22,height:22},[89,28,67,255]);
    }
    for (x,y) in active.cells() {
        f.fill(Rect{x:60+x as i32*24+1,y:35+(19-y as i32)*24+1,
            width:22,height:22},[220,60,148,255]);
    }
    let mut vision=vision::Vision::default();
    vision.seed(&previous);
    let observed=vision.observe(&f,g);
    assert_eq!(observed.cells.iter().flatten()
        .filter(|c|c.kind==CellKind::Ghost).count(),0);
    let board=Board::from_visible(&observed.cells,true).unwrap();
    for (x,y) in landing.cells() {
        assert_ne!(board.rows[y as usize] & (1<<x),0);
    }
}

#[test]
fn stable_board_gate_requires_same_active_position_not_just_piece_name() {
    let mut gate=vision::StableBoardGate::default();
    let mut o=Observation {
        cells:[[Cell{kind:CellKind::Empty,confidence:1.};10];20],
        active:Some(Location::spawn(Piece::T)),
        confidence:0.99,..Default::default()
    };
    assert!(!gate.observe(&o,true));
    let mut moved=o.active.unwrap();
    moved.x+=1;
    o.active=Some(moved);
    assert!(!gate.observe(&o,true));
    assert_eq!(gate.count(),1);
    assert!(gate.observe(&o,true));
}
