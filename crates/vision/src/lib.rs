//! Palette-free cell features, exact geometry and conservative temporal classification.
use capture::Frame;
use detection::{Grid, contrast};
use game_state::*;
#[derive(Default)]
pub struct Vision {
    previous: Option<Observation>,
    previous_raw: Option<[[bool; 10]; 20]>,
    pub motion_seen: bool,
}
fn median_color(v: &[[u8; 3]]) -> [u8; 3] {
    std::array::from_fn(|c| {
        let mut a: Vec<_> = v.iter().map(|p| p[c]).collect();
        a.sort_unstable();
        a[a.len() / 2]
    })
}
fn sample(f: &Frame, cx: f32, cy: f32, cw: f32, ch: f32) -> ([u8; 3], f32) {
    let mut v = Vec::with_capacity(9);
    for dy in [-0.18, 0., 0.18] {
        for dx in [-0.18, 0., 0.18] {
            v.push(f.pixel((cx + dx * cw).round() as i32, (cy + dy * ch).round() as i32));
        }
    }
    let median = median_color(&v);
    let spread = v.iter().map(|&p| contrast(p, median)).sum::<f32>() / 9.;
    (median, spread)
}
/// Hollow ghost silhouettes often use a nearly background-colored center with
/// brighter rims. Detect *geometry*, not an RGB/gray palette or PNG template.
/// This is a candidate only: we later require exact agreement with the active
/// piece's computed landing before classifying it as Ghost.
fn hollow_ghost_candidate(f: &Frame, cx: f32, cy: f32, cw: f32, ch: f32, bg: [u8; 3]) -> bool {
    let center = contrast(f.pixel(cx.round() as i32, cy.round() as i32), bg);
    let rim = [(-0.30, 0.0), (0.30, 0.0), (0.0, -0.30), (0.0, 0.30)];
    rim.iter()
        .filter(|&&(dx, dy)| {
            let value = contrast(
                f.pixel(
                    (cx + dx * cw).round() as i32,
                    (cy + dy * ch).round() as i32,
                ),
                bg,
            );
            value > 22.0 && value > center + 18.0
        })
        .count()
        >= 3
}

