//! Whether the seat's own knowledge allows a building footprint. The check
//! reads only the fog-honest observation, so hidden units and unseen buildings
//! can never change its verdict; the simulation re-checks on arrival and a
//! refused order is re-planned.

use crate::frame::{gap, ring, tiles_of};
use crate::map::{MapModel, Shot};
use crate::memory::Memory;
use aftermine_sim::BuildingKind;
use aftermine_sim::observation::{BuildingObs, ObservationData};
use aftermine_sim::stats::{
    Domain, FOUNDRY_REPAIR_TICKS, HARVEST_INCIDENT_DANGER_RADIUS, WeaponStats,
};
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;

/// Why the seat's knowledge rules a footprint out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// A required building is not complete.
    Prerequisite,
    /// Part of the footprint is off the map or unexplored.
    Unexplored,
    /// An Extractor off a frame, or anything else on one.
    Frame,
    /// Known rock, pit, peak or scrap.
    Terrain,
    /// A known building stands there.
    Occupied,
    /// It would touch one of the seat's own buildings, which could wall in a
    /// producer.
    Crowded,
    /// A visible hostile ground unit stands there.
    HostileUnit,
    /// A footprint this decision already planned holds the ground.
    Claimed,
    /// No explored open tile borders it.
    NoEgress,
}

/// How near the seat's own buildings a footprint may stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    /// A tile clear of every own building, as a Foundry needs for the
    /// workers and new units around it, and a defense on its own.
    Apart,
    /// Packed into a block of the base's layout, whose lanes keep the way
    /// open: clear only of own Foundries and their rings.
    Packed,
}

/// A footprint the seat may claim, and whether it must be claimed as a
/// provisional scaffold because part of it is out of sight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Allowed {
    pub(crate) defer: bool,
}

/// Damage per thousand ticks that the armed enemies in sight and the known
/// enemy guns within reach of the footprint of `kind` at `anchor` deal it.
fn fire_on(observation: &ObservationData, kind: BuildingKind, anchor: TilePos) -> u64 {
    let size = kind.size();
    let fire = |weapons: &[WeaponStats], gap: i32| -> u64 {
        weapons
            .iter()
            .filter(|weapon| weapon.targets.ground && gap <= weapon.range.ceil().to_num::<i32>())
            .map(|weapon| {
                u64::from(weapon.building_damage.unwrap_or(weapon.damage)) * 1_000
                    / u64::from(weapon.cooldown_ticks.max(1))
            })
            .sum()
    };
    let units: u64 = observation
        .enemy_units
        .iter()
        .map(|enemy| {
            fire(
                enemy.kind.stats().weapons,
                gap(anchor, size, enemy.tile, (1, 1)),
            )
        })
        .sum();
    let guns: u64 = observation
        .enemy_buildings
        .iter()
        .map(|enemy| {
            fire(
                enemy.kind.tier_stats(enemy.tier).weapons,
                gap(anchor, size, enemy.anchor, enemy.kind.size()),
            )
        })
        .sum();
    units + guns
}

/// Whether a site of `kind` raised at `anchor` would stand before the fire
/// that reaches it pulls it down: it rises from a fifth of its health along
/// its build time, so it stands while that fire takes less than its whole
/// health over the build time. A site that would not only feeds the attackers
/// its price and its builders.
pub(crate) fn stands(observation: &ObservationData, kind: BuildingKind, anchor: TilePos) -> bool {
    let stats = kind.base_stats();
    u64::from(stats.max_hp) * 1_000
        > fire_on(observation, kind, anchor) * u64::from(stats.construction.build_ticks)
}

/// Whether one worker welding the own `kind` at `tier` at `anchor` adds
/// health faster than the fire that reaches it takes it away, along the
/// simulation's welding ramp: a Foundry's own, any other building's build
/// time.
pub(crate) fn mends(
    observation: &ObservationData,
    kind: BuildingKind,
    tier: u8,
    anchor: TilePos,
) -> bool {
    let stats = kind.tier_stats(tier);
    let ramp_ticks = if kind == BuildingKind::Foundry {
        FOUNDRY_REPAIR_TICKS
    } else {
        stats.construction.build_ticks
    };
    let ramp = u64::from(stats.max_hp - stats.max_hp / 5);
    ramp * 1_000 / u64::from(ramp_ticks.max(1)) > fire_on(observation, kind, anchor)
}

