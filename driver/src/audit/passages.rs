//! Narrow passages: ground a match's traffic crosses where a body three
//! tiles wide cannot pass. Rock, peak, pit, unmined scrap and Foundries
//! block, exactly as `State::passable` says.
//!
//! A tile is narrow where a 3x3 body could not stand on it and travel on
//! without leaving its neighbourhood: a slot, a two-wide staircase, or a
//! diagonal gap between two corners. A connected group of narrow tiles is
//! a passage, and it counts when some shortest ground
//! route between two points of interest crosses it: Foundry doorsteps,
//! Extractor frame doorsteps, and starting ground units. Every shortest
//! route counts, not one chosen path, so ties never change the verdict;
//! a notch beside a route that nothing has reason to enter never does.

use chassis::grid::{TilePos, as_index};
use oxide_sim::stats::{BuildingKind, Domain};
use oxide_sim::{PlayerId, State};
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

/// A trafficked passage narrower than three tiles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NarrowPassage {
    /// Top-left corner of the passage's bounding box.
    pub min: (i32, i32),
    /// Bottom-right corner of the passage's bounding box.
    pub max: (i32, i32),
    /// Narrow tiles in the passage.
    pub tiles: usize,
}

/// Grid helpers over one built state.
struct Grid {
    width: i32,
    height: i32,
    open: Vec<bool>,
}

impl Grid {
    fn new(state: &State) -> Self {
        let (width, height) = (state.map().width(), state.map().height());
        let mut open = vec![false; as_index(width * height)];
        for y in 0..height {
            for x in 0..width {
                open[as_index(y * width + x)] = state.passable(TilePos::new(x, y));
            }
        }
        Self {
            width,
            height,
            open,
        }
    }

    fn index(&self, t: TilePos) -> usize {
        as_index(t.y * self.width + t.x)
    }

    fn inside(&self, t: TilePos) -> bool {
        t.x >= 0 && t.y >= 0 && t.x < self.width && t.y < self.height
    }

    fn open(&self, t: TilePos) -> bool {
        self.inside(t) && self.open[self.index(t)]
    }

    fn cells(&self) -> usize {
        self.open.len()
    }

    /// Ground distances in tenths from a source set under the sim's
    /// movement costs: 8-connected, no corner cutting, diagonals at 14.
    fn distances(&self, sources: &[TilePos]) -> Vec<u32> {
        let mut dist = vec![u32::MAX; self.cells()];
        let mut heap = BinaryHeap::new();
        for &s in sources {
            if self.open(s) && dist[self.index(s)] != 0 {
                dist[self.index(s)] = 0;
                heap.push(Reverse((0u32, s.y, s.x)));
            }
        }
        while let Some(Reverse((d, y, x))) = heap.pop() {
            let t = TilePos::new(x, y);
            if d > dist[self.index(t)] {
                continue;
            }
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if (dx, dy) == (0, 0) {
                        continue;
                    }
                    let n = t.offset(dx, dy);
                    if !self.open(n) {
                        continue;
                    }
                    let diagonal = dx != 0 && dy != 0;
                    if diagonal && !(self.open(t.offset(dx, 0)) && self.open(t.offset(0, dy))) {
                        continue;
                    }
                    let nd = d + if diagonal { 14 } else { 10 };
                    if nd < dist[self.index(n)] {
                        dist[self.index(n)] = nd;
                        heap.push(Reverse((nd, n.y, n.x)));
                    }
                }
            }
        }
        dist
    }
}

/// How far around a tile, in square anchors, its slide groups are traced:
/// wide enough to see a 3x3 body round a corner, narrow enough that a detour
/// across the map never joins two sides of a squeeze.
const WINDOW: i32 = 4;

