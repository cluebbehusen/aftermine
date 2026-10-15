//! Brain-phase reachability: which ground or sky a unit can reach, the
//! nearest tile it can settle for when its goal lies beyond, and the parked
//! crowds that end such a walk early.
//!
//! Everything here is scratch owned by one brain phase and rebuilt lazily.
//! Nothing reaches [`State`]; the answers depend only on the world the phase
//! started from plus the passability writes the owner reports.

use super::goals::ring;
use super::spatial::UnitIndex;
use crate::ids::{PlayerId, UnitId};
use crate::state::{Order, State};
use crate::stats::Domain;
use chassis::fx::Fx;
use chassis::grid::{CARDINALS, TilePos};
use std::collections::BTreeMap;

/// Up to four component labels a unit can route into, ascending and padded
/// with zeros. An empty set means the unit is sealed in.
type LabelSet = [u32; 4];

/// Reachability answers for one brain phase.
///
/// Component labelings match [`chassis::path::astar`] reachability under the
/// same passability the routes use, so a target outside the unit's labels is
/// one no route can reach. Labels are built on first use and describe
/// passability at that moment. The only passability write inside the brain
/// phase is a scrap node running dry, which a harvester reports with
/// [`crate::Event::NodeDepleted`]; the owner must call
/// [`Reach::forget_ground`] after any brain that emitted one. Sky
/// passability never changes during a match.
///
/// Settled crowds are judged against the bodies that stood idle, pathless,
/// and stopped when the phase began, so the answer never depends on which
/// brain asked first.
pub(super) struct Reach {
    ground: Option<Vec<u32>>,
    air: Option<Vec<u32>>,
    /// Ground a hull wider than a tile can cross, by its radius.
    wide: BTreeMap<i64, Vec<u32>>,
    endpoints: BTreeMap<(i64, LabelSet, TilePos, bool), Option<TilePos>>,
    /// Crowd chains by movement layer, the asking walker's team, and
    /// endpoint.
    crowds: BTreeMap<(u8, u8, TilePos), Vec<usize>>,
    /// Movement layer and owner of each unit slot that is parked for crowd
    /// purposes.
    parked: Vec<Option<(Domain, PlayerId)>>,
}

fn domain_key(domain: Domain) -> u8 {
    match domain {
        Domain::Ground => 0,
        Domain::Air => 1,
    }
}

/// The ground or sky a body routes over: small ground bodies share one
/// layer, the sky another, and each wider hull radius its own ground.
#[derive(Clone, Copy)]
enum Layer {
    Ground,
    Air,
    Wide(Fx),
}

impl Layer {
    fn of(unit: &crate::state::Unit) -> Self {
        let stats = unit.kind.stats();
        match stats.domain {
            Domain::Air => Self::Air,
            Domain::Ground if crate::stats::hull_excess(stats.radius) > Fx::ZERO => {
                Self::Wide(stats.radius)
            }
            Domain::Ground => Self::Ground,
        }
    }

    /// A key no two layers share: a wide radius is always above one.
    fn key(self) -> i64 {
        match self {
            Self::Ground => 0,
            Self::Air => 1,
            Self::Wide(radius) => radius.to_bits(),
        }
    }
}

impl Reach {
    /// Scratch for a brain phase starting from `state`.
    pub(super) fn new(state: &State) -> Self {
        let parked = state
            .units
            .iter()
            .map(|unit| {
                (unit.hp > 0
                    && unit.path.is_none()
                    && unit.drive_speed() == Fx::ZERO
                    && unit.order == Order::Idle)
                    .then(|| (unit.domain(), unit.player))
            })
            .collect();
        Self {
            ground: None,
            air: None,
            wide: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            crowds: BTreeMap::new(),
            parked,
        }
    }