/// Checks `kind` at `anchor` against the seat's knowledge and the footprints
/// this decision already planned, standing as `layout` allows. A buried own
/// Scuttle Charge blocks nothing, so it keeps no other building away.
pub(crate) fn check(
    map: &MapModel,
    observation: &ObservationData,
    kind: BuildingKind,
    anchor: TilePos,
    planned: &[(BuildingKind, TilePos)],
    layout: Layout,
) -> Result<Allowed, Refusal> {
    if !crate::investments::buildable(observation, kind) {
        return Err(Refusal::Prerequisite);
    }
    let footprint = Footprint::of(kind, anchor);
    if !footprint.tiles().all(|tile| observation.explored(tile)) {
        return Err(Refusal::Unexplored);
    }
    let frame = |tile: TilePos| {
        observation
            .known_frames
            .iter()
            .any(|frame| Footprint::of(BuildingKind::Extractor, *frame).contains(tile))
    };
    let on_frame = if kind == BuildingKind::Extractor {
        !observation.known_frames.contains(&anchor)
    } else {
        footprint.tiles().any(frame)
    };
    if on_frame {
        return Err(Refusal::Frame);
    }
    if footprint
        .tiles()
        .any(|tile| blocked(observation, tile) || (map.ramp(tile) && !kind.fits_ramp()))
    {
        return Err(Refusal::Terrain);
    }
    let known = observation
        .my_buildings
        .iter()
        .chain(&observation.ally_buildings)
        .chain(&observation.enemy_buildings);
    if known
        .clone()
        .any(|building| footprint.overlaps(building_footprint(building)))
    {
        return Err(Refusal::Occupied);
    }
    // Frames sit where the map put them, so an Extractor keeps no gap.
    let clearance = if kind == BuildingKind::Extractor {
        footprint
    } else {
        footprint.grown()
    };
    // Whether an own building or claim keeps this footprint away. A buried
    // charge stands anywhere a building could not.
    let charge = kind == BuildingKind::ScuttleCharge;
    let keeps = |other_kind: BuildingKind, other_anchor: TilePos| {
        let other = Footprint::of(other_kind, other_anchor);
        match layout {
            _ if charge => false,
            Layout::Apart => clearance.overlaps(other),
            Layout::Packed if other_kind == BuildingKind::Foundry => {
                footprint.overlaps(other.grown())
            }
            Layout::Packed => footprint.overlaps(other),
        }
    };
    if observation
        .my_buildings
        .iter()
        .filter(|building| building.kind != BuildingKind::ScuttleCharge)
        .any(|building| keeps(building.kind, building.anchor))
    {
        return Err(Refusal::Crowded);
    }
    if observation
        .enemy_units
        .iter()
        .any(|unit| unit.body_domain() == Domain::Ground && footprint.contains(unit.tile))
    {
        return Err(Refusal::HostileUnit);
    }
    for &(claimed, at) in planned {
        if keeps(claimed, at) {
            return Err(Refusal::Claimed);
        }
    }
    let egress = footprint.grown().tiles().any(|tile| {
        !footprint.contains(tile)
            && observation.explored(tile)
            && !blocked(observation, tile)
            && !known
                .clone()
                .any(|building| building_footprint(building).contains(tile))
    });
    if !egress {
        return Err(Refusal::NoEgress);
    }
    Ok(Allowed {
        defer: !footprint.tiles().all(|tile| observation.visible(tile)),
    })
}

fn blocked(observation: &ObservationData, tile: TilePos) -> bool {
    observation.known_rock_at(tile) || observation.known_scrap_at(tile)
}

fn building_footprint(building: &BuildingObs) -> Footprint {
    Footprint::of(building.kind, building.anchor)
}

