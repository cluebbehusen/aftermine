//! The home reserve: army value against ground and against aircraft that
//! offense leaves at home while an enemy could come for it, bounded by
//! stance. Defense takes every unit regardless; the reserve only limits what
//! attacks, strikes, raids and lifts take away.

use super::attack::{Force, force_of, gun, minimum};
use super::{Missions, Task, carrier, force, hits, mine, value};
use crate::counters::{Piece, merged, weigh, weight};
use crate::frame::{HomeFrame, doubled, gap};
use crate::map::{MapModel, UNREACHABLE};
use crate::memory::{Memory, SeenUnit};
use aftermine_sim::geometry::footprint_distance;
use aftermine_sim::observation::{ObservationData, UnitObs};
use aftermine_sim::scenario::BotStance;
use aftermine_sim::stats::Domain;
use aftermine_sim::{BuildingKind, PlayerId, UnitKind};
use chassis::grid::TilePos;
use std::cmp::Reverse;

/// The body domains a reserve covers, in the order its values keep.
const DOMAINS: [Domain; 2] = [Domain::Ground, Domain::Air];

/// Empty tiles around a guarded building within which known enemies
/// threaten it and units training nearby answer for it.
pub(super) const LOCAL_REACH: i32 = 12;

/// What the seat keeps at one of its buildings against the known enemies
/// that could reach it, in each domain: the stance's share of those enemies,
/// at least the stance's floor at the start's Foundry, less the guns that
/// already cover it. Each side counts by what it trades against the other,
/// by the counter chart: against Lancers, Sentinels hold less than their
/// price and Wardens more.
#[derive(Clone, Default)]
pub(crate) struct Keep {
    /// Whether anything is kept against each domain: the start's Foundry
    /// keeps nothing against a domain no enemy could reach it in.
    open: [bool; 2],
    floor: u64,
    /// Per mille of the threat kept.
    share: u64,
    /// The known enemies near the building, and the seat's guns covering
    /// it, in each domain, each piece once with its value, so that weighing
    /// a unit against them grows with the kinds there, not the units.
    threat: [Force; 2],
    guns: [Force; 2],
    /// What each kind the seat owns trades against the enemies in each
    /// domain, worked out once rather than for every unit weighed.
    weights: Vec<(UnitKind, [u64; 2])>,
    /// The home reserve's need, weighed against the seat's army at home when
    /// the decision began.
    held: [u64; 2],
    /// Kept beyond the need: free clearing gunships at a seat whose ground
    /// reaches no enemy leave regardless, so nothing else leaves in their
    /// place.
    extra: [u64; 2],
}

impl Keep {
    /// What the seat keeps home from offense, before the units already out
    /// defending count: nothing while no enemy could reach home that way,
    /// else the stance's floor or its share of the known enemy army that
    /// could, whichever is more, less the static defenses on the start's
    /// ground that already cover it.
    pub(super) fn home(
        observation: &ObservationData,
        map: &MapModel,
        memory: &Memory,
        stance: BotStance,
    ) -> Self {
        let Some(anchor) = map.start(observation.me) else {
            return Self::default();
        };
        let mut keep = Self::of(
            observation,
            map,
            memory,
            stance,
            BuildingKind::Foundry,
            anchor,
        );
        // Weighed once a decision, against every armed unit at home, rather
        // than against whatever is still free each time a mission asks.
        let home = map.component(anchor);
        let army = force_of(
            observation
                .my_units
                .iter()
                .filter(|unit| unit.kind.stats().can_fight() && at_home(map, home, unit)),
            value,
        );
        keep.held = keep.weighed(&army);
        keep
    }

