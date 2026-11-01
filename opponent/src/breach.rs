//! Breaching: when known fortification is more than the seat's army could
//! attack, a cradle to assemble the experimental that cuts through it, and
//! where a cradle may stand so its machine can drive out. Nothing here
//! persists between decisions.

use crate::gunship::gunship;
use crate::investments::{Investment, capped, weighed};
use crate::map::{MapModel, UNREACHABLE};
use crate::memory::Memory;
use crate::missions::worth;
use crate::profile::PersonalityTraits;
use aftermine_sim::observation::ObservationData;
use aftermine_sim::{BuildingKind, UnitKind};
use chassis::fx::Fx;
use chassis::grid::{Grid, TilePos};
use std::collections::VecDeque;

/// How far past the seat's furthest building a machine must get to have left
/// the base, in the map model's distance units (ten a tile): three tiles.
const EXIT_MARGIN: u16 = 30;

/// How many times the base a cradle scores once a mission its machine would
/// have led has been given up: the scale of the seat's upgrades, so direct
/// evidence that the seat cannot break in competes with its tech.
const GIVEN_UP: u64 = 3;

/// A cradle for each breaching experimental once a Crucible stands. It is
/// scored by [`guns_shortfall`]. While the seat fields and assembles no such
/// machine and no gunship, it is also wanted at [`GIVEN_UP`] times the base
/// while an attack on ground it reaches was lately given up: that attack went
/// without one. Siege weighs it and income scales it, as they do a Spire.
pub(crate) fn investments(
    observation: &ObservationData,
    map: &MapModel,
    memory: &Memory,
    traits: PersonalityTraits,
    income: u32,
    margin: u64,
) -> Vec<(Investment, u32)> {
    let crucible = observation
        .my_buildings
        .iter()
        .any(|building| building.kind == BuildingKind::Crucible && building.built);
    if !crucible {
        return Vec::new();
    }
    let me = observation.me;
    let shortfall = guns_shortfall(observation, map, margin);
    let given_up = fielded(observation, gunship) == 0
        && memory
            .given_up(observation.tick)
            .any(|(_, anchor)| map.distance(me, anchor) != UNREACHABLE);
    let base = u64::from(weighed(traits.siege, income));
    cradles(
        observation,
        crate::missions::breacher,
        shortfall,
        given_up,
        base,
    )
}

/// How far the army, with the machines the seat fields and assembles at their
/// price, falls short of what an attack on the known guns its ground reaches
/// would need, those guns by the attack `margin` per mille. A breacher cuts
/// through them and a gunship flies over them, so either answers this need.
pub(crate) fn guns_shortfall(observation: &ObservationData, map: &MapModel, margin: u64) -> u64 {
    let me = observation.me;
    let fortification: u64 = observation
        .enemy_buildings
        .iter()
        .filter(|building| {
            building
                .kind
                .tier_stats(building.tier)
                .weapons
                .iter()
                .any(|weapon| weapon.targets.ground)
                && map.distance(me, building.anchor) != UNREACHABLE
        })
        .map(crate::missions::building_value)
        .sum();
    let army: u64 = observation
        .my_units
        .iter()
        .filter(|unit| {
            matches!(
                crate::composition::role(unit.kind),
                Some(
                    crate::composition::Role::Line
                        | crate::composition::Role::Siege
                        | crate::composition::Role::AntiHeavy
                )
            )
        })
        .map(|unit| worth(unit.kind, unit.hp))
        .sum();
    let machines = fielded(observation, crate::missions::breacher) + fielded(observation, gunship);
    (fortification * margin / 1_000).saturating_sub(army + machines)
}

/// What the seat's machines that `fits` are worth: those it fields at their
/// health, and each one its cradles are assembling at its price.
pub(crate) fn fielded(observation: &ObservationData, fits: fn(UnitKind) -> bool) -> u64 {
    observation
        .my_units
        .iter()
        .filter(|unit| unit.kind.assembled() && fits(unit.kind))
        .map(|unit| worth(unit.kind, unit.hp))
        .chain(observation.my_buildings.iter().filter_map(|building| {
            building
                .kind
                .assembles()
                .filter(|machine| fits(*machine))
                .map(|machine| u64::from(machine.stats().cost))
        }))
        .sum()
}

/// A score for each cradle whose machine `fits`: `base` for each quarter of
/// the machine's price in `shortfall`, a quarter being what a modest fortress
/// or its flak is worth, and at least [`GIVEN_UP`] times `base` while
/// `given_up` holds and the seat neither fields that machine nor has its
/// cradle.
pub(crate) fn cradles(
    observation: &ObservationData,
    fits: fn(UnitKind) -> bool,
    shortfall: u64,
    given_up: bool,
    base: u64,
) -> Vec<(Investment, u32)> {
    BuildingKind::ALL
        .into_iter()
        .filter_map(|cradle| cradle.assembles().map(|machine| (cradle, machine)))
        .filter(|(_, machine)| fits(*machine))
        .filter_map(|(cradle, machine)| {
            let without = given_up
                && !observation.my_units.iter().any(|unit| unit.kind == machine)
                && !observation
                    .my_buildings
                    .iter()
                    .any(|building| building.kind == cradle);
            let bar = (u64::from(machine.stats().cost) / 4).max(1);
            let score = capped(base, shortfall, bar).max(if without { GIVEN_UP * base } else { 0 });
            (score > 0).then(|| {
                (
                    Investment::Tech(cradle),
                    u32::try_from(score).unwrap_or(u32::MAX),
                )
            })
        })
        .collect()
}

