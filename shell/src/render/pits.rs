//! Pit drops: the quarry boundary's descending sibling. Benches step down
//! from a lit lip into darkness so a cut reads as a lower level of the same
//! quarry rather than a hole punched through the floor.
//!
//! The terraces are a distance field over pit tiles measured in quarter-tile
//! cells, so a wide cut carries several benches across tile boundaries before
//! it vanishes. A pit clear of the map edge steps its own way, its benches
//! varying by stretch along each side as a peak's do; a pit that runs off
//! the map keeps a jog per block of tiles so it joins the boundary. The field
//! reads the map alone, so a drop never shifts as the fog lifts; the fog veil
//! hides pits the player has not explored.

use super::cell_field::{CELLS, CellField, cell_edges};
use super::environment::{
    Layer, Material, WAVE_LIFTS, draw_fracture, draw_lip, draw_missing_slab, draw_rebar,
    draw_strata, hash, mixed, quarry_color, rgba, shifted,
};
use super::stretches::{Place, Shapes};
use crate::game::Scene;
use crate::numeric::Fit;
use chassis::grid::TilePos;
use chassis::grid::as_index;
use macroquad::prelude::*;
use oxide_sim::map::{Map, Terrain};

const SALT: u32 = 911;
const VIGNETTE_CELLS: i32 = 4;
/// Bench widths vary per block of this many tiles so lips jog along the rim
/// the way the boundary's do per side segment.
const BLOCK_TILES: i32 = 3;
const MAX_BENCH_CELLS: i32 = 3;
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "the drop table is a handful of entries"
)]
const MAX_DEPTH: i32 = DROP.len() as i32 * (1 + MAX_BENCH_CELLS) + VIGNETTE_CELLS;
const VOID: Color = rgba(8, 9, 12);
const GLINT: Color = rgba(22, 24, 34);

/// Steeper than the boundary: half-tile benches between risers, each
/// level darker than the last, so the drop fades to the void within a
/// few tiles of the lip.
const DROP: [Layer; 3] = [
    Layer {
        bench_cells: 2,
        riser: rgba(13, 13, 18),
        top: rgba(27, 27, 32),
        lip: rgba(62, 60, 66),
    },
    Layer {
        bench_cells: 2,
        riser: rgba(10, 10, 14),
        top: rgba(20, 20, 24),
        lip: rgba(44, 43, 48),
    },
    Layer {
        bench_cells: 2,
        riser: rgba(8, 8, 12),
        top: rgba(15, 15, 19),
        lip: rgba(31, 30, 35),
    },
];

/// Bench widths for a cell of a pit clear of the map edge: the rim's first
/// bench varies by stretch and the deeper benches keep one width round the
/// pit, so a narrow cut never notches down its middle. A thin pit varies its
/// first bench only along its length.
fn stretch_widths(place: Place) -> [i32; DROP.len()] {
    let own = hash(place.seed, 0, SALT + 316);
    let deeper = [
        1 + (own % 3).fit::<i32>(),
        2 + ((own >> 4) % 2).fit::<i32>(),
    ];
    let key = if place.thin {
        place.length
    } else {
        place.stretch
    };
    let first = match hash(place.seed, key, SALT + 314) % 10 {
        0..3 => 1,
        3..7 => 2,
        _ => 3,
    };
    [first, deeper[0], deeper[1]]
}

/// Bench width per level for one block: the base width, one cell narrower
/// about a third of the time, rarely one wider.
fn bench_widths(block_x: i32, block_y: i32) -> [i32; DROP.len()] {
    let mut widths = [0; DROP.len()];
    for (level, layer) in DROP.iter().enumerate() {
        let token = hash(
            block_x + level.fit::<i32>() * 11,
            block_y + level.fit::<i32>() * 7,
            SALT,
        ) % 11;
        let shift = match token {
            0..=3 => -1,
            4 => 1,
            _ => 0,
        };
        widths[level] = (layer.bench_cells + shift).clamp(1, MAX_BENCH_CELLS);
    }
    widths
}

