//! Raids: a few free raiders of one kind go after an enemy harvest line or a
//! lightly defended building while no defense is under way, and come back
//! before they are caught. Scuttlers, and ground-attack aircraft too few for
//! a strike, harry a harvest line: an enemy Extractor, or a Foundry its
//! Harvesters haul to. Sappers blow up a valuable building. A raided target
//! is skipped for a while, which spaces the raids out.

use super::Scratch;
use super::air::{self, Hazard, Legs};
use super::attack::{
    FIT, Force, defense_mix, force_of, fortified, healthy, margin, minimum, outweighed, priced,
    recruit_by, striking, worth_against,
};
use super::reserve::LOCAL_REACH;
use super::{
    MISSION_CAP, Mission, Missions, Objective, RaidPhase, Task, approach, distant_worth, hunt,
    meeting, mine, run, standing, value,
};
use crate::counters::{Piece, weigh, weight};
use crate::decision::Ledger;
use crate::frame::{HomeFrame, centre_distance, doubled, footprint_centre, ring};
use crate::map::MapModel;
use crate::memory::Memory;
use crate::profile::ResolvedProfile;
use chassis::grid::TilePos;

use aftermine_sim::observation::{BuildingObs, ObservationData, UnitObs};
use aftermine_sim::scenario::BotStance;
use aftermine_sim::{AttackTarget, BuildingKind, Command, RememberedBuilding, UnitId, UnitKind};
use std::cmp::Reverse;

/// Health, per mille, under which a Scuttler or bomber raid turns back.
const WOUNDED: u32 = 500;

/// Tiles between a raider and its goal at which the raid strikes.
const CONTACT_TILES: i32 = 6;

const TRAVEL_TICKS: u64 = 2_400;
const STRIKE_TICKS: u64 = 1_200;
const WITHDRAW_TICKS: u64 = 1_200;

/// Ticks a raid's `phase` should end within.
pub(super) fn timeout(phase: RaidPhase) -> u64 {
    match phase {
        RaidPhase::Travel => TRAVEL_TICKS,
        RaidPhase::Strike => STRIKE_TICKS,
        RaidPhase::Withdraw => WITHDRAW_TICKS,
    }
}

/// The raids a unit can join.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Raider {
    Sapper,
    Scuttler,
    Bomber,
}

fn raider(kind: UnitKind) -> Option<Raider> {
    match kind {
        UnitKind::Sapper => Some(Raider::Sapper),
        UnitKind::Scuttler => Some(Raider::Scuttler),
        UnitKind::Buzzard | UnitKind::Condor => Some(Raider::Bomber),
        UnitKind::Harvester
        | UnitKind::Excavator
        | UnitKind::Sentinel
        | UnitKind::Warden
        | UnitKind::Breaker
        | UnitKind::Lancer
        | UnitKind::Bombard
        | UnitKind::Avalanche
        | UnitKind::Flakhound
        | UnitKind::Talon
        | UnitKind::Kestrel
        | UnitKind::Surveyor
        | UnitKind::Skyhook
        | UnitKind::Mantle
        | UnitKind::Piledriver
        | UnitKind::Maw
        | UnitKind::Roc
        | UnitKind::Drone => None,
    }
}

/// What one decision knows about raiding.
struct Foray<'a> {
    observation: &'a ObservationData,
    map: &'a MapModel,
    frame: HomeFrame,
    memory: &'a Memory,
    /// Known fire against ground units, and against aircraft.
    ground: &'a [Hazard],
    air: &'a [Hazard],
    /// What offense leaves at home.
    reserve: &'a super::reserve::Keep,
    /// Targets another raid holds or this decision is done with.
    held: Vec<Objective>,
    /// Per mille of a target's known guard a raid must bring.
    margin: u64,
    /// Per mille of its strength the enemies around it must reach before it
    /// turns back.
    retreat: u64,
    /// The most known guard a Scuttler or Sapper raid takes on: a target
    /// guarded beyond it is an attack's work, not a raid's.
    light: u64,
}