    /// Drops everything derived from ground passability after a scrap node
    /// opened its tile.
    pub(super) fn forget_ground(&mut self) {
        self.ground = None;
        self.wide.clear();
        self.endpoints
            .retain(|&(layer, ..), _| layer == Layer::Air.key());
    }

    /// The tile unit `id` should route to for `target`: the target itself
    /// when it can be reached, otherwise the reachable tile nearest it.
    /// `None` means the unit cannot move anywhere.
    ///
    /// Among equally near tiles the ring scan runs in the spread-slot order,
    /// half-turned by the frame between the target and the unit, so mirrored
    /// units settle on mirrored tiles. `stored` is the endpoint the order
    /// already holds; it wins a tie so a recompute never hops the unit to an
    /// equally near partner tile.
    pub(super) fn endpoint(
        &mut self,
        state: &State,
        id: UnitId,
        target: TilePos,
        stored: Option<TilePos>,
    ) -> Option<TilePos> {
        let unit = state.unit(id)?;
        let layer = Layer::of(unit);
        let from = unit.tile();
        let player = unit.player;
        let labels = layer_labels(
            &mut self.ground,
            &mut self.air,
            &mut self.wide,
            state,
            layer,
        );
        let width = state.map.width();
        let height = state.map.height();
        let label = |tile: TilePos| label_at(labels, width, height, tile);
        // A* refuses a start off the map, so a unit standing there is sealed.
        let on_map = from.x >= 0 && from.y >= 0 && from.x < width && from.y < height;
        let set = if on_map {
            label_set(&label, from)
        } else {
            [0; 4]
        };
        if set[0] == 0 {
            return None;
        }
        let member = |tile: TilePos| {
            let l = label(tile);
            l != 0 && set.contains(&l)
        };
        if member(target) {
            return Some(target);
        }
        let center = TilePos::new(
            target.x.clamp(0, width.max(1) - 1),
            target.y.clamp(0, height.max(1) - 1),
        );
        let reverse = scan_reversed(state, player, center, from);
        let key = (layer.key(), set, center, reverse);
        let best = if let Some(&best) = self.endpoints.get(&key) {
            best
        } else {
            let best = nearest(center, reverse, width.max(height), member);
            self.endpoints.insert(key, best);
            best
        };
        let best = best?;
        if let Some(stored) = stored
            && member(stored)
            && distance_sq(stored, center) == distance_sq(best, center)
        {
            return Some(stored);
        }
        Some(best)
    }

    /// Whether any route can bring unit `id` onto one of `tiles`.
    pub(super) fn reaches(
        &mut self,
        state: &State,
        id: UnitId,
        tiles: impl IntoIterator<Item = TilePos>,
    ) -> bool {
        let Some(unit) = state.unit(id) else {
            return false;
        };
        let layer = Layer::of(unit);
        let from = unit.tile();
        let labels = layer_labels(
            &mut self.ground,
            &mut self.air,
            &mut self.wide,
            state,
            layer,
        );
        let width = state.map.width();
        let height = state.map.height();
        let label = |tile: TilePos| label_at(labels, width, height, tile);
        if from.x < 0 || from.y < 0 || from.x >= width || from.y >= height {
            return false;
        }
        let set = label_set(&label, from);
        tiles.into_iter().any(|tile| {
            let l = label(tile);
            l != 0 && set.contains(&l)
        })
    }

