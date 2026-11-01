//! Quarry terraces: the boundary wall and the peaks standing inside it, as
//! one terrace system. Collapsed, dark industrial benches rise away from the
//! battlefield floor; a peak is the same wall left standing in the field, so
//! a peak meeting the map edge is a spur whose every level joins the
//! boundary's matching level.
//!
//! Depth is a chessboard distance in quarter-tile cells from the open floor,
//! measured across the wall and the peaks together. Each level is a dark
//! riser crowned by a lit lip and then a bench, with the boundary's widths
//! jogging along each side. Upper levels follow opened plateaus, so a level
//! never traces a thin arm of an outline as a ring or a letter. Off the
//! playfield the last bench fades into the dark; inside it, a peak's last
//! bench runs on as its plateau.
//!
//! A freestanding peak steps its own way rather than borrowing the wall's
//! widths: each side is cut into short stretches whose first two benches
//! vary, a corner's stretch wrapping both its sides so steps never seam on a
//! diagonal. A peak too thin to step instead stands at one of two heights
//! per stretch. Peaks that mirror each other under the map's half-turn
//! step as mirror images; their wear marks fall independently.
//!
//! The field reads the map alone, so a terrace never shifts as the fog lifts;
//! the fog veil hides peaks the player has not explored.

use super::cell_field::{CELLS, CellField, cell_edges};
use super::environment::{
    BoundaryInsets, LAYERS, Layer, Material, SALT, WAVE_LIFTS, draw_fracture, draw_lip,
    draw_missing_slab, draw_rebar, draw_strata, hash, mixed, quarry_color, shifted,
};
use super::ground::GroundView;
use super::stretches::Shapes;
use crate::numeric;
use crate::numeric::Fit;
use aftermine_sim::map::{Map, Terrain};
use chassis::grid::TilePos;
use chassis::grid::as_index;
use macroquad::prelude::*;

/// Tiles of field beyond the map edge: past the vignette's darkest step.
pub(super) const MARGIN: i32 = 8;
/// Upper plateaus narrower than about twice this many cells are dropped.
const PLATEAU_RADIUS: u8 = 3;
/// Cells the last off-playfield bench fades over before the void.
const VIGNETTE_CELLS: i32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

/// The playfield: the map less the fully blocked edge rows and columns the
/// boundary draws over, in tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame {
    x0: i32,
    y0: i32,
    width: i32,
    height: i32,
    /// Bench widths per side and sixth of a side, then the corners' widths.
    jogs: [[i32; 3]; 25],
}

impl Frame {
    fn from_rows(rows: &[String]) -> Self {
        let insets = BoundaryInsets::from_rows(rows);
        let map_width = rows.first().map_or(0, String::len).fit::<i32>();
        let map_height = rows.len().fit::<i32>();
        let (left, top) = (i32::from(insets.left), i32::from(insets.top));
        Self {
            x0: left,
            y0: top,
            width: map_width - left - i32::from(insets.right),
            height: map_height - top - i32::from(insets.bottom),
            jogs: std::array::from_fn(|slot| {
                let segment = (slot < 24).then(|| {
                    let side = [Side::Top, Side::Right, Side::Bottom, Side::Left][slot / 6];
                    (side, (slot % 6).fit::<i32>())
                });
                jogged(segment)
            }),
        }
    }

    fn contains(&self, pos: TilePos) -> bool {
        pos.x >= self.x0
            && pos.y >= self.y0
            && pos.x < self.x0 + self.width
            && pos.y < self.y0 + self.height
    }

    /// A cell relative to the playfield's top-left cell.
    fn inner(&self, cx: i32, cy: i32) -> (i32, i32) {
        (cx - self.x0 * CELLS, cy - self.y0 * CELLS)
    }

