//! Breaching: when known fortification is more than the seat's army could
//! attack, a cradle to assemble the experimental that cuts through it, and
//! where a cradle may stand so its machine can drive out. Nothing here
//! persists between decisions.

use crate::investments::Investment;
use crate::map::{MapModel, UNREACHABLE};
use crate::memory::Memory;
use crate::profile::PersonalityTraits;
use chassis::fx::Fx;
use chassis::grid::{Grid, TilePos};
use oxide_sim::BuildingKind;
use oxide_sim::observation::ObservationData;
use std::collections::VecDeque;

/// How far past a cradle's bar, a quarter of its machine's price, the
/// fortification it answers raises its score, in multiples of the bar: a
/// scoring shape that keeps a deep fortress on the scale of the seat's other
/// investments.
const WORTH_CAP: u64 = 2;

/// How far past the seat's furthest building a machine must get to have left
/// the base, in the map model's distance units (ten a tile): three tiles.
const EXIT_MARGIN: u16 = 30;

/// How many times the base a cradle scores once an attack on ground the seat
/// reaches has been given up: the scale of the seat's upgrades, so direct
/// evidence that its army cannot break in competes with its tech.
const GIVEN_UP: u64 = 3;

/// A cradle for each breaching experimental once a Crucible stands; a
/// machine that does not break structures is no answer to guns. It is scored by
/// how far the army, with the machines the seat fields and assembles at
/// their price, falls short of what an attack on the known fortification its
/// ground reaches would need, that fortification by the attack `margin` per
/// mille. While the seat neither fields nor assembles that machine, it is
/// also wanted at [`GIVEN_UP`] times the base while an attack on that ground
/// was lately given up: that attack went without one. Siege weighs it and
/// income scales it, as they do a Spire.
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
        .map(|unit| {
            let stats = unit.kind.stats();
            u64::from(stats.cost) * u64::from(unit.hp) / u64::from(stats.max_hp.max(1))
        })
        .sum();
    let machines: u64 = observation
        .my_units
        .iter()
        .filter(|unit| assembled(unit.kind) && crate::missions::breacher(unit.kind))
        .map(|unit| {
            let stats = unit.kind.stats();
            u64::from(stats.cost) * u64::from(unit.hp) / u64::from(stats.max_hp.max(1))
        })
        .chain(observation.my_buildings.iter().filter_map(|building| {
            building
                .kind
                .assembles()
                .filter(|machine| crate::missions::breacher(*machine))
                .map(|machine| u64::from(machine.stats().cost))
        }))
        .sum();
    let shortfall = (fortification * margin / 1_000).saturating_sub(army + machines);
    let given_up = memory
        .given_up(observation.tick)
        .any(|(_, anchor)| map.distance(me, anchor) != UNREACHABLE);
    let base = u64::from((150 + 3 * u32::from(traits.siege)) * income.min(360) / 360);
    BuildingKind::ALL
        .into_iter()
        .filter_map(|cradle| cradle.assembles().map(|machine| (cradle, machine)))
        .filter(|(_, machine)| crate::missions::breacher(*machine))
        .filter_map(|(cradle, machine)| {
            let without = given_up
                && !observation.my_units.iter().any(|unit| unit.kind == machine)
                && !observation
                    .my_buildings
                    .iter()
                    .any(|building| building.kind == cradle);
            // A shortfall of a quarter of the machine's price, the guns of a
            // modest fortress, makes it as wanted as the base.
            let bar = (u64::from(machine.stats().cost) / 4).max(1);
            let guns = base * shortfall.min(WORTH_CAP * bar) / bar;
            let score = guns.max(if without { GIVEN_UP * base } else { 0 });
            (score > 0).then(|| {
                (
                    Investment::Tech(cradle),
                    u32::try_from(score).unwrap_or(u32::MAX),
                )
            })
        })
        .collect()
}

/// Whether some cradle assembles `kind`.
fn assembled(kind: oxide_sim::UnitKind) -> bool {
    BuildingKind::ALL
        .into_iter()
        .any(|cradle| cradle.assembles() == Some(kind))
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
    if machine.stats().domain == oxide_sim::stats::Domain::Air {
        return true;
    }
    let me = observation.me;
    let (width, height) = map.size();
    let (w, h) = kind.size();
    let blocked = known_footprints(map, observation);
    let beyond = observation
        .my_buildings
        .iter()
        .map(|building| map.distance(me, building.anchor))
        .filter(|distance| *distance != UNREACHABLE)
        .max()
        .unwrap_or(0)
        .saturating_add(EXIT_MARGIN);
    let radius = machine.stats().radius;
    let open = |tile: TilePos| {
        oxide_sim::geometry::hull_open(tile, radius, |cell| {
            map.component(cell).is_some()
                && (!blocked.get(cell).copied().unwrap_or(true)
                    || (cell.x >= anchor.x
                        && cell.x < anchor.x + w
                        && cell.y >= anchor.y
                        && cell.y < anchor.y + h))
        })
    };
    let start = anchor.offset(w / 2, h / 2);
    let mut seen = Grid::new(width, height, false);
    let mut frontier = vec![start];
    if let Some(cell) = seen.get_mut(start) {
        *cell = true;
    }
    while let Some(tile) = frontier.pop() {
        if !open(tile) {
            continue;
        }
        let distance = map.distance(me, tile);
        if distance != UNREACHABLE && distance > beyond {
            return true;
        }
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let next = tile.offset(dx, dy);
            if let Some(cell) = seen.get_mut(next)
                && !*cell
            {
                *cell = true;
                frontier.push(next);
            }
        }
    }
    false
}

/// Every tile under a building the seat knows: its own, its allies' and the
/// enemy buildings it remembers.
fn known_footprints(map: &MapModel, observation: &ObservationData) -> Grid<bool> {
    let (width, height) = map.size();
    let mut blocked = Grid::new(width, height, false);
    for building in observation
        .my_buildings
        .iter()
        .chain(&observation.ally_buildings)
        .chain(&observation.enemy_buildings)
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
            reached,
            frontier,
        }
    }

    /// Whether the body could drive to `tile`.
    pub(crate) fn reaches(&mut self, tile: TilePos) -> bool {
        while self.reached.get(tile) == Some(&false) {
            let Some(from) = self.frontier.pop_front() else {
                return false;
            };
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let next = from.offset(dx, dy);
                if self.reached.get(next) == Some(&false) && self.open(next) {
                    if let Some(cell) = self.reached.get_mut(next) {
                        *cell = true;
                    }
                    self.frontier.push_back(next);
                }
            }
        }
        self.reached.get(tile) == Some(&true)
    }

    fn open(&self, tile: TilePos) -> bool {
        oxide_sim::geometry::hull_open(tile, self.radius, |cell| {
            self.map.component(cell).is_some() && !self.blocked.get(cell).copied().unwrap_or(true)
        })
    }
}