/// The most known guard a raid takes on, weighed against the raiders, as a
/// share of the stance's attack minimum: a stance limit on what counts as
/// lightly defended. Raiders sent at a real guard die before they reach
/// their target, and a guard they trade poorly against is a real one.
fn light(stance: BotStance) -> u64 {
    minimum(stance) / 4
}

impl Missions {
    /// Advances every raid under way, then, while no defense is under way,
    /// sends more free raiders at enemy harvest lines or lightly defended
    /// buildings no raid holds. Returns the targets raids are done with, so
    /// the seat leaves them for a while.
    pub(crate) fn raid(
        &mut self,
        observation: &ObservationData,
        map: &MapModel,
        profile: &ResolvedProfile,
        memory: &Memory,
        scratch: &Scratch,
        ledger: &mut Ledger,
    ) -> Vec<(BuildingKind, TilePos)> {
        let raiding = self
            .list
            .iter()
            .any(|mission| matches!(mission.task, Task::Raid { .. }));
        let ready = self
            .free(observation, ledger)
            .into_iter()
            .filter_map(|id| mine(observation, id))
            .any(|unit| raider(unit.kind).is_some());
        if !raiding && !ready {
            return Vec::new();
        }
        let raiding_kind: fn(&Task) -> bool = |task| matches!(task, Task::Raid { .. });
        let mut foray = Foray::new(observation, map, profile, memory, scratch);
        let mut done: Vec<Objective> = Vec::new();
        for id in self.ids(raiding_kind) {
            let Some(index) = self.index_of(id) else {
                continue;
            };
            foray.held = self.held(raiding_kind, Some(id), &done);
            if let Some(target) = self.advance_raid(index, &foray, ledger) {
                done.push(target);
            }
        }
        loop {
            foray.held = self.held(raiding_kind, None, &done);
            if !self.form_raid(&foray, minimum(profile.stance), ledger) {
                break;
            }
        }
        done.into_iter()
            .map(|target| (target.building, target.anchor))
            .collect()
    }

    /// Scuttlers a raid on the least guarded known harvest line not raided
    /// lately and not under known guns needs: its known guard by the margin,
    /// each side weighed against the other, so the stock it trains is one a
    /// raid would set out with; at least one. Zero while no such line is
    /// known.
    pub(crate) fn raid_squad(
        observation: &ObservationData,
        map: &MapModel,
        profile: &ResolvedProfile,
        memory: &Memory,
        scratch: &Scratch,
    ) -> usize {
        Foray::new(observation, map, profile, memory, scratch)
            .target(Raider::Scuttler, None)
            .map_or(0, |(_, _, need, guard)| {
                // Each Scuttler counts at what it trades against the guard.
                let one = u64::from(UnitKind::Scuttler.stats().cost)
                    * weight(UnitKind::Scuttler, &guard)
                    / 1_000;
                usize::try_from(need.div_ceil(one.max(1))).unwrap_or(usize::MAX)
            })
    }

    /// Sends the first raider kind with a squad beyond the reserve at a target
    /// no raid holds. Returns whether a raid formed.
    fn form_raid(&mut self, foray: &Foray<'_>, minimum: u64, ledger: &mut Ledger) -> bool {
        let observation = foray.observation;
        let defending = self.list.iter().any(|mission| {
            matches!(mission.task, Task::Defend { .. }) && mission.holds(observation)
        });
        if self.list.len() >= MISSION_CAP || defending {
            return false;
        }
        let free: Vec<&UnitObs> = self
            .free(observation, ledger)
            .into_iter()
            .filter_map(|id| mine(observation, id))
            .filter(|unit| unit.idle && healthy(unit, FIT))
            .collect();
        let spare = self.spare(observation, foray.map, foray.reserve);
        for kind in [Raider::Sapper, Raider::Scuttler, Raider::Bomber] {
            let squad: Vec<&UnitObs> = free
                .iter()
                .copied()
                .filter(|unit| raider(unit.kind) == Some(kind))
                .collect();
            let squad = spare.clone().outermost(foray.map, foray.frame, squad);
            let own = force_of(squad.iter().copied(), value);
            // Bombers enough for a strike are the strike's.
            let striking: u64 = squad.iter().map(|unit| striking(unit)).sum();
            if squad.is_empty() || (kind == Raider::Bomber && striking >= minimum) {
                continue;
            }
            let Some((target, goal, need, guard)) = foray.target(kind, Some(&own)) else {
                continue;
            };
            // The raiders nearest the goal until they are worth its need.
            let worth = worth_against(&guard, value);
            let units = recruit_by(foray.frame, &squad, goal, need, usize::MAX, worth);
            let sent = match kind {
                Raider::Sapper => foray
                    .demolish(target)
                    .is_some_and(|aim| ledger.order(blast(units.clone(), aim))),
                Raider::Scuttler => ledger.order(hunt(units.clone(), goal)),
                Raider::Bomber => foray.fly(units.clone(), goal, ledger),
            };
            if sent {
                self.list.push(Mission {
                    id: self.next,
                    since: observation.tick,
                    units,
                    goal,
                    task: Task::Raid {
                        target,
                        phase: RaidPhase::Travel,
                    },
                });
                self.next += 1;
            }
            return sent;
        }
        false
    }

