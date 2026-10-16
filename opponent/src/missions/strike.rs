//! The strike mission: free ground-attack aircraft hit a known or presumed
//! enemy building whose known anti-air they outweigh, gathering beside home,
//! flying around known anti-air, and withdrawing when it outweighs them.
//! Ground need not reach the target.

use super::Scratch;
use super::air::{self, Hazard, Legs};
use super::attack::{FIT, healthy, margin, minimum, outweighed, recruit, striking};
use super::sortie::Sortie;
use super::{
    MISSION_CAP, Mission, Missions, Objective, StrikePhase, Task, UNIT_CAP, distant_worth, mine,
    run, standing,
};
use crate::composition::{self, Role};
use crate::decision::Ledger;
use crate::frame::{HomeFrame, centre_distance, doubled, footprint_centre, ring};
use crate::map::MapModel;
use crate::memory::Memory;
use crate::profile::ResolvedProfile;
use chassis::grid::TilePos;
use oxide_sim::observation::{ObservationData, UnitObs};
use oxide_sim::{BuildingKind, UnitId, UnitKind};

const GATHER_TICKS: u64 = 600;
const TRAVEL_TICKS: u64 = 1_800;
const ENGAGE_TICKS: u64 = 2_400;
const WITHDRAW_TICKS: u64 = 1_200;

/// Ticks a strike's `phase` should end within.
pub(super) fn timeout(phase: StrikePhase) -> u64 {
    match phase {
        StrikePhase::Gather => GATHER_TICKS,
        StrikePhase::Travel => TRAVEL_TICKS,
        StrikePhase::Engage { .. } => ENGAGE_TICKS,
        StrikePhase::Withdraw => WITHDRAW_TICKS,
    }
}

/// What one decision knows about striking.
pub(super) struct Raid<'a> {
    pub(super) observation: &'a ObservationData,
    map: &'a MapModel,
    frame: HomeFrame,
    memory: &'a Memory,
    minimum: u64,
    margin: u64,
    /// Per mille of its strength the enemies around it must reach before it
    /// withdraws.
    pub(super) retreat: u64,
    /// Known fire against aircraft.
    pub(super) air: &'a [Hazard],
    objectives: &'a [Objective],
    /// What offense leaves at home.
    reserve: [u64; 2],
    /// The enemy to go after first, when there are several.
    rival: Option<oxide_sim::PlayerId>,
    /// Targets another strike holds or this decision gave up on.
    held: Vec<Objective>,
    /// Ground reaches no enemy.
    severed: bool,
}

impl Missions {
    /// Advances every strike under way, then sends more free ground-attack
    /// aircraft beyond the home reserve at a target no strike holds whose
    /// known anti-air they outweigh. Returns the targets given up on.
    pub(crate) fn strike(
        &mut self,
        observation: &ObservationData,
        map: &MapModel,
        profile: &ResolvedProfile,
        memory: &Memory,
        scratch: &Scratch,
        ledger: &mut Ledger,
    ) -> Vec<(BuildingKind, TilePos)> {
        let frame = scratch.frame;
        let striking_kind: fn(&Task) -> bool = |task| matches!(task, Task::Strike { .. });
        let mut raid = Raid {
            observation,
            map,
            frame,
            memory,
            minimum: minimum(profile.stance),
            margin: margin(profile.difficulty),
            retreat: outweighed(profile),
            air: &scratch.air,
            objectives: &scratch.objectives,
            reserve: scratch.reserve,
            rival: scratch.rival,
            held: Vec::new(),
            severed: scratch.severed,
        };
        let mut given_up: Vec<Objective> = Vec::new();
        for id in self.ids(striking_kind) {
            let Some(index) = self.index_of(id) else {
                continue;
            };
            raid.held = self.held(striking_kind, Some(id), &given_up);
            if let Some(target) = self.advance_sortie(index, Sortie::Strike(&raid), ledger) {
                given_up.push(target);
            }
        }
        loop {
            raid.held = self.held(striking_kind, None, &given_up);
            if !self.form_strike(&raid, ledger) {
                break;
            }
        }
        given_up
            .into_iter()
            .map(|target| (target.building, target.anchor))
            .collect()
    }

