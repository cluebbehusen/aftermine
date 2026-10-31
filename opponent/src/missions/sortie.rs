//! The flight strikes and clearances share: aircraft gather beside home, fly
//! at their aim around known anti-air, fight there, then go on to the next
//! aim they can take or withdraw when the anti-air around them outweighs them.

use super::air;
use super::attack::striking;
use super::clear::{self, Sweep};
use super::strike::{self, Raid};
use super::{Mission, Missions, Objective, StrikePhase, Task, mine, run, standing};
use crate::decision::Ledger;
use aftermine_sim::UnitId;
use aftermine_sim::observation::{ObservationData, UnitObs};
use chassis::grid::TilePos;

/// Tiles between a member and the aim at which the fight begins.
const CONTACT_TILES: i32 = 6;

/// What a sortie goes at: a strike's building, or the tile a clearance
/// sweeps.
#[derive(Clone, Copy)]
enum Aim {
    Building(Objective),
    Tile(TilePos),
}

/// What one decision knows for a strike or for a clearance.
#[derive(Clone, Copy)]
pub(super) enum Sortie<'s, 'a> {
    Strike(&'s Raid<'a>),
    Clear(&'s Sweep<'a>),
}

impl<'a> Sortie<'_, 'a> {
    fn observation(self) -> &'a ObservationData {
        match self {
            Self::Strike(raid) => raid.observation,
            Self::Clear(sweep) => sweep.observation,
        }
    }

    /// Ticks `phase` should end within.
    fn ticks(self, phase: StrikePhase) -> u64 {
        match self {
            Self::Strike(_) => strike::timeout(phase),
            Self::Clear(_) => clear::timeout(phase),
        }
    }

    /// Whether the known anti-air reaching `members` outweighs `strength`.
    fn outweighed(self, members: &[&UnitObs], strength: u64) -> bool {
        let (hazards, retreat) = match self {
            Self::Strike(raid) => (raid.air, raid.retreat),
            Self::Clear(sweep) => (sweep.air, sweep.retreat),
        };
        air::opposition(hazards, members) * 1_000 > strength * retreat
    }

    fn rally(self, aim: Aim) -> Option<TilePos> {
        match (self, aim) {
            (Self::Strike(raid), Aim::Building(target)) => raid.rally(target),
            (Self::Clear(sweep), Aim::Tile(tile)) => sweep.rally(tile),
            _ => unreachable!("a sortie's aim"),
        }
    }

    /// Whether a gathered sortie of `strength` still goes at `aim`.
    fn sets_out(self, aim: Aim, strength: u64) -> bool {
        match (self, aim) {
            (Self::Strike(raid), Aim::Building(target)) => raid.sets_out(target, strength),
            (Self::Clear(sweep), Aim::Tile(tile)) => sweep.sets_out(tile, strength),
            _ => unreachable!("a sortie's aim"),
        }
    }

    /// Whether a strike's building still stands after its fight.
    fn stands(self, aim: Aim) -> bool {
        match aim {
            Aim::Building(target) => standing(self.observation(), target),
            Aim::Tile(_) => false,
        }
    }

    /// The next aim `strength` can take after `aim`, its fight at `goal`.
    fn next(self, strength: u64, aim: Aim, goal: TilePos) -> Option<Aim> {
        match (self, aim) {
            (Self::Strike(raid), Aim::Building(target)) => {
                raid.best(strength, Some(target)).map(Aim::Building)
            }
            (Self::Clear(sweep), Aim::Tile(_)) => sweep.best(strength, Some(goal)).map(Aim::Tile),
            _ => unreachable!("a sortie's aim"),
        }
    }

    /// Sends `units` from `from` at `aim`. Returns where they went.
    fn send(
        self,
        units: Vec<UnitId>,
        from: TilePos,
        aim: Aim,
        ledger: &mut Ledger,
    ) -> Option<TilePos> {
        match (self, aim) {
            (Self::Strike(raid), Aim::Building(target)) => raid.send(units, from, target, ledger),
            (Self::Clear(sweep), Aim::Tile(tile)) => sweep.send(units, from, tile, ledger),
            _ => unreachable!("a sortie's aim"),
        }
    }
}

