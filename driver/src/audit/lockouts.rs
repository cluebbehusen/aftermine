//! Lockout prizes: Extractor frames no starting base owns, clustered where
//! one seat could take them first and then hold them without being shelled.
//!
//! A prize is the pocket a holder controls around a group of unclaimed
//! frames. Every Foundry site seeds one from the frames it would support,
//! when it supports at least two. The pocket is the seed's side of the
//! narrowest cut that parts it from every starting Foundry and from ground
//! more than [`POCKET_REACH`] steps away: a door bounds it where one exists,
//! open ground where none does. Ground no starting Foundry reaches on foot
//! is one pocket however its frames spread, so spacing them apart never
//! hides an air-only prize.
//!
//! A prize locks a match when it holds [`BIG_PRIZE`] frames or more, the
//! nearest rival arrives about as soon as the first seat, and fewer than
//! [`SHELLING_FLOOR`] ground tiles outside it can shell its frames at the
//! longest artillery reach. Terrain alone decides routes and Foundry
//! sites, since scrap and buildings clear in play.

use super::longest_reach;
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::{TilePos, as_index};
use oxide_sim::geometry::foundry_supports;
use oxide_sim::map::{Map, Terrain};
use oxide_sim::stats::BuildingKind;
use oxide_sim::{PlayerId, State};
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, VecDeque};

/// Frames a prize holds before it can lock a match. Above three, the first
/// seat to arrive out-earns everyone else for the rest of the match.
pub const BIG_PRIZE: usize = 4;

/// Ground tiles that must be able to shell a big contested prize. The worst
/// shipped prizes before their fixes had at most 124; Sunken Floor's basin,
/// the model of a prize that pays for its exposure, has 312.
pub const SHELLING_FLOOR: usize = 160;

/// Ground steps a pocket reaches from its seed frames before open ground
/// ends it: room for a plateau, never the far side of the map.
const POCKET_REACH: u32 = 16;

/// How soon a seat reaches a prize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Arrival {
    /// Seat index.
    pub seat: u8,
    /// Ground steps from its Foundry, or the air route around peaks when no
    /// ground route exists.
    pub steps: usize,
    /// Whether the seat can only fly there.
    pub by_air: bool,
}

/// A pocket of unclaimed frames and how hard it is to contest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Prize {
    /// Anchors of the frames wholly inside the pocket, row-major.
    pub frames: Vec<(i32, i32)>,
    /// Whether no starting Foundry reaches the pocket on foot.
    pub island: bool,
    /// Tiles in the narrowest cut sealing the pocket; zero on an island.
    pub doors: usize,
    /// The seat that arrives first; ties go to the lower seat.
    pub first: Arrival,
    /// The nearest seat hostile to `first`, if any.
    pub rival: Option<Arrival>,
    /// Ground tiles outside the pocket and its doors that can shell one of
    /// its frames: within the longest artillery reach, at the frame's
    /// level or above, with no peak in the line.
    pub shelling: usize,
}

impl Prize {
    /// Whether the rival arrives within a tenth plus two steps of the first.
    pub fn contested(&self) -> bool {
        self.rival
            .is_some_and(|rival| rival.steps * 10 <= self.first.steps * 11 + 20)
    }

    /// Whether whoever takes the prize first can hold it unanswered.
    pub fn locked(&self) -> bool {
        self.frames.len() >= BIG_PRIZE && self.contested() && self.shelling < SHELLING_FLOOR
    }
}

/// How a body crosses the map.
#[derive(Clone, Copy)]
enum Domain {
    Ground,
    Air,
}

/// Terrain-only routes: walkable or flyable tiles, ramps included, joined
/// by the sim's steps (8-connected, no corner cutting).
struct Ground<'a> {
    map: &'a Map,
    width: i32,
    height: i32,
}