    /// Whether unit `id` touches a parked body of its own layer that
    /// connects, through touching parked bodies, to one within
    /// [`crate::stats::ARRIVAL_NEAR`] of `endpoint`, counting only bodies
    /// its side may count. A short walk that meets the crowd already
    /// standing at its endpoint has gone as far as it can.
    pub(super) fn crowd_touches(
        &mut self,
        state: &State,
        index: &UnitIndex,
        id: UnitId,
        endpoint: TilePos,
    ) -> bool {
        let Some(unit) = state.unit(id) else {
            return false;
        };
        // Every chain body's edge lies within CROWD_CHAIN_REACH of the
        // endpoint and any touch spans less than the contact window.
        let window = index.contact_window();
        let span = crate::stats::CROWD_CHAIN_REACH
            + Fx::from_num(index.wide_reach())
            + Fx::from_num(window);
        if unit.pos.dist_sq(endpoint.center()) > span * span {
            return false;
        }
        let domain = unit.domain();
        let team = state.player(unit.player).team;
        let key = (domain_key(domain), team, endpoint);
        if !self.crowds.contains_key(&key) {
            let chain = self.crowd(state, index, domain, unit.player, endpoint);
            self.crowds.insert(key, chain);
        }
        let chain = &self.crowds[&key];
        if chain.is_empty() {
            return false;
        }
        let tile = unit.tile();
        (tile.y - window..=tile.y + window).any(|y| {
            index
                .row_span(y, tile.x - window, tile.x + window)
                .iter()
                .any(|&(_, slot)| {
                    let other = &state.units[slot];
                    other.id != id && chain.binary_search(&slot).is_ok() && touching(unit, other)
                })
        })
    }

    /// The parked bodies of `domain` connected to the arrival disk around
    /// `endpoint`, as sorted unit slots, as `player`'s side may count them.
    /// Membership is a set, so the visit order cannot change it.
    ///
    /// Only bodies not hostile to `player` count, whether they seed the
    /// chain inside the disk or link it beyond: a hostile body may stand
    /// outside the side's sight, and counting it would reveal where it
    /// stands and whether it is idle.
    fn crowd(
        &self,
        state: &State,
        index: &UnitIndex,
        domain: Domain,
        player: PlayerId,
        endpoint: TilePos,
    ) -> Vec<usize> {
        let center = endpoint.center();
        // A wide hull counts once its edge is within the bound.
        let countable = |slot: usize, within: Fx| {
            let body = &state.units[slot];
            let within = within + crate::stats::hull_excess(body.kind.stats().radius);
            self.parked
                .get(slot)
                .copied()
                .flatten()
                .is_some_and(|(layer, owner)| layer == domain && !state.hostile(player, owner))
                && body.pos.dist_sq(center) <= within * within
        };
        let mut chain = Vec::new();
        let mut frontier = Vec::new();
        let seed = crate::stats::ARRIVAL_NEAR.to_num::<i32>() + 1 + index.wide_reach();
        for y in endpoint.y - seed..=endpoint.y + seed {
            for &(_, slot) in index.row_span(y, endpoint.x - seed, endpoint.x + seed) {
                if countable(slot, crate::stats::ARRIVAL_NEAR) {
                    chain.push(slot);
                    frontier.push(slot);
                }
            }
        }
        chain.sort_unstable();
        let window = index.contact_window();
        while let Some(slot) = frontier.pop() {
            let body = &state.units[slot];
            let tile = body.tile();
            for y in tile.y - window..=tile.y + window {
                for &(_, other) in index.row_span(y, tile.x - window, tile.x + window) {
                    if !countable(other, crate::stats::CROWD_CHAIN_REACH)
                        || !touching(body, &state.units[other])
                    {
                        continue;
                    }
                    if let Err(at) = chain.binary_search(&other) {
                        chain.insert(at, other);
                        frontier.push(other);
                    }
                }
            }
        }
        chain
    }
}

/// One layer's component labels, built on first use.
/// The component labels of `layer`, built on first use.
fn layer_labels<'a>(
    ground: &'a mut Option<Vec<u32>>,
    air: &'a mut Option<Vec<u32>>,
    wide: &'a mut BTreeMap<i64, Vec<u32>>,
    state: &State,
    layer: Layer,
) -> &'a [u32] {
    match layer {
        Layer::Ground => labels(ground, state, |t| state.passable(t)),
        Layer::Air => labels(air, state, |t| state.passable_for(Domain::Air, t)),
        Layer::Wide(radius) => wide.entry(radius.to_bits()).or_insert_with(|| {
            chassis::path::cardinal_components(state.map.width(), state.map.height(), |t| {
                crate::geometry::hull_open(t, radius, |c| state.passable(c))
            })
        }),
    }
}

