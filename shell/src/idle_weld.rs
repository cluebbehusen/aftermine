//! The selected own workers' idle welding switch: what the Auto-weld card
//! shows and what it and its hotkey send.

use crate::action::{Action, BindingMap};
use crate::game::Scene;
use crate::panel::{Card, CardAction, CardIcon, VerbIcon};
use oxide_sim::{Command, UnitId};

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

    /// The switch the card sends: on unless every selected worker already
    /// welds while idle.
    pub fn toggle(&self) -> Option<Command> {
        (!self.workers.is_empty()).then(|| Command::SetIdleWeld {
            units: self.workers.iter().map(|(id, _)| *id).collect(),
            enabled: !self.all_on(),
        })
    }

    /// The Auto-weld card, when the selection holds a worker of the
    /// viewer's.
    pub fn card(&self, bindings: &BindingMap) -> Option<Card> {
        (!self.workers.is_empty()).then(|| Card {
            icon: CardIcon::Verb(VerbIcon::Repair),
            title: format!("Auto-weld: {}", if self.all_on() { "on" } else { "off" }),
            cost: None,
            hotkey: bindings.labels(Action::ToggleIdleWeld),
            action: CardAction::Dispatch(Action::ToggleIdleWeld),
            enabled: true,
            why: None,
            desc: vec![
                "Idle workers weld wounded machines near their post, then return.".into(),
                "Patrolling workers always weld. Welding consumes scrap.".into(),
            ],
            progress: None,
        })
    }
}

#[cfg(test)]
mod tests;
