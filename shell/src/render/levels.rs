//! Levels on a tiered map. Ground is tinted by its level, brighter above the
//! floor the Foundries stand on and darker below it. A cliff tile sits at the
//! foot of its drop, so the drop is drawn as a terrace step over the last tile
//! of the ground above, which stays buildable, with its face reaching a cell
//! into the cliff tile; the rest of the cliff tile reads as the lower ground.
//! A ramp is a graded cut with ruts, its walls stepping down beside it and
//! turning along the cliff's foot.
//!
//! Everything is laid out once per map in cells, [`CELLS`] to a tile edge, and
//! drawn per visible tile.

use super::cell_field::{CELLS, cell_edges};
use super::environment::{LAYERS, WAVE_LIFTS, draw_fracture, draw_missing_slab, hash, rgba};
use crate::game::Scene;
use crate::numeric::{self, Fit};
use chassis::grid::{TilePos, as_index};
use macroquad::prelude::*;
use oxide_kit::render::Shading;
use oxide_sim::map::{Elevation, Map, Terrain};

/// Mean brightness of each ground sprite variant, and their median. A tinted
/// tile is calmed toward the median so the variants' waves stay subtle on
/// bright levels.
const SHADE_MEAN: [f32; 6] = [31.7, 34.0, 37.0, 39.0, 41.0, 43.3];
const SHADE_MEDIAN: f32 = 38.0;
/// Flat ground's grey, which bench fills scale by their level's tone.
const FLOOR_SHADE: f32 = 36.0;
/// Where light comes from, for a ramp's slope shading.
const LIGHT: Vec2 = Vec2::new(-0.62, -0.78);
/// A lip's share of its cell.
const LIP: f32 = 0.22;
/// How far a cliff looks for the ground above it, in tiles: far enough to
/// see past a second tile of a taller cliff.
const REACH: i32 = 2;
/// How far past its own cells a step reads the ground around it, in cells.
/// Its foot and top lie well inside this on any cliff a few tiles thick.
const WINDOW: i32 = 8 * CELLS;
const SIDES: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

/// What a tile is drawn as, which for a cliff and the ground above it is not
/// what it is: the step stands on the upper ground's last tile and the cliff
/// tile shows the ground at its foot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Open(u8),
    /// Part of a step from `upper` down to `lower`.
    Band {
        upper: u8,
        lower: u8,
    },
    Ramp(usize),
    Other,
}

/// A ramp's rectangle and the way it runs downhill.
#[derive(Clone, Copy, Debug)]
struct Ramp {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    dir: (i32, i32),
    low: u8,
}

impl Ramp {
    fn along_x(&self) -> bool {
        self.dir.0 != 0
    }

    fn length(&self) -> f32 {
        if self.along_x() {
            (self.x1 + 1 - self.x0) as f32
        } else {
            (self.y1 + 1 - self.y0) as f32
        }
    }

    fn width(&self) -> f32 {
        if self.along_x() {
            (self.y1 + 1 - self.y0) as f32
        } else {
            (self.x1 + 1 - self.x0) as f32
        }
    }

    /// How far down the ramp `p` lies, from 0 at the top to 1 at the foot.
    fn progress(&self, p: Vec2) -> f32 {
        let along = if self.along_x() {
            (p.x - self.x0 as f32) / self.length()
        } else {
            (p.y - self.y0 as f32) / self.length()
        };
        if self.dir.0 < 0 || self.dir.1 < 0 {
            1.0 - along
        } else {
            along
        }
    }

    /// Progress with a short flat at each end.
    fn graded(&self, p: Vec2) -> f32 {
        ((self.progress(p) - 0.08) / 0.84).clamp(0.0, 1.0)
    }

    fn height(&self, p: Vec2) -> f32 {
        let high = f32::from(self.low) + 1.0;
        high - self.graded(p)
    }
}

/// A fill inside one cell of a tile: the cell, then the fill's offset and
/// size as shares of the cell.
#[derive(Clone, Copy, Debug)]
struct Op {
    sx: u8,
    sy: u8,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
}