impl Ground<'_> {
    fn cells(&self) -> usize {
        as_index(self.width * self.height)
    }

    fn index(&self, t: TilePos) -> usize {
        as_index(t.y * self.width + t.x)
    }

    fn tile(&self, i: usize) -> TilePos {
        let i = i32::try_from(i).expect("map cells fit in i32");
        TilePos::new(i % self.width, i / self.width)
    }

    /// Ground terrain, scrap or not: scrap is mined out in play.
    fn open(&self, t: TilePos) -> bool {
        self.passes(t, Domain::Ground)
    }

    fn passes(&self, t: TilePos, domain: Domain) -> bool {
        self.map.tile(t).is_some_and(|tile| match domain {
            Domain::Ground => !tile.terrain.blocks_ground(),
            Domain::Air => !tile.terrain.blocks_air(),
        })
    }

    /// Steps out of `t` with their costs in tenths.
    fn steps(&self, t: TilePos, domain: Domain) -> impl Iterator<Item = (TilePos, u32)> + '_ {
        (-1..=1).flat_map(move |dy| {
            (-1..=1).filter_map(move |dx| {
                let n = t.offset(dx, dy);
                let diagonal = dx != 0 && dy != 0;
                let legal = (dx, dy) != (0, 0)
                    && self.passes(n, domain)
                    && (!diagonal
                        || (self.passes(t.offset(dx, 0), domain)
                            && self.passes(t.offset(0, dy), domain)));
                legal.then_some((n, if diagonal { 14 } else { 10 }))
            })
        })
    }

    /// Distances in tenths from a source set.
    fn distances(&self, sources: &[TilePos], domain: Domain) -> Vec<u32> {
        let mut dist = vec![u32::MAX; self.cells()];
        let mut heap = BinaryHeap::new();
        for &s in sources {
            if self.passes(s, domain) {
                dist[self.index(s)] = 0;
                heap.push(Reverse((0, s.y, s.x)));
            }
        }
        while let Some(Reverse((d, y, x))) = heap.pop() {
            let t = TilePos::new(x, y);
            if d > dist[self.index(t)] {
                continue;
            }
            for (n, cost) in self.steps(t, domain) {
                if d + cost < dist[self.index(n)] {
                    dist[self.index(n)] = d + cost;
                    heap.push(Reverse((d + cost, n.y, n.x)));
                }
            }
        }
        dist
    }

    /// The level a body standing on `t` fires from: a ramp's upper level.
    fn top(&self, t: TilePos) -> u8 {
        self.map.elevation(t).map_or(0, |e| e.span().1)
    }

    /// The single level a footprint stands on, if it stands on one and off
    /// ramps.
    fn footing(&self, anchor: TilePos, (w, h): (i32, i32)) -> Option<u8> {
        let mut level = None;
        for dy in 0..h {
            for dx in 0..w {
                let t = anchor.offset(dx, dy);
                if !self.open(t) || self.map.is_ramp(t) {
                    return None;
                }
                let here = self.top(t);
                if *level.get_or_insert(here) != here {
                    return None;
                }
            }
        }
        level
    }
}

fn footprint(anchor: TilePos, (w, h): (i32, i32)) -> impl Iterator<Item = TilePos> {
    (0..h).flat_map(move |dy| (0..w).map(move |dx| anchor.offset(dx, dy)))
}

/// Ground ringing a footprint, where traffic enters it.
fn ring(ground: &Ground, anchor: TilePos, (w, h): (i32, i32)) -> Vec<TilePos> {
    let mut out = Vec::new();
    for dy in -1..=h {
        for dx in -1..=w {
            let inside = (0..w).contains(&dx) && (0..h).contains(&dy);
            let t = anchor.offset(dx, dy);
            if !inside && ground.open(t) {
                out.push(t);
            }
        }
    }
    out
}

