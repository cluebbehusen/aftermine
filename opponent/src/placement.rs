//! Whether the seat's own knowledge allows a building footprint. The check
//! reads only the fog-honest observation, so hidden units and unseen buildings
//! can never change its verdict; the simulation re-checks on arrival and a
//! refused order is re-planned.

use crate::frame::{gap, ring, tiles_of};
use crate::map::{MapModel, Shot};
use crate::memory::Memory;
use aftermine_sim::observation::{BuildingObs, ObservationData, UnitObs};
use aftermine_sim::stats::{
    Domain, FOUNDRY_REPAIR_TICKS, HARVEST_INCIDENT_DANGER_RADIUS, WeaponStats,
};
use aftermine_sim::{BuildingKind, UnitId};
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

/// What the fire of the armed enemies in sight and the known enemy guns within
/// reach of a footprint deals one target there: one volley of every weapon at
/// once, then their damage per thousand ticks.
#[derive(Clone, Copy, Default)]
struct Barrage {
    volley: u64,
    rate: u64,
}

impl Barrage {
    /// Whether `hp` of health outlasts this fire, all turned on it, for
    /// `ticks`: the volley lands at once and the rest follows on it.
    fn spares(self, hp: u32, ticks: u32) -> bool {
        u64::from(hp) * 1_000 > self.volley * 1_000 + self.rate * u64::from(ticks)
    }

    /// Whether `worker` pays its way at a job beside the footprint worth
    /// `worth` scrap that takes it `ticks`, were all this fire turned on it:
    /// it finishes before it falls, or does at least its own price of the job
    /// first.
    fn pays(self, worker: &UnitObs, worth: u64, ticks: u32) -> bool {
        if self.spares(worker.hp, ticks) {
            return true;
        }
        let lasts =
            (u64::from(worker.hp) * 1_000).saturating_sub(self.volley * 1_000) / self.rate.max(1);
        worth * lasts >= u64::from(worker.kind.stats().cost) * u64::from(ticks)
    }
}

/// The fire known to reach the footprint of `kind` at `anchor`: what it deals
/// the building, and what it deals a worker anywhere on the ring beside it.
/// Enemies in sight count in full, and those remembered out of sight by how
/// sure the seat is they are still there: a gun that outranges what it hits
/// shoots from the fog.
struct Fire {
    building: Barrage,
    worker: Barrage,
}

impl Fire {
    fn on(
        observation: &ObservationData,
        map: &MapModel,
        memory: &Memory,
        kind: BuildingKind,
        anchor: TilePos,
    ) -> Self {
        let footprint = Footprint::of(kind, anchor);
        let ring = footprint.grown();
        let mut fire = Fire {
            building: Barrage::default(),
            worker: Barrage::default(),
        };
        let world = |(x, y): (i64, i64)| Vec2Fx::new(Fx::from_num(x) / 2, Fx::from_num(y) / 2);
        // A weapon's range runs from its shooter's doubled `centre` to the
        // target's nearest point, and the shot there must clear rock and
        // levels. A worker stands somewhere on the ring, so a gun reaches it
        // when any of the ring lies between the gun's blind ring and its
        // range.
        // `sure` is per mille of a shooter's fire counted: all of it for one in
        // sight, less for one remembered. A breacher never turns its weapon on
        // a machine, so it deals a worker nothing.
        let mut add = |weapons: &[WeaponStats],
                       centre: (i64, i64),
                       footing: TilePos,
                       (from, machines): (Domain, bool),
                       sure: u64| {
            for weapon in weapons.iter().filter(|weapon| weapon.targets.ground) {
                let (reach, minimum) = (
                    weapon.range + weapon.range,
                    weapon.minimum_range + weapon.minimum_range,
                );
                let (reach, minimum) = (reach * reach, minimum * minimum);
                let shot = Shot::of(weapon, from, Domain::Ground);
                let clear = |target: Footprint| {
                    let near = target.nearest(centre);
                    map.clear_shot(
                        world(centre),
                        world(near),
                        (footing, target.tile_at(near)),
                        shot,
                    )
                };
                let cooldown = u64::from(weapon.cooldown_ticks.max(1));
                let building = Fx::from_num(footprint.near(centre)) <= reach
                    && Fx::from_num(footprint.near(centre)) >= minimum
                    && clear(footprint);
                let worker = machines
                    && Fx::from_num(ring.near(centre)) <= reach
                    && Fx::from_num(ring.far(centre)) >= minimum
                    && clear(ring);
                for (barrage, hits, damage) in [
                    (
                        &mut fire.building,
                        building,
                        weapon.building_damage.unwrap_or(weapon.damage),
                    ),
                    (&mut fire.worker, worker, weapon.damage),
                ] {
                    if hits {
                        barrage.volley += u64::from(damage) * sure / 1_000;
                        barrage.rate += u64::from(damage) * sure / cooldown;
                    }
                }
            }
        };
        for enemy in &observation.enemy_units {
            let centre = (
                i64::from(2 * enemy.tile.x + 1),
                i64::from(2 * enemy.tile.y + 1),
            );
            add(
                enemy.kind.stats().weapons,
                centre,
                enemy.tile,
                (enemy.body_domain(), turns_on_machines(enemy.kind)),
                1_000,
            );
        }
        let now = observation.tick;
        let home = map
            .start(observation.me)
            .and_then(|start| map.component(start));
        let mut visible: Vec<UnitId> = observation
            .enemy_units
            .iter()
            .map(|enemy| enemy.id)
            .collect();
        visible.sort_unstable();
        for seen in memory
            .units()
            .iter()
            .filter(|seen| visible.binary_search(&seen.id).is_err())
        {
            let centre = (
                i64::from(2 * seen.tile.x + 1),
                i64::from(2 * seen.tile.y + 1),
            );
            add(
                seen.kind.stats().weapons,
                centre,
                seen.tile,
                (seen.kind.stats().domain, turns_on_machines(seen.kind)),
                // One that cannot leave its ground stays where it was seen.
                if crate::missions::stranded(seen, map, home) {
                    1_000
                } else {
                    u64::from(seen.confidence(now))
                },
            );
        }
        // A site seen still going up fires at nothing yet.
        for enemy in observation
            .enemy_buildings
            .iter()
            .filter(|enemy| enemy.built || !enemy.seen)
        {
            let (width, height) = enemy.kind.size();
            let centre = (
                i64::from(2 * enemy.anchor.x + width),
                i64::from(2 * enemy.anchor.y + height),
            );
            add(
                enemy.kind.tier_stats(enemy.tier).weapons,
                centre,
                enemy.anchor,
                (Domain::Ground, true),
                1_000,
            );
        }
        fire
    }
}

