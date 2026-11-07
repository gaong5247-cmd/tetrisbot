use capture::{Frame, Rect};
use detection::*;
use gui::lab::fixture;
fn assert_scene(w: u32, h: u32, x: i32, y: i32, c: u32, lines: bool) {
    let (f, g) = fixture(w, h, (x, y), c, 0, lines);
    let candidates = scan(&f, &DetectionConfig::default());
    eprintln!("{w}x{h} ({x},{y}) cell={c}: {candidates:?}");
    assert!(
        candidates.iter().any(|p| iou(p.rect(), g.rect()) > 0.95),
        "expected grid not discovered"
    );
}
#[test]
fn resolutions_positions_and_small_window() {
    for (w, h, x, y, c) in [
        (1280, 720, 140, 90, 24),
        (1920, 1080, 1480, 90, 32),
        (2560, 1440, 160, 100, 48),
        (1280, 720, 960, 100, 12),
        (1920, 1080, 700, 30, 48),
    ] {
        assert_scene(w, h, x, y, c, true)
    }
}
#[test]
fn no_board_and_rectangle_decoys() {
    let mut f = Frame::new(
        Rect {
            x: 0,
            y: 0,
            width: 1280,
            height: 720,
        },
        [12, 12, 12, 255],
    );
    f.fill(
        Rect {
            x: 100,
            y: 100,
            width: 200,
            height: 400,
        },
        [60, 60, 60, 255],
    );
    assert!(
        scan(&f, &DetectionConfig::default())
            .iter()
            .all(|g| g.score < 0.72 || !g.shape_evidence)
    );
}
#[test]
fn multiple_boards_nms() {
    let (mut f, a) = fixture(1920, 1080, (160, 100), 28, 0, true);
    let (b, g) = fixture(1920, 1080, (1300, 100), 28, 1, true);
    for y in 100..661 {
        for x in 1300..1581 {
            f.set(
                x,
                y,
                [b.pixel(x, y)[0], b.pixel(x, y)[1], b.pixel(x, y)[2], 255],
            );
        }
    }
    let found = scan(&f, &DetectionConfig::default());
    assert!(found.iter().any(|p| iou(p.rect(), a.rect()) > 0.95));
    assert!(found.iter().any(|p| iou(p.rect(), g.rect()) > 0.95));
    for (i, a) in found.iter().enumerate() {
        for b in found.iter().skip(i + 1) {
            assert!(iou(a.rect(), b.rect()) < 0.45)
        }
    }
}
#[test]
fn faint_grid() {
    let (mut f, g) = fixture(1280, 720, (200, 100), 24, 0, true);
    for px in f.rgba.chunks_exact_mut(4) {
        if px[..3] == [47, 51, 59] {
            px[..3].copy_from_slice(&[25, 27, 31]);
        }
    }
    let found = scan(&f, &DetectionConfig::default());
    assert!(found.iter().any(|p| iou(p.rect(), g.rect()) > 0.95));
}
#[test]
fn borderless_visible_lattice() {
    let (mut f, g) = fixture(1280, 720, (300, 100), 24, 0, true);
    let r = g.rect();
    for x in r.x..=r.x + r.width as i32 {
        f.set(x, r.y, [16, 19, 25, 255]);
        f.set(x, r.y + r.height as i32, [16, 19, 25, 255]);
    }
    let found = scan(&f, &DetectionConfig::default());
    assert!(found.iter().any(|p| iou(p.rect(), g.rect()) > 0.9));
}
#[test]
fn no_grid_sparse_board() {
    assert_scene(1280, 720, 300, 100, 24, false);
}