/// A rectangle of tiles.
#[derive(Clone, Copy)]
struct Footprint {
    anchor: TilePos,
    width: i32,
    height: i32,
}

impl Footprint {
    fn of(kind: BuildingKind, anchor: TilePos) -> Self {
        let (width, height) = kind.size();
        Self {
            anchor,
            width,
            height,
        }
    }

    /// The footprint and the ring of tiles around it.
    fn grown(self) -> Self {
        Self {
            anchor: self.anchor.offset(-1, -1),
            width: self.width + 2,
            height: self.height + 2,
        }
    }

    fn contains(self, tile: TilePos) -> bool {
        (self.anchor.x..self.anchor.x + self.width).contains(&tile.x)
            && (self.anchor.y..self.anchor.y + self.height).contains(&tile.y)
    }

    fn overlaps(self, other: Self) -> bool {
        self.anchor.x < other.anchor.x + other.width
            && other.anchor.x < self.anchor.x + self.width
            && self.anchor.y < other.anchor.y + other.height
            && other.anchor.y < self.anchor.y + self.height
    }

    fn tiles(self) -> impl Iterator<Item = TilePos> + Clone {
        crate::frame::tiles_of(self.anchor, (self.width, self.height))
    }

    /// The footprint's point nearest a doubled `point`, doubled.
    fn nearest(self, point: (i64, i64)) -> (i64, i64) {
        let low = (i64::from(2 * self.anchor.x), i64::from(2 * self.anchor.y));
        let high = (
            i64::from(2 * (self.anchor.x + self.width)),
            i64::from(2 * (self.anchor.y + self.height)),
        );
        (point.0.clamp(low.0, high.0), point.1.clamp(low.1, high.1))
    }

    /// The squared doubled distance from a doubled `point` to the footprint.
    fn near(self, point: (i64, i64)) -> i64 {
        let (x, y) = self.nearest(point);
        (point.0 - x).pow(2) + (point.1 - y).pow(2)
    }
}

/// What the seat knows can hit a building it would raise: known enemy guns,
/// remembered armed enemy units, and where it lately took losses. Built once
/// a decision, from the seat's own knowledge only.
pub(crate) struct Danger<'a> {
    map: &'a MapModel,
    /// Each known enemy gun's weapons that fire at ground.
    guns: Vec<Gun>,
    /// Each remembered armed enemy unit's tile centre and longest reach at
    /// buildings, doubled.
    units: Vec<((i64, i64), i64)>,
    /// Tiles where the seat lately took damage or losses.
    incidents: Vec<TilePos>,
}

/// A known enemy gun's weapon, by its centre and its reach either side,
/// doubled, the anchor it stands on, and how its shot at a building travels.
struct Gun {
    centre: (i64, i64),
    minimum: i64,
    range: i64,
    footing: TilePos,
    shot: Shot,
}

impl Gun {
    /// The weapons of a known enemy `building` that fire at ground. A site
    /// seen still going up fires at nothing yet.
    fn of(building: &BuildingObs) -> impl Iterator<Item = Gun> + '_ {
        let (width, height) = building.kind.size();
        let centre = (
            i64::from(2 * building.anchor.x + width),
            i64::from(2 * building.anchor.y + height),
        );
        building
            .kind
            .tier_stats(building.tier)
            .weapons
            .iter()
            .filter(move |weapon| weapon.targets.ground && (building.built || !building.seen))
            .map(move |weapon| Gun {
                centre,
                minimum: doubled(weapon.minimum_range),
                range: doubled(weapon.range),
                footing: building.anchor,
                shot: Shot::of(weapon, Domain::Ground, Domain::Ground),
            })
    }

    /// Whether this gun can hit `footprint`: the footprint's nearest point,
    /// which the gun aims at and measures its range to, lies within its
    /// reach and the shot there is clear.
    fn hits(&self, map: &MapModel, footprint: Footprint) -> bool {
        let distance = footprint.near(self.centre);
        let world = |(x, y): (i64, i64)| Vec2Fx::new(Fx::from_num(x) / 2, Fx::from_num(y) / 2);
        distance <= self.range * self.range
            && distance >= self.minimum * self.minimum
            && map.clear_shot(
                world(self.centre),
                world(footprint.nearest(self.centre)),
                (self.footing, footprint.anchor),
                self.shot,
            )
    }
}