fn labels<'a>(
    slot: &'a mut Option<Vec<u32>>,
    state: &State,
    open: impl Fn(TilePos) -> bool,
) -> &'a [u32] {
    slot.get_or_insert_with(|| {
        chassis::path::cardinal_components(state.map.width(), state.map.height(), open)
    })
}

/// Gap two bodies may leave and still touch, as the arrival wave counts it.
pub(super) const CONTACT_SLACK: Fx = Fx::lit("0.05");

/// Whether two bodies are in contact, with the slack the arrival wave uses.
fn touching(a: &crate::state::Unit, b: &crate::state::Unit) -> bool {
    a.pos.dist(b.pos) <= a.kind.stats().radius + b.kind.stats().radius + CONTACT_SLACK
}

/// A tile's component label; off-map tiles read as closed.
fn label_at(labels: &[u32], width: i32, height: i32, tile: TilePos) -> u32 {
    if tile.x < 0 || tile.y < 0 || tile.x >= width || tile.y >= height {
        return 0;
    }
    labels.get(tile.row_major(width)).copied().unwrap_or(0)
}

/// The components a route from `from` can enter: its own, or, when it stands
/// on a closed tile, those of its open cardinal neighbours. A* never cuts a
/// corner, so a diagonal neighbour alone opens nothing.
fn label_set(label: &impl Fn(TilePos) -> u32, from: TilePos) -> LabelSet {
    let mut set = [0; 4];
    let own = label(from);
    if own != 0 {
        set[0] = own;
        return set;
    }
    let mut len = 0;
    for (dx, dy) in CARDINALS {
        let l = label(from.offset(dx, dy));
        if l != 0 && !set[..len].contains(&l) {
            set[len] = l;
            len += 1;
        }
    }
    set[..len].sort_unstable();
    set
}

/// The half-turn frame for a unit's endpoint scan: the unit's approach to
/// the target, falling back to its owner's first Foundry when it stands on
/// the target itself.
fn scan_reversed(
    state: &State,
    player: crate::ids::PlayerId,
    center: TilePos,
    from: TilePos,
) -> bool {
    let foundry = state
        .buildings
        .iter()
        .find(|building| {
            building.player == player
                && !building.provisional()
                && building.kind == crate::stats::BuildingKind::Foundry
        })
        .map(|building| (building.anchor, building.kind.size()));
    super::group_spread_scan_reversed(
        center,
        [from],
        foundry,
        (state.map.width(), state.map.height()),
        player,
    )
}

fn distance_sq(a: TilePos, b: TilePos) -> i64 {
    let dx = i64::from(a.x) - i64::from(b.x);
    let dy = i64::from(a.y) - i64::from(b.y);
    dx * dx + dy * dy
}

/// The accepted tile nearest `center` by squared distance, ties to the
/// earliest in the spread-slot ring order. Rings grow until no tile in the
/// next one could be strictly nearer.
fn nearest(
    center: TilePos,
    reverse: bool,
    max_radius: i32,
    accept: impl Fn(TilePos) -> bool,
) -> Option<TilePos> {
    let mut best: Option<(i64, TilePos)> = None;
    for r in 0..=max_radius {
        if best.is_some_and(|(d, _)| i64::from(r) * i64::from(r) >= d) {
            break;
        }
        for (dx, dy) in ring(r) {
            let (dx, dy) = if reverse { (-dx, -dy) } else { (dx, dy) };
            let tile = center.offset(dx, dy);
            if !accept(tile) {
                continue;
            }
            let d = i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy);
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, tile));
            }
        }
    }
    best.map(|(_, tile)| tile)
}

#[cfg(test)]
mod tests;
