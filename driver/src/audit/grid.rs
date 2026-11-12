//! The simulation's ground step rule over a map, for the audits that
//! measure routes and regions: 8-connected moves, no corner cutting, and
//! diagonals at 14 tenths of a tile, as in `chassis::path::astar`. Which
//! tiles are open is the caller's: ground the sim lets bodies cross now,
//! ground terrain alone, or sky.

use chassis::grid::{TilePos, as_index};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

/// Which tiles of a map a body may stand on, and the steps between them.
pub(super) struct Grid {
    pub(super) width: i32,
    pub(super) height: i32,
    open: Vec<bool>,
}

impl Grid {
    /// The `width` by `height` grid whose open tiles `open` accepts.
    pub(super) fn new(width: i32, height: i32, open: impl Fn(TilePos) -> bool) -> Self {
        let mut tiles = vec![false; as_index(width * height)];
        for y in 0..height {
            for x in 0..width {
                tiles[as_index(y * width + x)] = open(TilePos::new(x, y));
            }
        }
        Self {
            width,
            height,
            open: tiles,
        }
    }

    pub(super) fn cells(&self) -> usize {
        self.open.len()
    }

    pub(super) fn index(&self, t: TilePos) -> usize {
        as_index(t.y * self.width + t.x)
    }

    pub(super) fn tile(&self, i: usize) -> TilePos {
        let i = i32::try_from(i).expect("map cells fit in i32");
        TilePos::new(i % self.width, i / self.width)
    }

    pub(super) fn inside(&self, t: TilePos) -> bool {
        t.x >= 0 && t.y >= 0 && t.x < self.width && t.y < self.height
    }

    pub(super) fn open(&self, t: TilePos) -> bool {
        self.inside(t) && self.open[self.index(t)]
    }

    /// Legal steps out of `t`, with their costs in tenths.
    pub(super) fn steps(&self, t: TilePos) -> impl Iterator<Item = (TilePos, u32)> + '_ {
        (-1..=1).flat_map(move |dy| {
            (-1..=1).filter_map(move |dx| {
                let n = t.offset(dx, dy);
                let diagonal = dx != 0 && dy != 0;
                let legal = (dx, dy) != (0, 0)
                    && self.open(n)
                    && (!diagonal || (self.open(t.offset(dx, 0)) && self.open(t.offset(0, dy))));
                legal.then_some((n, if diagonal { 14 } else { 10 }))
            })
        })
    }

    /// Route lengths in tenths from the open tiles of `sources`; unreached
    /// tiles hold `u32::MAX`.
    pub(super) fn distances(&self, sources: &[TilePos]) -> Vec<u32> {
        let mut dist = vec![u32::MAX; self.cells()];
        let mut heap = BinaryHeap::new();
        for &s in sources {
            if self.open(s) && dist[self.index(s)] != 0 {
                dist[self.index(s)] = 0;
                heap.push(Reverse((0, s.y, s.x)));
            }
        }
        while let Some(Reverse((d, y, x))) = heap.pop() {
            let t = TilePos::new(x, y);
            if d > dist[self.index(t)] {
                continue;
            }
            for (n, cost) in self.steps(t) {
                if d + cost < dist[self.index(n)] {
                    dist[self.index(n)] = d + cost;
                    heap.push(Reverse((d + cost, n.y, n.x)));
                }
            }
        }
        dist
    }

    /// Marks every tile reachable from `starts` with `mark`, skipping tiles
    /// already marked, and returns the tiles it marked in the order it
    /// reached them.
    pub(super) fn flood(&self, starts: &[TilePos], marks: &mut [usize], mark: usize) -> Vec<usize> {
        let mut queue = VecDeque::new();
        for &s in starts {
            if marks[self.index(s)] == usize::MAX {
                marks[self.index(s)] = mark;
                queue.push_back(s);
            }
        }
        let mut tiles = Vec::new();
        while let Some(t) = queue.pop_front() {
            tiles.push(self.index(t));
            for (n, _) in self.steps(t) {
                if marks[self.index(n)] == usize::MAX {
                    marks[self.index(n)] = mark;
                    queue.push_back(n);
                }
            }
        }
        tiles
    }

    /// The open tiles ringing a `w` by `h` footprint at `anchor`, where
    /// traffic enters and leaves it.
    pub(super) fn ring(&self, anchor: TilePos, (w, h): (i32, i32)) -> Vec<TilePos> {
        let mut out = Vec::new();
        for dy in -1..=h {
            for dx in -1..=w {
                let inside = (0..w).contains(&dx) && (0..h).contains(&dy);
                let t = anchor.offset(dx, dy);
                if !inside && self.open(t) {
                    out.push(t);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