/// Whether a ground weapon of the known enemy `building` can hit a `kind` at
/// `anchor`.
pub(crate) fn gun_hits(
    map: &MapModel,
    building: &BuildingObs,
    kind: BuildingKind,
    anchor: TilePos,
) -> bool {
    let footprint = Footprint::of(kind, anchor);
    Gun::of(building).any(|gun| gun.hits(map, footprint))
}

/// A range in tiles, doubled.
fn doubled(range: Fx) -> i64 {
    (range + range).to_num::<i64>()
}

impl<'a> Danger<'a> {
    pub(crate) fn of(observation: &ObservationData, map: &'a MapModel, memory: &Memory) -> Self {
        let guns = observation
            .enemy_buildings
            .iter()
            .flat_map(Gun::of)
            .collect();
        let units = memory
            .units()
            .iter()
            .map(|unit| {
                (
                    (
                        i64::from(2 * unit.tile.x + 1),
                        i64::from(2 * unit.tile.y + 1),
                    ),
                    doubled(crate::defenses::reach(unit.kind)),
                )
            })
            .filter(|(_, reach)| *reach > 0)
            .collect();
        Danger {
            map,
            guns,
            units,
            incidents: observation.salvage_incidents.clone(),
        }
    }

    /// Whether something the seat knows of could hit a `kind` at `anchor`,
    /// or it lately took losses beside it.
    pub(crate) fn hits(&self, kind: BuildingKind, anchor: TilePos) -> bool {
        let footprint = Footprint::of(kind, anchor);
        let gun = self.guns.iter().any(|gun| gun.hits(self.map, footprint));
        let unit = self
            .units
            .iter()
            .any(|(at, reach)| footprint.near(*at) <= reach * reach);
        let incident = self.incidents.iter().any(|tile| {
            let gap_x = (footprint.anchor.x - tile.x)
                .max(tile.x - (footprint.anchor.x + footprint.width - 1));
            let gap_y = (footprint.anchor.y - tile.y)
                .max(tile.y - (footprint.anchor.y + footprint.height - 1));
            gap_x.max(gap_y) <= HARVEST_INCIDENT_DANGER_RADIUS
        });
        gun || unit || incident
    }
}