    /// Moves the raid at `index` through its phases. Returns its target once
    /// it turns back.
    fn advance_raid(
        &mut self,
        index: usize,
        foray: &Foray<'_>,
        ledger: &mut Ledger,
    ) -> Option<Objective> {
        let observation = foray.observation;
        let now = observation.tick;
        let mission = &self.list[index];
        let Task::Raid { target, phase } = mission.task else {
            unreachable!("the raid mission's task");
        };
        let members: Vec<&UnitObs> = mission
            .units
            .iter()
            .filter_map(|id| mine(observation, *id))
            .collect();
        let kind = members.iter().find_map(|unit| raider(unit.kind))?;
        let all_idle = members.iter().all(|unit| unit.idle);
        let age = now - mission.since;
        let goal = mission.goal;
        let units = mission.units.clone();
        let centre = footprint_centre(target.building, target.anchor);
        let home = air::pad(observation, foray.map, foray.frame, centre)?;
        // Sappers spend themselves, so only a raid of fighters counts its
        // losses and wounds.
        let spent = kind != Raider::Sapper && members.iter().any(|unit| !healthy(unit, WOUNDED));
        let own = force_of(members.iter().copied(), value);
        let enemy = foray.opposition(kind, &members);
        let outweighed = weigh(&enemy, &own) * 1_000 > weigh(&own, &enemy) * foray.retreat;
        let lost = target;
        let withdraw = |missions: &mut Self, ledger: &mut Ledger| {
            let turned = ledger.order(run(units.clone(), home));
            if turned {
                let mission = &mut missions.list[index];
                mission.task = Task::Raid {
                    target,
                    phase: RaidPhase::Withdraw,
                };
                mission.since = now;
                mission.goal = home;
            }
            turned
        };
        match phase {
            RaidPhase::Travel => {
                if spent || outweighed || age >= TRAVEL_TICKS || !standing(observation, target) {
                    return withdraw(self, ledger).then_some(lost);
                }
                let arrived = members
                    .iter()
                    .any(|unit| unit.tile.chebyshev(goal) <= CONTACT_TILES);
                if arrived {
                    let mission = &mut self.list[index];
                    mission.task = Task::Raid {
                        target,
                        phase: RaidPhase::Strike,
                    };
                    mission.since = now;
                }
                None
            }
            RaidPhase::Strike => {
                // Sappers at the target blow it up whatever stands there.
                let outweighed = outweighed && kind != Raider::Sapper;
                let done = all_idle || age >= STRIKE_TICKS || !standing(observation, target);
                // A raider drawn past local reach of its target, chasing a
                // fleeing worker, runs home rather than on into the enemy's
                // base; the rest strike on. It stays the raid's until it is
                // home and idle, so nothing else takes it on the way. One
                // still on its way in from home is no stray.
                let away = |unit: &UnitObs| unit.tile.chebyshev(goal) > LOCAL_REACH;
                let home_again: Vec<&UnitObs> = members
                    .iter()
                    .copied()
                    .filter(|unit| away(unit))
                    .filter(|unit| unit.idle && unit.tile.chebyshev(home) <= CONTACT_TILES)
                    .collect();
                let strays: Vec<UnitId> = members
                    .iter()
                    .filter(|unit| away(unit))
                    .filter(|unit| unit.tile.chebyshev(home) > goal.chebyshev(home))
                    .filter(|unit| home_again.iter().all(|other| other.id != unit.id))
                    .map(|unit| unit.id)
                    .collect();
                if spent || outweighed || done || strays.len() == units.len() {
                    return withdraw(self, ledger).then_some(lost);
                }
                if !strays.is_empty() {
                    ledger.order(run(strays, home));
                }
                self.list[index]
                    .units
                    .retain(|id| home_again.iter().all(|unit| unit.id != *id));
                None
            }
            RaidPhase::Withdraw => {
                if all_idle || age >= WITHDRAW_TICKS {
                    self.list.remove(index);
                }
                None
            }
        }
    }
}