/// A worn block on a step: the tile's index, the bench's colour, a token.
type Decal = (usize, Color, u32);

/// The levels of one tiered map, laid out for drawing.
pub(crate) struct Levels {
    width: i32,
    height: i32,
    shading: Shading,
    class: Vec<Class>,
    ramps: Vec<Ramp>,
    /// Every tile's fills, in draw order: tile `i`'s run from `starts[i]`.
    ops: Vec<Op>,
    starts: Vec<u32>,
    /// Worn blocks, by tile index.
    decals: Vec<Decal>,
}

/// Chessboard distance in cells to the nearest seed.
fn distance(width: i32, height: i32, seed: impl Fn(i32, i32) -> bool) -> Vec<u8> {
    let mut d: Vec<u8> = (0..width * height)
        .map(|i| {
            if seed(i % width, i / width) {
                0
            } else {
                u8::MAX
            }
        })
        .collect();
    let at = |d: &[u8], x: i32, y: i32| {
        if x < 0 || y < 0 || x >= width || y >= height {
            u8::MAX
        } else {
            d[as_index(y * width + x)]
        }
    };
    for backward in [false, true] {
        let s = if backward { -1 } else { 1 };
        for step_y in 0..height {
            let y = if backward {
                height - 1 - step_y
            } else {
                step_y
            };
            for step_x in 0..width {
                let x = if backward { width - 1 - step_x } else { step_x };
                let mut best = d[as_index(y * width + x)];
                for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0)] {
                    best = best.min(at(&d, x + dx * s, y + dy * s).saturating_add(1));
                }
                d[as_index(y * width + x)] = best;
            }
        }
    }
    d
}

fn lifted(color: Color, lift: i16) -> Color {
    let amount = f32::from(lift) / 255.0;
    Color::new(
        (color.r + amount).clamp(0.0, 1.0),
        (color.g + amount).clamp(0.0, 1.0),
        (color.b + amount).clamp(0.0, 1.0),
        color.a,
    )
}

fn ground_at(map: &Map, pos: TilePos) -> Option<Elevation> {
    map.tile(pos)
        .filter(|tile| tile.terrain == Terrain::Ground)
        .and(map.elevation(pos))
}

/// Connected ramp tiles as rectangles, and each ramp tile's ramp.
fn find_ramps(map: &Map) -> (Vec<Ramp>, Vec<usize>) {
    let (width, height) = (map.width(), map.height());
    let mut ramp_of = vec![usize::MAX; as_index(width * height)];
    let mut ramps = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let Some(Elevation::Ramp { low }) = map.elevation(TilePos::new(x, y)) else {
                continue;
            };
            if ramp_of[as_index(y * width + x)] != usize::MAX {
                continue;
            }
            let mut stack = vec![(x, y)];
            let (mut x0, mut y0, mut x1, mut y1) = (x, y, x, y);
            ramp_of[as_index(y * width + x)] = ramps.len();
            while let Some((cx, cy)) = stack.pop() {
                (x0, y0, x1, y1) = (x0.min(cx), y0.min(cy), x1.max(cx), y1.max(cy));
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (cx + dx, cy + dy);
                    if nx < 0 || ny < 0 || nx >= width || ny >= height {
                        continue;
                    }
                    let ni = as_index(ny * width + nx);
                    if ramp_of[ni] == usize::MAX
                        && matches!(
                            map.elevation(TilePos::new(nx, ny)),
                            Some(Elevation::Ramp { .. })
                        )
                    {
                        ramp_of[ni] = ramps.len();
                        stack.push((nx, ny));
                    }
                }
            }
            // The side whose outside neighbours stand at the upper level is
            // the top; the ramp runs downhill away from it.
            let count = |cells: Vec<(i32, i32)>| {
                cells
                    .into_iter()
                    .filter(|&(cx, cy)| {
                        ground_at(map, TilePos::new(cx, cy)) == Some(Elevation::Level(low + 1))
                    })
                    .count()
            };
            let sides = [
                ((1, 0), count((y0..=y1).map(|cy| (x0 - 1, cy)).collect())),
                ((-1, 0), count((y0..=y1).map(|cy| (x1 + 1, cy)).collect())),
                ((0, 1), count((x0..=x1).map(|cx| (cx, y0 - 1)).collect())),
                ((0, -1), count((x0..=x1).map(|cx| (cx, y1 + 1)).collect())),
            ];
            let dir = sides
                .iter()
                .max_by_key(|(_, n)| *n)
                .map_or((1, 0), |(d, _)| *d);
            ramps.push(Ramp {
                x0,
                y0,
                x1,
                y1,
                dir,
                low,
            });
        }
    }
    (ramps, ramp_of)
}

