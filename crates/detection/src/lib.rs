//! Edge-run grid hypotheses, lattice evidence, shape evidence, NMS and temporal lock.
use capture::{Frame, Rect};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy)]
pub struct Grid {
    pub x: f32,
    pub y: f32,
    pub cw: f32,
    pub ch: f32,
    pub score: f32,
    pub shape_evidence: bool,
}
impl Grid {
    pub fn rect(self) -> Rect {
        Rect {
            x: self.x.round() as i32,
            y: self.y.round() as i32,
            width: (10. * self.cw).round() as u32,
            height: (20. * self.ch).round() as u32,
        }
    }
    pub fn center(self, x: usize, y: usize) -> (i32, i32) {
        (
            (self.x + (x as f32 + 0.5) * self.cw).round() as i32,
            (self.y + (y as f32 + 0.5) * self.ch).round() as i32,
        )
    }
    pub fn near(self, b: Self) -> bool {
        (self.x - b.x).abs() < self.cw * 0.35
            && (self.y - b.y).abs() < self.ch * 0.35
            && (self.cw - b.cw).abs() < 1.5
            && (self.ch - b.ch).abs() < 1.5
    }
    pub fn roi(self, screen: Rect) -> Rect {
        Rect {
            x: (self.x - 6. * self.cw) as i32,
            y: (self.y - self.ch) as i32,
            width: (22. * self.cw) as u32,
            height: (22. * self.ch) as u32,
        }
        .intersection(screen)
        .unwrap_or(screen)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DetectionConfig {
    pub stable_frames: u32,
    pub lock_score: f32,
    pub min_cell: u32,
    pub max_cell: u32,
    pub edge_threshold: u8,
    pub rescan_ms: u64,
}
impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            stable_frames: 5,
            lock_score: 0.72,
            min_cell: 6,
            max_cell: 100,
            edge_threshold: 8,
            rescan_ms: 700,
        }
    }
}
pub fn contrast(a: [u8; 3], b: [u8; 3]) -> f32 {
    ((0..3)
        .map(|i| (a[i] as f32 - b[i] as f32).powi(2))
        .sum::<f32>()
        / 3.)
        .sqrt()
}
pub fn edge(f: &Frame, x: i32, y: i32, vertical: bool) -> f32 {
    if vertical {
        contrast(f.pixel(x, y), f.pixel(x - 1, y))
    } else {
        contrast(f.pixel(x, y), f.pixel(x, y - 1))
    }
}
fn boundary(f: &Frame, g: Grid, vertical: bool, index: usize, threshold: f32) -> f32 {
    let mut hit = 0.;
    let n = if vertical { 40 } else { 20 };
    for i in 0..n {
        let (x, y) = if vertical {
            (
                g.x + index as f32 * g.cw,
                g.y + (i as f32 + 0.37) * 20. * g.ch / n as f32,
            )
        } else {
            (
                g.x + (i as f32 + 0.37) * 10. * g.cw / n as f32,
                g.y + index as f32 * g.ch,
            )
        };
        let strength = (-2..=2)
            .map(|d| {
                edge(
                    f,
                    x.round() as i32 + if vertical { d } else { 0 },
                    y.round() as i32 + if vertical { 0 } else { d },
                    vertical,
                )
            })
            .fold(0., f32::max);
        if strength > threshold {
            hit += 1.;
        }
    }
    hit / n as f32
}
fn shape_evidence(f: &Frame, g: Grid) -> bool {
    let colors: Vec<_> = (0..200)
        .map(|i| {
            let (x, y) = g.center(i % 10, i / 10);
            f.pixel(x, y)
        })
        .collect();
    let bg = std::array::from_fn(|c| {
        let mut a: Vec<_> = colors.iter().map(|p| p[c]).collect();
        a.sort_unstable();
        a[100]
    });
    let mut mask: Vec<bool> = colors.iter().map(|&p| contrast(p, bg) > 20.).collect();
    for start in 0..200 {
        if !mask[start] {
            continue;
        }
        let mut q = vec![start];
        mask[start] = false;
        let mut k = 0;
        while k < q.len() {
            let i = q[k];
            k += 1;
            for j in [
                if i % 10 > 0 { Some(i - 1) } else { None },
                if i % 10 < 9 { Some(i + 1) } else { None },
                i.checked_sub(10),
                if i < 190 { Some(i + 10) } else { None },
            ]
            .into_iter()
            .flatten()
            {
                if mask[j] {
                    mask[j] = false;
                    q.push(j)
                }
            }
        }
        if q.len() == 4 {
            let minx = q.iter().map(|i| i % 10).min().unwrap();
            let maxx = q.iter().map(|i| i % 10).max().unwrap();
            let miny = q.iter().map(|i| i / 10).min().unwrap();
            let maxy = q.iter().map(|i| i / 10).max().unwrap();
            if maxx - minx <= 3 && maxy - miny <= 3 {
                return true;
            }
        }
    }
    false
}
pub fn score(f: &Frame, mut g: Grid, cfg: &DetectionConfig) -> Grid {
    if g.rect().intersection(f.rect) != Some(g.rect()) {
        g.score = 0.;
        return g;
    }
    let v = (1..10)
        .map(|i| boundary(f, g, true, i, cfg.edge_threshold as f32))
        .sum::<f32>()
        / 9.;
    let h = (1..20)
        .map(|i| boundary(f, g, false, i, cfg.edge_threshold as f32))
        .sum::<f32>()
        / 19.;
    let border = (boundary(f, g, true, 0, cfg.edge_threshold as f32)
        + boundary(f, g, true, 10, cfg.edge_threshold as f32)
        + boundary(f, g, false, 0, cfg.edge_threshold as f32)
        + boundary(f, g, false, 20, cfg.edge_threshold as f32))
        / 4.;
    g.shape_evidence = shape_evidence(f, g);
    g.score = 0.35 * v + 0.35 * h + 0.1 * border + if g.shape_evidence { 0.2 } else { 0. };
    // A grid-free board can be proposed from its extent plus repeated block-cell rims.
    // The vision pipeline requires real piece motion before accepting this weaker case.
    if g.score < 0.72 && g.shape_evidence && border > 0.85 {
        let colors: Vec<_> = (0..200)
            .map(|i| {
                let (x, y) = g.center(i % 10, i / 10);
                f.pixel(x, y)
            })
            .collect();
        let bg = std::array::from_fn(|c| {
            let mut a: Vec<_> = colors.iter().map(|p| p[c]).collect();
            a.sort_unstable();
            a[100]
        });
        let mut hit = 0.;
        let mut total = 0.;
        for (i, &p) in colors.iter().enumerate() {
            if contrast(p, bg) < 24. {
                continue;
            }
            for (dx, dy) in [(-0.49, 0.), (0.49, 0.), (0., -0.49), (0., 0.49)] {
                let cx = g.x + (i % 10) as f32 * g.cw + g.cw * 0.5;
                let cy = g.y + (i / 10) as f32 * g.ch + g.ch * 0.5;
                let rim = f.pixel(
                    (cx + dx * g.cw).round() as i32,
                    (cy + dy * g.ch).round() as i32,
                );
                let inner = f.pixel(
                    (cx + dx * g.cw * 0.45).round() as i32,
                    (cy + dy * g.ch * 0.45).round() as i32,
                );
                if contrast(rim, p) > 20. && contrast(inner, p) < 12. {
                    hit += 1.;
                }
                total += 1.;
            }
        }
        if total >= 16. && hit / total > 0.65 {
            g.score = 0.76;
        }
    }
    g
}
#[derive(Clone, Copy)]
struct Run {
    x: i32,
    y: i32,
    len: i32,
}
pub fn scan(f: &Frame, cfg: &DetectionConfig) -> Vec<Grid> {
    let r = f.rect;
    let mut runs = Vec::new();
    let min_len = (cfg.min_cell * 20) as i32;
    // Vertical runs give extent hypotheses even when the board is off-center.
    for x in r.x + 1..r.x + r.width as i32 - 1 {
        let mut start = r.y + 1;
        let mut gaps = 0;
        for y in r.y + 1..r.y + r.height as i32 {
            if edge(f, x, y, true) > cfg.edge_threshold as f32 {
                gaps = 0;
            } else {
                gaps += 1;
            }
            if gaps > 2 || y == r.y + r.height as i32 - 1 {
                let end = y - gaps + 1;
                let len = end - start;
                if len >= min_len && len <= (cfg.max_cell * 20 + 5) as i32 {
                    runs.push(Run { x, y: start, len });
                }
                start = y + 1;
            }
        }
    }
    let mut out = Vec::new();
    for (i, a) in runs.iter().enumerate() {
        for b in runs.iter().skip(i + 1) {
            let width = b.x - a.x;
            if width < (cfg.min_cell * 10) as i32 || width > (cfg.max_cell * 10) as i32 {
                continue;
            }
            if (a.y - b.y).abs() > 3 || (a.len - b.len).abs() > 4 {
                continue;
            }
            let mut cw = width as f32 / 10.;
            let mut ch = a.len as f32 / 20.;
            if !(0.6..1.7).contains(&(cw / ch)) {
                continue;
            }
            // Single-pixel exterior strokes belong to the extent, not to every cell pitch.
            if (cw - cw.round()).abs() < 0.12 {
                cw = cw.round();
            }
            if (ch - cw).abs() < 0.16 {
                ch = cw;
            }
            let g = score(
                f,
                Grid {
                    x: a.x as f32,
                    y: a.y as f32,
                    cw,
                    ch,
                    score: 0.,
                    shape_evidence: false,
                },
                cfg,
            );
            if g.score > 0.48 {
                out.push(g);
            }
        }
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut keep: Vec<Grid> = vec![];
    for g in out {
        if keep.iter().all(|&k| iou(g.rect(), k.rect()) < 0.45) {
            keep.push(g);
            if keep.len() == 12 {
                break;
            }
        }
    }
    keep
}
pub fn iou(a: Rect, b: Rect) -> f32 {
    let intersection = a
        .intersection(b)
        .map(|r| r.width as f32 * r.height as f32)
        .unwrap_or(0.);
    intersection
        / (a.width as f32 * a.height as f32 + b.width as f32 * b.height as f32 - intersection)
            .max(1.)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Searching,
    Candidate,
    Validating,
    Locked,
    Lost,
    Recovering,
}
impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Searching => "SEARCHING",
                Self::Candidate => "CANDIDATE",
                Self::Validating => "VALIDATING",
                Self::Locked => "LOCKED",
                Self::Lost => "LOST",
                Self::Recovering => "RECOVERING",
            }
        )
    }
}
pub struct Tracker {
    pub state: State,
    pub grid: Option<Grid>,
    pub stable: u32,
    pub generation: u64,
    cfg: DetectionConfig,
}
impl Tracker {
    pub fn new(cfg: DetectionConfig) -> Self {
        Self {
            state: State::Searching,
            grid: None,
            stable: 0,
            generation: 0,
            cfg,
        }
    }
    pub fn reset(&mut self) {
        self.state = State::Searching;
        self.grid = None;
        self.stable = 0;
        self.generation += 1;
    }
    /// Revalidate the existing grid using the same temporal rules as acquisition.
    /// A piece touching the stack no longer has an isolated four-cell component;
    /// this must not discard an otherwise stable, strongly observed grid.
    pub fn observe_frame(&mut self, frame: &Frame) -> bool {
        let Some(grid) = self.grid else { return false };
        let checked = score(frame, grid, &self.cfg);
        self.update(&[checked]);
        matches!(
            self.state,
            State::Candidate | State::Validating | State::Locked
        )
    }
    pub fn update(&mut self, candidates: &[Grid]) {
        let best = candidates
            .iter()
            .find(|g| {
                g.score >= self.cfg.lock_score
                    && (g.shape_evidence
                        || (self.state == State::Locked && self.grid.is_some_and(|p| p.near(**g))))
            })
            .copied();
        match best {
            Some(g) => {
                let same = self.grid.is_some_and(|p| p.near(g));
                if same {
                    self.stable += 1;
                } else {
                    self.stable = 1;
                    self.generation += 1;
                }
                self.grid = Some(g);
                self.state = if self.stable >= self.cfg.stable_frames {
                    State::Locked
                } else if self.stable == 1 {
                    State::Candidate
                } else {
                    State::Validating
                };
            }
            None => {
                self.stable = 0;
                self.state = match self.state {
                    State::Locked => {
                        self.generation += 1;
                        State::Lost
                    }
                    State::Lost => State::Recovering,
                    State::Recovering => State::Searching,
                    _ => State::Searching,
                };
            }
        }
    }
    pub fn may_input(&self) -> bool {
        self.state == State::Locked
    }
}