    /// What the seat keeps at the building of `kind` at `anchor`. The
    /// start's Foundry keeps the home reserve; any other building keeps the
    /// stance's share of the known enemies near it alone. Completed guns
    /// covering it count against either: every gun on the start's ground,
    /// elsewhere those whose range reaches the building.
    pub(super) fn of(
        observation: &ObservationData,
        map: &MapModel,
        memory: &Memory,
        stance: BotStance,
        kind: BuildingKind,
        anchor: TilePos,
    ) -> Self {
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
        let threat = DOMAINS.map(|domain| {
            memory
                .units()
                .iter()
                .filter(|unit| unit.kind.stats().can_fight())
                .filter(|unit| unit.kind.stats().domain == domain)
                .filter(|unit| domain == Domain::Air || map.component(unit.tile) == home)
                .filter(|unit| {
                    primary || gap(anchor, kind.size(), unit.tile, (1, 1)) <= LOCAL_REACH
                })
                .map(|unit| (Piece::Unit(unit.kind), unit.value(now)))
                .collect::<Vec<_>>()
        });
        let guns = DOMAINS.map(|domain| {
            observation
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
                .map(gun)
                .collect::<Vec<_>>()
        });
        let threat = threat.map(|force| merged(&force));
        let mut kinds: Vec<UnitKind> = observation
            .my_units
            .iter()
            .map(|unit| unit.kind)
            .chain(observation.my_carried_units.iter().map(|unit| unit.kind))
            .collect();
        kinds.sort_unstable();
        kinds.dedup();
        let weights = kinds
            .into_iter()
            .map(|kind| (kind, [0, 1].map(|index| weight(kind, &threat[index]))))
            .collect();
        Self {
            open: [0, 1].map(|index| !primary || reach[index]),
            floor: if primary { floor } else { 0 },
            share,
            threat,
            guns: guns.map(|force| merged(&force)),
            weights,
            held: [0; 2],
            extra: [0; 2],
        }
    }

    /// Value kept against each domain, weighed against those of the units
    /// `against` that would hold the building there: the ones that hit it.
    /// With none to weigh against, the enemies count at their price.
    pub(crate) fn need(&self, against: &[(Piece, u64)]) -> [u64; 2] {
        let weighed = self.weighed(against);
        [0, 1].map(|index| weighed[index] + self.extra[index])
    }

    /// What home keeps from offense against each domain: [`Keep::need`]
    /// against the seat's army at home when the decision began.
    pub(super) fn kept(&self) -> [u64; 2] {
        [0, 1].map(|index| self.held[index] + self.extra[index])
    }

    /// [`Keep::need`] without what is kept beyond it.
    fn weighed(&self, against: &[(Piece, u64)]) -> [u64; 2] {
        let against = merged(against);
        [0, 1].map(|index| {
            if !self.open[index] {
                return 0;
            }
            let (threat, guns) = (&self.threat[index], &self.guns[index]);
            let holding: Vec<(Piece, u64)> = against
                .iter()
                .filter(|(piece, _)| match piece {
                    Piece::Unit(kind) => kind
                        .stats()
                        .weapons
                        .iter()
                        .any(|weapon| weapon.targets.covers(DOMAINS[index])),
                    Piece::Gun(..) => true,
                })
                .copied()
                .collect();
            self.floor
                .max(weigh(threat, &holding) * self.share / 1_000)
                .saturating_sub(weigh(guns, threat))
        })
    }

    /// What a unit of `kind` at `hp` holds against each domain: its
    /// [`force`] there, weighed by what its kind trades against the enemies
    /// in that domain.
    pub(super) fn force(&self, kind: UnitKind, hp: u32) -> [u64; 2] {
        let made = force(kind, hp);
        let weights = self.weights(kind);
        [0, 1].map(|index| made[index] * weights[index] / 1_000)
    }

    /// What `kind` trades against the enemies in each domain.
    fn weights(&self, kind: UnitKind) -> [u64; 2] {
        self.weights
            .iter()
            .find(|(owned, _)| *owned == kind)
            .map_or_else(
                || [0, 1].map(|index| weight(kind, &self.threat[index])),
                |(_, weights)| *weights,
            )
    }

    /// What `unit` holds against each domain, at its value weighed by what its
    /// kind trades against the enemies there.
    fn charge(&self, unit: &UnitObs) -> [u64; 2] {
        let weights = self.weights(unit.kind);
        [0, 1].map(|index| value(unit) * weights[index] / 1_000)
    }