/// Tiles a 3x3 body cannot pass. Around each tile, the open 3x3 squares
/// near it fall into groups a body can slide between without leaving that
/// neighbourhood. The tile is narrow when no square covers it, when the
/// squares covering it belong to different groups, or when a ground step
/// leaves it for a tile whose covering squares share no group with its
/// own: a slot, a two-wide staircase, or a diagonal gap between corners.
/// Tracing groups locally matters: a wide detour elsewhere never excuses
/// the squeeze small units take here.
fn narrow_tiles(grid: &Grid) -> Vec<bool> {
    let square_open =
        |x: i32, y: i32| (0..3).all(|dy| (0..3).all(|dx| grid.open(TilePos::new(x + dx, y + dy))));
    let mut open = vec![false; grid.cells()];
    for y in 0..=grid.height - 3 {
        for x in 0..=grid.width - 3 {
            open[as_index(y * grid.width + x)] = square_open(x, y);
        }
    }
    let anchored = |x: i32, y: i32| {
        x >= 0
            && y >= 0
            && x <= grid.width - 3
            && y <= grid.height - 3
            && open[as_index(y * grid.width + x)]
    };
    let side = as_index(2 * WINDOW - 1);
    let mut narrow = vec![false; grid.cells()];
    let mut group = vec![u8::MAX; side * side];
    let mut queue = VecDeque::new();
    for y in 0..grid.height {
        for x in 0..grid.width {
            let t = TilePos::new(x, y);
            if !grid.open(t) {
                continue;
            }
            // Anchors x-WINDOW..x+WINDOW-2 by y-WINDOW..y+WINDOW-2.
            let (left, top) = (x - WINDOW, y - WINDOW);
            let slot = |ax: i32, ay: i32| as_index((ay - top) * (2 * WINDOW - 1) + (ax - left));
            group.fill(u8::MAX);
            let mut next = 0u8;
            for ay in top..top + 2 * WINDOW - 1 {
                for ax in left..left + 2 * WINDOW - 1 {
                    if group[slot(ax, ay)] != u8::MAX || !anchored(ax, ay) {
                        continue;
                    }
                    group[slot(ax, ay)] = next;
                    queue.push_back((ax, ay));
                    while let Some((cx, cy)) = queue.pop_front() {
                        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            let (nx, ny) = (cx + dx, cy + dy);
                            let inside = (left..left + 2 * WINDOW - 1).contains(&nx)
                                && (top..top + 2 * WINDOW - 1).contains(&ny);
                            if inside && group[slot(nx, ny)] == u8::MAX && anchored(nx, ny) {
                                group[slot(nx, ny)] = next;
                                queue.push_back((nx, ny));
                            }
                        }
                    }
                    next += 1;
                }
            }
            // The groups of the squares covering a tile, as a bit set.
            let cover = |c: TilePos| {
                let mut bits = 0u64;
                for ay in c.y - 2..=c.y {
                    for ax in c.x - 2..=c.x {
                        let g = group[slot(ax, ay)];
                        if g != u8::MAX {
                            bits |= 1 << g;
                        }
                    }
                }
                bits
            };
            let here = cover(t);
            let split = || {
                (-1..=1).any(|dy| {
                    (-1..=1).any(|dx| {
                        let n = t.offset(dx, dy);
                        let diagonal = dx != 0 && dy != 0;
                        let step = (dx, dy) != (0, 0)
                            && grid.open(n)
                            && (!diagonal
                                || (grid.open(t.offset(dx, 0)) && grid.open(t.offset(0, dy))));
                        step && {
                            let there = cover(n);
                            there != 0 && there & here == 0
                        }
                    })
                })
            };
            narrow[grid.index(t)] = here.count_ones() != 1 || split();
        }
    }
    narrow
}

/// Passable tiles ringing a footprint.
fn ring(grid: &Grid, anchor: TilePos, (w, h): (i32, i32)) -> Vec<TilePos> {
    let mut out = Vec::new();
    for dy in -1..=h {
        for dx in -1..=w {
            let inside = (0..w).contains(&dx) && (0..h).contains(&dy);
            let t = anchor.offset(dx, dy);
            if !inside && grid.open(t) {
                out.push(t);
            }
        }
    }
    out
}

