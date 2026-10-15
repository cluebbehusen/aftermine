//! Drone bays: ordering, cancelling and building drones, and launching and
//! docking them. Drones build one at a time in the production phase; they
//! launch and dock after the brains, as riders board, since the unit list
//! must hold still under them.

use crate::command::RejectReason;
use crate::event::Event;
use crate::ids::{PlayerId, UnitId};
use crate::state::State;

/// The drones flying for `host`.
fn flying(state: &State, host: UnitId) -> usize {
    state.units.iter().filter(|u| u.host == Some(host)).count()
}

pub(super) fn apply_order(
    state: &mut State,
    player: PlayerId,
    unit: UnitId,
) -> Result<(), RejectReason> {
    let host = state
        .unit(unit)
        .filter(|u| u.player == player)
        .ok_or(RejectReason::NoValidUnits)?;
    let (Some(stats), Some(bay)) = (host.kind.stats().bay, host.bay) else {
        return Err(RejectReason::CannotProduce);
    };
    let held = usize::from(bay.docked) + usize::from(bay.ordered) + flying(state, unit);
    if held >= usize::from(stats.capacity) {
        return Err(RejectReason::QueueFull);
    }
    let bank = &mut state.player_mut(player).scrap;
    if *bank < stats.cost {
        return Err(RejectReason::NotEnoughScrap);
    }
    *bank -= stats.cost;
    state
        .unit_mut(unit)
        .and_then(|u| u.bay.as_mut())
        .expect("checked above")
        .ordered += 1;
    Ok(())
}

/// Refunds the ordered drone at `index` and switches the bay's refill off,
/// so it does not buy the drone straight back. Only cancelling the one
/// building loses work: the next starts fresh.
pub(super) fn apply_cancel(
    state: &mut State,
    player: PlayerId,
    unit: UnitId,
    index: u8,
) -> Result<(), RejectReason> {
    let host = state
        .unit_mut(unit)
        .filter(|u| u.player == player)
        .ok_or(RejectReason::NoValidUnits)?;
    let cost = host
        .kind
        .stats()
        .bay
        .ok_or(RejectReason::InvalidTarget)?
        .cost;
    let bay = host
        .bay
        .as_mut()
        .filter(|bay| index < bay.ordered)
        .ok_or(RejectReason::InvalidTarget)?;
    bay.ordered -= 1;
    if index == 0 {
        bay.progress = 0;
    }
    bay.autofill = false;
    let bank = &mut state.player_mut(player).scrap;
    *bank = bank.saturating_add(cost);
    Ok(())
}

/// `units` is the canonical set; every one must be the issuer's machine
/// with a bay or nothing changes.
pub(super) fn apply_set_autofill(
    state: &mut State,
    player: PlayerId,
    units: &[UnitId],
    enabled: bool,
) -> Result<(), RejectReason> {
    if units.is_empty()
        || units
            .iter()
            .any(|id| state.unit(*id).is_none_or(|u| u.player != player))
    {
        return Err(RejectReason::NoValidUnits);
    }
    if units
        .iter()
        .any(|id| state.unit(*id).is_some_and(|u| u.bay.is_none()))
    {
        return Err(RejectReason::InvalidTarget);
    }
    for id in units {
        state
            .unit_mut(*id)
            .and_then(|u| u.bay.as_mut())
            .expect("validated")
            .autofill = enabled;
    }
    Ok(())
}

/// Production-phase work on every drone bay, in unit-id order: a refilling
/// bay with room and nothing ordered buys one drone when the bank covers it
/// without dipping into a stranded seat's recovery reserve, then the drone
/// building advances one tick.
pub(super) fn assemble(state: &mut State, events: &mut Vec<Event>) {
    for index in 0..state.units.len() {
        let host = &state.units[index];
        let (Some(stats), Some(bay)) = (host.kind.stats().bay, host.bay) else {
            continue;
        };
        let (id, player) = (host.id, host.player);
        let held = usize::from(bay.docked) + flying(state, id);
        if bay.autofill
            && bay.ordered == 0
            && held < usize::from(stats.capacity)
            && state
                .player(player)
                .scrap
                .checked_sub(stats.cost)
                .is_some_and(|left| left >= state.recovery_reserve(player))
        {
            state.player_mut(player).scrap -= stats.cost;
            state.units[index]
                .bay
                .as_mut()
                .expect("checked above")
                .ordered = 1;
            events.push(Event::DroneAutofilled {
                unit: id,
                player,
                cost: stats.cost,
            });
        }
        let bay = state.units[index].bay.as_mut().expect("checked above");
        if bay.ordered == 0 {
            continue;
        }
        bay.progress += 1;
        if bay.progress < stats.build_ticks {
            continue;
        }
        bay.progress = 0;
        bay.ordered -= 1;
        bay.docked += 1;
        let docked = bay.docked;
        events.push(Event::DroneAssembled {
            unit: id,
            player,
            docked,
        });
    }
}

/// Docks the drones that reached their hosts this tick, then launches every
/// docked drone of a host on alert from its hull, each whole.
pub(super) fn resolve(state: &mut State, docks: &[UnitId], events: &mut Vec<Event>) {
    for &drone in docks {
        let Ok(index) = state.units.binary_search_by_key(&drone, |u| u.id) else {
            continue;
        };
        let (Some(host), player) = (state.units[index].host, state.units[index].player) else {
            continue;
        };
        // The brains queued the dock before this tick's hits landed: a drone
        // they killed dies in cleanup, and a host they put back on alert
        // keeps its drones out.
        let calm = state
            .unit(host)
            .is_some_and(|h| h.hp > 0 && h.bay.is_some_and(|bay| bay.alert == 0));
        if state.units[index].hp == 0 || !calm {
            continue;
        }
        state.units.remove(index);
        if let Some(bay) = state.unit_mut(host).and_then(|u| u.bay.as_mut()) {
            bay.docked += 1;
        }
        events.push(Event::DroneDocked {
            host,
            drone,
            player,
        });
    }
    let launching: Vec<UnitId> = state
        .units
        .iter()
        .filter(|u| u.hp > 0 && u.bay.is_some_and(|bay| bay.alert > 0 && bay.docked > 0))
        .map(|u| u.id)
        .collect();
    for host in launching {
        let unit = state.unit_mut(host).expect("just seen");
        let (player, pos, kind) = (
            unit.player,
            unit.pos,
            unit.kind.stats().bay.expect("a host").drone,
        );
        let bay = unit.bay.as_mut().expect("a host");
        let count = std::mem::take(&mut bay.docked);
        for _ in 0..count {
            let drone = state.spawn_unit(player, kind, pos);
            state.unit_mut(drone).expect("just spawned").host = Some(host);
            events.push(Event::DroneLaunched {
                host,
                drone,
                player,
            });
        }
    }
}

/// Zeroes every drone whose host fell this tick, so it dies with it.
pub(super) fn fall_with_hosts(state: &mut State) {
    let fallen: Vec<UnitId> = state
        .units
        .iter()
        .filter(|u| u.hp == 0 && u.bay.is_some())
        .map(|u| u.id)
        .collect();
    if fallen.is_empty() {
        return;
    }
    for unit in &mut state.units {
        if unit.host.is_some_and(|host| fallen.contains(&host)) {
            unit.hp = 0;
        }
    }
}
