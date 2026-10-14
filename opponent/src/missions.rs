//! Missions: units the seat commands together for one purpose, in a phase
//! that persists between decisions so repeated scoring does not replan them.
//! Missions own only units that exist; production never works for one.
//!
//! Missions take units in a fixed order each decision: defense first, then
//! lift, then attacks, anti-air clearing, strikes and raids, then scouting.
//! Each takes from what [`Missions::available`] leaves free when it runs.
//! Between defense and the rest, free wounded units go to a Repair Bay and
//! sit out the other missions while they heal.

use crate::decision::Ledger;
use crate::frame::{HomeFrame, doubled, ring};
use crate::map::{MapModel, UNREACHABLE};
use crate::memory::Memory;
use chassis::grid::TilePos;
use oxide_sim::observation::{ObservationData, UnitObs};
use oxide_sim::scenario::BotStance;
use oxide_sim::stats::Domain;
use oxide_sim::{BuildingId, BuildingKind, Command, PlayerId, UnitId, UnitKind};
use serde::{Deserialize, Serialize};

mod air;
mod attack;
mod clear;
mod defense;
mod focus;
mod guarding;
mod lift;
mod raid;
mod repair;
mod reserve;
mod rival;
mod scouting;
mod settlement;
mod strike;

pub(crate) use air::{Hazard, hazards};
pub(crate) use attack::{building_value, margin, minimum};
pub(crate) use defense::Shortfall;
pub(crate) use lift::{carrier, payload};
pub(crate) use repair::{Meal, feeds, treating};
pub(crate) use scouting::{look, points};
pub(crate) use settlement::Settlement;
pub(crate) use strike::strike_need;

/// Missions the seat runs at once: a computation bound on the missions each
/// decision advances, above a scout at every scouting point a map can hold
/// together with the missions of every other kind in normal play.
pub(crate) const MISSION_CAP: usize = 128;

/// Units one mission holds: a computation bound on a mission's orders, above
/// any army a seat fields in normal play.
const UNIT_CAP: usize = 4_096;

/// The seat's missions, by id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Missions {
    /// The id the next mission takes.
    next: u64,
    list: Vec<Mission>,
    /// Since when a free army that could attack has not, or production has
    /// sat idle, with no attack launching.
    waiting: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Mission {
    id: u64,
    /// Tick the current phase began.
    since: u64,
    /// Members, by id.
    units: Vec<UnitId>,
    /// Where the members were last sent.
    goal: TilePos,
    task: Task,
}

/// What a mission is doing, with the phases its kind can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "task", rename_all = "snake_case", deny_unknown_fields)]
enum Task {
    Defend {
        asset: BuildingId,
        phase: DefendPhase,
    },
    Attack {
        target: Objective,
        phase: AttackPhase,
    },
    Scout {
        point: u16,
    },
    Lift {
        target: Objective,
        phase: LiftPhase,
    },
    Settle {
        target: settlement::Target,
        phase: settlement::SettlePhase,
    },
    Strike {
        target: Objective,
        phase: StrikePhase,
    },
    Raid {
        target: Objective,
        phase: RaidPhase,
    },
    Clear {
        aim: TilePos,
        phase: StrikePhase,
    },
}