    /// The side and sixth of it a cell's benches jog with. Playfield cells
    /// take their nearest side, so a spur's benches match the wall's.
    fn segment(&self, cx: i32, cy: i32) -> Option<(Side, i32)> {
        let (mut ix, mut iy) = self.inner(cx, cy);
        let (w, h) = (self.width * CELLS, self.height * CELLS);
        if (0..w).contains(&ix) && (0..h).contains(&iy) {
            let nearest = [(iy + 1, 0), (w - ix, 1), (h - iy, 2), (ix + 1, 3)]
                .into_iter()
                .min()
                .map_or(0, |(_, side)| side);
            match nearest {
                0 => iy = -1,
                1 => ix = w,
                2 => iy = h,
                _ => ix = -1,
            }
        }
        let (side, along, length) = if iy < 0 && (0..w).contains(&ix) {
            (Side::Top, ix, w)
        } else if ix >= w && (0..h).contains(&iy) {
            (Side::Right, iy, h)
        } else if iy >= h && (0..w).contains(&ix) {
            (Side::Bottom, w - 1 - ix, w)
        } else if ix < 0 && (0..h).contains(&iy) {
            (Side::Left, h - 1 - iy, h)
        } else {
            return None;
        };
        Some((side, (along * 6 / length.max(1)).clamp(0, 5)))
    }

    /// Bench widths per level at a cell.
    fn widths(&self, cx: i32, cy: i32) -> [i32; 3] {
        let slot = self
            .segment(cx, cy)
            .map_or(24, |(side, segment)| as_index(side as i32 * 6 + segment));
        self.jogs[slot]
    }
}

/// The boundary's bench widths for one side segment, jogging one cell
/// either way; corners keep the base widths.
fn jogged(segment: Option<(Side, i32)>) -> [i32; 3] {
    std::array::from_fn(|layer| {
        let Some((side, segment)) = segment else {
            return LAYERS[layer].bench_cells;
        };
        let token = hash(
            segment + layer.fit::<i32>() * 11,
            side as i32 + layer.fit::<i32>() * 7,
            SALT,
        ) % 11;
        (LAYERS[layer].bench_cells
            + match token {
                0..=3 => -1,
                4 => 1,
                _ => 0,
            })
        .clamp(3, 5)
    })
}

/// Depths of the second and third risers for the given bench widths.
fn risers(widths: [i32; 3]) -> (u8, u8) {
    let second = 1 + widths[0] + 1;
    let third = second + widths[1] + 1;
    (second.fit::<u8>(), third.fit::<u8>())
}

/// Material at a remapped depth. Off the playfield the last bench fades into
/// the dark; on it the last bench runs on as a plateau.
fn material(depth: u8, widths: [i32; 3], off_playfield: bool) -> Option<Material> {
    if depth == 0 {
        return None;
    }
    let depth = i32::from(depth);
    let mut cursor = 0;
    for (layer, width) in widths.iter().enumerate() {
        cursor += 1;
        if depth <= cursor {
            return Some(Material::Riser(layer));
        }
        cursor += width;
        if depth <= cursor {
            return Some(Material::Bench(layer));
        }
    }
    Some(if !off_playfield {
        Material::Bench(widths.len() - 1)
    } else if depth <= cursor + VIGNETTE_CELLS {
        Material::Vignette((depth - cursor).fit::<u8>())
    } else {
        Material::Void
    })
}

fn color(material: Material, ix: i32, iy: i32, layers: &[Layer; LAYERS.len()]) -> Option<Color> {
    match material {
        Material::Riser(level) => Some(layers[level].riser),
        Material::Bench(level) => {
            let phase = (ix.div_euclid(CELLS) - iy.div_euclid(CELLS)).rem_euclid(6);
            Some(shifted(layers[level].top, WAVE_LIFTS[as_index(phase)]))
        }
        Material::Vignette(step) => Some(mixed(
            layers[LAYERS.len() - 1].top,
            super::FOG_UNEXPLORED,
            (0.25 + f32::from(step) / 4.0 * 0.75).min(1.0),
        )),
        Material::Void => None,
    }
}