/// Whether a `kind` at `anchor` still leaves its ground open: an exit beside
/// every own producer, every worked home scrap node and every hostile start
/// on that ground, a tile to work from beside itself, every own site still
/// going up and every own building it borders, which would otherwise be
/// beyond repair, and no open ground beside it cut off, where units would
/// be trapped or trained into a pocket they never leave. One flood fill over
/// the ground from the rings of the seat's Foundries on it, with known
/// buildings, `planned` footprints, known live scrap and the new footprint
/// in the way. A buried Scuttle Charge blocks nothing.
pub(crate) fn keeps_paths(
    observation: &ObservationData,
    map: &MapModel,
    kind: BuildingKind,
    anchor: TilePos,
    planned: &[(BuildingKind, TilePos)],
) -> bool {
    let me = observation.me;
    let Some(ground) = map.component(anchor) else {
        return false;
    };
    let (width, height) = (observation.map_width, observation.map_height);
    let index = |tile: TilePos| {
        ((0..width).contains(&tile.x) && (0..height).contains(&tile.y))
            .then(|| tile.row_major(width))
    };
    let tiles = |kind: BuildingKind, anchor: TilePos| tiles_of(anchor, kind.size());
    let claims: Vec<(BuildingKind, TilePos)> = planned
        .iter()
        .copied()
        .filter(|(kind, _)| *kind != BuildingKind::ScuttleCharge)
        .collect();
    let mut blocked = vec![false; chassis::grid::cell_count(width, height)];
    let solid = observation
        .my_buildings
        .iter()
        .chain(&observation.ally_buildings)
        .chain(&observation.enemy_buildings)
        .filter(|building| building.kind != BuildingKind::ScuttleCharge)
        .map(|building| (building.kind, building.anchor))
        .chain(claims.iter().copied())
        .chain([(kind, anchor)]);
    for (kind, anchor) in solid {
        for tile in tiles(kind, anchor) {
            if let Some(at) = index(tile) {
                blocked[at] = true;
            }
        }
    }
    for (node, amount) in &observation.known_scrap {
        if let Some(at) = index(*node)
            && *amount > 0
        {
            blocked[at] = true;
        }
    }
    let open = |tile: TilePos| {
        index(tile).is_some_and(|at| !blocked[at]) && map.component(tile) == Some(ground)
    };
    let foundry = BuildingKind::Foundry.size();
    let mut reached = vec![false; chassis::grid::cell_count(width, height)];
    let foundries: Vec<TilePos> = crate::investments::built_foundries(&observation.my_buildings)
        .filter(|building| map.component(building.anchor) == Some(ground))
        .map(|building| building.anchor)
        .collect();
    // Ground no Foundry stands on has no exits to keep; a Foundry ringed
    // shut has none left.
    if foundries.is_empty() {
        return true;
    }
    let mut frontier: Vec<TilePos> = foundries
        .iter()
        .flat_map(|anchor| ring(*anchor, foundry))
        .filter(|tile| open(*tile))
        .collect();
    if frontier.is_empty() {
        return false;
    }
    for tile in &frontier {
        if let Some(at) = index(*tile) {
            reached[at] = true;
        }
    }
    while let Some(tile) = frontier.pop() {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let next = tile.offset(dx, dy);
            if let Some(at) = index(next)
                && !reached[at]
                && open(next)
            {
                reached[at] = true;
                frontier.push(next);
            }
        }
    }
    let on = |tile: TilePos| map.component(tile) == Some(ground);
    let beside = |anchor: TilePos, size: (i32, i32)| {
        ring(anchor, size).any(|tile| index(tile).is_some_and(|at| reached[at]))
    };
    let reached_at = |tile: TilePos| index(tile).is_some_and(|at| reached[at]);
    // Workers build from a tile beside a footprint's edge, not its corner.
    let sides = |kind: BuildingKind, anchor: TilePos| {
        let (w, h) = kind.size();
        let inside = |v: i32, low: i32, len: i32| (low..low + len).contains(&v);
        ring(anchor, (w, h))
            .filter(move |tile| inside(tile.x, anchor.x, w) || inside(tile.y, anchor.y, h))
    };
    let workable = |kind: BuildingKind, anchor: TilePos| sides(kind, anchor).any(reached_at);
    // Any pocket the footprint closes off borders one of its sides.
    let sealing = sides(kind, anchor).any(|tile| open(tile) && !reached_at(tile));
    let covered = |tile: TilePos| tiles(kind, anchor).any(|own| own == tile);
    // Only a neighbour can lose its last side to this footprint: it cuts
    // nothing else off without sealing a pocket beside itself.
    let sites = observation
        .my_buildings
        .iter()
        .filter(|building| building.kind != BuildingKind::ScuttleCharge)
        .filter(|building| !building.built || sides(building.kind, building.anchor).any(covered))
        .map(|building| (building.kind, building.anchor))
        .chain(claims.iter().copied())
        .chain([(kind, anchor)])
        .filter(|(_, anchor)| on(*anchor))
        .all(|(kind, anchor)| workable(kind, anchor));
    let producers = observation
        .my_buildings
        .iter()
        .filter(|building| crate::investments::built_producer(building))
        .filter(|building| on(building.anchor))
        .all(|building| beside(building.anchor, building.kind.size()));
    let nodes = map
        .home_nodes(me)
        .iter()
        .filter(|(node, _)| observation.known_scrap_at(*node) && on(*node))
        .all(|(node, _)| beside(*node, (1, 1)));
    let starts = map
        .hostiles(me)
        .filter_map(|owner| map.start(owner))
        .filter(|anchor| ring(*anchor, foundry).any(on))
        .all(|anchor| beside(anchor, foundry));
    !sealing && sites && producers && nodes && starts
}
