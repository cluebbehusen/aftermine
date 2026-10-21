//! The attack mission: when the free army, above a stance-bounded minimum,
//! can beat the known army it would meet at a known or presumed enemy
//! building, the army gathers near home, travels, fights, withdraws from a
//! losing fight, and recovers to go again or disband. Above the lowest rung
//! it stops short of known defenses on the way to gather again, then goes in
//! together. Against known defenses free Sappers join while it regroups, and
//! each blows up the nearest defense once the fight begins.

use super::Scratch;
use super::defense::{artillery, battery, lobs};
use super::raid::{aim, blast};
use super::{
    AttackPhase, MISSION_CAP, Mission, Missions, Objective, Task, UNIT_CAP, against_buildings,
    approach, breacher, distant_worth, hits, hunt, insert, mine, run, standing, value,
};
use crate::composition::{self, Role};
use crate::decision::Ledger;
use crate::frame::{HomeFrame, doubled, footprint_centre, gap, ring};
use crate::map::{MapModel, UNREACHABLE};
use crate::memory::Memory;
use crate::profile::ResolvedProfile;
use chassis::fx::Fx;
use chassis::grid::{Grid, TilePos};
use chassis::rng::Pcg32;
use oxide_sim::observation::{BuildingObs, ObservationData, UnitObs};
use oxide_sim::scenario::{BotDifficulty, BotStance};
use oxide_sim::stats::Domain;
use oxide_sim::{BuildingKind, PlayerId, UnitId, UnitKind};
use std::borrow::Cow;
use std::cmp::Reverse;

/// Tiles between a member and an enemy that make contact.
const CONTACT_TILES: i32 = 8;

/// Tiles around a target inside which known enemies defend it.
const DEFENSE_TILES: i32 = 10;

/// Ground distance from the seat's start, in tenths of a tile, inside which
/// the army gathers.
const RALLY_REACH: u16 = 80;

/// Tiles around the start searched for the rally point.
const RALLY_SEARCH: i32 = 10;

/// Tenths of a tile a walk to a target may be longer than the shortest and
/// still count as a way there: which known defenses an attack expects to
/// pass, not a count of anything it sends.
const ROUTE_SLACK: u16 = 30;

/// Empty tiles a staging spot keeps beyond the reach of known defenses and
/// of the guns shelling the army, and from every known enemy building.
const STAGE_SPACE: i32 = 3;

/// Tiles from its staging spot within which a member has arrived there.
const STAGE_TILES: i32 = 5;

/// Tiles from a member within which a hostile shell's impact puts the army
/// under fire.
const SHELLED_TILES: i32 = 2;

/// Ticks without an attack that lower the margin one step.
const WAIT_STEP: u64 = 2_400;

/// Per-mille margin one waiting step removes, and the floor it stops at.
const WAIT_RELIEF: u64 = 250;
const MARGIN_FLOOR: u64 = 1_000;

/// Health, per mille, under which a member leaves between fights, and at or
/// over which a unit joins.
const WOUNDED: u32 = 350;
pub(super) const FIT: u32 = 500;

const GATHER_TICKS: u64 = 1_200;
const TRAVEL_TICKS: u64 = 3_600;
const ENGAGE_TICKS: u64 = 3_600;
const WITHDRAW_TICKS: u64 = 1_200;
const RECOVER_TICKS: u64 = 1_200;

/// Scrap of known defense around a target that Veteran and Prime weigh like
/// one tile of distance when choosing what to attack: a difficulty limit on
/// how well a seat finds the soft target.
const GUARD_PER_TILE: u64 = 10;

/// Ticks an attack's `phase` should end within.
pub(super) fn timeout(phase: AttackPhase) -> u64 {
    match phase {
        AttackPhase::Gather => GATHER_TICKS,
        AttackPhase::Advance | AttackPhase::Stage | AttackPhase::Travel => TRAVEL_TICKS,
        AttackPhase::Engage { .. } => ENGAGE_TICKS,
        AttackPhase::Withdraw => WITHDRAW_TICKS,
        AttackPhase::Recover => RECOVER_TICKS,
    }
}

/// A building to attack and where the army goes to reach it.
#[derive(Clone, Copy)]
struct Target {
    owner: PlayerId,
    building: BuildingKind,
    anchor: TilePos,
    approach: TilePos,
    score: u64,
}

impl Target {
    fn objective(self) -> Objective {
        Objective {
            owner: self.owner,
            building: self.building,
            anchor: self.anchor,
        }
    }
}

/// What one decision knows about attacking.
struct Plan<'a> {
    observation: &'a ObservationData,
    map: &'a MapModel,
    frame: HomeFrame,
    memory: &'a Memory,
    minimum: u64,
    margin: u64,
    /// Per mille of its strength the enemies around it must reach before it
    /// withdraws.
    retreat: u64,
    difficulty: BotDifficulty,
    /// Per mille of the enemy's known strength the seat believes it faces
    /// this decision.
    misjudge: u64,
    /// Idle free Sappers.
    sappers: Vec<&'a UnitObs>,
    /// Idle free breachers.
    breachers: Vec<&'a UnitObs>,
    /// Free point defenders fit to walk with an army.
    screens: Vec<&'a UnitObs>,
    /// The enemy to go after first, when there are several.
    rival: Option<PlayerId>,
    /// Targets another attack holds or this decision gave up on.
    held: Vec<Objective>,
}

impl Mission {
    /// Moves an attack to `phase` against `target` from `now`, sending its
    /// members to `goal` when one is given.
    fn attack_phase(
        &mut self,
        target: Target,
        phase: AttackPhase,
        now: u64,
        goal: Option<TilePos>,
    ) {
        self.task = Task::Attack {
            target: target.objective(),
            phase,
        };
        self.since = now;
        if let Some(goal) = goal {
            self.goal = goal;
        }
    }
}

impl Missions {
    /// Advances the attacks under way, then launches more while the free
    /// army beyond the home reserve can beat the known army it would meet at
    /// a target. Returns the targets given up on.
    pub(crate) fn attack(
        &mut self,
        observation: &ObservationData,
        map: &MapModel,
        profile: &ResolvedProfile,
        memory: &Memory,
        scratch: &Scratch,
        ledger: &mut Ledger,
    ) -> Vec<(BuildingKind, TilePos)> {
        let now = observation.tick;
        let attacking: fn(&Task) -> bool = |task| matches!(task, Task::Attack { .. });
        let mut plan = Plan::new(observation, map, profile, memory, scratch);
        let mut given_up: Vec<Objective> = Vec::new();
        for id in self.ids(attacking) {
            let Some(index) = self.index_of(id) else {
                continue;
            };
            let fit = self.refresh(&mut plan, scratch, ledger);
            plan.held = self.held(attacking, Some(id), &given_up);
            if let Some(failed) = self.advance(index, &plan, &fit, ledger) {
                given_up.push(failed);
            }
            if let Some(mission) = self.list.iter().find(|mission| mission.id == id)
                && let Task::Attack { phase, .. } = mission.task
                && afield(phase)
            {
                keep_screens(&plan, mission, ledger);
            }
        }

        let mut fit = self.refresh(&mut plan, scratch, ledger);
        let free: u64 = fit.iter().map(|unit| value(unit)).sum();
        let mut producers = observation
            .my_buildings
            .iter()
            .zip(&observation.my_queues)
            .filter(|(building, _)| {
                building.built && !building.kind.base_stats().produces.is_empty()
            })
            .peekable();
        let idle = producers.peek().is_some() && producers.all(|(_, queue)| queue.is_empty());
        if free >= plan.minimum || idle {
            self.waiting.get_or_insert(now);
        } else {
            self.waiting = None;
        }
        let waited = self.waiting.map_or(0, |since| (now - since) / WAIT_STEP);
        plan.margin = plan
            .margin
            .saturating_sub(waited * WAIT_RELIEF)
            .max(MARGIN_FLOOR);
        loop {
            plan.held = self.held(attacking, None, &given_up);
            if !self.launch(&plan, &fit, ledger) {
                break;
            }
            fit = self.refresh(&mut plan, scratch, ledger);
        }
        given_up
            .into_iter()
            .map(|target| (target.building, target.anchor))
            .collect()
    }