impl Vision {
    /// Carry trusted temporal evidence across the vision/engine worker boundary.
    /// This is an observed state, not an inferred spawn position.
    pub fn seed(&mut self, observed: &Observation) {
        self.previous_raw = Some(std::array::from_fn(|y| {
            std::array::from_fn(|x| {
                matches!(
                    observed.cells[y][x].kind,
                    CellKind::Locked | CellKind::Active
                )
            })
        }));
        self.previous = Some(observed.clone());
    }
    pub fn reset(&mut self) {
        *self = Self::default()
    }
    pub fn observe(&mut self, f: &Frame, g: Grid) -> Observation {
        let features: Vec<_> = (0..200)
            .map(|i| {
                sample(
                    f,
                    g.x + (i % 10) as f32 * g.cw + g.cw * 0.5,
                    g.y + (i / 10) as f32 * g.ch + g.ch * 0.5,
                    g.cw,
                    g.ch,
                )
            })
            .collect();
        let centers: Vec<_> = features.iter().map(|p| p.0).collect();
        let bg = median_color(&centers);
        let mut o = Observation::default();
        let mut raw = [[false; 10]; 20];
        let mut outlines = [[false; 10]; 20];
        let mut hollow = [[false; 10]; 20];
        for y in 0..20 {
            for x in 0..10 {
                let (color, spread) = features[y * 10 + x];
                let d = contrast(color, bg);
                let mut outline = 0;
                for (dx, dy) in [(0.12, 0.5), (0.88, 0.5), (0.5, 0.12), (0.5, 0.88)] {
                    if contrast(
                        f.pixel(
                            (g.x + (x as f32 + dx) * g.cw) as i32,
                            (g.y + (y as f32 + dy) * g.ch) as i32,
                        ),
                        bg,
                    ) > 18.
                    {
                        outline += 1;
                    }
                }
                let cx = g.x + (x as f32 + 0.5) * g.cw;
                let cy = g.y + (y as f32 + 0.5) * g.ch;
                hollow[y][x] = hollow_ghost_candidate(f, cx, cy, g.cw, g.ch, bg);
                // Ghost pixels must not become locked terrain, nor be mistaken
                // for an active tetromino in connected-component matching.
                raw[y][x] = d > 24. && spread < 25. && !hollow[y][x];
                outlines[y][x] = d < 12. && outline >= 3;
                let kind = if hollow[y][x] {
                    CellKind::Uncertain
                } else if raw[y][x] {
                    CellKind::Locked
                } else if d < 10. && spread < 4. {
                    CellKind::Empty
                } else {
                    CellKind::Uncertain
                };
                o.cells[y][x] = Cell {
                    kind,
                    confidence: if kind == CellKind::Uncertain {
                        0.25
                    } else {
                        0.97
                    },
                };
            }
        }
        let components = components(&raw);
        let mut moving = Vec::new();
        if let Some(prev) = self.previous_raw {
            for y in 0..20 {
                for x in 0..10 {
                    if raw[y][x] && !prev[y][x] {
                        moving.push((x as i8, 19 - y as i8));
                    }
                }
            }
        }
        let mut hypotheses = Vec::new();
        for c in &components {
            if c.len() == 4 {
                let shapes = match_shape(c);
                if !shapes.is_empty() {
                    hypotheses.push((c.clone(), shapes[0]));
                }
            }
        }
        // A newly spawned upper silhouette supersedes the old piece that just became stack.
        // For contact with the stack, subtract the previously observed locked occupancy.
        let touching = self
            .previous
            .as_ref()
            .and_then(|prev| prev.active.map(|p| (prev, p)))
            .and_then(|(prev, p)| {
                let delta: Vec<_> = (0..200)
                    .filter_map(|i| {
                        let x = i % 10;
                        let y = i / 10;
                        (raw[y][x] && prev.cells[y][x].kind != CellKind::Locked)
                            .then_some((x as i8, 19 - y as i8))
                    })
                    .collect();
                match_shape(&delta)
                    .into_iter()
                    .find(|n| {
                        n.piece == p.piece && (n.x - p.x).abs() <= 3 && (n.y - p.y).abs() <= 3
                    })
                    .map(|n| (delta, n))
            });
        let chosen = hypotheses
            .iter()
            .filter(|(c, p)| p.y >= 12 && c.iter().any(|&(x, y)| moving.contains(&(x, y))))
            .max_by_key(|(_, p)| p.y)
            .cloned()
            .or(touching)
            .or_else(|| {
                self.previous
                    .as_ref()
                    .and_then(|p| p.active)
                    .and_then(|prev| {
                        hypotheses
                            .iter()
                            .filter(|(_, p)| {
                                p.piece == prev.piece
                                    && (p.x - prev.x).abs() + (p.y - prev.y).abs() <= 4
                            })
                            .min_by_key(|(_, p)| (p.x - prev.x).abs() + (p.y - prev.y).abs())
                            .cloned()
                    })
            })
            .or_else(|| {
                hypotheses
                    .iter()
                    .filter(|(c, _)| c.iter().any(|&(x, y)| moving.contains(&(x, y))))
                    .max_by_key(|(_, p)| p.y)
                    .cloned()
            })
            .or_else(|| {
                let upper: Vec<_> = hypotheses
                    .iter()
                    .filter(|(_, p)| p.y >= 12)
                    .cloned()
                    .collect();
                if upper.len() == 1 {
                    Some(upper[0].clone())
                } else {
                    None
                }
            });
        if let Some((cells, loc)) = chosen {
            o.active = Some(loc);
            if self
                .previous
                .as_ref()
                .and_then(|p| p.active)
                .is_some_and(|p| {
                    p.piece == loc.piece
                        && (p.x != loc.x || p.y != loc.y)
                        && (p.x - loc.x).abs() <= 2
                        && (p.y - loc.y).abs() <= 2
                })
            {
                self.motion_seen = true;
            }
            for (x, y) in cells {
                o.cells[(19 - y) as usize][x as usize].kind = CellKind::Active;
            }
            let mut locked = Board::default();
            for y in 0..20 {
                for x in 0..10 {
                    if o.cells[y][x].kind == CellKind::Locked {
                        locked.rows[19 - y] |= 1 << x;
                    }
                }
            }
            let landing = locked.dropped(loc);
            for (x, y) in landing.cells() {
                if (0..20).contains(&y)
                    && (outlines[(19 - y) as usize][x as usize]
                        || hollow[(19 - y) as usize][x as usize])
                {
                    o.cells[(19 - y) as usize][x as usize] = Cell {
                        kind: CellKind::Ghost,
                        confidence: 0.94,
                    };
                }
            }
        } else {
            o.reasons
                .push("No unambiguous four-cell active silhouette".into());
        }
        // Unexplained outlines or hollow cells may be decorated terrain or
        // animations, so they remain uncertain instead of being erased blindly.
        for y in 0..20 {
            for x in 0..10 {
                if (outlines[y][x] || hollow[y][x]) && o.cells[y][x].kind != CellKind::Ghost {
                    o.cells[y][x].kind = CellKind::Uncertain;
                    o.cells[y][x].confidence = 0.3;
                }
            }
        }
        // A row completed by the still-falling piece is legal. Only a fully
        // locked row signals an unresolved line-clear/animation frame.
        for row in &mut o.cells {
            if row.iter().all(|c| c.kind == CellKind::Locked) {
                for c in row {
                    c.kind = CellKind::Uncertain;
                    c.confidence = 0.3;
                }
            }
        }
        // Board ROI only: do not scan or classify NEXT/HOLD panels.
        o.confidence = o
            .cells
            .iter()
            .flatten()
            .map(|c| c.confidence)
            .fold(1., f32::min);
        if o.active.is_none() {
            o.confidence = o.confidence.min(0.4);
        }
        self.previous_raw = Some(raw);
        self.previous = Some(o.clone());
        o
    }
}
pub fn components(mask: &[[bool; 10]; 20]) -> Vec<Vec<(i8, i8)>> {
    let mut visited = [[false; 10]; 20];
    let mut out = vec![];
    for y in 0..20 {
        for x in 0..10 {
            if !mask[y][x] || visited[y][x] {
                continue;
            }
            let mut q = vec![(x, y)];
            visited[y][x] = true;
            let mut k = 0;
            while k < q.len() {
                let (a, b) = q[k];
                k += 1;
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let nx = a as i32 + dx;
                    let ny = b as i32 + dy;
                    if (0..10).contains(&nx)
                        && (0..20).contains(&ny)
                        && mask[ny as usize][nx as usize]
                        && !visited[ny as usize][nx as usize]
                    {
                        visited[ny as usize][nx as usize] = true;
                        q.push((nx as usize, ny as usize));
                    }
                }
            }
            out.push(
                q.into_iter()
                    .map(|(a, b)| (a as i8, 19 - b as i8))
                    .collect(),
            );
        }
    }
    out
}