/// Whether a unit of `kind` ever turns its weapons on a machine: a breacher
/// picks only structures.
fn turns_on_machines(kind: aftermine_sim::UnitKind) -> bool {
    kind.stats().target_priority != aftermine_sim::stats::TargetPriority::Buildings
}

/// A site going up: its kind and anchor, its health, and the construction
/// work done on it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Raising {
    kind: BuildingKind,
    anchor: TilePos,
    hp: u32,
    progress: u32,
}

impl Raising {
    /// A site the seat would claim: placed at a fifth of its health with no
    /// work done.
    pub(crate) fn fresh(kind: BuildingKind, anchor: TilePos) -> Self {
        Self {
            kind,
            anchor,
            hp: kind.base_stats().max_hp / 5,
            progress: 0,
        }
    }

    /// The own unfinished `site` as its owner sees it.
    pub(crate) fn of(observation: &ObservationData, site: &BuildingObs) -> Self {
        let progress = observation
            .my_buildings
            .iter()
            .position(|building| building.id == site.id)
            .and_then(|index| observation.own_build_progress(index))
            .unwrap_or(0);
        Self {
            kind: site.kind,
            anchor: site.anchor,
            hp: site.hp,
            progress,
        }
    }
}

/// Whether `site` would stand before the fire that reaches it pulls it down,
/// raised by the `crew` already at it together with the `sent` workers, each
/// of whom pays their way under that fire. Nobody works a site before a
/// builder reaches it, so it stands at its health until the nearest of them
/// walks there; then it rises to full health over the work left at their
/// combined rate. A site that would not stand only
/// feeds the attackers its price and its builders, as does one in a crusher's
/// way before it stands.
pub(crate) fn stands(
    observation: &ObservationData,
    map: &MapModel,
    memory: &Memory,
    site: Raising,
    crew: &[&UnitObs],
    sent: &[&UnitObs],
) -> bool {
    let stats = site.kind.base_stats();
    let (build, max) = (stats.construction.build_ticks, stats.max_hp);
    let rate: u32 = crew
        .iter()
        .chain(sent)
        .map(|worker| worker.kind.stats().build_rate)
        .sum::<u32>()
        .max(1);
    let left = build.saturating_sub(site.progress);
    let ticks = left.div_ceil(rate);
    let walk = crew
        .iter()
        .chain(sent)
        .map(|worker| walk_ticks(worker, site))
        .min()
        .unwrap_or(0);
    let worth = u64::from(stats.construction.cost) * u64::from(left) / u64::from(build.max(1));
    let fire = Fire::on(observation, map, memory, site.kind, site.anchor);
    !crate::crush::in_way(
        observation,
        site.kind,
        site.anchor,
        u64::from(walk.saturating_add(ticks)),
    ) && fire.building.spares(site.hp.min(max), walk)
        && fire.building.spares(max, walk.saturating_add(ticks))
        && sent.iter().all(|worker| {
            let share = worth * u64::from(worker.kind.stats().build_rate) / u64::from(rate);
            fire.worker.pays(worker, share, ticks)
        })
}