/// The world cell of a field index.
fn world_cell(field: &CellField, index: usize) -> (i32, i32) {
    let row = field.width * field.cells;
    let index = index.fit::<i32>();
    (
        field.origin.x * field.cells + index % row,
        field.origin.y * field.cells + index / row,
    )
}

/// Rewrites a field's depth so each upper level follows an opened plateau
/// rather than the outline itself.
fn terrace(field: &mut CellField, frame: &Frame, peaks: &Peaks) {
    let thresholds: Vec<(u8, u8)> = (0..field.depth.len())
        .map(|index| {
            let (cx, cy) = world_cell(field, index);
            risers(
                peaks
                    .steps(cx, cy)
                    .map_or_else(|| frame.widths(cx, cy), |steps| steps.widths),
            )
        })
        .collect();
    let core1 = opened(field, |index, depth| depth >= thresholds[index].0, None);
    let core2 = opened(
        field,
        |index, depth| depth >= thresholds[index].1,
        Some(&core1),
    );
    let d1 = field.masked(|index| core1[index]);
    let d2 = field.masked(|index| core2[index]);
    for (index, depth) in field.depth.iter_mut().enumerate() {
        if *depth == 0 {
            continue;
        }
        let (second, third) = thresholds[index];
        *depth = if core2[index] {
            third.saturating_add(d2.depth[index] - 1)
        } else if core1[index] {
            second.saturating_add(d1.depth[index] - 1).min(third - 1)
        } else {
            (*depth).min(second - 1)
        };
    }
}

/// Morphological opening of the cells where `keep` holds, clipped to
/// `within`: erode by [`PLATEAU_RADIUS`], then grow back by the same.
fn opened(
    field: &CellField,
    keep: impl Fn(usize, u8) -> bool,
    within: Option<&[bool]>,
) -> Vec<bool> {
    let inside = |index: usize| within.is_none_or(|mask| mask[index]);
    let raw = field.masked(|index| keep(index, field.depth[index]) && inside(index));
    let reach = field.masked(|index| raw.depth[index] <= PLATEAU_RADIUS);
    (0..field.depth.len())
        .map(|index| reach.depth[index] <= PLATEAU_RADIUS && inside(index))
        .collect()
}

/// Whether a tile is wall: everything off the playfield, and every peak.
fn wall(on_playfield: bool, terrain: Option<Terrain>) -> bool {
    !on_playfield || terrain == Some(Terrain::Peak)
}

/// How one cell of a freestanding peak steps: its bench widths, and how many
/// levels a thin peak's stretch stands raised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Steps {
    widths: [i32; 3],
    lift: usize,
}

/// The freestanding peaks. Spurs that meet the wall are left out and keep
/// the wall's widths, so their levels join it.
#[derive(Debug)]
struct Peaks(Shapes);

impl Peaks {
    fn find(map: &Map, frame: &Frame) -> Self {
        Self(Shapes::find(
            map.width(),
            map.height(),
            |pos| {
                frame.contains(pos)
                    && map
                        .tile(pos)
                        .is_some_and(|tile| tile.terrain == Terrain::Peak)
            },
            |pos| frame.contains(pos),
            SALT + 313,
        ))
    }

    /// How a cell of a freestanding peak steps; `None` elsewhere.
    fn steps(&self, cx: i32, cy: i32) -> Option<Steps> {
        let place = self.0.place(cx, cy)?;
        if place.thin {
            // Too thin to step: each stretch of its length stands at one of
            // two heights instead.
            let height = hash(place.seed, place.length, SALT + 314) % 100;
            return Some(Steps {
                widths: [2, 2, 2],
                lift: usize::from(height >= 45),
            });
        }
        let first = hash(place.seed, place.stretch, SALT + 314) % 100;
        // The second bench jogs on the offset stretches, so the two levels
        // seldom jog together.
        let second = hash(place.seed, place.offset, SALT + 315) % 10;
        let own = hash(place.seed, 0, SALT + 316);
        let w0 = match first {
            0..25 => 1,
            25..50 => 2,
            50..75 => 3,
            75..90 => 4,
            _ => 5,
        };
        let w1 = (2
            + (own % 3).fit::<i32>()
            + match second {
                0..3 => -1,
                3..7 => 0,
                _ => 1,
            })
        .clamp(1, 5);
        let w2 = 3 + ((own >> 4) % 2).fit::<i32>();
        Some(Steps {
            widths: [w0, w1, w2],
            lift: 0,
        })
    }

