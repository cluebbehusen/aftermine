//! The home reserve: army value against ground and against aircraft that
//! offense leaves at home while an enemy could come for it, bounded by
//! stance. Defense takes every unit regardless; the reserve only limits what
//! attacks, strikes, raids and lifts take away.

use super::attack::{building_value, minimum};
use super::{Missions, Task, carrier, hits, mine, value};
use crate::frame::{HomeFrame, doubled, gap};
use crate::map::{MapModel, UNREACHABLE};
use crate::memory::{Memory, SeenUnit};
use chassis::grid::TilePos;
use oxide_sim::geometry::footprint_distance;
use oxide_sim::observation::{ObservationData, UnitObs};
use oxide_sim::scenario::BotStance;
use oxide_sim::stats::Domain;
use oxide_sim::{BuildingKind, PlayerId};
use std::cmp::Reverse;

/// The body domains a reserve covers, in the order its values keep.
const DOMAINS: [Domain; 2] = [Domain::Ground, Domain::Air];

/// Empty tiles around a guarded building within which known enemies
/// threaten it and units training nearby answer for it.
pub(super) const LOCAL_REACH: i32 = 12;

/// Value against each domain the seat keeps home from offense, before the
/// units already out defending count: nothing while no enemy could reach
/// home that way, else the stance's floor or its share of the known enemy
/// army that could, whichever is more, less the static defenses on the
/// start's ground that already cover it.
pub(super) fn reserve(
    observation: &ObservationData,
    map: &MapModel,
    memory: &Memory,
    stance: BotStance,
) -> [u64; 2] {
    let Some(anchor) = map.start(observation.me) else {
        return [0; 2];
    };
    need(
        observation,
        map,
        memory,
        stance,
        BuildingKind::Foundry,
        anchor,
    )
}

/// Value against each domain the seat keeps at the building of `kind` at
/// `anchor`. The start's Foundry keeps the home reserve: nothing while no
/// enemy could reach it that way, else the stance's floor or its share of
/// the known enemy army that could, whichever is more. Any other building
/// keeps the stance's share of the known enemies near it alone. Completed
/// guns covering it count against either: every gun on the start's ground,
/// elsewhere those whose range reaches the building.
pub(super) fn need(
    observation: &ObservationData,
    map: &MapModel,
    memory: &Memory,
    stance: BotStance,
    kind: BuildingKind,
    anchor: TilePos,
) -> [u64; 2] {
    let me = observation.me;
    let now = observation.tick;
    let home = map.component(anchor);
    let primary = kind == BuildingKind::Foundry && map.start(me) == Some(anchor);
    let (floor, share) = match stance {
        BotStance::Turtle => (minimum(stance), 1_500),
        BotStance::Balanced => (minimum(stance) / 2, 1_000),
        BotStance::Aggressive => (0, 500),
    };
    let armed = |unit: &&SeenUnit| !unit.kind.stats().weapons.is_empty();
    let airworks = observation
        .enemy_buildings
        .iter()
        .any(|building| building.kind == BuildingKind::Airworks);
    let walks = map.hostile_distance(me, anchor) != UNREACHABLE;
    let reach = [
        walks || airworks || memory.units().iter().any(|unit| carrier(unit.kind)),
        airworks
            || memory
                .units()
                .iter()
                .filter(armed)
                .any(|unit| unit.kind.stats().domain == Domain::Air),
    ];
    std::array::from_fn(|index| {
        if primary && !reach[index] {
            return 0;
        }
        let domain = DOMAINS[index];
        let threat = memory
            .units()
            .iter()
            .filter(|unit| unit.kind.stats().can_fight())
            .filter(|unit| unit.kind.stats().domain == domain)
            .filter(|unit| domain == Domain::Air || map.component(unit.tile) == home)
            .filter(|unit| primary || gap(anchor, kind.size(), unit.tile, (1, 1)) <= LOCAL_REACH)
            .map(|unit| unit.value(now))
            .sum::<u64>();
        let standing: u64 = observation
            .my_buildings
            .iter()
            .filter(|building| building.built)
            .filter(|building| {
                building
                    .kind
                    .tier_stats(building.tier)
                    .weapons
                    .iter()
                    .any(|weapon| {
                        weapon.targets.covers(domain)
                            && if primary {
                                map.component(building.anchor) == home
                            } else {
                                footprint_distance(
                                    anchor,
                                    kind.size(),
                                    building.anchor,
                                    building.kind.size(),
                                ) <= weapon.range
                            }
                    })
            })
            .map(building_value)
            .sum();
        (if primary { floor } else { 0 })
            .max(threat * share / 1_000)
            .saturating_sub(standing)
    })
}