/// Whether the machine a cradle of `kind` at `anchor` stands up could drive
/// out of the seat's base: a flood over the ground its whole hull fits,
/// clear of every known building but the cradle's own footprint, from the
/// cradle's centre to a tile farther from the seat's start than any of its
/// buildings. The flood visits each tile of the map at most once.
pub(crate) fn exits(
    map: &MapModel,
    observation: &ObservationData,
    kind: BuildingKind,
    anchor: TilePos,
) -> bool {
    let Some(machine) = kind.assembles() else {
        return true;
    };
    // A machine that flies leaves however the base is built.
    if machine.stats().domain == aftermine_sim::stats::Domain::Air {
        return true;
    }
    let me = observation.me;
    let (w, h) = kind.size();
    let beyond = observation
        .my_buildings
        .iter()
        .map(|building| map.distance(me, building.anchor))
        .filter(|distance| *distance != UNREACHABLE)
        .max()
        .unwrap_or(0)
        .saturating_add(EXIT_MARGIN);
    let start = anchor.offset(w / 2, h / 2);
    let mut flood = HullFlood::through(
        map,
        observation,
        machine.stats().radius,
        start,
        Some((anchor, (w, h))),
    );
    if !flood.open(start) {
        return false;
    }
    while let Some(tile) = flood.step() {
        let distance = map.distance(me, tile);
        if distance != UNREACHABLE && distance > beyond {
            return true;
        }
    }
    false
}

/// Every tile under a building the seat knows: its own, its allies' and the
/// enemy buildings it remembers. A buried Scuttle Charge blocks nothing.
fn known_footprints(map: &MapModel, observation: &ObservationData) -> Grid<bool> {
    let (width, height) = map.size();
    let mut blocked = Grid::new(width, height, false);
    for building in observation
        .my_buildings
        .iter()
        .chain(&observation.ally_buildings)
        .chain(&observation.enemy_buildings)
        .filter(|building| building.kind != BuildingKind::ScuttleCharge)
    {
        let (bw, bh) = building.kind.size();
        for dy in 0..bh {
            for dx in 0..bw {
                if let Some(cell) = blocked.get_mut(building.anchor.offset(dx, dy)) {
                    *cell = true;
                }
            }
        }
    }
    blocked
}

/// A flood over the ground a body of `radius` fits, clear of every known
/// building, from where it stands. It grows breadth-first only as far as
/// each question needs and visits each tile of the map at most once.
pub(crate) struct HullFlood<'a> {
    map: &'a MapModel,
    blocked: Grid<bool>,
    radius: Fx,
    /// A footprint the body may cross though a building stands on it.
    exempt: Option<(TilePos, (i32, i32))>,
    reached: Grid<bool>,
    frontier: VecDeque<TilePos>,
}

impl<'a> HullFlood<'a> {
    pub(crate) fn new(
        map: &'a MapModel,
        observation: &ObservationData,
        radius: Fx,
        start: TilePos,
    ) -> Self {
        Self::through(map, observation, radius, start, None)
    }

    /// As [`HullFlood::new`], crossing the `exempt` footprint too.
    fn through(
        map: &'a MapModel,
        observation: &ObservationData,
        radius: Fx,
        start: TilePos,
        exempt: Option<(TilePos, (i32, i32))>,
    ) -> Self {
        let (width, height) = map.size();
        let mut reached = Grid::new(width, height, false);
        let mut frontier = VecDeque::new();
        if let Some(cell) = reached.get_mut(start) {
            *cell = true;
            frontier.push_back(start);
        }
        Self {
            map,
            blocked: known_footprints(map, observation),
            radius,
            exempt,
            reached,
            frontier,
        }
    }

    /// Whether the body could drive to `tile`.
    pub(crate) fn reaches(&mut self, tile: TilePos) -> bool {
        while self.reached.get(tile) == Some(&false) {
            if self.step().is_none() {
                return false;
            }
        }
        self.reached.get(tile) == Some(&true)
    }

    /// Grows the flood from the next tile it has reached, and returns that
    /// tile: every tile the flood reaches, once each, nearest first.
    fn step(&mut self) -> Option<TilePos> {
        let from = self.frontier.pop_front()?;
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let next = from.offset(dx, dy);
            if self.reached.get(next) == Some(&false) && self.open(next) {
                if let Some(cell) = self.reached.get_mut(next) {
                    *cell = true;
                }
                self.frontier.push_back(next);
            }
        }
        Some(from)
    }

    fn open(&self, tile: TilePos) -> bool {
        aftermine_sim::geometry::hull_open(tile, self.radius, |cell| {
            self.map.component(cell).is_some()
                && (!self.blocked.get(cell).copied().unwrap_or(true)
                    || self.exempt.is_some_and(|(anchor, (w, h))| {
                        cell.x >= anchor.x
                            && cell.x < anchor.x + w
                            && cell.y >= anchor.y
                            && cell.y < anchor.y + h
                    }))
        })
    }
}