impl Task {
    /// The objective an attack, lift, strike or raid goes after.
    fn target(&self) -> Option<Objective> {
        match *self {
            Task::Attack { target, .. }
            | Task::Lift { target, .. }
            | Task::Strike { target, .. }
            | Task::Raid { target, .. } => Some(target),
            Task::Defend { .. } | Task::Scout { .. } | Task::Clear { .. } | Task::Settle { .. } => {
                None
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum RaidPhase {
    Travel,
    Strike,
    Withdraw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum StrikePhase {
    Gather,
    Travel,
    Engage { focus: Option<UnitId> },
    Withdraw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum LiftPhase {
    Load,
    Fly,
    Fight { focus: Option<UnitId> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum DefendPhase {
    /// A quiet local guard, kept by the guard pass at what known threats near
    /// its building call for. A defense that answers an attack there turns
    /// it to Engage with its guards aboard.
    Guard,
    /// Answering enemies at the building, owned by the defense pass.
    Engage { focus: Option<UnitId> },
    /// Holding briefly after the threat left, owned by the defense pass until
    /// it ends and the guard pass may form a guard again.
    Recover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum AttackPhase {
    Gather,
    Travel,
    Engage { focus: Option<UnitId> },
    Withdraw,
    Recover,
}

/// An enemy building a mission goes after.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Objective {
    owner: PlayerId,
    building: BuildingKind,
    anchor: TilePos,
}

/// What a mission is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "mission", rename_all = "snake_case")]
pub enum MissionKind {
    /// Lifts a builder, and an escort while known enemies stand near, to a
    /// resource site no own worker stands on.
    Settle {
        /// The building being established.
        building: BuildingKind,
        /// Its chosen footprint.
        anchor: TilePos,
    },
    /// Answers enemies threatening the seat's buildings around a Foundry.
    Defend {
        /// The Foundry the threats are nearest.
        asset: BuildingId,
    },
    /// Takes an army to a known or presumed enemy building.
    Attack {
        /// The building's owner.
        owner: PlayerId,
        /// What it is.
        building: BuildingKind,
        /// Its footprint anchor.
        anchor: TilePos,
    },
    /// Looks at a place the seat has not seen for a while.
    Scout {
        /// The place, by its index among the seat's scouting points.
        point: u16,
    },
    /// Flies ground units to an enemy building no ground route reaches.
    Lift {
        /// The building's owner.
        owner: PlayerId,
        /// What it is.
        building: BuildingKind,
        /// Its footprint anchor.
        anchor: TilePos,
    },
    /// Sends ground-attack aircraft at a known or presumed enemy building.
    Strike {
        /// The building's owner.
        owner: PlayerId,
        /// What it is.
        building: BuildingKind,
        /// Its footprint anchor.
        anchor: TilePos,
    },
    /// Sends a few raiders at an enemy harvest line or a lightly defended
    /// building, and back.
    Raid {
        /// The building's owner.
        owner: PlayerId,
        /// What it is.
        building: BuildingKind,
        /// Its footprint anchor.
        anchor: TilePos,
    },
    /// Sends ground-attack aircraft at enemy anti-air on ground the seat
    /// cannot walk to.
    Clear {
        /// Where the anti-air was last seen.
        aim: TilePos,
    },
}

/// Where a mission stands, as reports see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Keeps the local force needed for a base's protection.
    Guard,
    /// Holding a landing until the builder places its building.
    Found,
    /// Assembling at a rally point.
    Gather,
    /// On the way to its target.
    Travel,
    /// Fighting what it was formed for.
    Engage,
    /// Pulling back from a fight it was losing.
    Withdraw,
    /// Its purpose is gone for now; it waits, then either resumes or lets its
    /// units go.
    Recover,
    /// Boarding carriers.
    Load,
    /// Carriers on their way to the landing.
    Fly,
    /// Landed units fighting.
    Fight,
}

/// An attack one decision launched, with what the seat believed when it
/// launched it. Reported to hosts for evaluation; never saved and never
/// read by a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Launch {
    /// The attack's mission id.
    pub mission: u64,
    /// The attack and the building it went after.
    pub kind: MissionKind,
    /// Known local defense at the target's approach, in scrap.
    pub defense: u64,
    /// Per mille of the known army it would meet that the attack had to
    /// bring.
    pub margin: u64,
    /// Strength the attack needed: the known army it would meet times the
    /// margin, never under the stance minimum.
    pub need: u64,
    /// Strength against buildings of the units sent.
    pub sent: u64,
    /// The units sent, by id.
    pub units: Vec<UnitId>,
}

/// One mission as a decision left it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MissionStatus {
    /// Stable for the mission's life.
    pub id: u64,
    /// What it is for.
    pub kind: MissionKind,
    /// Where it stands.
    pub phase: Phase,
    /// Tick the phase began.
    pub since: u64,
    /// Ticks the phase should end within, if it has a deadline. A phase much
    /// older than this is a mission that failed to recover; a quiet guard
    /// has none.
    pub timeout: Option<u64>,
    /// Units it holds.
    pub units: u32,
    /// Where its units were last sent.
    pub goal: TilePos,
}

impl MissionKind {
    /// A short name for reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Settle { .. } => "settle",
            Self::Defend { .. } => "defend",
            Self::Attack { .. } => "attack",
            Self::Scout { .. } => "scout",
            Self::Lift { .. } => "lift",
            Self::Strike { .. } => "strike",
            Self::Raid { .. } => "raid",
            Self::Clear { .. } => "clear",
        }
    }
}

impl Phase {
    /// A short name for reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Guard => "guard",
            Self::Found => "found",
            Self::Gather => "gather",
            Self::Travel => "travel",
            Self::Engage => "engage",
            Self::Withdraw => "withdraw",
            Self::Recover => "recover",
            Self::Load => "load",
            Self::Fly => "fly",
            Self::Fight => "fight",
        }
    }
}

impl Task {
    fn kind(self) -> MissionKind {
        match self {
            Self::Settle { target, .. } => MissionKind::Settle {
                building: target.kind(),
                anchor: target.anchor(),
            },
            Self::Defend { asset, .. } => MissionKind::Defend { asset },
            Self::Attack { target, .. } => MissionKind::Attack {
                owner: target.owner,
                building: target.building,
                anchor: target.anchor,
            },
            Self::Scout { point } => MissionKind::Scout { point },
            Self::Lift { target, .. } => MissionKind::Lift {
                owner: target.owner,
                building: target.building,
                anchor: target.anchor,
            },
            Self::Strike { target, .. } => MissionKind::Strike {
                owner: target.owner,
                building: target.building,
                anchor: target.anchor,
            },
            Self::Raid { target, .. } => MissionKind::Raid {
                owner: target.owner,
                building: target.building,
                anchor: target.anchor,
            },
            Self::Clear { aim, .. } => MissionKind::Clear { aim },
        }
    }