/// Value of the armed enemy units of `domain` the seat remembers that could
/// come at home: aircraft anywhere, ground units only on ground connected to
/// its start.
pub(crate) fn threat(
    map: &MapModel,
    memory: &Memory,
    me: PlayerId,
    now: u64,
    domain: Domain,
) -> u64 {
    memory
        .units()
        .iter()
        .filter(|unit| unit.kind.stats().can_fight())
        .filter(|unit| unit.kind.stats().domain == domain)
        .filter(|unit| domain == Domain::Air || map.distance(me, unit.tile) != UNREACHABLE)
        .map(|unit| unit.value(now))
        .sum()
}

/// What free units at home may still take away on offense: their value
/// against each domain beyond what the reserve keeps.
#[derive(Clone)]
pub(super) struct Spare {
    home: Option<u32>,
    left: [u64; 2],
}

impl Spare {
    /// Whether `unit` may leave, charging its value against every domain it
    /// hits. A unit away from home keeps nothing there, so it may always go.
    fn take(&mut self, map: &MapModel, unit: &UnitObs) -> bool {
        if !at_home(map, self.home, unit) {
            return true;
        }
        let cost = value(unit);
        let charged = DOMAINS.map(|domain| hits(unit, domain));
        if charged
            .iter()
            .zip(self.left)
            .any(|(charged, left)| *charged && cost > left)
        {
            return false;
        }
        for (charged, left) in charged.iter().zip(&mut self.left) {
            if *charged {
                *left -= cost;
            }
        }
        true
    }

    /// Those of `units` that may leave, farthest from home first, so that
    /// the ones kept are those nearest it.
    pub(super) fn outermost<'a>(
        &mut self,
        map: &MapModel,
        frame: HomeFrame,
        mut units: Vec<&'a UnitObs>,
    ) -> Vec<&'a UnitObs> {
        units.sort_by_key(|unit| (Reverse(frame.rank(frame.home, doubled(unit.tile))), unit.id));
        self.trim(map, units)
    }

    /// Those of `units` that may leave, taken in order.
    pub(super) fn trim<'a>(&mut self, map: &MapModel, units: Vec<&'a UnitObs>) -> Vec<&'a UnitObs> {
        units
            .into_iter()
            .filter(|unit| self.take(map, unit))
            .collect()
    }

    /// A lift's payload of value against ground, cut to what may leave.
    fn payload(&self, value: u64) -> u64 {
        value.min(self.left[0])
    }
}

impl Missions {
    /// What free units at home may take away on offense now: their value
    /// against each domain beyond the part of `reserve` the units out
    /// defending do not already cover.
    pub(super) fn spare(
        &self,
        observation: &ObservationData,
        map: &MapModel,
        reserve: [u64; 2],
    ) -> Spare {
        let (home, free, defending) = self.at_home(observation, map);
        Spare {
            home,
            left: std::array::from_fn(|index| {
                free[index].saturating_sub(reserve[index].saturating_sub(defending[index]))
            }),
        }
    }

    /// The start's ground, and the value against each domain of the free
    /// units at home and of the units out defending the start's ground.
    fn at_home(
        &self,
        observation: &ObservationData,
        map: &MapModel,
    ) -> (Option<u32>, [u64; 2], [u64; 2]) {
        let home = map
            .start(observation.me)
            .and_then(|start| map.component(start));
        let worth = |units: &[&UnitObs]| {
            DOMAINS.map(|domain| {
                units
                    .iter()
                    .filter(|unit| hits(unit, domain))
                    .map(|unit| value(unit))
                    .sum::<u64>()
            })
        };
        // A quiet guard keeps its units for its own base, and a defense off
        // the start's ground answers for that ground, not home.
        let defending: Vec<&UnitObs> = self
            .list
            .iter()
            .filter(|mission| match mission.task {
                Task::Defend { asset, .. } => {
                    !mission.guarding()
                        && observation.my_buildings.iter().any(|building| {
                            building.id == asset && map.component(building.anchor) == home
                        })
                }
                _ => false,
            })
            .flat_map(|mission| &mission.units)
            .filter_map(|id| mine(observation, *id))
            .collect();
        let free: Vec<&UnitObs> = self
            .available(observation, None)
            .into_iter()
            .filter_map(|id| mine(observation, id))
            .filter(|unit| at_home(map, home, unit))
            .collect();
        (home, worth(&free), worth(&defending))
    }

    /// The home `payload` a lift could take, cut to what the reserve lets
    /// leave.
    pub(super) fn liftable(
        &self,
        observation: &ObservationData,
        map: &MapModel,
        reserve: [u64; 2],
        payload: u64,
    ) -> u64 {
        self.spare(observation, map, reserve).payload(payload)
    }
}

/// Whether `unit` counts as at home: in the air, or on the start's ground.
fn at_home(map: &MapModel, home: Option<u32>, unit: &UnitObs) -> bool {
    unit.kind.stats().domain == Domain::Air || (home.is_some() && map.component(unit.tile) == home)
}