    /// Sends the free aircraft beyond the reserve that the best target no
    /// strike holds needs. Returns whether a strike formed.
    fn form_strike(&mut self, raid: &Raid<'_>, ledger: &mut Ledger) -> bool {
        let observation = raid.observation;
        if self.list.len() >= MISSION_CAP {
            return false;
        }
        let fit: Vec<&UnitObs> = self
            .free(observation, ledger)
            .into_iter()
            .filter_map(|id| mine(observation, id))
            .filter(|unit| bomber(unit.kind) && healthy(unit, FIT))
            // A seat that can only lift gathers its splash bombers to clear
            // the way for its carriers.
            .filter(|unit| !(raid.severed && super::clear::splash(unit.kind)))
            .collect();
        let fit = self
            .spare(observation, raid.map, raid.reserve)
            .outermost(raid.map, raid.frame, fit);
        let strength: u64 = fit.iter().map(|unit| striking(unit)).sum();
        if strength < raid.minimum {
            return false;
        }
        let Some(target) = raid.best(strength, None) else {
            return false;
        };
        let centre = footprint_centre(target.building, target.anchor);
        let Some(rally) = air::pad(observation, raid.map, raid.frame, centre) else {
            return false;
        };
        let recruits = recruit(raid.frame, &fit, rally, raid.need(target), UNIT_CAP);
        if recruits.is_empty() || !ledger.order(run(recruits.clone(), rally)) {
            return false;
        }
        self.list.push(Mission {
            id: self.next,
            since: observation.tick,
            units: recruits,
            goal: rally,
            task: Task::Strike {
                target,
                phase: StrikePhase::Gather,
            },
        });
        self.next += 1;
        true
    }
}

impl Raid<'_> {
    /// The most valuable target for its distance that `strength` can strike,
    /// other than `skip`: a known enemy building or hostile start not
    /// recently given up and not seen gone, the rival's first when there are
    /// several enemies.
    pub(super) fn best(&self, strength: u64, skip: Option<Objective>) -> Option<Objective> {
        let rival = |target: &Objective| self.rival.is_none_or(|rival| target.owner == rival);
        self.best_of(strength, skip, rival)
            .or_else(|| self.best_of(strength, skip, |_| true))
    }

    fn best_of(
        &self,
        strength: u64,
        skip: Option<Objective>,
        eligible: impl Fn(&Objective) -> bool,
    ) -> Option<Objective> {
        let observation = self.observation;
        let start = self.map.start(observation.me)?;
        self.objectives
            .iter()
            .copied()
            .filter(|target| eligible(target))
            .filter(|target| Some(*target) != skip && !self.held.contains(target))
            .filter(|target| {
                !self
                    .memory
                    .abandoned(target.building, target.anchor, observation.tick)
            })
            .filter(|target| self.need(*target) <= strength)
            .max_by_key(|target| {
                let distance = centre_distance(
                    footprint_centre(BuildingKind::Foundry, start),
                    footprint_centre(target.building, target.anchor),
                );
                (
                    distant_worth(target.building, distance),
                    std::cmp::Reverse(self.frame.rank(
                        self.frame.home,
                        footprint_centre(target.building, target.anchor),
                    )),
                )
            })
    }

    /// Strength a strike on `target` needs: the known anti-air that reaches
    /// over it times the margin, and never under the stance minimum.
    fn need(&self, target: Objective) -> u64 {
        (cover(self.air, target) * self.margin / 1_000).max(self.minimum)
    }

    /// Where a strike on `target` gathers and comes back to.
    pub(super) fn rally(&self, target: Objective) -> Option<TilePos> {
        let centre = footprint_centre(target.building, target.anchor);
        air::pad(self.observation, self.map, self.frame, centre)
    }

    /// Whether a gathered strike of `strength` still goes at `target`.
    pub(super) fn sets_out(&self, target: Objective, strength: u64) -> bool {
        strength >= self.need(target) && standing(self.observation, target)
    }

    /// Sends `units` from `from` at the side of `target` nearest it, around
    /// known anti-air when the straight line crosses it. Returns where they
    /// went.
    pub(super) fn send(
        &self,
        units: Vec<UnitId>,
        from: TilePos,
        target: Objective,
        ledger: &mut Ledger,
    ) -> Option<TilePos> {
        let aim = ring(target.anchor, target.building.size()).min_by_key(|tile| {
            (
                tile.chebyshev(from),
                self.frame.rank(self.frame.home, doubled(*tile)),
            )
        })?;
        air::fly_around(
            self.observation,
            self.frame,
            self.air,
            units,
            from,
            aim,
            Legs::Strike,
            ledger,
        )
        .then_some(aim)
    }
}

/// Known anti-air reaching over `target`.
fn cover(air: &[Hazard], target: Objective) -> u64 {
    air::cover(air, footprint_centre(target.building, target.anchor))
}

/// What a strike needs against the easiest target the seat knows of and has
/// not given up on: the known anti-air reaching over it by the attack margin,
/// at least the stance's minimum. Nothing while it has given up on all.
pub(crate) fn strike_need(
    observation: &ObservationData,
    memory: &Memory,
    profile: &ResolvedProfile,
    scratch: &Scratch,
) -> u64 {
    let margin = margin(profile.difficulty);
    scratch
        .objectives
        .iter()
        .filter(|target| !memory.abandoned(target.building, target.anchor, observation.tick))
        .map(|target| cover(&scratch.air, *target) * margin / 1_000)
        .min()
        .map_or(0, |need| need.max(minimum(profile.stance)))
}

/// Whether `kind` is a ground-attack aircraft a strike takes.
fn bomber(kind: UnitKind) -> bool {
    composition::role(kind) == Some(Role::AirStrike)
}
