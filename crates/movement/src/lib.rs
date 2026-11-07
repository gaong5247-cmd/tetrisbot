//! Timed pulse-input BFS with SRS true-rotation kick offsets.
//! Offset tables are derived from CC2 movegen.rs (MIT), revision in THIRD_PARTY.md.
use anyhow::{Result, bail};
use game_state::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    Left,
    Right,
    Cw,
    Ccw,
    Down,
    HardDrop,
    Hold,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MovementProfile {
    pub pulse_ms: u32,
    pub gap_ms: u32,
    pub das_ms: u32,
    pub arr_ms: u32,
    pub gravity_ms: u32,
    pub lock_delay_ms: u32,
    pub max_nodes: usize,
    pub guideline_srs: bool,
    pub soft_drop_one_cell: bool,
}
impl Default for MovementProfile {
    fn default() -> Self {
        Self {
            pulse_ms: 18,
            gap_ms: 22,
            das_ms: 150,
            arr_ms: 30,
            gravity_ms: 1000,
            lock_delay_ms: 500,
            max_nodes: 100_000,
            guideline_srs: true,
            soft_drop_one_cell: true,
        }
    }
}
impl MovementProfile {
    pub fn validate(&self) -> Result<()> {
        if !self.guideline_srs
            || !self.soft_drop_one_cell
            || self.pulse_ms == 0
            || self.pulse_ms >= self.das_ms
            || self.gravity_ms < self.pulse_ms + self.gap_ms
            || self.lock_delay_ms < self.pulse_ms + self.gap_ms
            || self.max_nodes == 0
        {
            bail!("unsupported or unsafe movement profile")
        }
        Ok(())
    }
    pub fn step_ms(&self) -> u32 {
        self.pulse_ms + self.gap_ms
    }
}
fn offsets(p: Piece, r: Rotation) -> [(i8, i8); 5] {
    match p {
        Piece::O => match r {
            Rotation::North => [(0, 0); 5],
            Rotation::East => [(0, -1); 5],
            Rotation::South => [(-1, -1); 5],
            Rotation::West => [(-1, 0); 5],
        },
        Piece::I => match r {
            Rotation::North => [(0, 0), (-1, 0), (2, 0), (-1, 0), (2, 0)],
            Rotation::East => [(-1, 0), (0, 0), (0, 0), (0, 1), (0, -2)],
            Rotation::South => [(-1, 1), (1, 1), (-2, 1), (1, 0), (-2, 0)],
            Rotation::West => [(0, 1), (0, 1), (0, 1), (0, -1), (0, 2)],
        },
        _ => match r {
            Rotation::North | Rotation::South => [(0, 0); 5],
            Rotation::East => [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
            Rotation::West => [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
        },
    }
}
pub fn rotate(board: &Board, p: Location, cw: bool) -> Option<(Location, Spin)> {
    let r = if cw {
        p.rotation.cw()
    } else {
        p.rotation.ccw()
    };
    let a = offsets(p.piece, p.rotation);
    let b = offsets(p.piece, r);
    for i in 0..5 {
        let n = Location {
            rotation: r,
            x: p.x + a[i].0 - b[i].0,
            y: p.y + a[i].1 - b[i].1,
            ..p
        };
        if board.collides(n) {
            continue;
        }
        let spin = if p.piece != Piece::T {
            Spin::None
        } else {
            let corners = [(-1, -1), (1, -1), (-1, 1), (1, 1)]
                .iter()
                .filter(|&&(x, y)| board.occupied(n.x + x, n.y + y))
                .count();
            let front = [(-1, 1), (1, 1)]
                .iter()
                .map(|&c| r.transform(c))
                .filter(|&(x, y)| board.occupied(n.x + x, n.y + y))
                .count();
            if corners < 3 {
                Spin::None
            } else if front == 2 || i == 4 {
                Spin::Full
            } else {
                Spin::Mini
            }
        };
        return Some((n, spin));
    }
    None
}
pub fn apply(board: &Board, p: Location, a: Action) -> Option<(Location, Spin)> {
    match a {
        Action::Cw => rotate(board, p, true),
        Action::Ccw => rotate(board, p, false),
        Action::HardDrop => Some((board.dropped(p), Spin::None)),
        Action::Left | Action::Right | Action::Down => {
            let n = match a {
                Action::Left => Location { x: p.x - 1, ..p },
                Action::Right => Location { x: p.x + 1, ..p },
                _ => Location { y: p.y - 1, ..p },
            };
            (!board.collides(n)).then_some((n, Spin::None))
        }
        Action::Hold => None,
    }
}
#[derive(Debug, Clone)]
pub struct Plan {
    pub actions: Vec<Action>,
    pub expected: Board,
    pub placement: Placement,
    pub uses_hold: bool,
    pub expanded: usize,
    pub duration_ms: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Node {
    p: Location,
    spin: Spin,
    phase: u32,
    ground_ms: u32,
}
pub fn plan(snapshot: &Snapshot, target: Placement, profile: &MovementProfile) -> Result<Plan> {
    profile.validate()?;
    let hold = snapshot.needs_hold(target.location.piece)?;
    let board = &snapshot.board;
    if board.collides(target.location) || board.dropped(target.location) != target.location {
        bail!("engine placement not legal/grounded")
    }
    let start = if hold {
        Location::spawn(target.location.piece)
    } else {
        snapshot.active
    };
    if board.collides(start) {
        bail!("spawn/current state obstructed")
    }
    let root = Node {
        p: start,
        spin: Spin::None,
        phase: 0,
        ground_ms: 0,
    };
    let mut queue = VecDeque::from([root]);
    let mut parents: HashMap<Node, Option<(Node, Action)>> = HashMap::from([(root, None)]);
    let mut expanded = 0;
    while let Some(n) = queue.pop_front() {
        expanded += 1;
        if expanded > profile.max_nodes {
            bail!("planner node budget exhausted")
        }
        let dropped = board.dropped(n.p);
        let spin = if dropped == n.p { n.spin } else { Spin::None };
        if dropped.same_cells(target.location) && spin == target.spin {
            let mut actions = vec![Action::HardDrop];
            let mut cur = n;
            while let Some((prev, act)) = parents[&cur] {
                actions.push(act);
                cur = prev;
            }
            actions.reverse();
            if hold {
                actions.insert(0, Action::Hold);
            }
            let mut expected = *board;
            expected.lock(target.location)?;
            let duration_ms = actions.len() as u32 * profile.step_ms();
            return Ok(Plan {
                actions,
                expected,
                placement: target,
                uses_hold: hold,
                expanded,
                duration_ms,
            });
        }
        for a in [
            Action::Left,
            Action::Right,
            Action::Cw,
            Action::Ccw,
            Action::Down,
        ] {
            let Some((mut p, mut s)) = apply(board, n.p, a) else {
                continue;
            };
            let mut phase = n.phase + profile.step_ms();
            let mut ground = if p == n.p { n.ground_ms } else { 0 };
            while phase >= profile.gravity_ms {
                phase -= profile.gravity_ms;
                let down = Location { y: p.y - 1, ..p };
                if !board.collides(down) {
                    p = down;
                    s = Spin::None;
                }
            }
            if board.dropped(p) == p {
                ground += profile.step_ms();
            } else {
                ground = 0;
            }
            if ground >= profile.lock_delay_ms {
                continue;
            }
            let next = Node {
                p,
                spin: s,
                phase,
                ground_ms: ground,
            };
            if let std::collections::hash_map::Entry::Vacant(e) = parents.entry(next) {
                e.insert(Some((n, a)));
                queue.push_back(next);
            }
        }
    }
    bail!("no reachable SRS path for requested spin/placement")
}