    fn phase(self) -> Phase {
        match self {
            Self::Defend { phase, .. } => match phase {
                DefendPhase::Guard => Phase::Guard,
                DefendPhase::Engage { .. } => Phase::Engage,
                DefendPhase::Recover => Phase::Recover,
            },
            Self::Attack { phase, .. } => match phase {
                AttackPhase::Gather => Phase::Gather,
                AttackPhase::Travel => Phase::Travel,
                AttackPhase::Engage { .. } => Phase::Engage,
                AttackPhase::Withdraw => Phase::Withdraw,
                AttackPhase::Recover => Phase::Recover,
            },
            Self::Scout { .. } => Phase::Travel,
            Self::Settle { phase, .. } => phase.phase(),
            Self::Lift { phase, .. } => match phase {
                LiftPhase::Load => Phase::Load,
                LiftPhase::Fly => Phase::Fly,
                LiftPhase::Fight { .. } => Phase::Fight,
            },
            Self::Strike { phase, .. } | Self::Clear { phase, .. } => match phase {
                StrikePhase::Gather => Phase::Gather,
                StrikePhase::Travel => Phase::Travel,
                StrikePhase::Engage { .. } => Phase::Engage,
                StrikePhase::Withdraw => Phase::Withdraw,
            },
            Self::Raid { phase, .. } => match phase {
                RaidPhase::Travel => Phase::Travel,
                RaidPhase::Strike => Phase::Engage,
                RaidPhase::Withdraw => Phase::Withdraw,
            },
        }
    }

    /// Ticks the current phase should end within.
    fn timeout(self) -> Option<u64> {
        Some(match self {
            Self::Settle { phase, .. } => return phase.timeout(),
            Self::Defend { phase, .. } => return defense::timeout(phase),
            Self::Attack { phase, .. } => attack::timeout(phase),
            Self::Scout { .. } => scouting::TRAVEL_TICKS,
            Self::Lift { phase, .. } => lift::timeout(phase),
            Self::Strike { phase, .. } => strike::timeout(phase),
            Self::Raid { phase, .. } => raid::timeout(phase),
            Self::Clear { phase, .. } => clear::timeout(phase),
        })
    }

