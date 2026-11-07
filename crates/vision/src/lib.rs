//! Palette-free cell features, exact geometry and conservative temporal classification.
use capture::Frame;
use detection::{Grid, contrast};
use game_state::*;
#[derive(Default)]
pub struct Vision {
    previous: Option<Observation>,
    previous_raw: Option<[[bool; 10]; 20]>,
    // Persist the actual playfield background even when multiplayer garbage
    // occupies most of its cells. A full-board median would be garbage-colored.
    background: Option<[u8; 3]>,
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

/// Separate adjacent colored cells by appearance, so a weak solid ghost
/// that touches its bright falling piece does not merge into an 8-cell blob.
fn color_components(raw: &[[bool; 10]; 20], centers: &[[u8; 3]], bg: [u8; 3])
    -> Vec<Vec<(i8, i8)>>
{
    let mut used = [[false; 10]; 20];
    let mut out = Vec::new();
    for y in 0..20 {
        for x in 0..10 {
            if !raw[y][x] || used[y][x] { continue }
            let base = centers[y * 10 + x];
            let strength = contrast(base, bg);
            let mut queue = vec![(x,y)];
            used[y][x] = true;
            let mut n = 0;
            while n < queue.len() {
                let (px,py) = queue[n];
                n += 1;
                for (dx,dy) in [(-1,0),(1,0),(0,-1),(0,1)] {
                    let xx=px as i32+dx;
                    let yy=py as i32+dy;
                    if !(0..10).contains(&xx) || !(0..20).contains(&yy) { continue }
                    let (nx,ny)=(xx as usize,yy as usize);
                    if used[ny][nx] || !raw[ny][nx] { continue }
                    // Compare against the component's seed, not the preceding
                    // neighbor, to prevent gradual shading joining a ghost.
                    if contrast(base,centers[ny*10+nx]) > 10. + strength*0.22 {continue}
                    used[ny][nx]=true;
                    queue.push((nx,ny));
                }
            }
            out.push(queue.into_iter().map(|(x,y)|(x as i8,19-y as i8)).collect());
        }
    }
    out
}
fn mean_group_color(cells: &[(i8,i8)], centers: &[[u8;3]]) -> [f32;3] {
    std::array::from_fn(|k| cells.iter()
        .map(|&(x,y)|centers[(19-y as usize)*10+x as usize][k] as f32)
        .sum::<f32>() / cells.len() as f32)
}
fn dimmer_same_hue(bright: [f32;3], dim: [f32;3], bg: [u8;3]) -> bool {
    let a: [f32;3]=std::array::from_fn(|i|bright[i]-bg[i] as f32);
    let b: [f32;3]=std::array::from_fn(|i|dim[i]-bg[i] as f32);
    let aa: f32=a.iter().map(|v|v*v).sum();
    let bb: f32=b.iter().map(|v|v*v).sum();
    let dot: f32=(0..3).map(|i|a[i]*b[i]).sum();
    if aa < 400. || bb < 200. {return false}
    let ratio=(bb/aa).sqrt();
    (0.25..0.84).contains(&ratio) && dot > 0.95*(aa*bb).sqrt()
}
/// Only classify a four-cell dim overlay as ghost if it is a SAME-SHAPE,
/// same-hue dim copy at the exactly simulated landing of a brighter active
/// piece. Real locked pieces (including garbage) are never cleared merely
/// for being dark or sharing the active piece's color.
fn dimmed_ghost_cells(raw: &[[bool;10];20], colors:&[[u8;3]], bg:[u8;3],
    previous:Option<&Observation>) -> Option<(Location,Vec<(i8,i8)>)>
{
    let groups=color_components(raw,colors,bg);
    let shapes: Vec<_>=groups.iter().filter_map(|cells| {
        if cells.len()!=4 {return None}
        let shape=match_shape(cells).into_iter().next()?;
        Some((cells,shape,mean_group_color(cells,colors)))
    }).collect();
    let mut strongest:Option<(f32,Location,Vec<(i8,i8)>)>=None;
    for (a,aloc,acol) in &shapes {
        for (b,bloc,bcol) in &shapes {
            if std::ptr::eq(*a,*b) || aloc.piece!=bloc.piece {continue}
            if !dimmer_same_hue(*acol,*bcol,bg) {continue}
            if aloc.y <= bloc.y {continue}
            if previous.is_some_and(|prev| b.iter().any(|&(x,y)|
                prev.cells[(19-y) as usize][x as usize].kind==CellKind::Locked)) {continue}
            let mut board=Board::default();
            for y in 0..20 {
                for x in 0..10 {
                    if !raw[y][x] {continue}
                    let cell=(x as i8,19-y as i8);
                    if a.contains(&cell) || b.contains(&cell) {continue}
                    board.rows[19-y] |= 1<<x;
                }
            }
            if board.collides(*aloc) || !board.dropped(*aloc).same_cells(*bloc) {continue}
            let weight=acol.iter().zip(bg.iter())
                .map(|(v,c)|(v-*c as f32).abs()).sum::<f32>();
            if strongest.as_ref().is_none_or(|(old,_,_)|weight>*old) {
                strongest=Some((weight,*aloc,(*b).clone()));
            }
        }
    }
    strongest.map(|(_,loc,cells)|(loc,cells))
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
        // The upper cells usually expose bare board background even when
        // 16-19 lower garbage rows are filled. Once confirmed, keep that
        // reference across rising attacks instead of using the now-majority
        // garbage color as background.
        let top_row_bg=median_color(&centers[..10]);
        let top_row_support=centers[..10].iter()
            .filter(|&&p|contrast(p,top_row_bg)<12.).count();
        // At an extreme 19/20-row garbage stack the first row may still
        // expose six background cells, while rows 1-19 are nearly all filled.
        // Prefer that top row over a four-row median when it is unambiguous.
        let top_four_bg=median_color(&centers[..40]);
        let top_four_support=centers[..40].iter()
            .filter(|&&p|contrast(p,top_four_bg)<12.).count();
        let (top_bg,support)=if top_row_support>=5 {
            (top_row_bg,top_row_support)
        } else {
            (top_four_bg,top_four_support)
        };
        let bg=self.background.unwrap_or(top_bg);
        if self.background.is_none() && support>=5 {
            self.background=Some(top_bg);
        }
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
        // Find solid translucent ghost blocks BEFORE connected-components
        // matching, otherwise they join the bright active piece into 8 cells.
        let dimmed=dimmed_ghost_cells(&raw,&centers,bg,self.previous.as_ref());
        let dimmed_active=dimmed.as_ref().map(|(loc,_)|*loc);
        if let Some((_,cells))=dimmed {
            for (x,y) in cells {
                let (x,y)=(x as usize,(19-y) as usize);
                raw[y][x]=false;
                o.cells[y][x]=Cell {kind:CellKind::Ghost,confidence:0.94};
            }
        }
        let components = components(&raw);
        // On tall garbage stacks the falling piece can be in direct contact
        // with a locked garbage wall. Binary components fuse them into one
        // huge blob; color-separated components can still isolate the active
        // four cells without guessing an empty row.
        let color_groups = color_components(&raw,&centers,bg);
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
        for c in components.iter().chain(color_groups.iter()) {
            if c.len() == 4 {
                let shapes = match_shape(c);
                if !shapes.is_empty()
                    && !hypotheses.iter().any(|(existing, _): &(Vec<(i8,i8)>,Location)|
                        existing.len()==c.len() && existing.iter().all(|cell|c.contains(cell)))
                {
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
        let chosen = dimmed_active
            .and_then(|loc| hypotheses.iter()
                .find(|(_,p)|p.same_cells(loc)).cloned())
            .or_else(|| hypotheses
            .iter()
            .filter(|(c, p)| p.y >= 12 && c.iter().any(|&(x, y)| moving.contains(&(x, y))))
            .max_by_key(|(_, p)| p.y)
            .cloned())
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