    fn owns(&self, pos: TilePos) -> bool {
        self.0.owns(pos)
    }
}

/// The wall and the peaks as one terraced field over the map and its margin.
pub(super) struct Terraces {
    frame: Frame,
    field: CellField,
    peaks: Peaks,
}

impl Terraces {
    pub(super) fn build(map: &Map, rows: &[String]) -> Self {
        let frame = Frame::from_rows(rows);
        let peaks = Peaks::find(map, &frame);
        let mut field = CellField::seeded(
            TilePos::new(-MARGIN, -MARGIN),
            map.width() + 2 * MARGIN,
            map.height() + 2 * MARGIN,
            CELLS,
            |pos| wall(frame.contains(pos), map.tile(pos).map(|tile| tile.terrain)),
        );
        terrace(&mut field, &frame, &peaks);
        Self {
            frame,
            field,
            peaks,
        }
    }

    /// The material drawn at a cell.
    fn material(&self, cx: i32, cy: i32) -> Option<Material> {
        let pos = TilePos::new(cx.div_euclid(CELLS), cy.div_euclid(CELLS));
        let steps = self.peaks.steps(cx, cy);
        let found = material(
            self.field
                .at(pos, cx.rem_euclid(CELLS), cy.rem_euclid(CELLS)),
            steps.map_or_else(|| self.frame.widths(cx, cy), |steps| steps.widths),
            !self.frame.contains(pos),
        );
        let lift = steps.map_or(0, |steps| steps.lift);
        let top = LAYERS.len() - 1;
        match found {
            Some(Material::Riser(level)) => Some(Material::Riser((level + lift).min(top))),
            Some(Material::Bench(level)) => Some(Material::Bench((level + lift).min(top))),
            other => other,
        }
    }
}

/// Tiles in view, reaching past the map edge as far as the boundary draws.
fn screen_tiles(map: &Map, view: &GroundView) -> (TilePos, TilePos) {
    let (lo, hi) = view.world_rect();
    (
        TilePos::new(
            (numeric::to_i32(lo.x.floor()) - 1).max(-MARGIN),
            (numeric::to_i32(lo.y.floor()) - 1).max(-MARGIN),
        ),
        TilePos::new(
            (numeric::to_i32(hi.x.ceil()) + 1).min(map.width() + MARGIN),
            (numeric::to_i32(hi.y.ceil()) + 1).min(map.height() + MARGIN),
        ),
    )
}

