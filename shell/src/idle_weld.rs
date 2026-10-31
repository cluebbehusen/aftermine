//! The selected own workers' idle welding switch: the autocast the Weld
//! card shows, and what switching it sends.

use crate::action::Action;
use crate::game::Scene;
use crate::panel::{Autocast, AutocastState};
use aftermine_sim::{Command, UnitId};

/// The selected own workers, in id order, each with whether it welds while
/// idle once the commands staged for the next tick land.
#[derive(Debug, Clone)]
pub struct SelectedWorkers {
    workers: Vec<(UnitId, bool)>,
}

impl SelectedWorkers {
    pub fn inspect(game: &Scene<'_>) -> Self {
        let human = game.presentation.human;
        let selection = &game.presentation.selection.units;
        crate::unit_bays::with_pending(game, |units, _, accepts| Self {
            workers: units
                .iter()
                .filter(|u| accepts && u.player == human && selection.contains(&u.id))
                .filter(|u| u.worker.is_some())
                .map(|u| (u.id, u.idle_weld()))
                .collect(),
        })
    }

    fn all_on(&self) -> bool {
        self.workers.iter().all(|(_, on)| *on)
    }

    /// The switch the autocast sends: on unless every selected worker
    /// already welds while idle.
    pub fn toggle(&self) -> Option<Command> {
        (!self.workers.is_empty()).then(|| Command::SetIdleWeld {
            units: self.workers.iter().map(|(id, _)| *id).collect(),
            enabled: !self.all_on(),
        })
    }

    /// The Weld card's autocast, when the selection holds a worker of the
    /// viewer's.
    pub fn autocast(&self) -> Option<Autocast> {
        let on = self.workers.iter().filter(|(_, on)| *on).count();
        let state = match on {
            0 => AutocastState::Off,
            n if n == self.workers.len() => AutocastState::On,
            _ => AutocastState::Mixed,
        };
        (!self.workers.is_empty()).then_some(Autocast {
            state,
            toggle: Action::ToggleIdleWeld,
        })
    }
}

#[cfg(test)]
mod tests;
