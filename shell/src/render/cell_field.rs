//! A chessboard distance field over sub-tile cells: how far each cell of a
//! terrain kind lies from the nearest cell outside it. The pit, peak, and
//! boulder passes share it so drops, terraces, and slab sizes all measure
//! terrain the same way.

use crate::game::Scene;
use chassis::grid::TilePos;
use chassis::grid::as_index;
use macroquad::prelude::*;

pub(super) struct CellField {
    pub(super) origin: TilePos,
    /// Window size in tiles.
    pub(super) width: i32,
    pub(super) height: i32,
    /// Cells per tile side.
    pub(super) cells: i32,
    pub(super) depth: Vec<u8>,
}

impl CellField {
    /// A field over `width` by `height` tiles from `origin`, whose cells are
    /// inside wherever `inside` holds for their tile. Depth counts from one
    /// at the edge; cells outside stay zero.
    pub(super) fn seeded(
        origin: TilePos,
        width: i32,
        height: i32,
        cells: i32,
        mut inside: impl FnMut(TilePos) -> bool,
    ) -> Self {
        let (cw, ch) = (width * cells, height * cells);
        let mut depth = vec![0u8; as_index((cw * ch).max(0))];
        for ty in 0..height {
            for tx in 0..width {
                if !inside(TilePos::new(origin.x + tx, origin.y + ty)) {
                    continue;
                }
                for sy in 0..cells {
                    let row = as_index((ty * cells + sy) * cw + tx * cells);
                    depth[row..row + as_index(cells)].fill(u8::MAX);
                }
            }
        }
        let mut field = Self {
            origin,
            width,
            height,
            cells,
            depth,
        };
        field.relax();
        field
    }

    /// A field over the same window seeded directly from a cell mask.
    pub(super) fn masked(&self, mask: impl Fn(usize) -> bool) -> Self {
        let mut field = Self {
            origin: self.origin,
            width: self.width,
            height: self.height,
            cells: self.cells,
            depth: (0..self.depth.len())
                .map(|index| if mask(index) { u8::MAX } else { 0 })
                .collect(),
        };
        field.relax();
        field
    }

    /// Forward then backward chamfer passes with unit weights: the exact
    /// chessboard distance.
    pub(super) fn relax(&mut self) {
        self.relax_pass(false);
        self.relax_pass(true);
    }

    pub(super) fn relax_pass(&mut self, backward: bool) {
        let (cw, ch) = (self.width * self.cells, self.height * self.cells);
        let sign = if backward { -1 } else { 1 };
        let neighbors =
            [(-1, -1), (0, -1), (1, -1), (-1, 0)].map(|(dx, dy)| (dx * sign, dy * sign));
        for step_y in 0..ch {
            let y = if backward { ch - 1 - step_y } else { step_y };
            for step_x in 0..cw {
                let x = if backward { cw - 1 - step_x } else { step_x };
                let index = as_index(y * cw + x);
                if self.depth[index] == 0 {
                    continue;
                }
                let nearest = neighbors
                    .iter()
                    .map(|(dx, dy)| self.cell(x + dx, y + dy))
                    .min()
                    .unwrap_or(u8::MAX);
                self.depth[index] = self.depth[index].min(nearest.saturating_add(1));
            }
        }
    }

    /// Cells beyond the window are unknown: deep inside, never an edge.
    pub(super) fn cell(&self, x: i32, y: i32) -> u8 {
        let (cw, ch) = (self.width * self.cells, self.height * self.cells);
        if x < 0 || y < 0 || x >= cw || y >= ch {
            u8::MAX
        } else {
            self.depth[as_index(y * cw + x)]
        }
    }

    /// Depth of a cell addressed by absolute tile and sub-cell; the sub-cell
    /// may step outside the tile.
    pub(super) fn at(&self, pos: TilePos, sx: i32, sy: i32) -> u8 {
        self.cell(
            (pos.x - self.origin.x) * self.cells + sx,
            (pos.y - self.origin.y) * self.cells + sy,
        )
    }
}

/// Quarter-tile cells: the resolution of every terrace and drop.
pub(super) const CELLS: i32 = 4;

/// Screen-space cell edges for one tile at [`CELLS`] resolution, floored so
/// neighbors share exact edges and no hairline opens between cells.
pub(super) fn cell_edges(game: &Scene<'_>, pos: TilePos) -> ([f32; 5], [f32; 5]) {
    let mut xs = [0.0; 5];
    let mut ys = [0.0; 5];
    for (i, (x, y)) in xs.iter_mut().zip(ys.iter_mut()).enumerate() {
        let frac = i as f32 / CELLS as f32;
        let screen = game
            .presentation
            .camera
            .to_screen(vec2(pos.x as f32 + frac, pos.y as f32 + frac));
        *x = screen.x.floor();
        *y = screen.y.floor();
    }
    (xs, ys)
}

#[cfg(test)]
mod tests;