    /// Sappers the seat's attacks want: one for each known enemy defense
    /// around the targets of attacks under way and around the best target no
    /// attack holds yet.
    pub(crate) fn sappers_wanted(
        &self,
        observation: &ObservationData,
        map: &MapModel,
        profile: &ResolvedProfile,
        memory: &Memory,
        scratch: &Scratch,
    ) -> usize {
        let attacking: fn(&Task) -> bool = |task| matches!(task, Task::Attack { .. });
        let held = self.held(attacking, None, &[]);
        let plan = Plan {
            held: held.clone(),
            ..Plan::new(observation, map, profile, memory, scratch)
        };
        // Remembered buildings share one id, so a defense is told apart by
        // where it stands.
        let mut defenses: Vec<_> = held
            .iter()
            .filter_map(|target| plan.target(target.owner, target.building, target.anchor))
            .chain(plan.best(None))
            .flat_map(|target| {
                plan.defenses(target)
                    .map(|building| (building.player, building.kind, building.anchor))
            })
            .collect();
        defenses.sort_unstable();
        defenses.dedup();
        defenses.len()
    }

    /// The free army beyond the home reserve fit to fight, empty gunships
    /// among it while ground reaches the enemy, farthest from home first, and
    /// the idle free Sappers and fit point defenders into `plan`, as earlier
    /// missions of this decision left them.
    fn refresh<'a>(
        &self,
        plan: &mut Plan<'a>,
        scratch: &Scratch,
        ledger: &Ledger,
    ) -> Vec<&'a UnitObs> {
        let (observation, map) = (plan.observation, plan.map);
        let free: Vec<&UnitObs> = self
            .free(observation, ledger)
            .into_iter()
            .filter_map(|id| mine(observation, id))
            .collect();
        plan.sappers = free
            .iter()
            .copied()
            .filter(|unit| unit.kind == UnitKind::Sapper && unit.idle)
            .collect();
        plan.breachers = free
            .iter()
            .copied()
            .filter(|unit| breacher(unit.kind) && unit.idle)
            .collect();
        plan.screens = free
            .iter()
            .copied()
            .filter(|unit| screens(unit) && healthy(unit, FIT))
            .collect();
        // An empty gunship fights with the army while ground reaches the
        // enemy; a seat that can only lift keeps it to lead its lifts.
        let fit: Vec<&UnitObs> = free
            .iter()
            .copied()
            .filter(|unit| {
                eligible(unit, FIT)
                    || (!scratch.severed
                        && crate::gunship::gunship(unit.kind)
                        && unit.cargo == 0
                        && healthy(unit, FIT))
            })
            .collect();
        self.spare(observation, map, scratch.reserve)
            .outermost(map, scratch.frame, fit)
    }

    /// Launches an attack on the best target when the free army can beat the
    /// known army it would meet. A seat that concentrates sends the whole
    /// free army and launches none while another is under way, so one army
    /// hits together rather than several that each meet the enemy alone;
    /// units freed later join it when it regroups. Returns whether one
    /// launched.
    fn launch(&mut self, plan: &Plan<'_>, fit: &[&UnitObs], ledger: &mut Ledger) -> bool {
        let attacking = self
            .list
            .iter()
            .any(|mission| matches!(mission.task, Task::Attack { .. }));
        let concentrated = concentrates(plan.difficulty);
        if (attacking && concentrated) || self.list.len() >= MISSION_CAP {
            return false;
        }
        let Some(target) = plan.best(None) else {
            return false;
        };
        let Some(rally) = plan.rally(target.owner) else {
            return false;
        };
        let component = plan.map.component(target.approach);
        let breachers = plan.breachers(component, UNIT_CAP);
        let need = plan.need(target, breaching(plan.observation, &breachers));
        let fit: Vec<&UnitObs> = fit
            .iter()
            .copied()
            .filter(|unit| reaches(plan.map, unit, component))
            .collect();
        if fit.iter().map(|unit| striking(unit)).sum::<u64>() < need {
            return false;
        }
        let taken = if concentrated { u64::MAX } else { need };
        let mut recruits = recruit(plan.frame, &fit, rally, taken, UNIT_CAP);
        if recruits.is_empty() {
            return false;
        }
        for breacher in breachers {
            if recruits.len() < UNIT_CAP {
                insert(&mut recruits, breacher);
            }
        }
        recruits = plan.with_screens(recruits, rally, component, UNIT_CAP);
        if !ledger.order(hunt(recruits.clone(), rally)) {
            return false;
        }
        ledger.launched(super::Launch {
            mission: self.next,
            kind: Task::Attack {
                target: target.objective(),
                phase: AttackPhase::Gather,
            }
            .kind(),
            defense: defense(plan.observation, plan.memory, target.approach),
            margin: plan.margin,
            need,
            sent: recruits
                .iter()
                .filter_map(|id| mine(plan.observation, *id))
                .map(striking)
                .sum(),
            units: recruits.clone(),
        });
        self.list.push(Mission {
            id: self.next,
            since: plan.observation.tick,
            units: recruits,
            goal: rally,
            task: Task::Attack {
                target: target.objective(),
                phase: AttackPhase::Gather,
            },
        });
        self.next += 1;
        self.waiting = None;
        true
    }

    /// Moves the attack at `index` through its phases. Returns the target
    /// when the attack gave up on it.
    fn advance(
        &mut self,
        index: usize,
        plan: &Plan<'_>,
        fit: &[&UnitObs],
        ledger: &mut Ledger,
    ) -> Option<Objective> {
        let observation = plan.observation;
        let now = observation.tick;
        let mission = &self.list[index];
        let id = mission.id;
        let Task::Attack {
            target:
                Objective {
                    owner,
                    building,
                    anchor,
                },
            phase,
        } = mission.task
        else {
            unreachable!("the attack mission's task");
        };
        let rally = plan.rally(owner)?;
        let engaged = matches!(phase, AttackPhase::Engage { .. });
        let regrouping = matches!(phase, AttackPhase::Gather | AttackPhase::Recover);

        if !engaged {
            let wounded: Vec<UnitId> = mission
                .units
                .iter()
                .copied()
                .filter(|id| {
                    // A machine nothing at home mends stays with the attack.
                    mine(observation, *id).is_some_and(|unit| {
                        !healthy(unit, WOUNDED) && unit.kind.stats().repairable()
                    })
                })
                .collect();
            if !wounded.is_empty() && ledger.order(run(wounded.clone(), rally)) {
                self.release(&wounded);
            }
        }
        let index = self.index_of(id)?;

        let current = plan.target(owner, building, anchor);
        let target = if regrouping {
            let current =
                current.filter(|_| !plan.memory.abandoned(building, anchor, observation.tick));
            match (current, plan.best(None)) {
                (Some(current), Some(best)) if best.score * 4 < current.score * 5 => Some(current),
                (_, best) => best,
            }
        } else {
            current
        };
        let Some(target) = target else {
            if regrouping {
                self.list.remove(index);
                return None;
            }
            let mission = &mut self.list[index];
            if ledger.order(run(mission.units.clone(), rally)) {
                if let Task::Attack { phase, .. } = &mut mission.task {
                    *phase = AttackPhase::Recover;
                }
                mission.since = now;
                mission.goal = rally;
            }
            return None;
        };
        let component = plan.map.component(target.approach);
        let mission = &mut self.list[index];
        let breach = breaching(observation, &mission.units);
        mission.task = Task::Attack {
            target: target.objective(),
            phase,
        };
        let (members, stranded) = fighters(observation, &mission.units);
        if stranded {
            if ledger.order(run(mission.units.clone(), rally)) {
                self.list.remove(index);
            }
            return None;
        }
        let all_idle = members
            .iter()
            .filter(|unit| unit.kind != UnitKind::Sapper && !breacher(unit.kind))
            .all(|unit| unit.idle);
        let strength: u64 = members
            .iter()
            .filter(|unit| !unit.kind.stats().weapons.is_empty())
            .map(|unit| value(unit))
            .sum();
        let age = now - mission.since;

        let given_up = 'phase: {
            match phase {
                AttackPhase::Gather | AttackPhase::Recover => {
                    let need = plan.need(target, breach);
                    let room = UNIT_CAP - mission.units.len();
                    let fit: Vec<&UnitObs> = fit
                        .iter()
                        .copied()
                        .filter(|unit| reaches(plan.map, unit, component))
                        .collect();
                    let striking_strength: u64 = members.iter().map(|unit| striking(unit)).sum();
                    // The whole free army joins while it regroups, or only what
                    // the attack still lacks at a seat that attacks piecemeal.
                    let taken = if concentrates(plan.difficulty) {
                        u64::MAX
                    } else {
                        need.saturating_sub(striking_strength)
                    };
                    let recruits = recruit(plan.frame, &fit, rally, taken, room);
                    let joining = plan.with_screens(recruits.clone(), rally, component, room);
                    let ordered = !joining.is_empty() && ledger.order(hunt(joining.clone(), rally));
                    if ordered {
                        for id in &joining {
                            insert(&mut mission.units, *id);
                        }
                    }
                    let recruited = ordered && !recruits.is_empty();
                    plan.join_support(mission, &members, (target, rally, component), ledger);
                    // Recruits still on their way go along once the time to
                    // regroup is up, or a steady stream of new units would hold
                    // the army at home.
                    let ready = striking_strength >= need;
                    if ready && (age >= timeout(phase) || (all_idle && !recruited)) {
                        // A seat that stages stops short of the known
                        // defenses on the way, so the army goes in together
                        // rather than as a column.
                        let (next, goal) = match staging(plan.difficulty)
                            .and_then(|_| plan.stage(target, &[], UNREACHABLE))
                        {
                            Some(stage) => (AttackPhase::Advance, stage),
                            None => (AttackPhase::Travel, target.approach),
                        };
                        if ledger.order(hunt(mission.units.clone(), goal)) {
                            mission.attack_phase(target, next, now, Some(goal));
                        }
                    } else if age >= timeout(phase) {
                        self.list.remove(index);
                    }
                    None
                }
                AttackPhase::Advance | AttackPhase::Stage => stage_on(
                    plan,
                    mission,
                    (target, rally),
                    &members,
                    breach,
                    (strength, all_idle),
                    ledger,
                ),
                AttackPhase::Travel => travel_on(
                    observation,
                    mission,
                    (target, rally),
                    &members,
                    all_idle,
                    ledger,
                ),
                AttackPhase::Engage { .. } => {
                    // Sappers sent at a defense keep that order whatever the rest
                    // of the army is told next.
                    // Breachers, too, cut the nearest defense when idle, and stay
                    // at their work while the rest of the army comes and goes.
                    let mut blasting = Vec::new();
                    for sapper in members.iter().filter(|unit| {
                        (unit.kind == UnitKind::Sapper || breacher(unit.kind)) && unit.idle
                    }) {
                        if let Some(defense) = plan.nearest_defense(target, sapper)
                            && ledger.order(blast(vec![sapper.id], aim(defense)))
                        {
                            blasting.push(sapper.id);
                        }
                    }
                    let army: Vec<UnitId> = mission
                        .units
                        .iter()
                        .copied()
                        .filter(|id| {
                            !blasting.contains(id)
                                && mine(observation, *id).is_none_or(|unit| !breacher(unit.kind))
                        })
                        .collect();
                    let opposition = plan.opposition(&members);
                    if opposition * 1_000 > strength * plan.retreat {
                        if ledger.order(run(army.clone(), rally)) {
                            mission.attack_phase(target, AttackPhase::Withdraw, now, Some(rally));
                            break 'phase Some(target.objective());
                        }
                        break 'phase None;
                    }
                    if !all_idle && age < ENGAGE_TICKS {
                        break 'phase None;
                    }
                    // A fight still being won goes on past its time.
                    if presses(plan.difficulty) && !all_idle && strength >= opposition {
                        mission.since = now;
                        break 'phase None;
                    }
                    let standing = standing(observation, target.objective());
                    let next = plan.best(Some(target)).filter(|next| {
                        !standing
                            && members.iter().map(|unit| striking(unit)).sum::<u64>()
                                >= plan.need(*next, breaching(observation, &mission.units))
                    });
                    if let Some(next) = next {
                        if ledger.order(hunt(army, next.approach)) {
                            mission.attack_phase(
                                next,
                                AttackPhase::Travel,
                                now,
                                Some(next.approach),
                            );
                        }
                        break 'phase None;
                    }
                    if ledger.order(run(army, rally)) {
                        mission.attack_phase(target, AttackPhase::Recover, now, Some(rally));
                        if standing {
                            break 'phase Some(target.objective());
                        }
                    }
                    None
                }
                AttackPhase::Withdraw => {
                    if all_idle || age >= WITHDRAW_TICKS {
                        mission.attack_phase(target, AttackPhase::Recover, now, None);
                    }
                    None
                }
            }
        };
        // Units freed since the attack left join it where it is rather than
        // waiting at home for it to regroup, unless this decision pulled it
        // back or sent it on: they would only be turned around. A staging
        // army takes none: they would keep it waiting for them, and wait
        // at home for the next attack instead.
        if reinforces(plan.difficulty)
            && matches!(phase, AttackPhase::Travel | AttackPhase::Engage { .. })
            && let Some(mission) = self.list.get_mut(index).filter(|mission| mission.id == id)
            && let Task::Attack {
                target: held,
                phase: AttackPhase::Travel | AttackPhase::Engage { .. },
            } = mission.task
            && held == target.objective()
        {
            let fit: Vec<&UnitObs> = fit
                .iter()
                .copied()
                .filter(|unit| reaches(plan.map, unit, component))
                .collect();
            let room = UNIT_CAP - mission.units.len();
            let recruits = plan.with_screens(
                recruit(plan.frame, &fit, target.approach, u64::MAX, room),
                target.approach,
                component,
                room,
            );
            if !recruits.is_empty() && ledger.order(hunt(recruits.clone(), target.approach)) {
                for id in recruits {
                    insert(&mut mission.units, id);
                }
            }
        }
        given_up
    }
}

