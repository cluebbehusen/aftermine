//! The seat's own order failures, the machines every seat sees stand up
//! from public cradles, and the enemy deaths the seat sees, which the host
//! buffers between decisions.

use aftermine_sim::command::RejectReason;
use aftermine_sim::{Event, PlayerId, StallReason, State, UnitId, UnitKind};
use chassis::fx::Vec2Fx;
use chassis::grid::TilePos;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Events a buffer holds, the oldest dropped first beyond it: a computation
/// bound that normal play stays under, since the buffer keeps one stall per
/// unit and reason and one rejection per reason, a cradle stands its machine
/// up only after minutes of crew work, and even the slowest seat decides
/// every few seconds, between which few units die in its sight.
const CAP: usize = 1_024;

/// One of the seat's own order failures, another seat's machine standing up
/// from its cradle, or an enemy unit the seat saw die, as the simulation
/// reported it.
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
    /// A hostile unit died, or was taken apart, where the seat could see it.
    EnemyDied {
        /// The unit.
        unit: UnitId,
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
            _ => false,
        }
    }

    /// `player`'s view of `event`, with `state` the world after the tick
    /// that reported it: whether a death was in the seat's sight is read from
    /// its vision then, which its teammates share.
    fn of(player: PlayerId, state: &State, event: &Event) -> Option<Self> {
        let seen_die = |unit_kind: UnitKind, owner: PlayerId, pos: Vec2Fx| {
            state.hostile(player, owner)
                && !unit_kind.stats().docks()
                && state.vision(player).visible(TilePos::containing(pos))
        };
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
            Event::UnitDied {
                unit,
                kind,
                player: owner,
                pos,
                ..
            }
            | Event::UnitRetired {
                unit,
                kind,
                player: owner,
                pos,
                ..
            } if seen_die(kind, owner, pos) => Some(Self::EnemyDied { unit }),
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
    /// Appends `player`'s own stalls and rejections, other seats' machines
    /// standing up, and the hostile units it saw die, from one tick's events,
    /// in report order, with `state` the world after that tick. A repeat of a
    /// stall of the same unit for the same reason, or of a rejection for the
    /// same reason, replaces the earlier one in its place.
    pub fn record(&mut self, player: PlayerId, state: &State, events: &[Event]) {
        for event in events
            .iter()
            .filter_map(|event| OwnEvent::of(player, state, event))
        {
            if let Some(held) = self.0.iter_mut().find(|held| held.repeats(&event)) {
                *held = event;
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