    /// The enemy an engaged mission focuses, if it is engaged.
    fn focus(&mut self) -> Option<&mut Option<UnitId>> {
        match self {
            Self::Defend {
                phase: DefendPhase::Engage { focus },
                ..
            }
            | Self::Attack {
                phase: AttackPhase::Engage { focus },
                ..
            }
            | Self::Lift {
                phase: LiftPhase::Fight { focus },
                ..
            }
            | Self::Strike {
                phase: StrikePhase::Engage { focus },
                ..
            }
            | Self::Clear {
                phase: StrikePhase::Engage { focus },
                ..
            } => Some(focus),
            _ => None,
        }
    }
}

impl Mission {
    /// Whether the mission keeps its units from a defense. A defense keeps
    /// them while it fights for one of the seat's own buildings. An attack
    /// keeps them once it is fighting, or while travelling in contact, since
    /// it may have met the enemy since the last decision. A scout keeps its
    /// scout, a lift its units once it has left the ground, a settlement
    /// its units while they fly, and a strike, clear or raid its units once
    /// they have set out and until they turn back.
    fn holds(&self, observation: &ObservationData) -> bool {
        match self.task {
            // An ally's defense lends its units back to the seat's own.
            Task::Defend { asset, phase } => {
                matches!(phase, DefendPhase::Engage { .. })
                    && observation
                        .my_buildings
                        .iter()
                        .any(|building| building.id == asset)
            }
            Task::Attack { phase, .. } => match phase {
                AttackPhase::Engage { .. } => true,
                AttackPhase::Travel => {
                    let members: Vec<&UnitObs> = self
                        .units
                        .iter()
                        .filter_map(|id| mine(observation, *id))
                        .collect();
                    attack::contact(observation, &members)
                }
                AttackPhase::Gather | AttackPhase::Withdraw | AttackPhase::Recover => false,
            },
            Task::Scout { .. } => true,
            Task::Lift { phase, .. } => phase != LiftPhase::Load,
            Task::Settle { phase, .. } => phase == settlement::SettlePhase::Fly,
            Task::Strike { phase, .. } | Task::Clear { phase, .. } => {
                matches!(phase, StrikePhase::Travel | StrikePhase::Engage { .. })
            }
            Task::Raid { phase, .. } => phase != RaidPhase::Withdraw,
        }
    }

    /// Remembers losing every member at `now`: a target an attack, lift or
    /// strike was committed to is given up for a while, a raid's target is
    /// left to other raids for a while, and a scout's point counts as seen, so
    /// the next scout is not sent the same way at once.
    fn record_loss(&self, memory: &mut Memory, now: u64) {
        match self.task {
            Task::Attack {
                target,
                phase: AttackPhase::Travel | AttackPhase::Engage { .. },
            }
            | Task::Lift {
                target,
                phase: LiftPhase::Fly | LiftPhase::Fight { .. },
            }
            | Task::Strike {
                target,
                phase: StrikePhase::Travel | StrikePhase::Engage { .. },
            } => memory.abandon(target.building, target.anchor, now),
            Task::Raid {
                target,
                phase: RaidPhase::Travel | RaidPhase::Strike,
            } => memory.raid(target.building, target.anchor, now),
            Task::Scout { point } => memory.saw(usize::from(point), now),
            Task::Settle { target, .. } => memory.fail(target.kind(), target.anchor(), now),
            _ => {}
        }
    }
}

impl Missions {
    /// Every mission, by id.
    pub(crate) fn statuses(&self) -> Vec<MissionStatus> {
        self.list
            .iter()
            .map(|mission| MissionStatus {
                id: mission.id,
                kind: mission.task.kind(),
                phase: mission.task.phase(),
                since: mission.since,
                timeout: mission.task.timeout(),
                units: u32::try_from(mission.units.len()).expect("mission sizes fit in u32"),
                goal: mission.goal,
            })
            .collect()
    }