/// Moves a travelling attack on: it engages on arriving or meeting the
/// enemy, and turns home when its army stands idle short of the target or
/// its time is up. Returns the target when it gave up on it.
fn travel_on(
    observation: &ObservationData,
    mission: &mut Mission,
    (target, rally): (Target, TilePos),
    members: &[&UnitObs],
    all_idle: bool,
    ledger: &mut Ledger,
) -> Option<Objective> {
    let now = observation.tick;
    let arrived = members
        .iter()
        .any(|unit| unit.tile.chebyshev(target.approach) <= CONTACT_TILES);
    if arrived || contact(observation, members) {
        mission.attack_phase(target, AttackPhase::Engage { focus: None }, now, None);
        return None;
    }
    if (all_idle || now - mission.since >= TRAVEL_TICKS)
        && ledger.order(run(mission.units.clone(), rally))
    {
        mission.attack_phase(target, AttackPhase::Recover, now, Some(rally));
        return Some(target.objective());
    }
    None
}

/// Whether an attack in `phase` has left home: on its way, staging or
/// fighting.
fn afield(phase: AttackPhase) -> bool {
    matches!(
        phase,
        AttackPhase::Advance
            | AttackPhase::Stage
            | AttackPhase::Travel
            | AttackPhase::Engage { .. }
    )
}