/// The level of the highest ground within [`REACH`] of a cliff at `level`:
/// a cliff at a junction belongs to the tallest drop beside it.
fn upper_of(map: &Map, pos: TilePos, level: u8) -> u8 {
    (-REACH..=REACH)
        .flat_map(|dy| (-REACH..=REACH).map(move |dx| pos.offset(dx, dy)))
        .filter_map(|at| ground_at(map, at).map(|e| e.span().1))
        .filter(|&high| high > level)
        .max()
        .unwrap_or_else(|| level.saturating_add(1))
}

/// Each tile's class as drawn, and which cliff tiles show the ground at their
/// foot.
fn classes(map: &Map, ramp_of: &[usize]) -> (Vec<Class>, Vec<bool>) {
    let (width, height) = (map.width(), map.height());
    let data: Vec<Class> = (0..height)
        .flat_map(|y| (0..width).map(move |x| TilePos::new(x, y)))
        .map(|pos| {
            let terrain = map.tile(pos).map(|tile| tile.terrain);
            match (terrain, map.elevation(pos)) {
                (Some(Terrain::Ground), Some(Elevation::Ramp { .. })) => {
                    Class::Ramp(ramp_of[as_index(pos.y * width + pos.x)])
                }
                (Some(Terrain::Ground), Some(Elevation::Level(level))) => Class::Open(level),
                (Some(Terrain::Cliff), Some(Elevation::Level(lower))) => Class::Band {
                    upper: upper_of(map, pos, lower),
                    lower,
                },
                _ => Class::Other,
            }
        })
        .collect();
    let at = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= width || y >= height {
            Class::Other
        } else {
            data[as_index(y * width + x)]
        }
    };
    let around =
        |x: i32, y: i32| (-1..=1).flat_map(move |dy| (-1..=1).map(move |dx| at(x + dx, y + dy)));
    let mut class = data.clone();
    let mut foot = vec![false; data.len()];
    for y in 0..height {
        for x in 0..width {
            let i = as_index(y * width + x);
            match data[i] {
                // The upper ground's last tile carries the step.
                Class::Open(level) => {
                    if let Some(band) = around(x, y)
                        .find(|c| matches!(c, Class::Band { upper, .. } if *upper == level))
                    {
                        class[i] = band;
                    }
                }
                // A cliff tile beside lower ground shows that ground.
                Class::Band { upper, .. } => {
                    let below = around(x, y)
                        .filter_map(|c| match c {
                            Class::Open(level) if level < upper => Some(level),
                            _ => None,
                        })
                        .max();
                    if let Some(level) = below {
                        class[i] = Class::Open(level);
                        foot[i] = true;
                    }
                }
                Class::Ramp(_) | Class::Other => {}
            }
        }
    }
    (class, foot)
}

impl Levels {
    /// The layout of `map`'s levels with `floor` at flat ground's brightness;
    /// `None` on a flat map.
    pub(crate) fn build(map: &Map, floor: u8) -> Option<Self> {
        if !map.tiered() {
            return None;
        }
        let (ramps, ramp_of) = find_ramps(map);
        let (class, foot) = classes(map, &ramp_of);
        let mut levels = Self {
            width: map.width(),
            height: map.height(),
            shading: Shading::new(map, floor),
            class,
            ramps,
            ops: Vec::new(),
            starts: Vec::new(),
            decals: Vec::new(),
        };
        let (ops, decals) = levels.paint(&foot);
        let tiles = as_index(levels.width * levels.height);
        let mut starts = vec![0u32; tiles + 1];
        for &(tile, _) in &ops {
            starts[tile + 1] += 1;
        }
        for i in 0..tiles {
            starts[i + 1] += starts[i];
        }
        let mut sorted = ops;
        // A stable sort keeps each tile's fills in draw order.
        sorted.sort_by_key(|&(tile, _)| tile);
        levels.ops = sorted.into_iter().map(|(_, op)| op).collect();
        levels.starts = starts;
        levels.decals = decals;
        Some(levels)
    }