    /// Drops members that are gone, and missions left without members or
    /// without the own or allied Foundry they defend, remembering what the
    /// lost ones were after. Units aboard a carrier are alive.
    pub(crate) fn prune(&mut self, observation: &ObservationData, memory: &mut Memory) {
        let mut carried: Vec<UnitId> = observation
            .my_carried_units
            .iter()
            .map(|unit| unit.id)
            .collect();
        carried.sort_unstable();
        let alive = |id: &UnitId| {
            observation
                .my_units
                .binary_search_by_key(id, |unit| unit.id)
                .is_ok()
                || carried.binary_search(id).is_ok()
        };
        for mission in &mut self.list {
            mission.units.retain(alive);
        }
        for mission in self.list.iter().filter(|mission| mission.units.is_empty()) {
            mission.record_loss(memory, observation.tick);
        }
        self.list.retain(|mission| {
            !mission.units.is_empty()
                && match mission.task {
                    Task::Defend { asset, .. } => observation
                        .my_buildings
                        .iter()
                        .chain(&observation.ally_buildings)
                        .any(|building| building.id == asset),
                    Task::Attack { .. }
                    | Task::Scout { .. }
                    | Task::Lift { .. }
                    | Task::Settle { .. }
                    | Task::Strike { .. }
                    | Task::Raid { .. }
                    | Task::Clear { .. } => true,
                }
        });
    }

    /// Units a mission other than a raid holds, by id.
    pub(crate) fn held_outside_raids(&self) -> Vec<UnitId> {
        let mut held: Vec<UnitId> = self
            .list
            .iter()
            .filter(|mission| !matches!(mission.task, Task::Raid { .. }))
            .flat_map(|mission| mission.units.iter().copied())
            .collect();
        held.sort_unstable();
        held
    }

    /// Units no mission holds, by id. A defense may also `borrow` units that
    /// a recovering defense or an attack out of contact would lend it.
    fn available(&self, observation: &ObservationData, borrow: bool) -> Vec<UnitId> {
        let mut owned: Vec<UnitId> = self
            .list
            .iter()
            .filter(|mission| !borrow || mission.holds(observation))
            .flat_map(|mission| mission.units.iter().copied())
            .collect();
        owned.sort_unstable();
        observation
            .my_units
            .iter()
            .map(|unit| unit.id)
            .filter(|id| owned.binary_search(id).is_err())
            .collect()
    }

    /// Units no mission holds that may take orders: a unit sitting out after
    /// its order found no route joins no mission, since it would be sent
    /// nowhere, and a wounded unit at or bound for a Repair Bay stays there.
    fn free(&self, observation: &ObservationData, ledger: &Ledger) -> Vec<UnitId> {
        let mut free = self.available(observation, false);
        free.retain(|unit| !ledger.stuck(*unit) && !ledger.docked(*unit));
        free
    }

    /// Own units workers may weld: those no mission holds, and the members
    /// of attacks regrouping, in id order.
    pub(crate) fn patients(&self, observation: &ObservationData, ledger: &Ledger) -> Vec<UnitId> {
        let mut patients = self.free(observation, ledger);
        for mission in &self.list {
            if let Task::Attack {
                phase: AttackPhase::Gather | AttackPhase::Recover,
                ..
            } = mission.task
            {
                patients.extend(&mission.units);
            }
        }
        patients.sort_unstable();
        patients.dedup();
        patients
    }

    /// Takes `units` out of every mission, dropping missions left empty.
    fn release(&mut self, units: &[UnitId]) {
        for mission in &mut self.list {
            mission.units.retain(|id| !units.contains(id));
        }
        self.list.retain(|mission| !mission.units.is_empty());
    }

    /// Takes `units` out of every mission but the one at `keep`.
    fn release_from_others(&mut self, keep: usize, units: &[UnitId]) {
        let id = self.list[keep].id;
        for mission in self.list.iter_mut().filter(|mission| mission.id != id) {
            mission.units.retain(|unit| !units.contains(unit));
        }
        self.list.retain(|mission| !mission.units.is_empty());
    }