/// Moves a staging attack on: it goes in once enough of it has gathered or
/// none of the rest is still on its way, with strength enough; falls back
/// out of reach when shelled where it gathers; withdraws from a losing
/// fight; and turns home when it cannot reach its staging spot or is too
/// weak to go in.
/// Returns the target when it gave up on it.
fn stage_on(
    plan: &Plan<'_>,
    mission: &mut Mission,
    (target, rally): (Target, TilePos),
    members: &[&UnitObs],
    breach: u64,
    (strength, all_idle): (u64, bool),
    ledger: &mut Ledger,
) -> Option<Objective> {
    let observation = plan.observation;
    let now = observation.tick;
    let age = now - mission.since;
    let Task::Attack { phase, .. } = mission.task else {
        return None;
    };
    let need = plan.need(target, breach);
    let stage = mission.goal;
    let there = |unit: &UnitObs| unit.tile.chebyshev(stage) <= STAGE_TILES;
    // Sappers wait at the staging spot until the fight begins,
    // and breachers and Sappers never hold the army up.
    let movers: Vec<&UnitObs> = members
        .iter()
        .copied()
        .filter(|unit| unit.kind != UnitKind::Sapper && !breacher(unit.kind))
        .collect();
    let opposition = plan.opposition(members);
    if opposition * 1_000 > strength * plan.retreat {
        if ledger.order(run(mission.units.clone(), rally)) {
            mission.attack_phase(target, AttackPhase::Withdraw, now, Some(rally));
            return Some(target.objective());
        }
        return None;
    }
    let Some(share) = staging(plan.difficulty) else {
        push(observation, mission, target, now, ledger);
        return None;
    };
    // Shelled where it gathers before it can win, the army
    // falls back out of the guns' reach and gathers there;
    // members only passing through fire go on.
    let shells = shelled(observation, members);
    if shells
        .iter()
        .any(|(impact, reach)| stage.chebyshev(*impact) < reach + STAGE_SPACE)
    {
        let back = plan.stage(target, &shells, plan.map.distance(observation.me, stage));
        let (next, goal) = match back {
            Some(back) => (AttackPhase::Advance, back),
            None => (AttackPhase::Recover, rally),
        };
        if ledger.order(run(mission.units.clone(), goal)) {
            mission.attack_phase(target, next, now, Some(goal));
        }
        return None;
    }
    if phase == AttackPhase::Advance {
        if members.iter().any(|unit| there(unit)) {
            // The walk's clock keeps running while the army gathers.
            mission.attack_phase(target, AttackPhase::Stage, mission.since, None);
        } else if (all_idle || age >= TRAVEL_TICKS)
            && ledger.order(run(mission.units.clone(), rally))
        {
            mission.attack_phase(target, AttackPhase::Recover, now, Some(rally));
            return Some(target.objective());
        }
        return None;
    }
    let moving: u64 = movers.iter().map(|unit| value(unit)).sum();
    let near: u64 = movers
        .iter()
        .filter(|unit| there(unit))
        .map(|unit| value(unit))
        .sum();
    // What arrives first never goes in alone, even when it beats the known
    // army on paper: the known defense understates what it meets. It waits
    // for the rest while any of them still walks there; one that is stuck,
    // fighting or standing still holds no one up. A walk past its time
    // ends the wait as it ends any attack's travel.
    let coming = movers.iter().any(|unit| {
        !there(unit)
            && !unit.idle
            && !ledger.stuck(unit.id)
            && !contact(observation, std::slice::from_ref(unit))
    });
    if near * 1_000 >= moving * share || !coming || age >= TRAVEL_TICKS {
        let total: u64 = members.iter().map(|unit| striking(unit)).sum();
        if total >= need {
            push(observation, mission, target, now, ledger);
        } else if ledger.order(run(mission.units.clone(), rally)) {
            mission.attack_phase(target, AttackPhase::Recover, now, Some(rally));
        }
    }
    None
}

/// Sends a staged attack in: every member but its Sappers, which wait to be
/// sent at a defense once the fight begins.
fn push(
    observation: &ObservationData,
    mission: &mut Mission,
    target: Target,
    now: u64,
    ledger: &mut Ledger,
) {
    let going: Vec<UnitId> = mission
        .units
        .iter()
        .copied()
        .filter(|id| mine(observation, *id).is_none_or(|unit| unit.kind != UnitKind::Sapper))
        .collect();
    if ledger.order(hunt(going, target.approach)) {
        mission.attack_phase(target, AttackPhase::Travel, now, Some(target.approach));
    }
}

/// Hostile shells landing among `members` from guns they are not already
/// fighting, each with how far from its impact the gun that fired it may
/// stand. A shell any visible enemy within contact of a member could have
/// fired belongs to a fight the army is in.
fn shelled(observation: &ObservationData, members: &[&UnitObs]) -> Vec<(TilePos, i32)> {
    let near = |tile: TilePos| {
        members
            .iter()
            .any(|unit| unit.tile.chebyshev(tile) <= CONTACT_TILES)
    };
    observation
        .incoming_shells
        .iter()
        .copied()
        .filter(|impact| {
            members
                .iter()
                .any(|unit| unit.tile.chebyshev(*impact) <= SHELLED_TILES)
                && !observation
                    .enemy_units
                    .iter()
                    .any(|enemy| near(enemy.tile) && lobs(enemy, *impact))
        })
        .map(|impact| (impact, gun_reach(observation, impact)))
        .collect()
}

/// Tiles from `impact` the gun that fired it may stand: the longest reach of
/// the visible enemies and known enemy buildings that could have fired it,
/// or, with none of them, the longest artillery reach, since the gun is out
/// of sight.
fn gun_reach(observation: &ObservationData, impact: TilePos) -> i32 {
    let units = observation
        .enemy_units
        .iter()
        .filter(|enemy| lobs(enemy, impact))
        .map(|enemy| crate::defenses::reach(enemy.kind).ceil().to_num::<i32>());
    let buildings = observation
        .enemy_buildings
        .iter()
        .filter(|building| battery(building, impact))
        .filter_map(ground_reach);
    units.chain(buildings).max().unwrap_or_else(|| {
        artillery().map_or(0, |(reach, _)| i32::try_from(reach).unwrap_or(i32::MAX))
    })
}