/// Every prize on the map, islands first and then by seed, each pocket
/// once.
pub fn prizes(state: &State) -> Vec<Prize> {
    let map = state.map();
    let ground = Ground {
        map,
        width: map.width(),
        height: map.height(),
    };
    let frame = BuildingKind::Extractor.size();
    let foundry = BuildingKind::Foundry.size();
    let starts: Vec<(PlayerId, TilePos)> = state
        .buildings()
        .iter()
        .filter(|b| b.kind == BuildingKind::Foundry)
        .map(|b| (b.player, b.anchor))
        .collect();
    let frames = map.extractor_frames();
    let unclaimed: Vec<usize> = (0..frames.len())
        .filter(|&f| starts.iter().all(|&(_, s)| !foundry_supports(s, frames[f])))
        .collect();
    let doorsteps: Vec<TilePos> = starts
        .iter()
        .flat_map(|&(_, s)| ring(&ground, s, foundry))
        .collect();
    let home = ground.distances(&doorsteps, Domain::Ground);
    let reach = Reach {
        ground: &ground,
        starts: &starts,
        walks: starts
            .iter()
            .map(|&(_, s)| ground.distances(&ring(&ground, s, foundry), Domain::Ground))
            .collect(),
        flights: starts
            .iter()
            .map(|&(_, s)| {
                ground.distances(&footprint(s, foundry).collect::<Vec<_>>(), Domain::Air)
            })
            .collect(),
        state,
    };

    let mut out = Vec::new();
    let mut seen: BTreeSet<Vec<usize>> = BTreeSet::new();
    // Islands: every unclaimed frame on ground no start reaches on foot,
    // one pocket per stretch of ground.
    let mut island = vec![usize::MAX; ground.cells()];
    for &f in &unclaimed {
        let anchor = frames[f];
        if home[ground.index(anchor)] != u32::MAX || island[ground.index(anchor)] != usize::MAX {
            continue;
        }
        let tiles = flood(&ground, anchor, &mut island, f);
        let held: Vec<usize> = unclaimed
            .iter()
            .copied()
            .filter(|&g| footprint(frames[g], frame).all(|t| island[ground.index(t)] == f))
            .collect();
        if seen.insert(tiles.clone()) {
            out.extend(reach.prize(&held, &tiles, 0, true));
        }
    }

    // Seeds: the frames each Foundry site would support, kept when a site
    // supports two or more. A seed inside a larger one still gets its own
    // pocket, since a holder can leave the extra frames outside the door.
    let mut seeds: BTreeSet<Vec<usize>> = BTreeSet::new();
    for y in 0..ground.height - 1 {
        for x in 0..ground.width - 1 {
            let site = TilePos::new(x, y);
            let clear = ground.footing(site, foundry).is_some()
                && footprint(site, foundry).all(|t| {
                    !map.tile_in_extractor_frame(t)
                        && state.buildings().iter().all(|b| !b.contains(t))
                });
            if !clear {
                continue;
            }
            let group: Vec<usize> = unclaimed
                .iter()
                .copied()
                .filter(|&f| home[ground.index(frames[f])] != u32::MAX)
                .filter(|&f| foundry_supports(site, frames[f]))
                .collect();
            if group.len() >= 2 {
                seeds.insert(group);
            }
        }
    }
    for group in &seeds {
        let sources: Vec<TilePos> = group
            .iter()
            .flat_map(|&f| footprint(frames[f], frame))
            .collect();
        let Some((tiles, doors)) = pocket(&ground, &sources, &doorsteps) else {
            continue;
        };
        let inside: BTreeSet<usize> = tiles.iter().copied().collect();
        let held: Vec<usize> = (0..frames.len())
            .filter(|&f| footprint(frames[f], frame).all(|t| inside.contains(&ground.index(t))))
            .collect();
        if held.iter().any(|f| !unclaimed.contains(f)) || !seen.insert(tiles.clone()) {
            continue;
        }
        let mut sealed = tiles.clone();
        sealed.extend(doors.iter().copied());
        out.extend(reach.prize(&held, &sealed, doors.len(), false));
    }
    out
}

/// Marks the ground component holding `start` with `mark` and returns its
/// tiles in index order.
fn flood(ground: &Ground, start: TilePos, marks: &mut [usize], mark: usize) -> Vec<usize> {
    let mut queue = VecDeque::from([start]);
    marks[ground.index(start)] = mark;
    let mut tiles = Vec::new();
    while let Some(t) = queue.pop_front() {
        tiles.push(ground.index(t));
        for (n, _) in ground.steps(t, Domain::Ground) {
            if marks[ground.index(n)] == usize::MAX {
                marks[ground.index(n)] = mark;
                queue.push_back(n);
            }
        }
    }
    tiles.sort_unstable();
    tiles
}

/// Step directions, ordered so `7 - d` reverses direction `d`.
const DIRS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// How the pocket's flow search reached a node.
#[derive(Clone, Copy)]
enum Via {
    Unseen,
    Start,
    Join,
    Unjoin,
    /// From tile `.0`'s exit along direction `.1`.
    Step(usize, usize),
    /// Back from tile `.2`'s entry, cancelling flow tile `.0` sent along
    /// direction `.1`.
    Unstep(usize, usize, usize),
}