    /// Where the mission `id` stands in the list, while it runs.
    fn index_of(&self, id: u64) -> Option<usize> {
        self.list.iter().position(|mission| mission.id == id)
    }

    /// The ids of the missions whose task is of `kind`, oldest first.
    fn ids(&self, kind: fn(&Task) -> bool) -> Vec<u64> {
        self.list
            .iter()
            .filter(|mission| kind(&mission.task))
            .map(|mission| mission.id)
            .collect()
    }

    /// The targets missions of `kind` other than `except` go after, and
    /// `also`: what another mission of that kind may not take.
    fn held(
        &self,
        kind: fn(&Task) -> bool,
        except: Option<u64>,
        also: &[Objective],
    ) -> Vec<Objective> {
        self.list
            .iter()
            .filter(|mission| kind(&mission.task) && Some(mission.id) != except)
            .filter_map(|mission| mission.task.target())
            .chain(also.iter().copied())
            .collect()
    }

    /// Rejects restored missions that could not have been recorded by `now`
    /// on a map of the given size with `points` scouting points.
    pub(crate) fn validate(
        &self,
        now: u64,
        width: i32,
        height: i32,
        points: usize,
    ) -> Result<(), String> {
        if self.list.len() > MISSION_CAP {
            return Err("checkpoint holds too many missions".into());
        }
        let ordered = self.list.windows(2).all(|pair| pair[0].id < pair[1].id);
        // Missions form a few at a time, so a counter far ahead of the tick
        // was not recorded by this seat and could overflow.
        let exhausted =
            self.next == u64::MAX || self.next > (now + 1).saturating_mul(MISSION_CAP as u64);
        if !ordered || exhausted || self.list.last().is_some_and(|last| last.id >= self.next) {
            return Err("checkpoint mission ids are out of order".into());
        }
        if self.waiting.is_some_and(|since| since > now) {
            return Err("checkpoint mission could not have been recorded".into());
        }
        let on_map = |tile: TilePos| (0..width).contains(&tile.x) && (0..height).contains(&tile.y);
        for mission in &self.list {
            let sorted = mission.units.windows(2).all(|pair| pair[0] < pair[1]);
            if !sorted || mission.units.is_empty() || mission.units.len() > UNIT_CAP {
                return Err("checkpoint mission units are malformed".into());
            }
            let target_on_map = match mission.task {
                Task::Settle { target, .. } => on_map(target.anchor()),
                Task::Scout { point } => usize::from(point) < points,
                Task::Clear { aim, .. } => on_map(aim),
                ref task => task.target().is_none_or(|target| on_map(target.anchor)),
            };
            if mission.since > now || !on_map(mission.goal) || !target_on_map {
                return Err("checkpoint mission could not have been recorded".into());
            }
        }
        let mut owned: Vec<UnitId> = self
            .list
            .iter()
            .flat_map(|mission| mission.units.iter().copied())
            .collect();
        owned.sort_unstable();
        if owned.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err("checkpoint missions share a unit".into());
        }
        Ok(())
    }
}

/// Whether `unit` has a weapon for `domain`.
fn hits(unit: &UnitObs, domain: Domain) -> bool {
    unit.kind
        .stats()
        .weapons
        .iter()
        .any(|weapon| weapon.targets.covers(domain))
}

/// Whether `unit` is ground artillery: a walking gun that shells ground over
/// terrain.
fn walking_gun(unit: &UnitObs) -> bool {
    let stats = unit.kind.stats();
    stats.domain == Domain::Ground
        && stats
            .weapons
            .iter()
            .any(|weapon| weapon.indirect && weapon.targets.ground)
}

/// Whether `unit` is a ground unit on ground other than `home`, which it
/// cannot leave without carriers.
pub(crate) fn stranded(unit: &crate::memory::SeenUnit, map: &MapModel, home: Option<u32>) -> bool {
    let ground = map.component(unit.tile);
    unit.kind.stats().domain == Domain::Ground && ground.is_some() && ground != home
}

