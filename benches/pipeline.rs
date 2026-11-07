//! Wall-clock benchmark of real procedural images; reports measured latency only.
use std::time::Instant;
fn main() {
    let (f, g) = gui::lab::fixture(1920, 1080, (1300, 100), 32, 0, true);
    let mut v = vision::Vision::default();
    let mut vision = Vec::new();
    for _ in 0..500 {
        let start = Instant::now();
        std::hint::black_box(v.observe(&f, g));
        vision.push(start.elapsed().as_secs_f64() * 1000.);
    }
    vision.sort_by(f64::total_cmp);
    let start = Instant::now();
    let found = detection::scan(&f, &detection::DetectionConfig::default());
    println!(
        "vision n=500 mean={:.3}ms p95={:.3}ms; full scan={:.3}ms candidates={}",
        vision.iter().sum::<f64>() / 500.,
        vision[474],
        start.elapsed().as_secs_f64() * 1000.,
        found.len()
    );
}
