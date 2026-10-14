//! Magazines: ordering, cancelling and assembling prepaid rounds, and
//! arming a silo's strike. Rounds assemble one at a time in the production
//! phase; the brain tail turns silo decks, launches, intercepts and lands
//! warheads.

use crate::command::RejectReason;
use crate::event::Event;
use crate::ids::{BuildingId, PlayerId};
use crate::state::State;
use chassis::grid::TilePos;

pub(super) fn apply_order_round(
    state: &mut State,
    player: PlayerId,
    building: BuildingId,
) -> Result<(), RejectReason> {
    let b = state
        .building(building)
        .filter(|b| b.player == player)
        .ok_or(RejectReason::NotYourBuilding)?;
    let (Some(stats), Some(magazine)) = (b.kind.magazine(), b.magazine) else {
        return Err(RejectReason::CannotProduce);
    };
    if !b.built() {
        return Err(RejectReason::CannotProduce);
    }
    if u16::from(magazine.loaded) + u16::from(magazine.ordered) >= u16::from(stats.capacity) {
        return Err(RejectReason::QueueFull);
    }
    let bank = &mut state.player_mut(player).scrap;
    if *bank < stats.round_cost {
        return Err(RejectReason::NotEnoughScrap);
    }
    *bank -= stats.round_cost;
    state
        .building_mut(building)
        .and_then(|b| b.magazine.as_mut())
        .expect("checked above")
        .ordered += 1;
    Ok(())
}

/// Refunds the ordered round at `index` and switches autofill off, so the
/// magazine does not buy the round straight back. Paid rounds are alike, so
/// only cancelling the head loses work: the next round starts fresh.
pub(super) fn apply_cancel_round(
    state: &mut State,
    player: PlayerId,
    building: BuildingId,
    index: u8,
) -> Result<(), RejectReason> {
    let b = state
        .building_mut(building)
        .filter(|b| b.player == player)
        .ok_or(RejectReason::NotYourBuilding)?;
    let cost = b
        .kind
        .magazine()
        .ok_or(RejectReason::InvalidTarget)?
        .round_cost;
    let magazine = b
        .magazine
        .as_mut()
        .filter(|magazine| index < magazine.ordered)
        .ok_or(RejectReason::InvalidTarget)?;
    magazine.ordered -= 1;
    if index == 0 {
        magazine.progress = 0;
    }
    magazine.autofill = false;
    let bank = &mut state.player_mut(player).scrap;
    *bank = bank.saturating_add(cost);
    Ok(())
}

/// `buildings` is the canonical set; every one must be the issuer's
/// magazine building or nothing changes.
pub(super) fn apply_set_autofill(
    state: &mut State,
    player: PlayerId,
    buildings: &[BuildingId],
    enabled: bool,
) -> Result<(), RejectReason> {
    if buildings.is_empty()
        || buildings
            .iter()
            .any(|id| state.building(*id).is_none_or(|b| b.player != player))
    {
        return Err(RejectReason::NotYourBuilding);
    }
    if buildings
        .iter()
        .any(|id| state.building(*id).is_some_and(|b| b.magazine.is_none()))
    {
        return Err(RejectReason::InvalidTarget);
    }
    for id in buildings {
        state
            .building_mut(*id)
            .and_then(|b| b.magazine.as_mut())
            .expect("validated")
            .autofill = enabled;
    }
    Ok(())
}

/// Any tile on the map may be struck, seen or not.
pub(super) fn apply_launch(
    state: &mut State,
    player: PlayerId,
    building: BuildingId,
    target: TilePos,
) -> Result<(), RejectReason> {
    let on_map = state.map.tile(target).is_some();
    let b = state
        .building_mut(building)
        .filter(|b| b.player == player)
        .ok_or(RejectReason::NotYourBuilding)?;
    if !b.built() || b.launcher.is_none() {
        return Err(RejectReason::InvalidTarget);
    }
    if !on_map {
        return Err(RejectReason::OutOfBounds);
    }
    if b.magazine.is_none_or(|magazine| magazine.loaded == 0) {
        return Err(RejectReason::NothingLoaded);
    }
    let launcher = b.launcher.as_mut().expect("checked above");
    // A new strike lines up afresh.
    launcher.target = Some(target);
    launcher.ready = 0;
    Ok(())
}

/// Production-phase work on every magazine, in building-id order: an
/// autofilling magazine with room and nothing ordered buys one round when
/// the bank covers it without dipping into a stranded seat's recovery
/// reserve, then the front ordered round advances one tick.
pub(super) fn assemble(state: &mut State, events: &mut Vec<Event>) {
    for index in 0..state.buildings.len() {
        let b = &state.buildings[index];
        let (Some(stats), Some(magazine)) = (b.kind.magazine(), b.magazine) else {
            continue;
        };
        if !b.built() {
            continue;
        }
        let (id, player, kind) = (b.id, b.player, b.kind);
        if magazine.autofill
            && magazine.ordered == 0
            && magazine.loaded < stats.capacity
            && state
                .player(player)
                .scrap
                .checked_sub(stats.round_cost)
                .is_some_and(|left| left >= state.recovery_reserve(player))
        {
            state.player_mut(player).scrap -= stats.round_cost;
            state.buildings[index]
                .magazine
                .as_mut()
                .expect("checked above")
                .ordered = 1;
            events.push(Event::RoundAutofilled {
                building: id,
                player,
                kind,
                cost: stats.round_cost,
            });
        }
        let magazine = state.buildings[index]
            .magazine
            .as_mut()
            .expect("checked above");
        if magazine.ordered == 0 {
            continue;
        }
        magazine.progress += 1;
        if magazine.progress < stats.round_ticks {
            continue;
        }
        magazine.progress = 0;
        magazine.ordered -= 1;
        magazine.loaded += 1;
        let loaded = magazine.loaded;
        events.push(Event::RoundAssembled {
            building: id,
            player,
            kind,
            loaded,
        });
    }
}