impl Mission {
    fn set_sortie(&mut self, aim: Aim, phase: StrikePhase, now: u64, goal: TilePos) {
        self.task = match aim {
            Aim::Building(target) => Task::Strike { target, phase },
            Aim::Tile(aim) => Task::Clear { aim, phase },
        };
        self.since = now;
        self.goal = goal;
    }
}

impl Missions {
    /// Moves the strike or clearance at `index` through its phases. Returns a
    /// strike's target when it gave up on it.
    pub(super) fn advance_sortie(
        &mut self,
        index: usize,
        sortie: Sortie<'_, '_>,
        ledger: &mut Ledger,
    ) -> Option<Objective> {
        let now = sortie.observation().tick;
        let mission = &self.list[index];
        let (aim, phase) = match mission.task {
            Task::Strike { target, phase } => (Aim::Building(target), phase),
            Task::Clear { aim, phase } => (Aim::Tile(aim), phase),
            _ => unreachable!("a sortie's task"),
        };
        let members: Vec<&UnitObs> = mission
            .units
            .iter()
            .filter_map(|id| mine(sortie.observation(), *id))
            .collect();
        let all_idle = members.iter().all(|unit| unit.idle);
        let strength: u64 = members.iter().map(|unit| striking(unit)).sum();
        let age = now - mission.since;
        let Some(rally) = sortie.rally(aim) else {
            // A strike keeps its target; a clearance with nowhere to come
            // back to ends.
            if matches!(aim, Aim::Tile(_)) {
                self.list.remove(index);
            }
            return None;
        };
        let goal = mission.goal;
        let units = mission.units.clone();
        let lost = match aim {
            Aim::Building(target) => Some(target),
            Aim::Tile(_) => None,
        };
        // Gives a strike's target up only once the sortie has turned back, so
        // a decision out of orders does not keep refreshing the give-up.
        let withdraw = |missions: &mut Self, ledger: &mut Ledger| {
            let turned = ledger.order(run(units.clone(), rally));
            if turned {
                missions.list[index].set_sortie(aim, StrikePhase::Withdraw, now, rally);
            }
            turned
        };

        match phase {
            StrikePhase::Gather => {
                if !all_idle && age < sortie.ticks(phase) {
                    return None;
                }
                if !sortie.sets_out(aim, strength) {
                    self.list.remove(index);
                    return None;
                }
                if let Some(to) = sortie.send(units.clone(), rally, aim, ledger) {
                    self.list[index].set_sortie(aim, StrikePhase::Travel, now, to);
                }
                None
            }
            StrikePhase::Travel => {
                let arrived = members
                    .iter()
                    .any(|unit| unit.tile.chebyshev(goal) <= CONTACT_TILES);
                if arrived {
                    let engage = StrikePhase::Engage { focus: None };
                    self.list[index].set_sortie(aim, engage, now, goal);
                    return None;
                }
                if all_idle || age >= sortie.ticks(phase) || sortie.outweighed(&members, strength) {
                    return withdraw(self, ledger).then_some(lost).flatten();
                }
                None
            }
            StrikePhase::Engage { .. } => {
                if sortie.outweighed(&members, strength) {
                    return withdraw(self, ledger).then_some(lost).flatten();
                }
                if !all_idle && age < sortie.ticks(phase) {
                    return None;
                }
                if sortie.stands(aim) {
                    return withdraw(self, ledger).then_some(lost).flatten();
                }
                match sortie.next(strength, aim, goal) {
                    Some(next) => {
                        if let Some(to) = sortie.send(units.clone(), goal, next, ledger) {
                            self.list[index].set_sortie(next, StrikePhase::Travel, now, to);
                        }
                    }
                    None => {
                        withdraw(self, ledger);
                    }
                }
                None
            }
            StrikePhase::Withdraw => {
                if all_idle || age >= sortie.ticks(phase) {
                    self.list.remove(index);
                }
                None
            }
        }
    }
}