fn material(depth: u8, widths: [i32; DROP.len()]) -> Option<Material> {
    if depth == 0 {
        return None;
    }
    let depth = i32::from(depth);
    let mut cursor = 0;
    for (level, width) in widths.iter().enumerate() {
        cursor += 1;
        if depth <= cursor {
            return Some(Material::Riser(level));
        }
        cursor += width;
        if depth <= cursor {
            return Some(Material::Bench(level));
        }
    }
    Some(if depth <= cursor + VIGNETTE_CELLS {
        Material::Vignette((depth - cursor).fit::<u8>())
    } else {
        Material::Void
    })
}

fn cell_color(material: Material, cx: i32, cy: i32) -> Color {
    match material {
        Material::Riser(level) => DROP[level].riser,
        Material::Bench(level) => {
            let phase = (cx.div_euclid(CELLS) - cy.div_euclid(CELLS)).rem_euclid(6) as usize;
            shifted(DROP[level].top, WAVE_LIFTS[phase])
        }
        Material::Vignette(step) => mixed(
            DROP[DROP.len() - 1].top,
            VOID,
            f32::from(step) / (VIGNETTE_CELLS as f32 + 1.0),
        ),
        Material::Void => VOID,
    }
}

/// Chebyshev cell distance from every pit cell to the nearest standing
/// cell, over the map plus enough margin that a pit on the map edge sees its
/// true nearest rim.
pub(super) struct PitField {
    field: CellField,
    pits: Shapes,
}

impl PitField {
    /// Off-map positions are the boundary's highwall, never floor, so the
    /// cut continues into them.
    pub(super) fn build(map: &Map) -> Self {
        let pad = (MAX_DEPTH + CELLS - 1) / CELLS + 1;
        let field = CellField::seeded(
            TilePos::new(-pad, -pad),
            map.width() + 2 * pad,
            map.height() + 2 * pad,
            CELLS,
            |pos| {
                map.tile(pos)
                    .is_none_or(|tile| tile.terrain == Terrain::Pit)
            },
        );
        let pits = Shapes::find(
            map.width(),
            map.height(),
            |pos| {
                map.tile(pos)
                    .is_some_and(|tile| tile.terrain == Terrain::Pit)
            },
            |pos| map.tile(pos).is_some(),
            SALT + 313,
        );
        Self { field, pits }
    }

    fn at(&self, pos: TilePos, sx: i32, sy: i32) -> u8 {
        self.field.at(pos, sx, sy)
    }

    /// Bench widths at a cell: its pit's own stretches, or the per-block
    /// jog for a pit that runs off the map.
    fn widths(&self, cx: i32, cy: i32) -> [i32; DROP.len()] {
        self.pits.place(cx, cy).map_or_else(
            || {
                bench_widths(
                    cx.div_euclid(CELLS).div_euclid(BLOCK_TILES),
                    cy.div_euclid(CELLS).div_euclid(BLOCK_TILES),
                )
            },
            stretch_widths,
        )
    }

    fn material_at(&self, pos: TilePos, sx: i32, sy: i32) -> Option<Material> {
        let (cx, cy) = (pos.x * CELLS + sx, pos.y * CELLS + sy);
        material(self.at(pos, sx, sy), self.widths(cx, cy))
    }
}

fn draw_glints(xs: &[f32; 5], ys: &[f32; 5], pos: TilePos, zoom: f32) {
    let value = hash(pos.x, pos.y, SALT + 17);
    if !value.is_multiple_of(3) {
        return;
    }
    let size = (zoom * 0.045).max(1.0);
    let (w, h) = (xs[4] - xs[0], ys[4] - ys[0]);
    for shift in [8, 20] {
        let gx = ((value >> shift) % 14) as f32 + 1.0;
        let gy = ((value >> (shift + 4)) % 14) as f32 + 1.0;
        draw_rectangle(
            xs[0] + w * gx / 16.0,
            ys[0] + h * gy / 16.0,
            size,
            size,
            GLINT,
        );
    }
}

