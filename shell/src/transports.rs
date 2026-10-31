//! The selected own transports' drop verb: whether the Unload card can act,
//! and where each loaded sling sets its cargo down.

use crate::game::Scene;
use aftermine_sim::{Command, Order, Unit, UnitId};
use chassis::grid::TilePos;

/// One selected transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sling {
    id: UnitId,
    loaded: bool,
    /// Where it hovers now.
    here: TilePos,
}

/// The selected own transports, in id order, read from the world itself:
/// the commands staged for the next tick move no unit and fill or empty no
/// sling.
#[derive(Debug, Clone)]
pub struct SelectedTransports {
    slings: Vec<Sling>,
}

impl SelectedTransports {
    pub fn inspect(game: &Scene<'_>) -> Self {
        let human = game.presentation.human;
        let accepts = game.state.accepts_commands(human);
        let selection = &game.presentation.selection.units;
        Self {
            slings: game
                .state
                .units()
                .iter()
                .filter(|u| accepts && u.player == human && selection.contains(&u.id))
                .filter(|u| u.kind.stats().transport_capacity > 0)
                .map(|u| Sling {
                    id: u.id,
                    loaded: !u.cargo.is_empty(),
                    here: u.tile(),
                })
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.slings.is_empty()
    }

    /// Whether any selected transport carries a machine.
    pub fn any_loaded(&self) -> bool {
        self.slings.iter().any(|sling| sling.loaded)
    }

    /// One drop per loaded transport: where it hovers now, or, queued, where
    /// its program leaves it once the staged commands land. An empty sling
    /// keeps its orders.
    pub fn unload(&self, game: &Scene<'_>, queue: bool) -> Vec<Command> {
        let loaded = self.slings.iter().filter(|sling| sling.loaded);
        let drop = |sling: &Sling, at, queue| Command::Unload {
            transport: sling.id,
            at,
            queue,
        };
        if !queue {
            return loaded.map(|sling| drop(sling, sling.here, false)).collect();
        }
        crate::unit_bays::with_pending(game, |units, _, _| {
            loaded
                .filter_map(|sling| {
                    let unit = units.iter().find(|u| u.id == sling.id)?;
                    // A patrol never ends, and a drop queued into one
                    // rotates back in after every lap, so a patrolling
                    // transport drops where it hovers and stops patrolling.
                    Some(if unit.looping {
                        drop(sling, sling.here, false)
                    } else {
                        drop(sling, route_end(unit), true)
                    })
                })
                .collect()
        })
    }
}

/// The tile `unit`'s program leaves it on: the last fixed point an order
/// heads for, or where it hovers when none names one.
fn route_end(unit: &Unit) -> TilePos {
    std::iter::once(&unit.order)
        .chain(&unit.queue)
        .rev()
        .find_map(fixed_point)
        .unwrap_or_else(|| unit.tile())
}

/// Where an order leaves an airframe, when that is a fixed tile. A chase or
/// an escort ends wherever its target went, and a retirement dismantles the
/// unit.
fn fixed_point(order: &Order) -> Option<TilePos> {
    match order {
        Order::Run { goal }
        | Order::Advance { goal }
        | Order::Hunt { goal }
        | Order::Unload { at: goal, .. } => Some(goal.destination()),
        Order::Attack { resume, .. } => resume
            .and_then(|resume| resume.goal())
            .map(|goal| goal.destination()),
        Order::Scout { tile } | Order::Land { goal: tile, .. } => Some(*tile),
        Order::Idle | Order::Accompany { .. } | Order::Retire { .. } => None,
        // Worker and rider orders never reach a transport's program.
        Order::Harvest { .. }
        | Order::Build { .. }
        | Order::Repair { .. }
        | Order::Salvage { .. }
        | Order::Found { .. }
        | Order::RepairUnit { .. }
        | Order::Board { .. }
        | Order::ReturnCargo { .. }
        | Order::Tend { .. } => None,
    }
}

#[cfg(test)]
mod tests;
