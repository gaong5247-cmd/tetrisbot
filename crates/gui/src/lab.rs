//! Independent local test game. Runtime vision receives only captured pixels.
use capture::{Frame, Rect};
use detection::Grid;
use eframe::egui;
use game_state::*;
use movement::{Action, apply};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
pub const TITLE: &str = "SilhouetteTetris Local Lab";
pub struct Lab {
    pub board: Board,
    pub active: Location,
    pub hold: Option<Piece>,
    pub queue: VecDeque<Piece>,
    seed: u64,
    held: bool,
    last: Instant,
    ground_since: Option<Instant>,
    lock_resets: u8,
    pub placed: u32,
    pub over: bool,
    texture: Option<egui::TextureHandle>,
    pub gravity_ms: u64,
}
impl Default for Lab {
    fn default() -> Self {
        let mut s = Self {
            board: Board::default(),
            active: Location::spawn(Piece::T),
            hold: Some(Piece::I),
            queue: VecDeque::new(),
            seed: 137,
            held: false,
            last: Instant::now(),
            ground_since: None,
            lock_resets: 0,
            placed: 0,
            over: false,
            texture: None,
            gravity_ms: 1000,
        };
        s.refill();
        s
    }
}
impl Lab {
    fn refill(&mut self) {
        while self.queue.len() < 12 {
            let mut bag = PIECES;
            for i in (1..7).rev() {
                self.seed ^= self.seed << 13;
                self.seed ^= self.seed >> 7;
                self.seed ^= self.seed << 17;
                bag.swap(i, self.seed as usize % (i + 1));
            }
            self.queue.extend(bag);
        }
    }
    pub fn action(&mut self, a: Action) {
        if self.over {
            return;
        }
        if a == Action::Hold {
            if self.held {
                return;
            }
            let p = self
                .hold
                .replace(self.active.piece)
                .unwrap_or_else(|| self.queue.pop_front().unwrap());
            self.active = Location::spawn(p);
            self.held = true;
            self.last = Instant::now();
            self.ground_since = None;
            self.lock_resets = 0;
        } else if a == Action::HardDrop {
            let p = self.board.dropped(self.active);
            if self.board.lock(p).is_err() {
                self.over = true;
                return;
            }
            self.placed += 1;
            self.active = Location::spawn(self.queue.pop_front().unwrap());
            self.held = false;
            self.last = Instant::now();
            self.ground_since = None;
            self.lock_resets = 0;
            self.refill();
        } else if let Some((p, _)) = apply(&self.board, self.active, a) {
            if self.ground_since.is_some() && self.lock_resets < 15 && a != Action::Down {
                self.ground_since = Some(Instant::now());
                self.lock_resets += 1;
            }
            self.active = p;
        }
        if self.board.collides(self.active) {
            self.over = true;
        }
    }
    pub fn frame(
        &self,
        width: u32,
        height: u32,
        origin: (i32, i32),
        cell: u32,
        skin: u8,
        grid_lines: bool,
    ) -> Frame {
        let mut f = Frame::new(
            Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            [16, 19, 25, 255],
        );
        let (ox, oy) = origin;
        let c = cell as i32;
        f.fill(
            Rect {
                x: ox,
                y: oy,
                width: 10 * cell + 1,
                height: 20 * cell + 1,
            },
            [7, 9, 13, 255],
        );
        if grid_lines {
            for x in 0..=10 {
                f.fill(
                    Rect {
                        x: ox + x * c,
                        y: oy,
                        width: 1,
                        height: 20 * cell + 1,
                    },
                    [47, 51, 59, 255],
                );
            }
            for y in 0..=20 {
                f.fill(
                    Rect {
                        x: ox,
                        y: oy + y * c,
                        width: 10 * cell + 1,
                        height: 1,
                    },
                    [47, 51, 59, 255],
                );
            }
        }
        let color = if skin == 0 {
            [103, 210, 186, 255]
        } else {
            [214, 124, 190, 255]
        };
        let draw = |f: &mut Frame, x: i8, y: i8, col: [u8; 4]| {
            if (0..20).contains(&y) {
                f.fill(
                    Rect {
                        x: ox + x as i32 * c + 2,
                        y: oy + (19 - y) as i32 * c + 2,
                        width: cell - 3,
                        height: cell - 3,
                    },
                    col,
                );
            }
        };
        let landing = self.board.dropped(self.active);
        for (x, y) in landing.cells() {
            if (0..20).contains(&y) {
                let r = Rect {
                    x: ox + x as i32 * c + 2,
                    y: oy + (19 - y) as i32 * c + 2,
                    width: cell - 3,
                    height: cell - 3,
                };
                f.fill(r, [80, 95, 104, 255]);
                f.fill(
                    Rect {
                        x: r.x + (cell / 6) as i32,
                        y: r.y + (cell / 6) as i32,
                        width: r.width - cell / 3,
                        height: r.height - cell / 3,
                    },
                    [7, 9, 13, 255],
                );
            }
        }
        for y in 0..20 {
            for x in 0..10 {
                if self.board.occupied(x, y) {
                    draw(&mut f, x, y, [166, 178, 196, 255]);
                }
            }
        }
        if !self.over {
            for (x, y) in self.active.cells() {
                draw(&mut f, x, y, color);
            }
        }
        let mini = (cell as f32 * 0.66).round() as i32;
        let hud = |f: &mut Frame, p: Piece, x: i32, y: i32| {
            let cs = p.cells();
            let minx = cs.iter().map(|p| p.0).min().unwrap();
            let maxy = cs.iter().map(|p| p.1).max().unwrap();
            for (a, b) in cs {
                f.fill(
                    Rect {
                        x: x + (a - minx) as i32 * mini,
                        y: y + (maxy - b) as i32 * mini,
                        width: mini as u32,
                        height: mini as u32,
                    },
                    color,
                );
            }
        };
        if let Some(p) = self.hold {
            hud(&mut f, p, ox - 5 * c, oy + 20);
        }
        for (i, &p) in self.queue.iter().take(5).enumerate() {
            hud(&mut f, p, ox + 11 * c, oy + 20 + i as i32 * 3 * c);
        }
        f
    }
}
impl eframe::App for Lab {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        ctx.input(|i| {
            for (k, a) in [
                (egui::Key::ArrowLeft, Action::Left),
                (egui::Key::ArrowRight, Action::Right),
                (egui::Key::X, Action::Cw),
                (egui::Key::Z, Action::Ccw),
                (egui::Key::ArrowDown, Action::Down),
                (egui::Key::Space, Action::HardDrop),
                (egui::Key::C, Action::Hold),
            ] {
                if i.key_pressed(k) {
                    self.action(a)
                }
            }
        });
        if self.last.elapsed() >= Duration::from_millis(self.gravity_ms) {
            self.action(Action::Down);
            self.last = Instant::now();
        }
        if !self.over && self.board.dropped(self.active) == self.active {
            let since = self.ground_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= Duration::from_millis(500) {
                self.action(Action::HardDrop);
            }
        } else {
            self.ground_since = None;
        }
        egui::TopBottomPanel::top("lab_top").show(ctx,|ui|{ui.horizontal(|ui| {ui.heading("LOCAL LAB");ui.label(format!("Placed: {}",self.placed));if ui.button("Reset game").clicked(){*self=Self::default();}if self.over {ui.colored_label(egui::Color32::RED,"GAME OVER");}});ui.label("Arrows / X / Z / C / Space. Focus this window after Start AI. Reset invalidates the baseline.");});
        egui::CentralPanel::default().show(ctx, |ui| {
            let f = self.frame(550, 570, (140, 30), 24, 0, true);
            let image = egui::ColorImage::from_rgba_unmultiplied([550, 570], &f.rgba);
            if let Some(t) = self.texture.as_mut() {
                t.set(image, egui::TextureOptions::NEAREST);
            } else {
                self.texture = Some(ctx.load_texture("lab", image, egui::TextureOptions::NEAREST));
            }
            ui.image((self.texture.as_ref().unwrap().id(), egui::vec2(550., 570.)));
        });
        ctx.request_repaint_after(Duration::from_millis(8));
    }
}
pub fn run() -> anyhow::Result<()> {
    capture::dpi_aware();
    eframe::run_native(
        TITLE,
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([575., 650.])
                .with_position([15., 20.]),
            ..Default::default()
        },
        Box::new(|_| Ok(Box::<Lab>::default())),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
pub fn fixture(
    width: u32,
    height: u32,
    origin: (i32, i32),
    cell: u32,
    skin: u8,
    grid: bool,
) -> (Frame, Grid) {
    let lab = Lab::default();
    (
        lab.frame(width, height, origin, cell, skin, grid),
        Grid {
            x: origin.0 as f32,
            y: origin.1 as f32,
            cw: cell as f32,
            ch: cell as f32,
            score: 1.,
            shape_evidence: true,
        },
    )
}