impl<'a> Foray<'a> {
    fn new(
        observation: &'a ObservationData,
        map: &'a MapModel,
        profile: &ResolvedProfile,
        memory: &'a Memory,
        scratch: &'a Scratch,
    ) -> Self {
        Self {
            observation,
            map,
            frame: scratch.frame,
            memory,
            ground: &scratch.ground,
            air: &scratch.air,
            reserve: &scratch.reserve,
            held: Vec::new(),
            margin: margin(profile.difficulty),
            retreat: outweighed(profile),
            light: light(profile.stance),
        }
    }

    /// Where raiders of `kind` go, the tile they aim for, the strength they
    /// need there and the known defense they meet: for Sappers the most
    /// valuable known enemy building for its distance with little known
    /// defense, for the others the enemy Extractor or Foundry whose known
    /// defense weighs least against them, nearest first. Scuttlers leave
    /// lines that known enemy guns cover. Targets recently raided are
    /// skipped, and ground raiders need a ground route. With `own`, only a
    /// target that squad can take; without, the need is at its price, to
    /// size a squad.
    fn target(
        &self,
        kind: Raider,
        own: Option<&[(Piece, u64)]>,
    ) -> Option<(Objective, TilePos, u64, Force)> {
        // Without a squad, what a squad of the kind would face.
        let sample = match kind {
            Raider::Bomber => UnitKind::Buzzard,
            Raider::Sapper => UnitKind::Sapper,
            Raider::Scuttler => UnitKind::Scuttler,
        };
        let sample = [(Piece::Unit(sample), 1)];
        let against = own.unwrap_or(&sample);
        let observation = self.observation;
        let now = observation.tick;
        let start = self.map.start(observation.me)?;
        let home = footprint_centre(BuildingKind::Foundry, start);
        let candidates = observation
            .enemy_buildings
            .iter()
            .filter(|building| building.built)
            .map(|building| Objective {
                owner: building.player,
                building: building.kind,
                anchor: building.anchor,
            })
            .filter(|target| !self.held.contains(target))
            .filter(|target| !self.memory.raided(target.building, target.anchor, now))
            .filter_map(|target| {
                let goal = match kind {
                    Raider::Bomber => {
                        let size = target.building.size();
                        ring(target.anchor, size).min_by_key(|tile| {
                            (
                                centre_distance(home, doubled(*tile)),
                                self.frame.rank(self.frame.home, doubled(*tile)),
                            )
                        })?
                    }
                    Raider::Sapper | Raider::Scuttler => approach(
                        self.map,
                        observation.me,
                        self.frame,
                        target.building,
                        target.anchor,
                    )?,
                };
                Some((target, goal))
            });
        let distance = |target: &Objective| {
            centre_distance(home, footprint_centre(target.building, target.anchor))
        };
        let rank = |target: &Objective| {
            self.frame.rank(
                self.frame.home,
                footprint_centre(target.building, target.anchor),
            )
        };
        match kind {
            Raider::Sapper => candidates
                .filter(|(_, goal)| weigh(&self.guarded(*goal, against), against) <= self.light)
                .filter_map(|(target, goal)| {
                    // Enough blasts for what is left of the building.
                    let hp = observation
                        .enemy_buildings
                        .iter()
                        .find(|building| {
                            (building.player, building.kind, building.anchor)
                                == (target.owner, target.building, target.anchor)
                        })
                        .map_or(1, |building| building.hp);
                    let sapper = UnitKind::Sapper.stats();
                    let blast = sapper
                        .demolition
                        .map_or(1, |charge| charge.structure_damage);
                    let blasts = u64::from(hp.div_ceil(blast).max(1));
                    let need = blasts * u64::from(sapper.cost);
                    (own.is_none_or(|own| need <= priced(own))).then_some((
                        target,
                        goal,
                        need,
                        Vec::new(),
                    ))
                })
                .max_by_key(|(target, ..)| {
                    (
                        distant_worth(target.building, distance(target)),
                        Reverse(rank(target)),
                    )
                }),
            Raider::Scuttler | Raider::Bomber => candidates
                .filter(|(target, _)| {
                    matches!(
                        target.building,
                        BuildingKind::Extractor | BuildingKind::Foundry
                    )
                })
                // A line under known guns is an attack's work, not a raid's.
                .filter(|(_, goal)| kind != Raider::Scuttler || !fortified(observation, *goal))
                .filter_map(|(target, goal)| {
                    let guard = match kind {
                        Raider::Bomber => {
                            air::cover(self.air, footprint_centre(target.building, target.anchor))
                        }
                        _ => self.guarded(goal, against),
                    };
                    let met = weigh(&guard, against);
                    let light = kind != Raider::Scuttler || met <= self.light;
                    let need = (met * self.margin / 1_000).max(1);
                    let takes = own.is_none_or(|own| weigh(own, &guard) >= need);
                    (takes && light).then_some((met, target, goal, need, guard))
                })
                .min_by_key(|(met, target, ..)| (*met, distance(target), rank(target)))
                .map(|(_, target, goal, need, guard)| (target, goal, need, guard)),
        }
    }

