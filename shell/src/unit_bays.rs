//! The drone bays of the selected own machines: what the bay cards and
//! their hotkeys order, cancel and toggle.

use crate::action::{Action, BindingMap};
use crate::game::Scene;
use crate::panel::{Card, CardAction, CardIcon, VerbIcon, tick_time_label};
use aftermine_sim::stats::BayStats;
use aftermine_sim::{Command, Unit, UnitId};

/// One selected machine's bay as the viewer sees it.
#[derive(Debug, Clone, Copy)]
struct Host {
    id: UnitId,
    stats: BayStats,
    docked: u8,
    ordered: u8,
    flying: u8,
    autofill: bool,
}

impl Host {
    /// Drones the bay may still order.
    fn room(&self) -> u8 {
        self.stats
            .capacity
            .saturating_sub(self.docked + self.ordered + self.flying)
    }
}

/// The selected own machines with a drone bay, in id order.
#[derive(Debug, Clone)]
pub struct SelectedBays {
    hosts: Vec<Host>,
    scrap: u32,
}

/// `look` at the units, the viewer's bank and whether they may command,
/// with the commands staged for the next tick already applied.
pub(crate) fn with_pending<R>(game: &Scene<'_>, look: impl FnOnce(&[Unit], u32, bool) -> R) -> R {
    let human = game.presentation.human;
    if game.pending.is_empty() {
        let state = game.state;
        look(
            state.units(),
            state.player(human).scrap,
            state.accepts_commands(human),
        )
    } else {
        game.state.inspect_command_phase(game.pending, |state| {
            look(
                state.units(),
                state.scrap(human).unwrap_or(0),
                state.accepts_commands(human),
            )
        })
    }
}

fn flying(units: &[Unit], host: UnitId) -> u8 {
    let count = units.iter().filter(|d| d.host == Some(host)).count();
    u8::try_from(count).unwrap_or(u8::MAX)
}

impl SelectedBays {
    pub fn inspect(game: &Scene<'_>) -> Self {
        let human = game.presentation.human;
        let selection = &game.presentation.selection.units;
        with_pending(game, |units, scrap, accepts| Self {
            hosts: units
                .iter()
                .filter(|u| accepts && u.player == human && selection.contains(&u.id))
                .filter_map(|u| {
                    let (stats, bay) = (u.kind.stats().bay?, u.bay?);
                    Some(Host {
                        id: u.id,
                        stats,
                        docked: bay.docked,
                        ordered: bay.ordered,
                        flying: flying(units, u.id),
                        autofill: bay.autofill,
                    })
                })
                .collect(),
            scrap,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.hosts.is_empty()
    }

    /// The selected bay an order fills, the one with the most room, or why
    /// the order cannot go.
    pub fn order(&self) -> Result<UnitId, String> {
        let host = self
            .hosts
            .iter()
            .filter(|host| host.room() > 0)
            .min_by_key(|host| (std::cmp::Reverse(host.room()), host.id))
            .ok_or_else(|| "drone bay is full".to_string())?;
        if self.scrap < host.stats.cost {
            return Err(format!("needs {} scrap", host.stats.cost));
        }
        Ok(host.id)
    }

    /// The cancel refunding the latest paid drone of the selected bay with
    /// the most on order.
    pub fn cancel(&self) -> Option<Command> {
        self.hosts
            .iter()
            .filter(|host| host.ordered > 0)
            .max_by_key(|host| (host.ordered, std::cmp::Reverse(host.id)))
            .map(|host| Command::CancelDrone {
                unit: host.id,
                index: host.ordered - 1,
            })
    }

    /// The refill command the toggle issues: on unless every selected bay
    /// already refills.
    pub fn autofill_toggle(&self) -> Option<Command> {
        if self.hosts.is_empty() {
            return None;
        }
        let all_on = self.hosts.iter().all(|host| host.autofill);
        Some(Command::SetDroneAutofill {
            units: self.hosts.iter().map(|host| host.id).collect(),
            enabled: !all_on,
        })
    }

    /// The bay's Order, Cancel and Autofill cards.
    pub fn cards(&self, bindings: &BindingMap, cards: &mut Vec<Card>) {
        let Some(first) = self.hosts.first() else {
            return;
        };
        let order = self.order();
        cards.push(Card {
            icon: CardIcon::Unit(first.stats.drone),
            title: "Order drone".into(),
            cost: Some(first.stats.cost),
            hotkey: bindings.labels(Action::OrderRound),
            action: CardAction::Dispatch(Action::OrderRound),
            enabled: order.is_ok(),
            why: order.err(),
            desc: vec![format!(
                "Built one at a time in {}.",
                tick_time_label(first.stats.build_ticks)
            )],
            progress: None,
            autocast: None,
            hold: None,
        });
        let cancel = self.cancel();
        cards.push(Card {
            icon: CardIcon::Verb(VerbIcon::Cancel),
            title: "Cancel".into(),
            cost: None,
            hotkey: bindings.labels(Action::CancelRound),
            action: CardAction::Dispatch(Action::CancelRound),
            enabled: cancel.is_some(),
            why: cancel.is_none().then(|| "no drone on order".into()),
            desc: vec![
                "Refunds the latest paid drone in full.".into(),
                "Also turns autofill off, so it is not bought again.".into(),
            ],
            progress: None,
            autocast: None,
            hold: None,
        });
        let on = self.hosts.iter().all(|host| host.autofill);
        cards.push(Card {
            icon: CardIcon::Verb(VerbIcon::Patrol),
            title: format!("Autofill: {}", if on { "on" } else { "off" }),
            cost: None,
            hotkey: bindings.labels(Action::ToggleAutofill),
            action: CardAction::Dispatch(Action::ToggleAutofill),
            enabled: true,
            why: None,
            desc: vec!["While on, orders the next drone whenever there is room and scrap.".into()],
            progress: None,
            autocast: None,
            hold: None,
        });
    }
}

/// The bay readout for a selected machine: docked, flying and building
/// out of what it holds, counting orders staged for the next tick.
pub fn readout(game: &Scene<'_>, host: UnitId) -> Option<String> {
    with_pending(game, |units, _, _| {
        let unit = units.iter().find(|u| u.id == host)?;
        let (stats, bay) = (unit.kind.stats().bay?, unit.bay?);
        Some(format!(
            "{} docked, {} out, {} building of {}",
            bay.docked,
            flying(units, host),
            bay.ordered,
            stats.capacity
        ))
    })
}