fn draw_fill(game: &Scene<'_>, field: &PitField, pos: TilePos, zoom: f32, fractured: bool) {
    let (xs, ys) = cell_edges(game, pos);
    let color = |sx: i32, sy: i32| {
        let material = field.material_at(pos, sx, sy).unwrap_or(Material::Void);
        let color = cell_color(material, pos.x * CELLS + sx, pos.y * CELLS + sy);
        quarry_color(color, material, fractured)
    };
    let all_void = (0..CELLS)
        .flat_map(|sy| (0..CELLS).map(move |sx| (sx, sy)))
        .all(|(sx, sy)| field.material_at(pos, sx, sy) == Some(Material::Void));
    if all_void {
        draw_rectangle(xs[0], ys[0], xs[4] - xs[0], ys[4] - ys[0], VOID);
        draw_glints(&xs, &ys, pos, zoom);
        return;
    }
    for sy in 0..CELLS {
        let mut run_start = 0;
        let mut run_color = color(0, sy);
        for sx in 1..=CELLS {
            let next = (sx < CELLS).then(|| color(sx, sy));
            if next == Some(run_color) {
                continue;
            }
            draw_rectangle(
                xs[as_index(run_start)],
                ys[as_index(sy)],
                xs[as_index(sx)] - xs[as_index(run_start)],
                ys[as_index(sy) + 1] - ys[as_index(sy)],
                run_color,
            );
            if let Some(color) = next {
                run_start = sx;
                run_color = color;
            }
        }
    }
}

fn uniform_bench(field: &PitField, pos: TilePos, sx: i32, sy: i32) -> Option<usize> {
    let Some(Material::Bench(level)) = field.material_at(pos, sx, sy) else {
        return None;
    };
    [(1, 0), (0, 1), (1, 1)]
        .iter()
        .all(|(dx, dy)| field.material_at(pos, sx + dx, sy + dy) == Some(Material::Bench(level)))
        .then_some(level)
}

fn draw_relief(game: &Scene<'_>, field: &PitField, pos: TilePos, zoom: f32, fractured: bool) {
    let (xs, ys) = cell_edges(game, pos);
    let lip = (zoom * 0.055).max(1.0);
    let cell = zoom / CELLS as f32;
    for sy in 0..CELLS {
        for sx in 0..CELLS {
            let depth = field.at(pos, sx, sy);
            let rect = Rect::new(
                xs[as_index(sx)],
                ys[as_index(sy)],
                xs[as_index(sx) + 1] - xs[as_index(sx)],
                ys[as_index(sy) + 1] - ys[as_index(sy)],
            );
            match field.material_at(pos, sx, sy) {
                Some(Material::Riser(level)) => {
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        if field.at(pos, sx + dx, sy + dy) < depth {
                            draw_lip(rect, (dx, dy), lip, DROP[level].lip);
                            if fractured {
                                draw_strata(
                                    rect,
                                    (dx, dy),
                                    DROP[level],
                                    hash(pos.x * CELLS + sx, pos.y * CELLS + sy, SALT + 607),
                                );
                            }
                        }
                    }
                }
                Some(Material::Bench(_)) => {
                    let Some(level) = uniform_bench(field, pos, sx, sy) else {
                        continue;
                    };
                    let value = hash(pos.x * CELLS + sx, pos.y * CELLS + sy, SALT + 503);
                    if value.is_multiple_of(199) {
                        draw_missing_slab(rect, cell, DROP[level].top, value);
                    } else if value % 83 == 3 {
                        draw_fracture(rect, cell, zoom, DROP[level].top, value);
                    } else if value % 211 == 13 {
                        draw_rebar(rect, cell, zoom);
                    }
                }
                Some(Material::Vignette(_) | Material::Void) | None => {}
            }
        }
    }
}

pub(super) fn draw_pits(game: &Scene<'_>, field: &PitField, fractured: bool) {
    let (min, max) = super::visible_tiles(game);
    if min.x >= max.x || min.y >= max.y {
        return;
    }
    let map = game.state.map();
    let zoom = game.presentation.camera.zoom;
    let pit_tiles: Vec<TilePos> = (min.y..max.y)
        .flat_map(|y| (min.x..max.x).map(move |x| TilePos::new(x, y)))
        .filter(|&pos| {
            map.tile(pos)
                .is_some_and(|tile| tile.terrain == Terrain::Pit)
        })
        .collect();
    // Fills first, then relief: a decal that straddles two tiles must not
    // be buried under the neighbor's fill.
    for pos in &pit_tiles {
        draw_fill(game, field, *pos, zoom, fractured);
    }
    for pos in &pit_tiles {
        draw_relief(game, field, *pos, zoom, fractured);
    }
}

#[cfg(test)]
mod tests;