/// Sends the attack's point defenders that have drifted from its army back
/// over it: to the fighting member nearest the army's middle, once a
/// defender stands farther from that member than half its reach, so the
/// middle of the army stays covered. The only per-unit movement the attack
/// gives beyond its phase orders.
fn keep_screens(plan: &Plan<'_>, mission: &Mission, ledger: &mut Ledger) {
    let members: Vec<&UnitObs> = mission
        .units
        .iter()
        .filter_map(|id| mine(plan.observation, *id))
        .collect();
    let fighting: Vec<&UnitObs> = members
        .iter()
        .copied()
        .filter(|unit| !screens(unit) && unit.kind != UnitKind::Sapper)
        .collect();
    let count = i64::try_from(fighting.len()).unwrap_or(i64::MAX);
    let (sum_x, sum_y) = fighting.iter().fold((0_i64, 0_i64), |(x, y), unit| {
        (x + i64::from(unit.tile.x), y + i64::from(unit.tile.y))
    });
    // Distances to the middle are compared scaled by the member count, so
    // the middle never rounds onto one side of a mirrored army.
    let Some(middle) = fighting.iter().min_by_key(|unit| {
        let dx = i64::from(unit.tile.x) * count - sum_x;
        let dy = i64::from(unit.tile.y) * count - sum_y;
        (
            dx * dx + dy * dy,
            plan.frame.rank(doubled(mission.goal), doubled(unit.tile)),
            unit.id,
        )
    }) else {
        return;
    };
    let drifted: Vec<UnitId> = members
        .iter()
        .filter(|unit| screens(unit))
        .filter(|unit| {
            let half = unit
                .kind
                .stats()
                .point_defense
                .map_or(Fx::ZERO, |defense| defense.radius / 2);
            let dx = Fx::from_num(unit.tile.x - middle.tile.x);
            let dy = Fx::from_num(unit.tile.y - middle.tile.y);
            dx * dx + dy * dy > half * half
        })
        .map(|unit| unit.id)
        .collect();
    if !drifted.is_empty() {
        ledger.order(run(drifted, middle.tile));
    }
}

impl<'a> Plan<'a> {
    /// What a decision knows about attacking, with no target held and no
    /// support counted yet.
    fn new(
        observation: &'a ObservationData,
        map: &'a MapModel,
        profile: &ResolvedProfile,
        memory: &'a Memory,
        scratch: &Scratch,
    ) -> Self {
        Self {
            observation,
            map,
            frame: scratch.frame,
            memory,
            minimum: minimum(profile.stance),
            margin: margin(profile.difficulty),
            retreat: outweighed(profile),
            difficulty: profile.difficulty,
            misjudge: misjudge(profile, observation.tick),
            sappers: Vec::new(),
            breachers: Vec::new(),
            screens: Vec::new(),
            rival: scratch.rival,
            held: Vec::new(),
        }
    }

    /// Those of `units` that can get to a target whose approach lies in
    /// `component`, nearest `from` first, at most `room`.
    fn nearest(
        &self,
        units: &[&UnitObs],
        from: (i64, i64),
        component: Option<u32>,
        room: usize,
    ) -> Vec<UnitId> {
        let mut near: Vec<&UnitObs> = units
            .iter()
            .copied()
            .filter(|unit| reaches(self.map, unit, component))
            .collect();
        near.sort_by_key(|unit| (self.frame.rank(from, doubled(unit.tile)), unit.id));
        near.iter().take(room).map(|unit| unit.id).collect()
    }

    /// `units` joined by the free point defenders nearest `rally` that can
    /// walk to a target whose approach lies in `component`, `room` units in
    /// all at most. The seat trains them in proportion to the enemy's known
    /// shellfire, so an attack takes every one it can.
    fn with_screens(
        &self,
        mut units: Vec<UnitId>,
        rally: TilePos,
        component: Option<u32>,
        room: usize,
    ) -> Vec<UnitId> {
        let room = room.saturating_sub(units.len());
        for screen in self.nearest(&self.screens, doubled(rally), component, room) {
            insert(&mut units, screen);
        }
        units
    }

    /// Idle free Sappers nearest `rally`, at most `room`, that can get to a
    /// target whose approach lies in `component`, when known enemy defenses
    /// that can hit ground stand around `target`.
    fn sappers(
        &self,
        target: Target,
        rally: TilePos,
        component: Option<u32>,
        room: usize,
    ) -> Vec<UnitId> {
        if self.defenses(target).next().is_none() {
            return Vec::new();
        }
        let mut ids = self.nearest(&self.sappers, doubled(rally), component, room);
        ids.sort_unstable();
        ids
    }

