//! Palette-free cell features, exact geometry and conservative temporal classification.
use capture::{Frame, Rect};
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
                raw[y][x] = d > 24. && spread < 25.;
                outlines[y][x] = d < 12. && outline >= 3;
                let kind = if raw[y][x] {
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
                if (0..20).contains(&y) && outlines[(19 - y) as usize][x as usize] {
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
        // Non-solid outlines away from a proven landing relationship remain unknown.
        for y in 0..20 {
            for x in 0..10 {
                if outlines[y][x] && o.cells[y][x].kind != CellKind::Ghost {
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
        let hud = detect_hud(f, g);
        o.next = hud.next;
        o.hold = hud.hold;
        o.confidence = o
            .cells
            .iter()
            .flatten()
            .map(|c| c.confidence)
            .fold(1., f32::min);
        if o.active.is_none() {
            o.confidence = o.confidence.min(0.4);
        }
        if o.next.len() < 3 {
            o.reasons.push("NEXT sequence not yet confirmed".into());
        }
        if o.hold == Known::Unknown {
            o.reasons
                .push("HOLD unknown (absence is not proof of empty HOLD)".into());
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
#[derive(Debug, Clone)]
pub struct Hud {
    pub next: Vec<Piece>,
    pub hold: Known<Option<Piece>>,
    pub silhouettes: Vec<(Rect, Piece)>,
}
/// Discover isolated filled silhouettes in surrounding HUD, at independent scales.
/// Queue order is accepted only for at least three vertically aligned shapes.
pub fn detect_hud(f: &Frame, g: Grid) -> Hud {
    let mut shapes = vec![];
    for right in [false, true] {
        let area = Rect {
            x: if right {
                (g.x + 10. * g.cw + g.cw * 0.4) as i32
            } else {
                (g.x - 6. * g.cw) as i32
            },
            y: g.y as i32,
            width: (5. * g.cw) as u32,
            height: (15. * g.ch) as u32,
        };
        let Some(area) = area.intersection(f.rect) else {
            continue;
        };
        let step = (g.cw / 10.).max(1.).round() as i32;
        let w = (area.width as i32 / step) as usize;
        let h = (area.height as i32 / step) as usize;
        if w == 0 || h == 0 {
            continue;
        }
        let pixels: Vec<_> = (0..w * h)
            .map(|i| {
                f.pixel(
                    area.x + (i % w) as i32 * step,
                    area.y + (i / w) as i32 * step,
                )
            })
            .collect();
        let bg = median_color(&pixels);
        let mut mask: Vec<_> = pixels.iter().map(|&p| contrast(p, bg) > 25.).collect();
        for start in 0..mask.len() {
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
                    if i % w > 0 { Some(i - 1) } else { None },
                    if i % w + 1 < w { Some(i + 1) } else { None },
                    i.checked_sub(w),
                    if i + w < mask.len() {
                        Some(i + w)
                    } else {
                        None
                    },
                ]
                .into_iter()
                .flatten()
                {
                    if mask[j] {
                        mask[j] = false;
                        q.push(j);
                    }
                }
            }
            if q.len() < 12 {
                continue;
            }
            let minx = q.iter().map(|i| i % w).min().unwrap();
            let maxx = q.iter().map(|i| i % w).max().unwrap();
            let miny = q.iter().map(|i| i / w).min().unwrap();
            let maxy = q.iter().map(|i| i / w).max().unwrap();
            let rw = (maxx - minx + 1) as f32;
            let rh = (maxy - miny + 1) as f32;
            let mut found = Vec::new();
            for p in PIECES {
                for rot in ROTATIONS {
                    let cs = p.cells().map(|c| rot.transform(c));
                    let lx = cs.iter().map(|c| c.0).min().unwrap();
                    let ly = cs.iter().map(|c| c.1).min().unwrap();
                    let bw = cs.iter().map(|c| c.0).max().unwrap() - lx + 1;
                    let bh = cs.iter().map(|c| c.1).max().unwrap() - ly + 1;
                    let sx = rw / bw as f32;
                    let sy = rh / bh as f32;
                    if (sx / sy - 1.).abs() > 0.22
                        || sx * (step as f32) < g.cw * 0.3
                        || sx * (step as f32) > g.cw * 1.3
                    {
                        continue;
                    }
                    let mut expected = 0;
                    let mut mismatches = 0;
                    for yy in 0..bh {
                        for xx in 0..bw {
                            let px = minx + ((xx as f32 + 0.5) * sx) as usize;
                            let py = miny + ((bh as f32 - yy as f32 - 0.5) * sy) as usize;
                            let occupied =
                                contrast(pixels[py.min(h - 1) * w + px.min(w - 1)], bg) > 25.;
                            let want = cs.contains(&(xx + lx, yy + ly));
                            if want {
                                expected += 1;
                            }
                            if occupied != want {
                                mismatches += 1;
                            }
                        }
                    }
                    let fill = q.len() as f32 / (sx * sy * 4.);
                    if mismatches == 0 && expected == 4 && (0.65..1.15).contains(&fill) {
                        found.push(p);
                    }
                }
            }
            found.sort_by_key(|p| *p as usize);
            found.dedup();
            if found.len() == 1 {
                shapes.push((
                    Rect {
                        x: area.x + minx as i32 * step,
                        y: area.y + miny as i32 * step,
                        width: (rw * step as f32) as u32,
                        height: (rh * step as f32) as u32,
                    },
                    found[0],
                    right,
                ));
            }
        }
    }
    let mut right: Vec<_> = shapes.iter().filter(|s| s.2).collect();
    right.sort_by_key(|s| s.0.y);
    let aligned = right.len() >= 3
        && right
            .iter()
            .all(|s| (s.0.x - right[0].0.x).abs() < (g.cw * 2.) as i32)
        && right
            .windows(2)
            .all(|w| w[1].0.y > w[0].0.y + (g.ch * 1.5) as i32);
    let next = if aligned {
        right.iter().map(|s| s.1).collect()
    } else {
        vec![]
    };
    let left: Vec<_> = shapes.iter().filter(|s| !s.2).collect();
    let hold = if left.len() == 1 {
        Known::Value(Some(left[0].1))
    } else {
        Known::Unknown
    };
    Hud {
        next,
        hold,
        silhouettes: shapes.into_iter().map(|(r, p, _)| (r, p)).collect(),
    }
}