/// The ticks `worker` takes to walk to `site`, as the crow flies.
fn walk_ticks(worker: &UnitObs, site: Raising) -> u32 {
    let tiles = gap(site.anchor, site.kind.size(), worker.tile, (1, 1)).max(0);
    let speed = worker.kind.stats().speed;
    if speed <= Fx::ZERO {
        return u32::MAX;
    }
    (Fx::from_num(tiles) / speed).ceil().to_num::<u32>()
}

/// How many welders like `welder`, welding the own `building` together, add
/// health faster than the fire that reaches it takes it away, along the
/// simulation's welding ramp (a Foundry's own, any other building's build
/// time); none when they would not pay their way under that fire, the health
/// each restores worth its share of what the building cost.
pub(crate) fn welders(
    observation: &ObservationData,
    map: &MapModel,
    memory: &Memory,
    building: &BuildingObs,
    welder: &UnitObs,
) -> Option<u64> {
    let (kind, tier) = (building.kind, building.tier);
    let stats = kind.tier_stats(tier);
    let ramp_ticks = if kind == BuildingKind::Foundry {
        FOUNDRY_REPAIR_TICKS
    } else {
        stats.construction.build_ticks
    };
    let ramp = u64::from(stats.max_hp - stats.max_hp / 5) * 1_000 / u64::from(ramp_ticks.max(1));
    let missing = u64::from(stats.max_hp.saturating_sub(building.hp));
    let worth = u64::from(kind.invested_cost(tier)) * missing / u64::from(stats.max_hp.max(1));
    let fire = Fire::on(observation, map, memory, kind, building.anchor);
    let ramp = ramp.max(1);
    let welders = fire.building.rate / ramp + 1;
    let gain = welders * ramp - fire.building.rate;
    let ticks = u32::try_from((missing * 1_000).div_ceil(gain)).unwrap_or(u32::MAX);
    fire.worker
        .pays(welder, worth / welders, ticks)
        .then_some(welders)
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

    /// The squared doubled distance from a doubled `point` to the
    /// footprint's farthest point.
    fn far(self, point: (i64, i64)) -> i64 {
        let low = (i64::from(2 * self.anchor.x), i64::from(2 * self.anchor.y));
        let high = (
            i64::from(2 * (self.anchor.x + self.width)),
            i64::from(2 * (self.anchor.y + self.height)),
        );
        let dx = (point.0 - low.0).abs().max((point.0 - high.0).abs());
        let dy = (point.1 - low.1).abs().max((point.1 - high.1).abs());
        dx.pow(2) + dy.pow(2)
    }

    /// The footprint's tile holding the doubled `point` on or in it.
    fn tile_at(self, point: (i64, i64)) -> TilePos {
        let tile = |doubled: i64, low: i32, span: i32| {
            i32::try_from(doubled / 2)
                .unwrap_or(low)
                .clamp(low, low + span - 1)
        };
        TilePos::new(
            tile(point.0, self.anchor.x, self.width),
            tile(point.1, self.anchor.y, self.height),
        )
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

/// The seat's own structures hit lately by fire nothing it knows of accounts
/// for: an incident its team remembers stands on the footprint, and no enemy
/// in sight nor any known enemy gun can hit it. A Lancer outranges what an
/// Extractor sees, so it shoots from the fog.
pub(crate) fn unexplained<'a>(
    observation: &'a ObservationData,
    map: &MapModel,
) -> Vec<&'a BuildingObs> {
    if observation.salvage_incidents.is_empty() {
        return Vec::new();
    }
    observation
        .my_buildings
        .iter()
        .filter(|building| !building.provisional)
        .filter(|building| {
            let size = building.kind.size();
            observation
                .salvage_incidents
                .iter()
                .any(|tile| gap(building.anchor, size, *tile, (1, 1)) < 0)
        })
        .filter(|building| {
            // Explained only by fire in sight or a known gun that can really
            // reach it, past minimum ranges, rock and levels.
            let fire = Fire::on(
                observation,
                map,
                &Memory::default(),
                building.kind,
                building.anchor,
            );
            fire.building.volley == 0 && fire.building.rate == 0
        })
        .collect()
}