    /// Known enemy buildings around `target` that can hit ground.
    fn defenses(&self, target: Target) -> impl Iterator<Item = &'a BuildingObs> {
        let size = target.building.size();
        self.observation
            .enemy_buildings
            .iter()
            .filter(move |building| {
                building
                    .kind
                    .base_stats()
                    .weapons
                    .iter()
                    .any(|weapon| weapon.targets.ground)
                    && gap(target.anchor, size, building.anchor, building.kind.size())
                        < DEFENSE_TILES
            })
    }

    /// The known defense around `target` on `sapper`'s ground nearest it,
    /// else the target itself while the seat knows it.
    fn nearest_defense(&self, target: Target, sapper: &UnitObs) -> Option<&'a BuildingObs> {
        let from = doubled(sapper.tile);
        let ground = self.map.component(sapper.tile);
        self.defenses(target)
            .filter(|building| {
                ring(building.anchor, building.kind.size())
                    .any(|tile| ground.is_some() && self.map.component(tile) == ground)
            })
            .min_by_key(|building| {
                (
                    self.frame
                        .rank(from, footprint_centre(building.kind, building.anchor)),
                    building.id,
                )
            })
            .or_else(|| {
                self.observation.enemy_buildings.iter().find(|building| {
                    (building.player, building.kind, building.anchor)
                        == (target.owner, target.building, target.anchor)
                })
            })
    }

    /// The best target other than `skip`, or `None`. Known enemy buildings
    /// come first; with none, hostile starts are presumed held. With several
    /// enemies, the rival's targets come first.
    fn best(&self, skip: Option<Target>) -> Option<Target> {
        let now = self.observation.tick;
        let differs = |target: &Target| {
            skip.is_none_or(|skip| {
                (skip.owner, skip.building, skip.anchor)
                    != (target.owner, target.building, target.anchor)
            })
        };
        let pick = |targets: Vec<Target>| {
            let open: Vec<Target> = targets
                .into_iter()
                .filter(differs)
                .filter(|target| !self.held.contains(&target.objective()))
                .filter(|target| !self.memory.abandoned(target.building, target.anchor, now))
                .collect();
            // The lowest rung goes for the head: a Foundry whenever it knows
            // one, whatever guards it.
            let head = self.difficulty == BotDifficulty::Scrapheap
                && open
                    .iter()
                    .any(|target| target.building == BuildingKind::Foundry);
            open.into_iter()
                .filter(|target| !head || target.building == BuildingKind::Foundry)
                .max_by_key(|target| {
                    (
                        target.score,
                        std::cmp::Reverse(
                            self.frame.rank(self.frame.home, doubled(target.approach)),
                        ),
                    )
                })
        };
        let known: Vec<Target> = self
            .observation
            .enemy_buildings
            .iter()
            .filter_map(|building| self.target(building.player, building.kind, building.anchor))
            .collect();
        let starts: Vec<Target> = self
            .map
            .hostiles(self.observation.me)
            .filter_map(|owner| {
                let anchor = self.map.start(owner)?;
                self.target(owner, BuildingKind::Foundry, anchor)
            })
            .collect();
        let rival = |targets: &[Target]| -> Vec<Target> {
            targets
                .iter()
                .copied()
                .filter(|target| self.rival.is_none_or(|rival| target.owner == rival))
                .collect()
        };
        pick(rival(&known))
            .or_else(|| pick(rival(&starts)))
            .or_else(|| pick(known))
            .or_else(|| pick(starts))
    }

    /// `building` at `anchor` as a target, if the army can reach it.
    fn target(&self, owner: PlayerId, building: BuildingKind, anchor: TilePos) -> Option<Target> {
        let approach = approach(self.map, self.observation.me, self.frame, building, anchor)?;
        let tiles = u64::from(self.map.distance(self.observation.me, approach)) / 10;
        let score = match self.difficulty {
            // The upper rungs go after the weakest valuable target: known
            // defense around it counts like distance to cover.
            BotDifficulty::Veteran | BotDifficulty::Prime => {
                let guard = defense(self.observation, self.memory, approach);
                distant_worth(building, tiles + guard / GUARD_PER_TILE)
            }
            BotDifficulty::Scrapheap | BotDifficulty::Standard => distant_worth(building, tiles),
        };
        Some(Target {
            owner,
            building,
            anchor,
            approach,
            score,
        })
    }

    /// Army value the attack on `target` needs: the known army it would
    /// meet times the margin, and never under the stance minimum. What
    /// breachers bring against buildings, `breach`, answers the guarding
    /// buildings before the army has to. Above the lowest rung, the known
    /// defenses whose fire reaches the way there count with those around the
    /// target.
    fn need(&self, target: Target, breach: u64) -> u64 {
        let (units, mut buildings) =
            opposed_parts(self.observation, self.map, self.memory, &[target.approach]);
        if surveys(self.difficulty) {
            buildings = self
                .on_the_way(target, None)
                .into_iter()
                .map(building_value)
                .sum();
        }
        ((units + buildings.saturating_sub(breach)) * self.misjudge / 1_000 * self.margin / 1_000)
            .max(self.minimum)
    }

    /// Known enemy buildings that fire on ground and guard `target`, or whose
    /// fire reaches a walk there, over `way` when the caller has it.
    fn on_the_way(&self, target: Target, way: Option<&Grid<u16>>) -> Vec<&'a BuildingObs> {
        let (mut guarding, beyond): (Vec<&BuildingObs>, Vec<&BuildingObs>) = self
            .observation
            .enemy_buildings
            .iter()
            .filter(|building| ground_reach(building).is_some())
            .partition(|building| guards(building, target.approach));
        if beyond.is_empty() {
            return guarding;
        }
        let way = way.map_or_else(|| Cow::Owned(self.way(target)), Cow::Borrowed);
        guarding.extend(beyond.into_iter().filter(|building| covers(building, &way)));
        guarding
    }

    /// The walks from the seat's start to `target` that count as ways there:
    /// each tile's ground distance back to the approach, unreachable off
    /// them.
    fn way(&self, target: Target) -> Grid<u16> {
        self.map
            .corridor(self.observation.me, target.approach, ROUTE_SLACK)
    }

    /// Where an attack on `target` gathers before going in: the tile on the
    /// way nearest the target, nearer home than `within`, that stands short
    /// of where the first known defense's fire reaches the way, beyond the
    /// reach of every such defense and of the guns that fired `shells` by
    /// `STAGE_SPACE`, and that far from every known enemy building. Of tiles
    /// as near, the most direct walk wins, then the one nearest home. `None`
    /// while no known defense's fire reaches the way, or no tile qualifies.
    fn stage(&self, target: Target, shells: &[(TilePos, i32)], within: u16) -> Option<TilePos> {
        let me = self.observation.me;
        let way = self.way(target);
        // A site in sight cannot fire; a remembered one may have been
        // finished since it was seen.
        let guns: Vec<(&BuildingObs, i32)> = self
            .on_the_way(target, Some(&way))
            .into_iter()
            .filter(|building| building.built || !building.seen)
            .filter_map(|building| Some((building, ground_reach(building)?)))
            .collect();
        let reached = |tile: TilePos, space: i32| {
            guns.iter().any(|(building, reach)| {
                gap(building.anchor, building.kind.size(), tile, (1, 1)) < reach + space
            })
        };
        let on_way: Vec<(TilePos, u16, u16)> = way
            .iter()
            .filter(|(_, back)| **back != UNREACHABLE)
            .map(|(tile, back)| (tile, self.map.distance(me, tile), *back))
            .collect();
        let first = on_way
            .iter()
            .copied()
            .filter(|(tile, _, _)| reached(*tile, 0))
            .map(|(_, ahead, _)| ahead)
            .min()?;
        on_way
            .into_iter()
            .filter(|(tile, ahead, _)| {
                *ahead < first.min(within)
                    && !reached(*tile, STAGE_SPACE)
                    && shells
                        .iter()
                        .all(|(impact, reach)| tile.chebyshev(*impact) >= reach + STAGE_SPACE)
                    && self.observation.enemy_buildings.iter().all(|building| {
                        gap(building.anchor, building.kind.size(), *tile, (1, 1)) >= STAGE_SPACE
                    })
            })
            .max_by_key(|(tile, ahead, back)| {
                (
                    *ahead,
                    Reverse(u32::from(*ahead) + u32::from(*back)),
                    Reverse(self.frame.rank(self.frame.home, doubled(*tile))),
                )
            })
            .map(|(tile, _, _)| tile)
    }

    /// Brings support to a regrouping attack on `target`: a Sapper for each
    /// known defense around it, and every free breacher that can get there.
    fn join_support(
        &self,
        mission: &mut Mission,
        members: &[&UnitObs],
        (target, rally, component): (Target, TilePos, Option<u32>),
        ledger: &mut Ledger,
    ) {
        let sapping = members
            .iter()
            .filter(|unit| unit.kind == UnitKind::Sapper)
            .count();
        let room = self
            .defenses(target)
            .count()
            .saturating_sub(sapping)
            .min(UNIT_CAP.saturating_sub(mission.units.len()));
        let sappers = self.sappers(target, rally, component, room);
        if !sappers.is_empty() && ledger.order(run(sappers.clone(), rally)) {
            for sapper in sappers {
                insert(&mut mission.units, sapper);
            }
        }
        let room = UNIT_CAP.saturating_sub(mission.units.len());
        let breachers: Vec<UnitId> = self
            .breachers(component, room)
            .into_iter()
            .filter(|id| !mission.units.contains(id))
            .collect();
        if !breachers.is_empty() && ledger.order(run(breachers.clone(), rally)) {
            for breacher in breachers {
                insert(&mut mission.units, breacher);
            }
        }
    }

    /// Idle free breachers, at most `room`, that can get to a target whose
    /// approach lies in `component`, nearest home first.
    fn breachers(&self, component: Option<u32>, room: usize) -> Vec<UnitId> {
        let mut ids = self.nearest(&self.breachers, self.frame.home, component, room);
        ids.sort_unstable();
        ids
    }

    /// Value of the armed enemies the seat knows of within contact of the
    /// members: remembered units by confidence, and known armed buildings by
    /// health.
    fn opposition(&self, members: &[&UnitObs]) -> u64 {
        let now = self.observation.tick;
        let near = |tile: TilePos| {
            members
                .iter()
                .any(|unit| unit.tile.chebyshev(tile) <= CONTACT_TILES)
        };
        let units: u64 = self
            .memory
            .units()
            .iter()
            .filter(|unit| unit.kind.stats().can_fight() && near(unit.tile))
            .map(|unit| unit.value(now))
            .sum();
        let buildings: u64 = self
            .observation
            .enemy_buildings
            .iter()
            .filter(|building| !building.kind.base_stats().weapons.is_empty())
            .filter(|building| {
                members.iter().any(|unit| {
                    gap(building.anchor, building.kind.size(), unit.tile, (1, 1)) < CONTACT_TILES
                })
            })
            .map(building_value)
            .sum();
        // Breachers among the members answer the armed buildings first.
        let breach: u64 = members.iter().map(|unit| against_buildings(unit)).sum();
        (units + buildings.saturating_sub(breach)) * self.misjudge / 1_000
    }

    /// Where the army gathers against `owner`: the open tile within reach of
    /// the seat's start that is nearest `owner`'s start by ground.
    fn rally(&self, owner: PlayerId) -> Option<TilePos> {
        let me = self.observation.me;
        let start = self.map.start(me)?;
        let occupied = |tile: TilePos| {
            self.observation
                .my_buildings
                .iter()
                .any(|building| gap(building.anchor, building.kind.size(), tile, (1, 1)) < 0)
        };
        (-RALLY_SEARCH..=RALLY_SEARCH + 1)
            .flat_map(|dy| (-RALLY_SEARCH..=RALLY_SEARCH + 1).map(move |dx| start.offset(dx, dy)))
            .filter(|tile| self.map.distance(me, *tile) <= RALLY_REACH)
            .filter(|tile| !occupied(*tile))
            .min_by_key(|tile| {
                (
                    self.map.distance(owner, *tile),
                    self.frame.rank(self.frame.home, doubled(*tile)),
                )
            })
    }
}