    /// What a ground raid on `goal` meets: its known guard there, or the
    /// strongest known enemy force on its way, whichever weighs more against
    /// the raiders, `against`. A line held across the way stops raiders as
    /// surely as a guard at the target.
    fn guarded(&self, goal: TilePos, against: &[(Piece, u64)]) -> Force {
        let guard = defense_mix(self.observation, self.memory, &[goal], &[]);
        let way = meeting::on_the_way(
            self.observation,
            self.memory,
            (self.map, self.frame),
            goal,
            against,
        );
        if weigh(&way, against) > weigh(&guard, against) {
            way
        } else {
            guard
        }
    }

    /// What a Sapper aims at to blow up `target`.
    fn demolish(&self, target: Objective) -> Option<AttackTarget> {
        self.observation
            .enemy_buildings
            .iter()
            .find(|building| {
                (building.player, building.kind, building.anchor)
                    == (target.owner, target.building, target.anchor)
            })
            .map(aim)
    }

    /// Sends aircraft at `goal`, around known anti-air when the straight line
    /// crosses it. Returns whether they went.
    fn fly(&self, units: Vec<UnitId>, goal: TilePos, ledger: &mut Ledger) -> bool {
        let Some(pad) = air::pad(self.observation, self.map, self.frame, doubled(goal)) else {
            return false;
        };
        air::fly_around(
            self.observation,
            self.frame,
            self.air,
            units,
            pad,
            goal,
            Legs::Strike,
            ledger,
        )
    }

    /// The known fire reaching any member.
    fn opposition(&self, kind: Raider, members: &[&UnitObs]) -> Force {
        let hazards = match kind {
            Raider::Bomber => self.air,
            Raider::Sapper | Raider::Scuttler => self.ground,
        };
        air::opposition(hazards, members)
    }
}

/// What to attack to hit `building`: the building in sight, or the
/// footprint the seat remembers.
pub(super) fn aim(building: &BuildingObs) -> AttackTarget {
    if building.seen {
        AttackTarget::Building(building.id)
    } else {
        AttackTarget::RememberedBuilding(RememberedBuilding {
            owner: building.player,
            building_kind: building.kind,
            anchor: building.anchor,
        })
    }
}

pub(super) fn blast(units: Vec<UnitId>, target: AttackTarget) -> Command {
    Command::Attack {
        units,
        target,
        queue: false,
    }
}