/// The places a match's ground traffic runs between.
fn points_of_interest(state: &State, grid: &Grid) -> Vec<Vec<TilePos>> {
    let mut points: Vec<Vec<TilePos>> = Vec::new();
    for b in state.buildings() {
        if b.kind == BuildingKind::Foundry {
            points.push(ring(grid, b.anchor, b.kind.size()));
        }
    }
    for &frame in state.map().extractor_frames() {
        points.push(ring(grid, frame, BuildingKind::Extractor.size()));
    }
    for u in state.units() {
        if u.domain() == Domain::Ground && grid.open(u.tile()) {
            points.push(vec![u.tile()]);
        }
    }
    points.retain(|p| !p.is_empty());
    points.sort();
    points.dedup();
    points
}

/// Every trafficked passage narrower than three tiles, in row-major
/// order of their first tile.
pub fn narrow_passages(state: &State) -> Vec<NarrowPassage> {
    let grid = Grid::new(state);
    let narrow = narrow_tiles(&grid);
    let points = points_of_interest(state, &grid);
    let fields: Vec<Vec<u32>> = points.iter().map(|p| grid.distances(p)).collect();
    let mut traffic = vec![false; grid.cells()];
    for i in 0..fields.len() {
        for j in (i + 1)..fields.len() {
            let total = points[j]
                .iter()
                .map(|&t| fields[i][grid.index(t)])
                .min()
                .unwrap_or(u32::MAX);
            if total == u32::MAX {
                continue;
            }
            for (cell, flag) in traffic.iter_mut().enumerate() {
                let (a, b) = (fields[i][cell], fields[j][cell]);
                if a != u32::MAX && b != u32::MAX && a + b == total {
                    *flag = true;
                }
            }
        }
    }
    let mut grouped = vec![false; grid.cells()];
    let mut passages = Vec::new();
    for y in 0..grid.height {
        for x in 0..grid.width {
            let start = TilePos::new(x, y);
            if !narrow[grid.index(start)] || grouped[grid.index(start)] {
                continue;
            }
            grouped[grid.index(start)] = true;
            let mut queue = VecDeque::from([start]);
            let (mut min, mut max) = ((x, y), (x, y));
            let (mut tiles, mut crossed) = (0, false);
            while let Some(t) = queue.pop_front() {
                tiles += 1;
                crossed |= traffic[grid.index(t)];
                min = (min.0.min(t.x), min.1.min(t.y));
                max = (max.0.max(t.x), max.1.max(t.y));
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let n = t.offset(dx, dy);
                        if grid.inside(n) && narrow[grid.index(n)] && !grouped[grid.index(n)] {
                            grouped[grid.index(n)] = true;
                            queue.push_back(n);
                        }
                    }
                }
            }
            if crossed {
                passages.push(NarrowPassage { min, max, tiles });
            }
        }
    }
    passages
}

/// Whether terrain alone keeps some pair of hostile Foundries apart on
/// the ground: an island map, which the width rule leaves as authored.
pub fn ground_severed(state: &State) -> bool {
    let (width, height) = (state.map().width(), state.map().height());
    let flood = |seed: TilePos| {
        let mut seen = vec![false; as_index(width * height)];
        let walkable = |t: TilePos| {
            t.x >= 0
                && t.y >= 0
                && t.x < width
                && t.y < height
                && state
                    .map()
                    .tile(t)
                    .is_some_and(|tile| tile.terrain == oxide_sim::map::Terrain::Ground)
        };
        seen[as_index(seed.y * width + seed.x)] = true;
        let mut queue = VecDeque::from([seed]);
        while let Some(t) = queue.pop_front() {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let n = t.offset(dx, dy);
                if walkable(n) && !seen[as_index(n.y * width + n.x)] {
                    seen[as_index(n.y * width + n.x)] = true;
                    queue.push_back(n);
                }
            }
        }
        seen
    };
    let foundries: Vec<(PlayerId, TilePos)> = state
        .buildings()
        .iter()
        .filter(|b| b.kind == BuildingKind::Foundry)
        .map(|b| (b.player, b.anchor))
        .collect();
    foundries.iter().enumerate().any(|(i, &(a, at))| {
        let reach = flood(at);
        foundries[i + 1..]
            .iter()
            .any(|&(b, bt)| state.hostile(a, b) && !reach[as_index(bt.y * width + bt.x)])
    })
}

#[cfg(test)]
mod tests;