/// The known ground defense around `tile`: remembered armed enemy units by
/// confidence, and known enemy buildings that fire on ground by health.
pub(super) fn defense(observation: &ObservationData, memory: &Memory, tile: TilePos) -> u64 {
    defense_around(observation, memory, &[tile])
}

/// The known army an attack on `tiles` would meet: every armed enemy the seat
/// remembers that hits ground, on the first tile's ground or in the air, by
/// confidence, with the known enemy buildings guarding any of `tiles`, each
/// counted once. An army that beats only a target's own guard meets the rest
/// on its way in or at the target, and turns back.
pub(super) fn opposed(
    observation: &ObservationData,
    map: &MapModel,
    memory: &Memory,
    tiles: &[TilePos],
) -> u64 {
    let (units, buildings) = opposed_parts(observation, map, memory, tiles);
    units + buildings
}

/// [`opposed`] in its two parts: the armed enemy units, and the guarding
/// buildings.
fn opposed_parts(
    observation: &ObservationData,
    map: &MapModel,
    memory: &Memory,
    tiles: &[TilePos],
) -> (u64, u64) {
    let now = observation.tick;
    let ground = tiles.first().and_then(|tile| map.component(*tile));
    let home = map
        .start(observation.me)
        .and_then(|start| map.component(start));
    let units: u64 = memory
        .units()
        .iter()
        .filter(|unit| {
            crate::defenses::reach(unit.kind) > Fx::ZERO
                && (unit.kind.stats().domain == Domain::Air || map.component(unit.tile) == ground)
        })
        .map(|unit| super::remembered(unit, map, home, now))
        .sum();
    let buildings: u64 = observation
        .enemy_buildings
        .iter()
        .filter(|building| tiles.iter().any(|tile| guards(building, *tile)))
        .map(building_value)
        .sum();
    (units, buildings)
}

/// What the breachers among `units` bring against buildings.
fn breaching(observation: &ObservationData, units: &[UnitId]) -> u64 {
    units
        .iter()
        .filter_map(|id| mine(observation, *id))
        .map(against_buildings)
        .sum()
}

/// The known ground defense around any of `tiles`, each defender counted
/// once.
pub(super) fn defense_around(
    observation: &ObservationData,
    memory: &Memory,
    tiles: &[TilePos],
) -> u64 {
    let now = observation.tick;
    let units: u64 = memory
        .units()
        .iter()
        .filter(|unit| crate::defenses::reach(unit.kind) > Fx::ZERO)
        .filter(|unit| {
            tiles
                .iter()
                .any(|tile| unit.tile.chebyshev(*tile) <= DEFENSE_TILES)
        })
        .map(|unit| unit.value(now))
        .sum();
    let buildings: u64 = observation
        .enemy_buildings
        .iter()
        .filter(|building| tiles.iter().any(|tile| guards(building, *tile)))
        .map(building_value)
        .sum();
    units + buildings
}

/// Whether a known, built enemy building's ground fire reaches `tile`.
pub(super) fn fortified(observation: &ObservationData, tile: TilePos) -> bool {
    observation.enemy_buildings.iter().any(|building| {
        building.built
            && ground_reach(building).is_some_and(|reach| {
                gap(building.anchor, building.kind.size(), tile, (1, 1)) < reach
            })
    })
}

/// Tiles a known building's ground fire reaches at its current tier, rounded
/// up, or `None` when it cannot fire on ground.
fn ground_reach(building: &BuildingObs) -> Option<i32> {
    building
        .kind
        .tier_stats(building.tier)
        .weapons
        .iter()
        .filter(|weapon| weapon.targets.ground)
        .map(|weapon| weapon.range.ceil().to_num::<i32>())
        .max()
}

/// Whether `building`'s ground fire reaches a tile of `way`.
fn covers(building: &BuildingObs, way: &Grid<u16>) -> bool {
    let Some(reach) = ground_reach(building) else {
        return false;
    };
    let (anchor, (width, height)) = (building.anchor, building.kind.size());
    (anchor.y - reach..anchor.y + height + reach).any(|y| {
        (anchor.x - reach..anchor.x + width + reach).any(|x| {
            let tile = TilePos::new(x, y);
            way.get(tile).is_some_and(|back| *back != UNREACHABLE)
                && gap(anchor, (width, height), tile, (1, 1)) < reach
        })
    })
}

/// Whether `building` fires on ground and stands around `tile`.
fn guards(building: &BuildingObs, tile: TilePos) -> bool {
    building
        .kind
        .base_stats()
        .weapons
        .iter()
        .any(|weapon| weapon.targets.ground)
        && gap(building.anchor, building.kind.size(), tile, (1, 1)) < DEFENSE_TILES
}

/// Whether any visible armed enemy or seen enemy building is within contact
/// of a member.
pub(super) fn contact(observation: &ObservationData, members: &[&UnitObs]) -> bool {
    let near = |tile: TilePos| {
        members
            .iter()
            .any(|unit| unit.tile.chebyshev(tile) <= CONTACT_TILES)
    };
    observation
        .enemy_units
        .iter()
        .any(|enemy| enemy.kind.stats().can_fight() && near(enemy.tile))
        || observation.enemy_buildings.iter().any(|building| {
            building.seen
                && members.iter().any(|unit| {
                    gap(building.anchor, building.kind.size(), unit.tile, (1, 1)) < CONTACT_TILES
                })
        })
}

/// A known building's price with every upgrade it reached, discounted by its
/// missing health at that tier.
pub(crate) fn building_value(building: &BuildingObs) -> u64 {
    let paid = u64::from(building.kind.invested_cost(building.tier));
    let max_hp = building.kind.tier_stats(building.tier).max_hp;
    paid * u64::from(building.hp) / u64::from(max_hp.max(1))
}

/// Units nearest `rally` first until what they bring against a building
/// reaches `need`, at most `room` of them, by id.
pub(super) fn recruit(
    frame: HomeFrame,
    fit: &[&UnitObs],
    rally: TilePos,
    need: u64,
    room: usize,
) -> Vec<UnitId> {
    recruit_by(frame, fit, rally, need, room, striking)
}