    fn tone(&self, level: f32) -> f32 {
        self.shading.tone(level)
    }

    fn level_flat(&self, level: f32) -> Color {
        let v = numeric::to_u8((FLOOR_SHADE * self.tone(level)).round());
        rgba(v, v, v.saturating_add(4))
    }

    fn class_at(&self, x: i32, y: i32) -> Class {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            Class::Other
        } else {
            self.class[as_index(y * self.width + x)]
        }
    }

    fn cell_point(cx: i32, cy: i32) -> Vec2 {
        vec2(
            (cx as f32 + 0.5) / CELLS as f32,
            (cy as f32 + 0.5) / CELLS as f32,
        )
    }

    /// The tone of a tile's ground as drawn.
    fn tile_tone(&self, pos: TilePos) -> Option<f32> {
        match self.class_at(pos.x, pos.y) {
            Class::Open(level) => Some(self.tone(f32::from(level))),
            Class::Band { upper, .. } => Some(self.tone(f32::from(upper))),
            Class::Ramp(i) => {
                let center = vec2(pos.x as f32 + 0.5, pos.y as f32 + 0.5);
                Some(self.tone(self.ramps[i].height(center)))
            }
            Class::Other => None,
        }
    }

    /// The tone the minimap gives a tile: its level's, a ramp's halfway
    /// between, and a cliff its foot's.
    pub(crate) fn minimap_tone(&self, map: &Map, pos: TilePos) -> Option<f32> {
        match map.elevation(pos)? {
            Elevation::Level(level) => Some(self.tone(f32::from(level))),
            Elevation::Ramp { low } => Some(self.tone(f32::from(low) + 0.5)),
        }
    }

    /// How a ground tile's sprite is drawn: a multiplier of at most one for
    /// its tint, and a white wash for any brightening past that.
    pub(crate) fn tile_paint(&self, pos: TilePos, variant: usize) -> Option<(f32, f32)> {
        let tone = self.tile_tone(pos)?;
        let mean = SHADE_MEAN[variant % SHADE_MEAN.len()];
        let f = tone * (SHADE_MEDIAN / mean).sqrt();
        if f <= 1.0 {
            Some((f, 0.0))
        } else {
            let mean = mean / 255.0;
            Some((1.0, (mean * (f - 1.0) / (1.0 - mean)).min(1.0)))
        }
    }

    /// Each cell's class as drawn: the cliff's face comes a cell forward into
    /// the cliff tile, and a ramp's walls and the U they make at its mouth a
    /// cell into the ramp.
    fn cell_classes(&self, foot: &[bool]) -> Vec<Class> {
        let (cw, ch) = (self.width * CELLS, self.height * CELLS);
        let base = |cx: i32, cy: i32| self.class_at(cx.div_euclid(CELLS), cy.div_euclid(CELLS));
        let inside = |cx: i32, cy: i32| cx >= 0 && cy >= 0 && cx < cw && cy < ch;
        let mut cells: Vec<Class> = (0..ch)
            .flat_map(|cy| (0..cw).map(move |cx| (cx, cy)))
            .map(|(cx, cy)| base(cx, cy))
            .collect();
        for cy in 0..ch {
            for cx in 0..cw {
                let tile = as_index(cy.div_euclid(CELLS) * self.width + cx.div_euclid(CELLS));
                if !foot[tile] {
                    continue;
                }
                let band = (-1..=1)
                    .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
                    .filter(|&(dx, dy)| inside(cx + dx, cy + dy))
                    .map(|(dx, dy)| base(cx + dx, cy + dy))
                    .find(|c| matches!(c, Class::Band { .. }));
                if let Some(band) = band {
                    cells[as_index(cy * cw + cx)] = band;
                }
            }
        }
        // A ramp's outermost cells along each side take the class of what
        // stands beside them, so its walls and their corners all move a cell
        // inward together.
        let mut moved = Vec::new();
        for cy in 0..ch {
            for cx in 0..cw {
                let Class::Ramp(ri) = base(cx, cy) else {
                    continue;
                };
                let sides = if self.ramps[ri].along_x() {
                    [(0, -1), (0, 1)]
                } else {
                    [(-1, 0), (1, 0)]
                };
                let beside = sides
                    .into_iter()
                    .filter(|&(dx, dy)| inside(cx + dx, cy + dy))
                    .map(|(dx, dy)| cells[as_index((cy + dy) * cw + cx + dx)])
                    .find(|c| matches!(c, Class::Band { .. } | Class::Open(_)));
                if let Some(class) = beside {
                    moved.push((as_index(cy * cw + cx), class));
                }
            }
        }
        for (i, class) in moved {
            cells[i] = class;
        }
        cells
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one pass per layer of the art, in draw order"
    )]
    fn paint(&self, foot: &[bool]) -> (Vec<(usize, Op)>, Vec<Decal>) {
        let (cw, ch) = (self.width * CELLS, self.height * CELLS);
        let cells = self.cell_classes(foot);
        let inside = |cx: i32, cy: i32| cx >= 0 && cy >= 0 && cx < cw && cy < ch;
        let class_c = |cx: i32, cy: i32| {
            if inside(cx, cy) {
                cells[as_index(cy * cw + cx)]
            } else {
                Class::Other
            }
        };
        // A moved ramp side keeps its tile's ramp underneath.
        let ramp_tile =
            |cx: i32, cy: i32| match self.class_at(cx.div_euclid(CELLS), cy.div_euclid(CELLS)) {
                Class::Ramp(ri) => Some(ri),
                _ => None,
            };
        let surface = |cx: i32, cy: i32| -> Option<f32> {
            match class_c(cx, cy) {
                Class::Open(level) => Some(f32::from(level)),
                Class::Ramp(i) => Some(self.ramps[i].height(Self::cell_point(cx, cy))),
                Class::Band { .. } | Class::Other => None,
            }
        };
        let n = as_index(cw * ch);
        let mut riser = vec![false; n];
        let mut rank = vec![0i32; n];
        let mut bench_level = vec![0f32; n];
        let mut height: Vec<f32> = (0..ch)
            .flat_map(|cy| (0..cw).map(move |cx| (cx, cy)))
            .map(|(cx, cy)| surface(cx, cy).unwrap_or(f32::NAN))
            .collect();
        // Each distinct step, from its upper level down to its lower, climbs
        // from its foot in one terrace step per level it spans, as far as
        // its width allows.
        let mut pairs: Vec<(u8, u8)> = self
            .class
            .iter()
            .filter_map(|c| match c {
                Class::Band { upper, lower } => Some((*upper, *lower)),
                _ => None,
            })
            .collect();
        pairs.sort_unstable();
        pairs.dedup();
        // Each step's cells, bounded, so it reads only the ground around it.
        let mut bounds = vec![(cw, ch, -1, -1); pairs.len()];
        for cy in 0..ch {
            for cx in 0..cw {
                let Class::Band { upper, lower } = class_c(cx, cy) else {
                    continue;
                };
                if let Ok(p) = pairs.binary_search(&(upper, lower)) {
                    let b = &mut bounds[p];
                    *b = (b.0.min(cx), b.1.min(cy), b.2.max(cx), b.3.max(cy));
                }
            }
        }
        for (&(upper, lower), &(bx0, by0, bx1, by1)) in pairs.iter().zip(&bounds) {
            let (wx0, wy0) = ((bx0 - WINDOW).max(0), (by0 - WINDOW).max(0));
            let (wx1, wy1) = ((bx1 + WINDOW).min(cw - 1), (by1 + WINDOW).min(ch - 1));
            let ww = wx1 - wx0 + 1;
            let top = f32::from(upper) - 0.25;
            let field = |seed: &dyn Fn(f32) -> bool| {
                distance(ww, wy1 - wy0 + 1, |x, y| {
                    surface(wx0 + x, wy0 + y).is_some_and(seed)
                })
            };
            let up = field(&|h| h >= top);
            let low = field(&|h| h < top);
            let span = upper - lower;
            for cy in by0..=by1 {
                for cx in bx0..=bx1 {
                    if class_c(cx, cy) != (Class::Band { upper, lower }) {
                        continue;
                    }
                    let i = as_index(cy * cw + cx);
                    let w = as_index((cy - wy0) * ww + cx - wx0);
                    let (du, dl) = (i32::from(up[w]), i32::from(low[w]));
                    let thickness = (du + dl - 1).max(1);
                    let steps = i32::from(span).min((thickness / 2).max(1));
                    let segment = thickness as f32 / steps as f32;
                    let from_foot = dl - 1;
                    let k =
                        numeric::to_i32((from_foot as f32 / segment).floor()).clamp(0, steps - 1);
                    let within = from_foot as f32 - k as f32 * segment;
                    let per_step = f32::from(span) / steps as f32;
                    riser[i] = within < 1.0;
                    rank[i] = dl;
                    bench_level[i] = f32::from(lower) + per_step * (k + 1) as f32;
                    height[i] = bench_level[i];
                }
            }
        }
        let riser_color = LAYERS[0].riser;
        let lip_color = LAYERS[0].lip;
        let mut ops: Vec<(usize, Op)> = Vec::new();
        let mut push = |cx: i32, cy: i32, (x, y, w, h): (f32, f32, f32, f32), color: Color| {
            let tile = as_index(cy.div_euclid(CELLS) * self.width + cx.div_euclid(CELLS));
            ops.push((
                tile,
                Op {
                    sx: cx.rem_euclid(CELLS).fit(),
                    sy: cy.rem_euclid(CELLS).fit(),
                    x,
                    y,
                    w,
                    h,
                    color,
                },
            ));
        };
        let whole = (0.0, 0.0, 1.0, 1.0);
        let side = |(dx, dy): (i32, i32), t: f32| match (dx, dy) {
            (-1, 0) => (0.0, 0.0, t, 1.0),
            (1, 0) => (1.0 - t, 0.0, t, 1.0),
            (0, -1) => (0.0, 0.0, 1.0, t),
            _ => (0.0, 1.0 - t, 1.0, t),
        };
        // Step fills: risers dark, benches at their level's grey in the
        // ground's diagonal waves.
        for cy in 0..ch {
            for cx in 0..cw {
                if !matches!(class_c(cx, cy), Class::Band { .. }) {
                    continue;
                }
                let i = as_index(cy * cw + cx);
                let phase = as_index((cx.div_euclid(CELLS) - cy.div_euclid(CELLS)).rem_euclid(6));
                let color = if riser[i] {
                    riser_color
                } else {
                    lifted(self.level_flat(bench_level[i]), WAVE_LIFTS[phase])
                };
                push(cx, cy, whole, color);
            }
        }
        // Ramp surfaces: graded between their levels' tones, lit by the
        // slope's facing.
        for cy in 0..ch {
            for cx in 0..cw {
                let Class::Ramp(ri) = class_c(cx, cy) else {
                    continue;
                };
                let r = self.ramps[ri];
                let tile = TilePos::new(cx.div_euclid(CELLS), cy.div_euclid(CELLS));
                let tile_tone = self.tile_tone(tile).unwrap_or(1.0);
                let p = Self::cell_point(cx, cy);
                let facing = vec2(r.dir.0 as f32, r.dir.1 as f32).dot(LIGHT);
                let slope = (std::f32::consts::PI * r.graded(p)).sin().max(0.0).sqrt();
                let want = self.tone(r.height(p)) * (1.0 + 0.16 * facing * slope);
                let ratio = want / tile_tone;
                let base = 0.145 * tile_tone;
                let color = if ratio < 1.0 {
                    Color::new(0.0, 0.0, 0.0, 1.0 - ratio)
                } else {
                    Color::new(
                        1.0,
                        1.0,
                        1.0,
                        (base * (ratio - 1.0) / (1.0 - base)).min(1.0),
                    )
                };
                push(cx, cy, whole, color);
            }
        }
        // Ramp walls: where a ramp runs below the step beside it, the step's
        // edge stands as a wall; where it runs above the ground beside it, a
        // wall stands on that ground.
        let mut stubs: Vec<(i32, i32)> = Vec::new();
        for cy in 0..ch {
            for cx in 0..cw {
                if !matches!(class_c(cx, cy), Class::Ramp(_)) {
                    continue;
                }
                let hr = height[as_index(cy * cw + cx)];
                for d in SIDES {
                    let (nx, ny) = (cx + d.0, cy + d.1);
                    if !inside(nx, ny) {
                        continue;
                    }
                    let ni = as_index(ny * cw + nx);
                    match class_c(nx, ny) {
                        Class::Band { .. } if hr < height[ni] - 0.25 => {
                            push(nx, ny, whole, riser_color);
                            push(nx, ny, side(d, LIP), lip_color);
                            riser[ni] = true;
                        }
                        Class::Open(level) if f32::from(level) > hr + 0.25 => {
                            push(cx, cy, whole, riser_color);
                            push(cx, cy, side(d, LIP), lip_color);
                        }
                        Class::Open(level) if hr > f32::from(level) + 0.25 => {
                            push(nx, ny, whole, riser_color);
                            push(nx, ny, side((-d.0, -d.1), LIP), lip_color);
                            stubs.push((nx, ny));
                        }
                        _ => {}
                    }
                }
            }
        }
        // Haul-road ruts: two lanes down each ramp, broken here and there.
        for cy in 0..ch {
            for cx in 0..cw {
                let Class::Ramp(ri) = class_c(cx, cy) else {
                    continue;
                };
                let r = self.ramps[ri];
                if hash(cx, cy, 911).is_multiple_of(9) {
                    continue;
                }
                let quarter = 1.0 / CELLS as f32;
                let near = if r.along_x() {
                    cy as f32 * quarter - r.y0 as f32
                } else {
                    cx as f32 * quarter - r.x0 as f32
                };
                let w = r.width();
                for lane in [w * 0.3, w * 0.7] {
                    let off = (lane - near) / quarter;
                    if !(0.0..1.0).contains(&off) {
                        continue;
                    }
                    let t = 0.36;
                    let at = (off - t * 0.5).clamp(0.0, 1.0 - t);
                    let rect = if r.along_x() {
                        (0.0, at, 1.0, t)
                    } else {
                        (at, 0.0, t, 1.0)
                    };
                    push(cx, cy, rect, Color::new(0.0, 0.0, 0.0, 0.38));
                }
            }
        }
        // Lips: every riser cell crowns its higher side.
        for cy in 0..ch {
            for cx in 0..cw {
                let i = as_index(cy * cw + cx);
                let Class::Band { upper, .. } = class_c(cx, cy) else {
                    continue;
                };
                if !riser[i] {
                    continue;
                }
                for d in SIDES {
                    let (nx, ny) = (cx + d.0, cy + d.1);
                    if !inside(nx, ny) {
                        continue;
                    }
                    let ni = as_index(ny * cw + nx);
                    let higher = match class_c(nx, ny) {
                        Class::Band { upper: u, .. } if u == upper => {
                            !riser[ni] && rank[ni] > rank[i]
                        }
                        Class::Band { .. } | Class::Other => false,
                        Class::Open(_) | Class::Ramp(_) => height[ni] >= bench_level[i] - 0.25,
                    };
                    if higher {
                        push(cx, cy, side(d, LIP), lip_color);
                    }
                }
            }
        }
        // A wall standing on the ground beside a ramp's foot meets the
        // cliff's face with a lip, so wall and face read as one outline.
        for &(cx, cy) in &stubs {
            for d in SIDES {
                if matches!(class_c(cx + d.0, cy + d.1), Class::Band { .. }) {
                    push(cx, cy, side(d, LIP), lip_color);
                }
            }
        }
        // The foot of the U: where a ramp's side, moved a cell inward, meets
        // the lower ground under the cliff's face, its wall goes a cell
        // further, lit on the ramp's side and along the face.
        for cy in 0..ch {
            for cx in 0..cw {
                let Some(ri) = ramp_tile(cx, cy) else {
                    continue;
                };
                let Class::Open(level) = class_c(cx, cy) else {
                    continue;
                };
                let (along, across) = if self.ramps[ri].along_x() {
                    ([(1, 0), (-1, 0)], [(0, -1), (0, 1)])
                } else {
                    ([(0, 1), (0, -1)], [(-1, 0), (1, 0)])
                };
                let face = along.into_iter().find(|d| {
                    matches!(class_c(cx + d.0, cy + d.1), Class::Band { upper, .. } if upper > level)
                });
                let Some(face) = face else {
                    continue;
                };
                push(cx, cy, whole, riser_color);
                push(cx, cy, side(face, LIP), lip_color);
                for d in across {
                    if matches!(class_c(cx + d.0, cy + d.1), Class::Ramp(_)) {
                        push(cx, cy, side(d, LIP), lip_color);
                    }
                }
            }
        }
        // Wear on steps wide enough to show it.
        let mut decals = Vec::new();
        for ty in 0..self.height {
            for tx in 0..self.width {
                if !matches!(self.class_at(tx, ty), Class::Band { .. }) {
                    continue;
                }
                let i = |sx: i32, sy: i32| as_index((ty * CELLS + sy) * cw + tx * CELLS + sx);
                let bench = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().all(|&(sx, sy)| {
                    matches!(
                        class_c(tx * CELLS + sx, ty * CELLS + sy),
                        Class::Band { .. }
                    ) && !riser[i(sx, sy)]
                });
                let value = hash(tx, ty, 850);
                if bench && value.is_multiple_of(5) {
                    decals.push((
                        as_index(ty * self.width + tx),
                        self.level_flat(bench_level[i(0, 0)]),
                        value,
                    ));
                }
            }
        }
        (ops, decals)
    }

    /// Draws one tile's steps, ramp, and wear: over its ground, under any
    /// salvage or frame on it.
    pub(crate) fn draw_tile(&self, game: &Scene<'_>, pos: TilePos) {
        if pos.x < 0 || pos.y < 0 || pos.x >= self.width || pos.y >= self.height {
            return;
        }
        let tile = as_index(pos.y * self.width + pos.x);
        let ops = &self.ops[self.starts[tile].fit::<usize>()..self.starts[tile + 1].fit::<usize>()];
        if !ops.is_empty() {
            let (xs, ys) = cell_edges(game, pos);
            for op in ops {
                let (sx, sy) = (usize::from(op.sx), usize::from(op.sy));
                let (w, h) = (xs[sx + 1] - xs[sx], ys[sy + 1] - ys[sy]);
                draw_rectangle(
                    xs[sx] + (w * op.x).round(),
                    ys[sy] + (h * op.y).round(),
                    (w * op.w).round().max(1.0),
                    (h * op.h).round().max(1.0),
                    op.color,
                );
            }
        }
        if let Ok(found) = self.decals.binary_search_by_key(&tile, |decal| decal.0) {
            let (_, top, value) = self.decals[found];
            let camera = &game.presentation.camera;
            let zoom = camera.zoom;
            let cell = zoom / CELLS as f32;
            let a = camera.to_screen(vec2(pos.x as f32, pos.y as f32));
            let rect = Rect::new(a.x.floor(), a.y.floor(), cell, cell);
            if value % 3 == 0 {
                draw_missing_slab(rect, cell, top, value);
            } else {
                draw_fracture(rect, cell, zoom, top, value);
            }
        }
    }
}

#[cfg(test)]
mod tests;