pub(super) fn draw_terraces(
    map: &Map,
    view: &GroundView,
    terraces: &Terraces,
    fractured: bool,
    grade: super::grade::Grade,
) {
    let layers = LAYERS.map(|layer| grade.layer(layer));
    let (min, max) = screen_tiles(map, view);
    let frame = &terraces.frame;
    let material_at = |cx: i32, cy: i32| terraces.material(cx, cy);
    let tiles: Vec<TilePos> = (min.y..max.y)
        .flat_map(|y| (min.x..max.x).map(move |x| TilePos::new(x, y)))
        .filter(|&pos| {
            !frame.contains(pos)
                || map
                    .tile(pos)
                    .is_some_and(|tile| tile.terrain == Terrain::Peak)
        })
        .collect();
    for &pos in &tiles {
        let (xs, ys) = cell_edges(view, pos);
        for sy in 0..CELLS {
            for sx in 0..CELLS {
                let (cx, cy) = (pos.x * CELLS + sx, pos.y * CELLS + sy);
                let Some(m) = material_at(cx, cy) else {
                    continue;
                };
                let (ix, iy) = frame.inner(cx, cy);
                let Some(c) = color(m, ix, iy, &layers) else {
                    continue;
                };
                let (x, y) = (as_index(sx), as_index(sy));
                draw_rectangle(
                    xs[x],
                    ys[y],
                    xs[x + 1] - xs[x],
                    ys[y + 1] - ys[y],
                    quarry_color(c, m, fractured),
                );
            }
        }
    }
    let zoom = view.zoom();
    let cell = zoom / CELLS as f32;
    let rect_of = |xs: &[f32; 5], ys: &[f32; 5], sx: i32, sy: i32| {
        let (x, y) = (as_index(sx), as_index(sy));
        Rect::new(xs[x], ys[y], xs[x + 1] - xs[x], ys[y + 1] - ys[y])
    };
    // Decals sit on a tile whose first two-by-two cells share one bench.
    for &pos in &tiles {
        let (cx, cy) = (pos.x * CELLS, pos.y * CELLS);
        let Some(Material::Bench(level)) = material_at(cx, cy) else {
            continue;
        };
        let uniform = [(1, 0), (0, 1), (1, 1)]
            .iter()
            .all(|(dx, dy)| material_at(cx + dx, cy + dy) == Some(Material::Bench(level)));
        if !uniform {
            continue;
        }
        let (ix, iy) = frame.inner(cx, cy);
        let value = hash(ix.div_euclid(CELLS), iy.div_euclid(CELLS), SALT + 503);
        let (xs, ys) = cell_edges(view, pos);
        let rect = rect_of(&xs, &ys, 0, 0);
        // A peak in the field is more weathered than the wall.
        let (slab, fracture, rebar) = if terraces.peaks.owns(pos) {
            (value.is_multiple_of(14), value % 5 == 2, value % 13 == 4)
        } else {
            (value.is_multiple_of(41), value % 17 == 3, value % 43 == 13)
        };
        if slab {
            draw_missing_slab(rect, cell, layers[level].top, value);
        } else if fracture {
            draw_fracture(rect, cell, zoom, layers[level].top, value);
        } else if rebar {
            draw_rebar(rect, cell, zoom);
        }
    }
    // A lip crowns each riser along the bench above it; fractured quarries
    // also score the riser face.
    let lip = (zoom * 0.055).max(1.0);
    for &pos in &tiles {
        let (xs, ys) = cell_edges(view, pos);
        for sy in 0..CELLS {
            for sx in 0..CELLS {
                let (cx, cy) = (pos.x * CELLS + sx, pos.y * CELLS + sy);
                let Some(Material::Bench(level)) = material_at(cx, cy) else {
                    continue;
                };
                let rect = rect_of(&xs, &ys, sx, sy);
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    // Where a peak's stretches meet at different heights, the
                    // lower bench ends against a face and the higher is lit.
                    if let Some(Material::Bench(other)) = material_at(cx + dx, cy + dy) {
                        if other > level {
                            draw_lip(rect, (dx, dy), lip * 2.0, layers[other].riser);
                        } else if other < level {
                            draw_lip(rect, (dx, dy), lip, layers[level].lip);
                        }
                    }
                    if material_at(cx + dx, cy + dy) != Some(Material::Riser(level)) {
                        continue;
                    }
                    draw_lip(rect, (dx, dy), lip, layers[level].lip);
                    if fractured {
                        let face = Rect::new(
                            rect.x + dx as f32 * rect.w,
                            rect.y + dy as f32 * rect.h,
                            rect.w,
                            rect.h,
                        );
                        let (ix, iy) = frame.inner(cx + dx, cy + dy);
                        draw_strata(face, (-dx, -dy), layers[level], hash(ix, iy, SALT + 607));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