/// Units nearest `rally` first until their `worth` reaches `need`, at most
/// `room` of them, by id.
pub(super) fn recruit_by(
    frame: HomeFrame,
    fit: &[&UnitObs],
    rally: TilePos,
    need: u64,
    room: usize,
    worth: fn(&UnitObs) -> u64,
) -> Vec<UnitId> {
    let mut sorted = fit.to_vec();
    sorted.sort_by_key(|unit| (frame.rank(doubled(rally), doubled(unit.tile)), unit.id));
    let mut have = 0;
    let mut recruits = Vec::new();
    for unit in sorted.into_iter().take(room) {
        if have >= need {
            break;
        }
        have += worth(unit);
        recruits.push(unit.id);
    }
    recruits.sort_unstable();
    recruits
}

/// What `unit` adds against a building: its value if it can hit ground.
/// Anti-air escorts go along but cannot take the target.
pub(super) fn striking(unit: &UnitObs) -> u64 {
    if hits(unit, Domain::Ground) {
        value(unit)
    } else {
        0
    }
}

/// A line, siege, anti-air or anti-heavy unit at `health` per mille or better.
fn eligible(unit: &UnitObs, health: u32) -> bool {
    matches!(
        composition::role(unit.kind),
        Some(Role::Line | Role::Siege | Role::AntiAir | Role::AntiHeavy)
    ) && healthy(unit, health)
}

pub(super) fn healthy(unit: &UnitObs, health: u32) -> bool {
    u64::from(unit.hp) * 1_000 >= u64::from(unit.kind.stats().max_hp) * u64::from(health)
}

/// The members of `units` that fight, and whether only point defenders are
/// left. Point defenders follow an army; they never decide where it is or
/// what it meets.
fn fighters<'a>(observation: &'a ObservationData, units: &[UnitId]) -> (Vec<&'a UnitObs>, bool) {
    let everyone: Vec<&UnitObs> = units
        .iter()
        .filter_map(|id| mine(observation, *id))
        .collect();
    let fighting: Vec<&UnitObs> = everyone
        .iter()
        .copied()
        .filter(|unit| !screens(unit))
        .collect();
    let stranded = fighting.is_empty() && !everyone.is_empty();
    (fighting, stranded)
}

/// Whether `unit` shields an army from shells rather than fighting in it.
fn screens(unit: &UnitObs) -> bool {
    composition::role(unit.kind) == Some(Role::Screen)
}

/// Whether `unit` can get to a target whose approach lies in `component`.
fn reaches(map: &MapModel, unit: &UnitObs, component: Option<u32>) -> bool {
    unit.kind.stats().domain == Domain::Air || map.component(unit.tile) == component
}

/// Army value under which the seat does not attack.
pub(crate) fn minimum(stance: BotStance) -> u64 {
    match stance {
        BotStance::Turtle => 800,
        BotStance::Balanced => 600,
        BotStance::Aggressive => 450,
    }
}

/// Whether the seat attacks with one army of everything it can spare, rather
/// than several attacks each sized to its own target that meet the enemy one
/// at a time: a difficulty limit, the lowest rung's readable mistake.
pub(crate) fn concentrates(difficulty: BotDifficulty) -> bool {
    difficulty != BotDifficulty::Scrapheap
}

/// Per mille of the value of its moving members that must stand at a
/// staging spot before a seat's army goes in while the rest are still on
/// their way, or `None` for the lowest rung, which walks straight in as a
/// column: a difficulty limit on how well a seat attacks. Veteran and Prime
/// wait for more of the army than Standard.
fn staging(difficulty: BotDifficulty) -> Option<u64> {
    match difficulty {
        BotDifficulty::Scrapheap => None,
        BotDifficulty::Standard => Some(700),
        BotDifficulty::Veteran | BotDifficulty::Prime => Some(850),
    }
}

/// Whether a seat counts the known defenses whose fire reaches its way to a
/// target, not only those around it: a difficulty limit on estimate
/// accuracy, which the lowest rung lacks.
fn surveys(difficulty: BotDifficulty) -> bool {
    difficulty != BotDifficulty::Scrapheap
}

/// Whether an attack travelling out of contact lends its units to a defense
/// at home: the lowest rung's readable mistake, a difficulty limit. Higher
/// rungs keep an army that has set out together.
pub(super) fn lends(difficulty: BotDifficulty) -> bool {
    difficulty == BotDifficulty::Scrapheap
}

/// Whether units freed while an attack travels or fights join it where it
/// is, rather than waiting at home until it comes back to regroup: a
/// difficulty limit.
fn reinforces(difficulty: BotDifficulty) -> bool {
    matches!(difficulty, BotDifficulty::Veteran | BotDifficulty::Prime)
}

/// Whether a fight still being won goes on past its time: a difficulty
/// limit.
fn presses(difficulty: BotDifficulty) -> bool {
    matches!(difficulty, BotDifficulty::Veteran | BotDifficulty::Prime)
}

/// Per mille of a mission's strength the enemies around it must reach before
/// it withdraws. The upper rungs give a fight up once clearly lost and the
/// lower ones at even odds, a difficulty limit; guile gives it up up to a
/// tenth sooner and its lack up to a tenth later, a personality limit.
pub(super) fn outweighed(profile: &ResolvedProfile) -> u64 {
    let odds: u64 = match profile.difficulty {
        BotDifficulty::Scrapheap | BotDifficulty::Standard => 1_000,
        BotDifficulty::Veteran | BotDifficulty::Prime => 1_250,
    };
    odds * (1_100 - 2 * u64::from(profile.traits.guile.min(100))) / 1_000
}

/// Stream of the lowest rung's misjudgments of enemy strength.
const MISJUDGE_STREAM: u64 = 0x0B07_1630;

/// Per mille by which the lowest rung's sense of the enemy's strength strays
/// at most, either way: a difficulty limit on estimate accuracy.
const MISJUDGE: u32 = 400;

/// Ticks one misjudgment of enemy strength lasts: a difficulty limit on
/// estimate accuracy. A fresh misjudgment every decision would average out
/// into a bias: the seat would set out on its luckiest draw and pull back on
/// its unluckiest.
const MISJUDGE_TICKS: u64 = 600;

/// Per mille of the enemy's known strength a seat believes it faces at
/// `tick`: exact above the lowest rung; for Scrapheap, a deterministic draw
/// from its personality seed for each stretch of `MISJUDGE_TICKS`, so it
/// sometimes attacks into a stronger army and sometimes pulls back from a
/// weaker one.
fn misjudge(profile: &ResolvedProfile, tick: u64) -> u64 {
    if profile.difficulty != BotDifficulty::Scrapheap {
        return 1_000;
    }
    let stretch = tick / MISJUDGE_TICKS;
    let mut rng = Pcg32::new(
        profile.personality_seed.wrapping_add(stretch),
        MISJUDGE_STREAM,
    );
    u64::from(1_000 - MISJUDGE + rng.next_below(2 * MISJUDGE + 1))
}

/// Per mille of the known opposition at a target that a mission must bring.
pub(crate) fn margin(difficulty: BotDifficulty) -> u64 {
    match difficulty {
        BotDifficulty::Scrapheap => 2_500,
        BotDifficulty::Standard => 2_000,
        BotDifficulty::Veteran => 1_750,
        BotDifficulty::Prime => 1_500,
    }
}

#[cfg(test)]
mod tests;