/// A remembered enemy unit's price, discounted by how sure the seat is it is
/// still there, except that a stranded one counts in full until forgotten.
fn remembered(unit: &crate::memory::SeenUnit, map: &MapModel, home: Option<u32>, now: u64) -> u64 {
    if stranded(unit, map, home) {
        u64::from(unit.kind.stats().cost)
    } else {
        unit.value(now)
    }
}

/// A unit's price, discounted by its missing health, as strength against
/// machines: a breacher brings only the share of it its weapon deals a
/// machine of what it deals a structure.
fn value(unit: &UnitObs) -> u64 {
    worth(unit.kind, unit.hp) * against_machines(unit.kind) / 1_000
}

/// A unit's price, discounted by its missing health.
fn worth(kind: UnitKind, hp: u32) -> u64 {
    let stats = kind.stats();
    u64::from(stats.cost) * u64::from(hp) / u64::from(stats.max_hp.max(1))
}

/// Per mille of its price a unit brings against machines: what its primary
/// weapon deals a machine of what it deals a structure, and all of it for a
/// weapon that deals both alike.
fn against_machines(kind: UnitKind) -> u64 {
    kind.stats()
        .weapons
        .first()
        .and_then(|weapon| {
            weapon.building_damage.map(|structure| {
                u64::from(weapon.damage) * 1_000 / u64::from(structure.max(weapon.damage).max(1))
            })
        })
        .unwrap_or(1_000)
}

/// Whether `kind` is a breacher: its weapon deals structures more than
/// machines, so it goes after buildings and counts against them.
pub(crate) fn breacher(kind: UnitKind) -> bool {
    against_machines(kind) < 1_000
}

/// A breacher's price, discounted by its missing health, as strength against
/// buildings; nothing for any other unit.
fn against_buildings(unit: &UnitObs) -> u64 {
    if breacher(unit.kind) {
        worth(unit.kind, unit.hp)
    } else {
        0
    }
}

/// A unit of `kind` at `hp`, valued by cost and health like [`value`],
/// against ground and against air: nothing against a domain it cannot hit.
fn force(kind: UnitKind, hp: u32) -> [u64; 2] {
    let stats = kind.stats();
    let worth = worth(kind, hp) * against_machines(kind) / 1_000;
    [Domain::Ground, Domain::Air].map(|domain| {
        if stats
            .weapons
            .iter()
            .any(|weapon| weapon.targets.covers(domain))
        {
            worth
        } else {
            0
        }
    })
}

/// The summed [`force`] of the seat's units among `ids`, aboard carriers or
/// not, each counted once.
fn strength(observation: &ObservationData, ids: impl IntoIterator<Item = UnitId>) -> [u64; 2] {
    let mut ids: Vec<_> = ids.into_iter().collect();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .filter_map(|id| {
            mine(observation, id)
                .map(|u| force(u.kind, u.hp))
                .or_else(|| {
                    observation
                        .my_carried_units
                        .iter()
                        .find(|u| u.id == id)
                        .map(|u| force(u.kind, u.hp))
                })
        })
        .fold([0; 2], |sum, value| [sum[0] + value[0], sum[1] + value[1]])
}

fn mine(observation: &ObservationData, id: UnitId) -> Option<&UnitObs> {
    observation
        .my_units
        .binary_search_by_key(&id, |unit| unit.id)
        .ok()
        .map(|index| &observation.my_units[index])
}

fn insert(ids: &mut Vec<UnitId>, id: UnitId) {
    if let Err(index) = ids.binary_search(&id) {
        ids.insert(index, id);
    }
}

/// The tile beside the footprint of `building` at `anchor` nearest the
/// seat's start by ground, or `None` if no ground route reaches it.
fn approach(
    map: &MapModel,
    me: PlayerId,
    frame: HomeFrame,
    building: BuildingKind,
    anchor: TilePos,
) -> Option<TilePos> {
    ring(anchor, building.size())
        .filter(|tile| map.distance(me, *tile) != UNREACHABLE)
        .min_by_key(|tile| {
            (
                map.distance(me, *tile),
                frame.rank(frame.home, doubled(*tile)),
            )
        })
}