    /// The price of units of `mix`'s kinds that would hold `gap` against each
    /// domain: what a gap in held strength costs to close.
    pub(super) fn scrap(&self, gap: [u64; 2], mix: &[(Piece, u64)]) -> [u64; 2] {
        let total: u64 = mix.iter().map(|(_, value)| value).sum();
        [0, 1].map(|index| {
            if total == 0 {
                return gap[index];
            }
            let quality = mix
                .iter()
                .map(|(piece, value)| value * weight(*piece, &self.threat[index]))
                .sum::<u64>()
                / total;
            gap[index] * 1_000 / quality.max(1)
        })
    }

    /// Keeps `value` more against the domain at `index`.
    pub(super) fn add(&mut self, index: usize, value: u64) {
        self.extra[index] += value;
    }
}

/// Value of the armed enemy ground units the seat remembers that could come
/// at home: those on ground connected to its start.
pub(crate) fn threat(map: &MapModel, memory: &Memory, me: PlayerId, now: u64) -> u64 {
    memory
        .units()
        .iter()
        .filter(|unit| unit.kind.stats().can_fight())
        .filter(|unit| unit.kind.stats().domain == Domain::Ground)
        .filter(|unit| map.distance(me, unit.tile) != UNREACHABLE)
        .map(|unit| unit.value(now))
        .sum()
}

/// What free units at home may still take away on offense: their value
/// against each domain beyond what the reserve keeps.
#[derive(Clone)]
pub(super) struct Spare<'k> {
    home: Option<u32>,
    left: [u64; 2],
    /// What home keeps, which weighs what each leaving unit takes.
    keep: &'k Keep,
}

impl Spare<'_> {
    /// Whether `unit` may leave, charging what it holds against every
    /// domain it hits. A unit away from home keeps nothing there, so it may
    /// always go.
    fn take(&mut self, map: &MapModel, unit: &UnitObs) -> bool {
        if !at_home(map, self.home, unit) {
            return true;
        }
        let cost = self.keep.charge(unit);
        let charged = DOMAINS.map(|domain| hits(unit, domain));
        if (0..2).any(|index| charged[index] && cost[index] > self.left[index]) {
            return false;
        }
        for index in 0..2 {
            if charged[index] {
                self.left[index] -= cost[index];
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
    /// What free units at home may take away on offense now: what they hold
    /// against each domain beyond the part of the home `reserve` the units
    /// out defending do not already cover, each weighed against the threat
    /// home keeps against.
    pub(super) fn spare<'k>(
        &self,
        observation: &ObservationData,
        map: &MapModel,
        reserve: &'k Keep,
    ) -> Spare<'k> {
        let (home, free, defending) = self.at_home(observation, map);
        let need = reserve.kept();
        let worth = |units: &[&UnitObs]| {
            units.iter().fold([0; 2], |sum, unit| {
                let charge = reserve.charge(unit);
                [0, 1].map(|index| {
                    sum[index]
                        + if hits(unit, DOMAINS[index]) {
                            charge[index]
                        } else {
                            0
                        }
                })
            })
        };
        let (free, defending) = (worth(&free), worth(&defending));
        Spare {
            home,
            left: std::array::from_fn(|index| {
                free[index].saturating_sub(need[index].saturating_sub(defending[index]))
            }),
            keep: reserve,
        }
    }

    /// The start's ground, the free units at home and the units out
    /// defending the start's ground.
    fn at_home<'a>(
        &self,
        observation: &'a ObservationData,
        map: &MapModel,
    ) -> (Option<u32>, Vec<&'a UnitObs>, Vec<&'a UnitObs>) {
        let home = map
            .start(observation.me)
            .and_then(|start| map.component(start));
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
        (home, free, defending)
    }

    /// The home `payload` a lift could take, cut to what the reserve lets
    /// leave.
    pub(super) fn liftable(
        &self,
        observation: &ObservationData,
        map: &MapModel,
        reserve: &Keep,
        payload: u64,
    ) -> u64 {
        self.spare(observation, map, reserve).payload(payload)
    }
}

/// Whether `unit` counts as at home: in the air, or on the start's ground.
fn at_home(map: &MapModel, home: Option<u32>, unit: &UnitObs) -> bool {
    unit.kind.stats().domain == Domain::Air || (home.is_some() && map.component(unit.tile) == home)
}

#[cfg(test)]
mod tests;