/// The pocket around `sources`: the source side of the narrowest vertex
/// cut parting them from `doorsteps` and from ground [`POCKET_REACH`] steps
/// away, as (pocket tiles, cut tiles) in index order. `None` when nothing
/// parts them, as when a source touches a doorstep.
fn pocket(
    ground: &Ground,
    sources: &[TilePos],
    doorsteps: &[TilePos],
) -> Option<(Vec<usize>, Vec<usize>)> {
    let near = ground.distances(sources, Domain::Ground);
    let cells = ground.cells();
    let mut sink = vec![false; cells];
    for (i, &d) in near.iter().enumerate() {
        sink[i] = d != u32::MAX && d >= POCKET_REACH * 10;
    }
    for &t in doorsteps {
        sink[ground.index(t)] = true;
    }
    let mut source = vec![false; cells];
    for &t in sources {
        if sink[ground.index(t)] {
            return None;
        }
        source[ground.index(t)] = true;
    }
    let unbounded = |i: usize| source[i] || sink[i];
    // Unit vertex capacities through tile splitting: each tile has an entry
    // and an exit node joined by one unit, unbounded at sources and sinks.
    // `through[i]` is the flow across that join, `along[i * 8 + d]` the flow
    // from tile i's exit into its neighbour's entry in direction d.
    let neighbour = |i: usize, d: usize| {
        let t = ground.tile(i);
        let (dx, dy) = DIRS[d];
        let n = t.offset(dx, dy);
        let legal = ground.open(n)
            && near[ground.index(n)] != u32::MAX
            && (dx == 0
                || dy == 0
                || (ground.open(t.offset(dx, 0)) && ground.open(t.offset(0, dy))));
        legal.then(|| ground.index(n))
    };
    let mut through = vec![0u32; cells];
    let mut along = vec![0u32; cells * 8];
    // A node is (tile, exit?). Parents record how the search arrived.
    let mut via = vec![Via::Unseen; cells * 2];
    let node = |i: usize, exit: bool| i * 2 + usize::from(exit);
    loop {
        via.fill(Via::Unseen);
        let mut queue = VecDeque::new();
        for (i, &s) in source.iter().enumerate() {
            if s {
                via[node(i, false)] = Via::Start;
                queue.push_back(node(i, false));
            }
        }
        let mut reached = None;
        'search: while let Some(at) = queue.pop_front() {
            let (i, exit) = (at / 2, at % 2 == 1);
            let mut visit = |to: usize, how: Via, queue: &mut VecDeque<usize>| {
                if matches!(via[to], Via::Unseen) {
                    via[to] = how;
                    queue.push_back(to);
                }
            };
            if exit {
                if through[i] > 0 {
                    visit(node(i, false), Via::Unjoin, &mut queue);
                }
                for d in 0..8 {
                    if let Some(n) = neighbour(i, d) {
                        visit(node(n, false), Via::Step(i, d), &mut queue);
                    }
                }
            } else {
                if sink[i] {
                    reached = Some(i);
                    break 'search;
                }
                if unbounded(i) || through[i] == 0 {
                    visit(node(i, true), Via::Join, &mut queue);
                }
                for d in 0..8 {
                    // Undo flow some neighbour sent into this entry.
                    if let Some(n) = neighbour(i, d) {
                        let back = 7 - d;
                        if along[n * 8 + back] > 0 {
                            visit(node(n, true), Via::Unstep(n, back, i), &mut queue);
                        }
                    }
                }
            }
        }
        let Some(end) = reached else {
            break;
        };
        // Push one unit back along the path; a path through unbounded
        // tiles alone means no cut parts the sources.
        let mut at = node(end, false);
        let mut bounded = false;
        loop {
            match via[at] {
                Via::Start => break,
                Via::Join => {
                    let i = at / 2;
                    if !unbounded(i) {
                        through[i] += 1;
                        bounded = true;
                    }
                    at = node(i, false);
                }
                Via::Unjoin => {
                    let i = at / 2;
                    through[i] -= 1;
                    bounded = true;
                    at = node(i, true);
                }
                Via::Step(from, d) => {
                    along[from * 8 + d] += 1;
                    at = node(from, true);
                }
                Via::Unstep(from, d, to) => {
                    along[from * 8 + d] -= 1;
                    at = node(to, false);
                }
                Via::Unseen => unreachable!("a path node was reached"),
            }
        }
        if !bounded {
            return None;
        }
    }
    let mut pocket = Vec::new();
    let mut doors = Vec::new();
    for i in 0..cells {
        let entered = !matches!(via[node(i, false)], Via::Unseen);
        let left = !matches!(via[node(i, true)], Via::Unseen);
        if left {
            pocket.push(i);
        } else if entered {
            doors.push(i);
        }
    }
    Some((pocket, doors))
}

