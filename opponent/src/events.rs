//! The seat's own order failures and hits, and the machines every seat sees
//! stand up from public cradles, which the host buffers between decisions.

use aftermine_sim::command::RejectReason;
use aftermine_sim::{Event, PlayerId, StallReason, UnitId, UnitKind};
use chassis::fx::Vec2Fx;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Events a buffer holds, the oldest dropped first beyond it: a computation
/// bound that normal play stays under, since the buffer keeps one stall per
/// unit and reason, one rejection per reason and one hit per place, and a
/// cradle stands its machine up only after minutes of crew work.
const CAP: usize = 1_024;

/// Places hit a buffer holds, later ones dropped: a computation bound. A
/// structure is hit at one place, its footprint's centre, so a battle among
/// moving units fills it before a structure's hits could be lost.
const HITS: usize = 256;

/// One of the seat's own order failures, or another seat's machine standing
/// up from its cradle, as the simulation reported it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum OwnEvent {
    /// A unit gave up its order.
    OrderStalled {
        /// The unit that gave up.
        unit: UnitId,
        /// Where it stood when it gave up.
        pos: Vec2Fx,
        /// Why.
        reason: StallReason,
    },
    /// A command was dropped. The event names no command, so it cannot be
    /// tied to a particular one.
    CommandRejected {
        /// Why.
        reason: RejectReason,
    },
    /// One of the seat's own units or structures was hit. The simulation
    /// names neither combatant, only where the victim stood: for a
    /// structure, its footprint's centre.
    Hit {
        /// Where.
        pos: Vec2Fx,
    },
    /// Another seat's machine stood up from its cradle. A cradle is public
    /// once its crew starts, so every seat learns where its machine stands.
    MachineAssembled {
        /// The machine.
        unit: UnitId,
        /// Its kind.
        kind: UnitKind,
        /// Its owner.
        player: PlayerId,
        /// Where it stood up.
        pos: Vec2Fx,
    },
}

impl OwnEvent {
    /// Whether `other` reports the same failure again: the same unit stalling
    /// for the same reason, or a rejection for the same reason.
    fn repeats(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::OrderStalled { unit, reason, .. },
                Self::OrderStalled {
                    unit: again,
                    reason: why,
                    ..
                },
            ) => unit == again && reason == why,
            (Self::CommandRejected { reason }, Self::CommandRejected { reason: why }) => {
                reason == why
            }
            (Self::Hit { pos }, Self::Hit { pos: again }) => pos == again,
            _ => false,
        }
    }

    fn of(player: PlayerId, event: &Event) -> Option<Self> {
        match *event {
            Event::OrderStalled {
                unit,
                player: owner,
                pos,
                reason,
            } if owner == player => Some(Self::OrderStalled { unit, pos, reason }),
            Event::CommandRejected {
                player: issuer,
                reason,
            } if issuer == player => Some(Self::CommandRejected { reason }),
            Event::DamageTaken { player: owner, pos } if owner == player => Some(Self::Hit { pos }),
            Event::UnitAssembled {
                unit,
                kind,
                player: owner,
                pos,
                ..
            } if owner != player => Some(Self::MachineAssembled {
                unit,
                kind,
                player: owner,
                pos,
            }),
            _ => None,
        }
    }
}

/// A bounded, ordered buffer of one seat's events. The host keeps it
/// beside the controller and records every tick; the seat's next decision
/// consumes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<OwnEvent>", into = "Vec<OwnEvent>")]
pub struct OwnEvents(VecDeque<OwnEvent>);

impl OwnEvents {
    /// Appends `player`'s own stalls, rejections and hits, and other seats'
    /// machines standing up, from one tick's events, in report order. A
    /// repeat of a stall of the same unit for the same reason, of a
    /// rejection for the same reason or of a hit at the same place, replaces
    /// the earlier one in its place.
    pub fn record(&mut self, player: PlayerId, events: &[Event]) {
        for event in events
            .iter()
            .filter_map(|event| OwnEvent::of(player, event))
        {
            if let Some(held) = self.0.iter_mut().find(|held| held.repeats(&event)) {
                *held = event;
                continue;
            }
            let hit = |event: &OwnEvent| matches!(event, OwnEvent::Hit { .. });
            if hit(&event) && self.0.iter().filter(|held| hit(held)).count() >= HITS {
                continue;
            }
            if self.0.len() == CAP {
                self.0.pop_front();
            }
            self.0.push_back(event);
        }
    }

    pub(crate) fn take(&mut self) -> Vec<OwnEvent> {
        std::mem::take(&mut self.0).into()
    }
}

impl TryFrom<Vec<OwnEvent>> for OwnEvents {
    type Error = String;

    fn try_from(events: Vec<OwnEvent>) -> Result<Self, Self::Error> {
        if events.len() > CAP {
            return Err(format!("own-event buffer exceeds {CAP} events"));
        }
        Ok(Self(events.into()))
    }
}

impl From<OwnEvents> for Vec<OwnEvent> {
    fn from(events: OwnEvents) -> Self {
        events.0.into()
    }
}

#[cfg(test)]
mod tests;
