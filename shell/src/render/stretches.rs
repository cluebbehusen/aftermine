//! Shapes that step their own way. Each eight-connected shape of one kind of
//! terrain is cut into short stretches along its sides, and a corner's
//! stretch wraps both its sides so the first bench meets itself square at
//! the corner. Shapes that mirror each other under the map's half-turn read
//! their stretches from one canonical copy, so they draw as mirror images.
//! Peaks and pits share it; each turns a cell's [`Place`] into its own
//! widths.

use super::cell_field::CELLS;
use super::environment::hash;
use chassis::grid::TilePos;
use chassis::grid::as_index;

/// Cells of a side that step alike.
pub(super) const STRETCH: i32 = 8;

/// One shape: its bounds, read from the canonical copy when it is the
/// half-turn mirror of another.
#[derive(Debug)]
struct Shape {
    min: TilePos,
    max: TilePos,
    /// Steps as its half-turn mirror does, read from the mirrored cell.
    mirrored: bool,
    /// Its own half-turn mirror, so opposite sides share their stretches.
    folded: bool,
    seed: i32,
    /// At most two tiles across: too thin to step.
    thin: bool,
}

/// Where a cell sits within its shape: the shape's seed and the keys of the
/// stretches it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Place {
    pub(super) seed: i32,
    pub(super) thin: bool,
    /// The stretch of the cell's nearest side.
    pub(super) stretch: i32,
    /// The same side cut into stretches half a stretch out of phase.
    pub(super) offset: i32,
    /// A thin shape's stretch along its length, across its whole thickness.
    pub(super) length: i32,
}

/// The shapes of one kind of terrain and which one owns each map tile.
#[derive(Debug)]
pub(super) struct Shapes {
    width: i32,
    height: i32,
    owner: Vec<Option<usize>>,
    found: Vec<Shape>,
}

impl Shapes {
    /// Eight-connected shapes of `member` tiles on a `width` by `height` map.
    /// A shape with a tile beside one outside `clear` is left out, so it
    /// keeps whatever widths it would otherwise draw with.
    pub(super) fn find(
        width: i32,
        height: i32,
        member: impl Fn(TilePos) -> bool,
        clear: impl Fn(TilePos) -> bool,
        salt: u32,
    ) -> Self {
        let index = |pos: TilePos| as_index(pos.y * width + pos.x);
        let mut seen = vec![false; as_index(width * height)];
        let mut owner = vec![None; as_index(width * height)];
        let mut found = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let start = TilePos::new(x, y);
                if !member(start) || seen[index(start)] {
                    continue;
                }
                seen[index(start)] = true;
                let mut tiles = vec![start];
                let mut touches = false;
                let mut next = 0;
                while next < tiles.len() {
                    let pos = tiles[next];
                    next += 1;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let near = pos.offset(dx, dy);
                            touches |= !clear(near);
                            if member(near) && !seen[index(near)] {
                                seen[index(near)] = true;
                                tiles.push(near);
                            }
                        }
                    }
                }
                if touches {
                    continue;
                }
                let min = TilePos::new(
                    tiles.iter().map(|p| p.x).min().unwrap_or(x),
                    tiles.iter().map(|p| p.y).min().unwrap_or(y),
                );
                let max = TilePos::new(
                    tiles.iter().map(|p| p.x).max().unwrap_or(x),
                    tiles.iter().map(|p| p.y).max().unwrap_or(y),
                );
                let mirror_min = TilePos::new(width - 1 - max.x, height - 1 - max.y);
                let mirror_max = TilePos::new(width - 1 - min.x, height - 1 - min.y);
                let mirrored = (mirror_min.y, mirror_min.x, mirror_max.y, mirror_max.x)
                    < (min.y, min.x, max.y, max.x);
                let (low, high) = if mirrored {
                    (mirror_min, mirror_max)
                } else {
                    (min, max)
                };
                let id = found.len();
                for pos in &tiles {
                    owner[index(*pos)] = Some(id);
                }
                found.push(Shape {
                    min: low,
                    max: high,
                    mirrored,
                    folded: (mirror_min, mirror_max) == (min, max),
                    seed: hash(low.x + high.x * 257, low.y + high.y * 257, salt).cast_signed(),
                    thin: (max.x - min.x + 1).min(max.y - min.y + 1) <= 2,
                });
            }
        }
        Self {
            width,
            height,
            owner,
            found,
        }
    }

    /// Where a cell of a shape sits; `None` off every shape.
    pub(super) fn place(&self, cx: i32, cy: i32) -> Option<Place> {
        let (tx, ty) = (cx.div_euclid(CELLS), cy.div_euclid(CELLS));
        if tx < 0 || ty < 0 || tx >= self.width || ty >= self.height {
            return None;
        }
        let shape = &self.found[self.owner[as_index(ty * self.width + tx)]?];
        let (cx, cy) = if shape.mirrored {
            (self.width * CELLS - 1 - cx, self.height * CELLS - 1 - cy)
        } else {
            (cx, cy)
        };
        let (x0, y0) = (shape.min.x * CELLS, shape.min.y * CELLS);
        let (x1, y1) = ((shape.max.x + 1) * CELLS - 1, (shape.max.y + 1) * CELLS - 1);
        let (across, down) = (x1 - x0 + 1, y1 - y0 + 1);
        // The nearest side, and how far along it clockwise from its first
        // corner.
        let (_, side, along, length) = [
            (cy - y0, 0, cx - x0, across),
            (x1 - cx, 1, cy - y0, down),
            (y1 - cy, 2, x1 - cx, across),
            (cx - x0, 3, y1 - cy, down),
        ]
        .into_iter()
        .min()
        .unwrap_or((0, 0, 0, 1));
        let fold = |n: i32| if shape.folded { n % 2 } else { n };
        // A side's first and last stretches belong to its corners.
        let parts = ((length + STRETCH / 2) / STRETCH).max(2);
        let stretch = |along: i32| {
            let part = (along.clamp(0, length - 1) * parts / length).min(parts - 1);
            if part == 0 {
                100 + fold(side)
            } else if part == parts - 1 {
                100 + fold((side + 1) % 4)
            } else {
                fold(side) * 64 + part
            }
        };
        // A thin shape varies only along its length, in long stretches.
        let (long_along, long) = if across >= down {
            (cx - x0, across)
        } else {
            (cy - y0, down)
        };
        let long_parts = (long / (4 * STRETCH)).max(1);
        let long_part = (long_along * long_parts / long).min(long_parts - 1);
        let long_part = if shape.folded {
            long_part.min(long_parts - 1 - long_part)
        } else {
            long_part
        };
        Some(Place {
            seed: shape.seed,
            thin: shape.thin,
            stretch: stretch(along),
            offset: stretch(along + STRETCH / 2),
            length: 200 + long_part,
        })
    }

    /// Whether a shape owns this map tile.
    pub(super) fn owns(&self, pos: TilePos) -> bool {
        pos.x >= 0
            && pos.y >= 0
            && pos.x < self.width
            && pos.y < self.height
            && self.owner[as_index(pos.y * self.width + pos.x)].is_some()
    }
}

#[cfg(test)]
mod tests;
