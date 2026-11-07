//! TBP-compatible, bottom-up SRS true-rotation coordinates.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Piece {
    I,
    O,
    T,
    S,
    Z,
    J,
    L,
}
pub const PIECES: [Piece; 7] = [
    Piece::I,
    Piece::O,
    Piece::T,
    Piece::S,
    Piece::Z,
    Piece::J,
    Piece::L,
];
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    North,
    East,
    South,
    West,
}
pub const ROTATIONS: [Rotation; 4] = [
    Rotation::North,
    Rotation::East,
    Rotation::South,
    Rotation::West,
];
impl Rotation {
    pub fn cw(self) -> Self {
        ROTATIONS[(self as usize + 1) % 4]
    }
    pub fn ccw(self) -> Self {
        ROTATIONS[(self as usize + 3) % 4]
    }
    pub fn transform(self, (x, y): (i8, i8)) -> (i8, i8) {
        match self {
            Self::North => (x, y),
            Self::East => (y, -x),
            Self::South => (-x, -y),
            Self::West => (-y, x),
        }
    }
}
impl Piece {
    pub fn cells(self) -> [(i8, i8); 4] {
        match self {
            Self::I => [(-1, 0), (0, 0), (1, 0), (2, 0)],
            Self::O => [(0, 0), (1, 0), (0, 1), (1, 1)],
            Self::T => [(-1, 0), (0, 0), (1, 0), (0, 1)],
            Self::S => [(-1, 0), (0, 0), (0, 1), (1, 1)],
            Self::Z => [(-1, 1), (0, 1), (0, 0), (1, 0)],
            Self::J => [(-1, 0), (0, 0), (1, 0), (-1, 1)],
            Self::L => [(-1, 0), (0, 0), (1, 0), (1, 1)],
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Location {
    #[serde(rename = "type")]
    pub piece: Piece,
    #[serde(rename = "orientation")]
    pub rotation: Rotation,
    pub x: i8,
    pub y: i8,
}
impl Location {
    pub fn spawn(piece: Piece) -> Self {
        Self {
            piece,
            rotation: Rotation::North,
            x: 4,
            y: 18,
        }
    }
    pub fn cells(self) -> [(i8, i8); 4] {
        self.piece.cells().map(|c| {
            let (x, y) = self.rotation.transform(c);
            (x + self.x, y + self.y)
        })
    }
    pub fn same_cells(self, other: Self) -> bool {
        let mut a = self.cells();
        let mut b = other.cells();
        a.sort();
        b.sort();
        a == b
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spin {
    None,
    Mini,
    Full,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Placement {
    pub location: Location,
    pub spin: Spin,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Board {
    pub rows: [u16; 40],
}
impl Default for Board {
    fn default() -> Self {
        Self { rows: [0; 40] }
    }
}
impl Board {
    pub fn occupied(&self, x: i8, y: i8) -> bool {
        !(0..10).contains(&x) || !(0..40).contains(&y) || self.rows[y as usize] & (1 << x) != 0
    }
    pub fn collides(&self, p: Location) -> bool {
        p.cells().iter().any(|&(x, y)| self.occupied(x, y))
    }
    pub fn dropped(&self, mut p: Location) -> Location {
        while !self.collides(Location { y: p.y - 1, ..p }) {
            p.y -= 1;
        }
        p
    }
    pub fn lock(&mut self, p: Location) -> Result<u32> {
        if self.collides(p) || self.dropped(p) != p {
            bail!("placement is obstructed or not grounded")
        }
        for (x, y) in p.cells() {
            self.rows[y as usize] |= 1 << x;
        }
        let mut target = 0;
        let mut cleared = 0;
        for y in 0..40 {
            if self.rows[y] == 1023 {
                cleared += 1;
            } else {
                self.rows[target] = self.rows[y];
                target += 1;
            }
        }
        self.rows[target..].fill(0);
        Ok(cleared)
    }
    pub fn tbp_rows(&self) -> Vec<[Option<char>; 10]> {
        self.rows
            .iter()
            .map(|row| std::array::from_fn(|x| if row & (1 << x) != 0 { Some('G') } else { None }))
            .collect()
    }
    /// Visible rows are top-down. Hidden rows require a separately verified baseline.
    pub fn from_visible(rows: &[[Cell; 10]; 20], hidden_empty: bool) -> Result<Self> {
        if !hidden_empty {
            bail!("hidden rows unknown; fresh-game baseline required")
        }
        let mut b = Self::default();
        for (screen_y, row) in rows.iter().enumerate() {
            for (x, c) in row.iter().enumerate() {
                if c.kind == CellKind::Uncertain {
                    bail!("uncertain cell")
                }
                if c.kind == CellKind::Locked {
                    b.rows[19 - screen_y] |= 1 << x;
                }
            }
        }
        Ok(b)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Empty,
    Locked,
    Active,
    Ghost,
    Uncertain,
}
#[derive(Debug, Clone, Copy)]
pub struct Cell {
    pub kind: CellKind,
    pub confidence: f32,
}
impl Default for Cell {
    fn default() -> Self {
        Self {
            kind: CellKind::Uncertain,
            confidence: 0.0,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Known<T> {
    Unknown,
    Value(T),
}
#[derive(Debug, Clone)]
pub struct Observation {
    pub cells: [[Cell; 10]; 20],
    pub active: Option<Location>,
    pub hold: Known<Option<Piece>>,
    pub next: Vec<Piece>,
    pub confidence: f32,
    pub reasons: Vec<String>,
}
impl Default for Observation {
    fn default() -> Self {
        Self {
            cells: [[Cell::default(); 10]; 20],
            active: None,
            hold: Known::Unknown,
            next: vec![],
            confidence: 0.,
            reasons: vec![],
        }
    }
}
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub board: Board,
    pub active: Location,
    pub hold: Option<Piece>,
    pub next: Vec<Piece>,
    pub combo: u32,
    pub back_to_back: bool,
}
impl Snapshot {
    pub fn from_observation(o: &Observation, baseline: bool) -> Result<Self> {
        if o.confidence < 0.9 {
            bail!("vision confidence below threshold")
        }
        let active = o
            .active
            .ok_or_else(|| anyhow::anyhow!("active piece unknown"))?;
        let Known::Value(hold) = o.hold else {
            bail!("HOLD unknown")
        };
        if o.next.len() < 3 {
            bail!("need at least three observed NEXT pieces")
        }
        Ok(Self {
            board: Board::from_visible(&o.cells, baseline)?,
            active,
            hold,
            next: o.next.clone(),
            combo: 0,
            back_to_back: false,
        })
    }
    pub fn queue(&self) -> Vec<Piece> {
        std::iter::once(self.active.piece)
            .chain(self.next.iter().copied())
            .collect()
    }
    pub fn needs_hold(&self, p: Piece) -> Result<bool> {
        if p == self.active.piece {
            return Ok(false);
        }
        if self.hold.or_else(|| self.next.first().copied()) == Some(p) {
            Ok(true)
        } else {
            bail!("engine piece not present in active/HOLD/NEXT")
        }
    }
}
/// Current-piece-only Cold Clear 2 planning state.
///
/// The bot deliberately never reads NEXT/HOLD. Cold Clear 2 receives only
/// the observed active piece and the observed locked board. The duplicate
/// reserve is a private search implementation detail and is NEVER a real
/// game HOLD action. Every placed piece starts a fresh engine search.
#[derive(Debug, Clone)]
pub struct PlanningSnapshot {
    pub snapshot: Snapshot,
    /// Kept for compatibility with TBP workers; always true in this build.
    pub restricted_no_hold: bool,
}

impl PlanningSnapshot {
    pub fn from_observation(o: &Observation, baseline: bool) -> Result<Self> {
        Self::with_mode(o, baseline, true)
    }
    pub fn with_mode(o: &Observation, baseline: bool, _force_restricted: bool) -> Result<Self> {
        if o.confidence < 0.9 {
            bail!("vision confidence below threshold; cannot plan current piece")
        }
        let active = o.active.ok_or_else(|| anyhow::anyhow!("active piece unknown"))?;
        let board = Board::from_visible(&o.cells, baseline)?;
        Ok(Self {
            snapshot: Snapshot {
                board,
                active,
                hold: Some(active.piece), // synthetic CC2 reserve, never a real HOLD
                next: Vec::new(),          // never use or synthesize previews
                combo: 0,
                back_to_back: false,
            },
            restricted_no_hold: true,
        })
    }
}
/// Match exact four-cell silhouettes, returning every coordinate interpretation.
pub fn match_shape(cells: &[(i8, i8)]) -> Vec<Location> {
    if cells.len() != 4 {
        return vec![];
    }
    let mut actual = cells.to_vec();
    actual.sort();
    actual.dedup();
    if actual.len() != 4 {
        return vec![];
    }
    let mut out = vec![];
    for piece in PIECES {
        for rotation in ROTATIONS {
            let rotated = piece.cells().map(|c| rotation.transform(c));
            for &(a, b) in &actual {
                let (dx, dy) = rotated[0];
                let loc = Location {
                    piece,
                    rotation,
                    x: a - dx,
                    y: b - dy,
                };
                let mut test = loc.cells();
                test.sort();
                if actual == test {
                    out.push(loc);
                }
            }
        }
    }
    out
}