/// What one decision works out once for every mission that asks: known fire,
/// the targets there are, whether ground reaches any of them, what offense
/// leaves at home, and the army at home a lift could take. It lives only as
/// long as that decision.
pub(crate) struct Scratch {
    /// The seat's frame.
    pub(crate) frame: HomeFrame,
    /// Known fire against aircraft.
    pub(crate) air: Vec<Hazard>,
    /// Known fire against ground units.
    pub(crate) ground: Vec<Hazard>,
    /// Known enemy buildings, and hostile starts not seen cleared.
    objectives: Vec<Objective>,
    /// Whether the seat knows of targets and ground reaches none of them.
    pub(crate) severed: bool,
    /// The enemy attacks and strikes go after first, when there are several;
    /// set once defense has taken its units.
    pub(crate) rival: Option<PlayerId>,
    /// Value against ground and against aircraft that offense leaves home,
    /// before the units out defending count.
    pub(crate) reserve: [u64; 2],
    /// Value against ground of the units at home a lift could take without
    /// cutting into the reserve.
    pub(crate) payload: u64,
    /// Value of the known armed enemy ground units on ground connected to the
    /// seat's start.
    pub(crate) invaders: u64,
}

impl Scratch {
    /// Anchors of known enemy Foundries and of hostile starts not seen cleared.
    pub(crate) fn enemy_foundries(&self) -> impl Iterator<Item = TilePos> + '_ {
        self.objectives
            .iter()
            .filter(|objective| objective.building == BuildingKind::Foundry)
            .map(|objective| objective.anchor)
    }

    /// Works these out for `observation`, after memory has seen it.
    pub(crate) fn new(
        observation: &ObservationData,
        map: &MapModel,
        frame: HomeFrame,
        memory: &Memory,
        stance: BotStance,
        missions: &Missions,
    ) -> Self {
        let reserve = reserve::reserve(observation, map, memory, stance);
        let objectives = objectives(observation, map);
        let severed = !objectives.is_empty()
            && objectives.iter().all(|objective| {
                approach(
                    map,
                    observation.me,
                    frame,
                    objective.building,
                    objective.anchor,
                )
                .is_none()
            });
        Self {
            frame,
            air: hazards(observation, map, memory, Domain::Air),
            ground: hazards(observation, map, memory, Domain::Ground),
            objectives,
            severed,
            rival: None,
            reserve,
            payload: missions.liftable(observation, map, reserve, payload(observation, map)),
            invaders: reserve::threat(
                map,
                memory,
                observation.me,
                observation.tick,
                Domain::Ground,
            ),
        }
    }
}

/// Known enemy buildings, and hostile starts not seen cleared.
fn objectives(observation: &ObservationData, map: &MapModel) -> Vec<Objective> {
    observation
        .enemy_buildings
        .iter()
        .map(|building| Objective {
            owner: building.player,
            building: building.kind,
            anchor: building.anchor,
        })
        .chain(map.hostiles(observation.me).filter_map(|owner| {
            let start = Objective {
                owner,
                building: BuildingKind::Foundry,
                anchor: map.start(owner)?,
            };
            standing(observation, start).then_some(start)
        }))
        .collect()
}

/// Whether `target` may still stand: it is known, or its ground is out of
/// sight.
fn standing(observation: &ObservationData, target: Objective) -> bool {
    let known = observation.enemy_buildings.iter().any(|building| {
        (building.player, building.kind, building.anchor)
            == (target.owner, target.building, target.anchor)
    });
    let (width, height) = target.building.size();
    let seen = (0..height)
        .any(|dy| (0..width).any(|dx| observation.visible(target.anchor.offset(dx, dy))));
    known || !seen
}

fn run(units: Vec<UnitId>, goal: TilePos) -> Command {
    Command::Run {
        units,
        goal,
        queue: false,
    }
}

fn hunt(units: Vec<UnitId>, goal: TilePos) -> Command {
    Command::Hunt {
        units,
        goal,
        queue: false,
    }
}