/// Seat arrivals and shelling, measured against one map.
struct Reach<'a> {
    ground: &'a Ground<'a>,
    starts: &'a [(PlayerId, TilePos)],
    /// Per start, ground distances from its doorstep ring.
    walks: Vec<Vec<u32>>,
    /// Per start, air distances from its footprint.
    flights: Vec<Vec<u32>>,
    state: &'a State,
}

impl Reach<'_> {
    /// Measures the prize holding frames `held` (indices into the map's
    /// frames) inside the sealed tiles `sealed`; `None` when no seat can
    /// reach it.
    fn prize(&self, held: &[usize], sealed: &[usize], doors: usize, island: bool) -> Option<Prize> {
        let frames = self.ground.map.extractor_frames();
        let size = BuildingKind::Extractor.size();
        let nearest = |dist: &[u32]| {
            held.iter()
                .flat_map(|&f| footprint(frames[f], size))
                .map(|t| dist[self.ground.index(t)])
                .min()
                .filter(|&d| d != u32::MAX)
                .map(|d| as_index(i32::try_from((d + 5) / 10).expect("steps fit")))
        };
        let mut arrivals: Vec<(Arrival, PlayerId)> = self
            .starts
            .iter()
            .zip(self.walks.iter().zip(&self.flights))
            .filter_map(|(&(player, _), (walk, flight))| {
                let (steps, by_air) = nearest(walk)
                    .map(|steps| (steps, false))
                    .or_else(|| nearest(flight).map(|steps| (steps, true)))?;
                Some((
                    Arrival {
                        seat: player.0,
                        steps,
                        by_air,
                    },
                    player,
                ))
            })
            .collect();
        arrivals.sort_by_key(|(a, _)| (a.steps, a.seat));
        let &(first, leader) = arrivals.first()?;
        let rival = arrivals
            .iter()
            .find(|(_, p)| self.state.hostile(leader, *p))
            .map(|(a, _)| *a);
        Some(Prize {
            frames: held.iter().map(|&f| (frames[f].x, frames[f].y)).collect(),
            island,
            doors,
            first,
            rival,
            shelling: self.shelling(held, sealed),
        })
    }

    /// Ground outside `sealed` that can shell one of the `held` frames.
    fn shelling(&self, held: &[usize], sealed: &[usize]) -> usize {
        let ground = self.ground;
        let frames = ground.map.extractor_frames();
        let (w, h) = BuildingKind::Extractor.size();
        let reach = longest_reach();
        let mut inside = vec![false; ground.cells()];
        for &i in sealed {
            inside[i] = true;
        }
        let point = |x: f64, y: f64| Vec2Fx::new(Fx::from_num(x), Fx::from_num(y));
        let targets: Vec<(TilePos, u8)> = held
            .iter()
            .map(|&f| (frames[f], ground.top(frames[f])))
            .collect();
        (0..ground.cells())
            .filter(|&i| !inside[i] && ground.open(ground.tile(i)))
            .filter(|&i| {
                let t = ground.tile(i);
                let (cx, cy) = (f64::from(t.x) + 0.5, f64::from(t.y) + 0.5);
                targets.iter().any(|&(anchor, level)| {
                    let near_x = cx.clamp(f64::from(anchor.x), f64::from(anchor.x + w));
                    let near_y = cy.clamp(f64::from(anchor.y), f64::from(anchor.y + h));
                    ground.top(t) >= level
                        && (near_x - cx).hypot(near_y - cy) <= reach
                        && !chassis::path::line_blocked(point(cx, cy), point(near_x, near_y), |p| {
                            ground
                                .map
                                .tile(p)
                                .is_none_or(|tile| tile.terrain != Terrain::Peak)
                        })
                })
            })
            .count()
    }
}

#[cfg(test)]
mod tests;
