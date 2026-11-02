//! Tall faces: a pit or peak bordering two levels shares its steps on both sides
//! but for one wall. A pit's drops from its higher ground and a peak's rises from
//! its lower, a level further than on the other side, down to or up from the same
//! first bench. That wall is drawn as a wider first riser, taking its width from
//! the bench so every step past it lines up round the shape. It starts square
//! where a cliff meets the shape, so the step shows there.

use super::cell_field::{CELLS, CellField};
use aftermine_sim::map::{Elevation, Map, Terrain};
use chassis::grid::{TilePos, as_index};

/// Cells a tall face takes from its first bench.
const EXTRA_CELLS: i32 = 1;
/// The narrowest first bench of a shape with a tall face, which leaves the bench
/// room beside the wider riser.
const MIN_FIRST_BENCH: i32 = 3;
/// How far in from its rim a cell is still counted as on that side.
const SIDE_REACH: i32 = 6;

const ON_SHAPE: u8 = 1;
const ON_TALL_SIDE: u8 = 2;

/// Which cells lie in a shape with a tall face, and on which side. Empty on a map
/// with no such shape.
#[derive(Default)]
pub(super) struct TallFaces {
    columns: i32,
    cells: Vec<u8>,
}

impl TallFaces {
    /// `field` is `terrain`'s distance field, zero outside it.
    pub(super) fn build(map: &Map, terrain: Terrain, field: &CellField) -> Self {
        let mut faces = Self::default();
        if !map.tiered() {
            return faces;
        }
        for tiles in shapes(map, terrain) {
            let Some((low, high)) = rim(map, &tiles) else {
                continue;
            };
            let tall = |pos: TilePos| {
                standing(map, pos).is_some_and(|elevation| {
                    let (bottom, top) = elevation.span();
                    if terrain == Terrain::Pit {
                        top == high
                    } else {
                        bottom == low
                    }
                })
            };
            for &pos in &tiles {
                for sy in 0..CELLS {
                    for sx in 0..CELLS {
                        let (cx, cy) = (pos.x * CELLS + sx, pos.y * CELLS + sy);
                        let kind = if on_side(field, &tall, cx, cy) {
                            ON_TALL_SIDE
                        } else {
                            ON_SHAPE
                        };
                        faces.put(map, cx, cy, kind);
                    }
                }
            }
        }
        faces
    }

    fn put(&mut self, map: &Map, cx: i32, cy: i32, kind: u8) {
        if self.cells.is_empty() {
            self.columns = map.width() * CELLS;
            self.cells = vec![0; as_index(self.columns * map.height() * CELLS)];
        }
        self.cells[as_index(cy * self.columns + cx)] = kind;
    }

    fn at(&self, cx: i32, cy: i32) -> u8 {
        if cx < 0 || cy < 0 || cx >= self.columns {
            return 0;
        }
        self.cells
            .get(as_index(cy * self.columns + cx))
            .copied()
            .unwrap_or(0)
    }

    /// Whether a cell lies on the side of the shape with the tall face.
    pub(super) fn on_tall_side(&self, cx: i32, cy: i32) -> bool {
        self.at(cx, cy) == ON_TALL_SIDE
    }

    /// Whether a cell at `depth` stands in the tall face.
    pub(super) fn in_face(&self, cx: i32, cy: i32, depth: u8) -> bool {
        self.on_tall_side(cx, cy) && i32::from(depth) <= 1 + EXTRA_CELLS
    }

    /// A cell's bench widths: a shape with a tall face keeps its first bench wide
    /// enough to give up the cells.
    pub(super) fn widths(&self, cx: i32, cy: i32, mut widths: [i32; 3]) -> [i32; 3] {
        if self.at(cx, cy) != 0 {
            widths[0] = widths[0].max(MIN_FIRST_BENCH);
        }
        widths
    }
}

/// Whether the rim nearest a cell is on the tall side: straight out along a row or
/// column first, so a tall side ends square at a tile's edge.
fn on_side(field: &CellField, tall: &impl Fn(TilePos) -> bool, cx: i32, cy: i32) -> bool {
    let outside = |x: i32, y: i32| {
        let pos = TilePos::new(x.div_euclid(CELLS), y.div_euclid(CELLS));
        (field.at(pos, x.rem_euclid(CELLS), y.rem_euclid(CELLS)) == 0).then_some(pos)
    };
    for reach in 1..=SIDE_REACH {
        for ways in [
            [(1, 0), (-1, 0), (0, 1), (0, -1)],
            [(1, 1), (-1, 1), (1, -1), (-1, -1)],
        ] {
            let mut rims = ways
                .iter()
                .filter_map(|(dx, dy)| outside(cx + dx * reach, cy + dy * reach))
                .peekable();
            if rims.peek().is_some() {
                return rims.any(tall);
            }
        }
    }
    false
}

/// The elevation of a tile a shape's rim is measured from: a cliff reads as the
/// ground at its foot.
fn standing(map: &Map, pos: TilePos) -> Option<Elevation> {
    let terrain = map.tile(pos)?.terrain;
    matches!(terrain, Terrain::Ground | Terrain::Cliff)
        .then(|| map.elevation(pos))
        .flatten()
}

/// The eight-connected shapes of one terrain, in row order.
fn shapes(map: &Map, terrain: Terrain) -> Vec<Vec<TilePos>> {
    let is_member = |pos: TilePos| map.tile(pos).is_some_and(|tile| tile.terrain == terrain);
    let width = map.width();
    let mut seen = vec![false; as_index(width * map.height())];
    let mut found = Vec::new();
    for y in 0..map.height() {
        for x in 0..width {
            let start = TilePos::new(x, y);
            if seen[as_index(y * width + x)] || !is_member(start) {
                continue;
            }
            seen[as_index(y * width + x)] = true;
            let mut tiles = vec![start];
            let mut next = 0;
            while next < tiles.len() {
                let pos = tiles[next];
                next += 1;
                for near in neighbors(pos) {
                    if is_member(near) && !seen[as_index(near.y * width + near.x)] {
                        seen[as_index(near.y * width + near.x)] = true;
                        tiles.push(near);
                    }
                }
            }
            found.push(tiles);
        }
    }
    found
}

fn neighbors(pos: TilePos) -> impl Iterator<Item = TilePos> {
    (-1..=1).flat_map(move |dy| (-1..=1).map(move |dx| pos.offset(dx, dy)))
}

/// The lowest and highest level of the ground beside a shape, when it borders
/// more than one. A cliff touching the shape doesn't part it: a pit or peak on
/// one level meets a cliff's end the way it meets the map's border.
fn rim(map: &Map, tiles: &[TilePos]) -> Option<(u8, u8)> {
    let levels = || {
        tiles
            .iter()
            .flat_map(|&pos| neighbors(pos))
            .filter_map(|near| {
                let ground = map.tile(near)?.terrain == Terrain::Ground;
                match map.elevation(near)? {
                    Elevation::Level(level) if ground => Some(level),
                    _ => None,
                }
            })
    };
    let (low, high) = (levels().min()?, levels().max()?);
    (low < high).then_some((low, high))
}

#[cfg(test)]
mod tests;
