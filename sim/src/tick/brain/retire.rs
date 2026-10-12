//! Retirement: a unit goes to an own Crucible and stands, hovers, or lands
//! beside it while it is dismantled. The brain only runs the meter; cleanup
//! takes a unit apart the tick its meter fills, after every damage source
//! of the tick has landed.

use super::super::landing::{self, Pick};
use super::super::reach::Reach;
use super::locomotion::{Touchdown, approach_rect, fly_run_in};
use crate::event::{Event, StallReason};
use crate::ids::{BuildingId, UnitId};
use crate::state::{Order, State, Unit};
use crate::stats::{BuildingKind, RETIRE_REACH, RETIRE_TICKS};
use chassis::grid::TilePos;

/// Whether `building` is a completed Crucible of `player`'s still standing.
fn standing_crucible(state: &State, player: crate::ids::PlayerId, building: BuildingId) -> bool {
    state.building(building).is_some_and(|b| {
        b.player == player && b.hp > 0 && b.built() && b.kind == BuildingKind::Crucible
    })
}

/// The own completed Crucible nearest unit `id` that it can reach and, for a
/// turn-limited airframe, land beside. An exact distance tie goes to the
/// Crucible further ahead of the unit, then by its side of the unit's
/// heading: exact products in the unit's own frame, which a half-turn of the
/// scene preserves and which differ for any two footprints.
pub(in crate::tick) fn nearest_crucible(
    state: &State,
    reach: &mut Reach,
    id: UnitId,
) -> Option<BuildingId> {
    let unit = state.unit(id)?;
    let (player, pos, lands) = (unit.player, unit.pos, unit.kind.stats().turn_rate > 0);
    let ahead = chassis::compass::dir(unit.heading);
    let (ahead_x, ahead_y) = (i128::from(ahead.x.to_bits()), i128::from(ahead.y.to_bits()));
    let mut crucibles: Vec<_> = state
        .buildings()
        .iter()
        .filter(|b| standing_crucible(state, player, b.id))
        .map(|b| {
            let toward = b.center() - pos;
            let (x, y) = (
                i128::from(toward.x.to_bits()),
                i128::from(toward.y.to_bits()),
            );
            (
                pos.dist_sq(b.closest_point_to(pos)),
                std::cmp::Reverse(ahead_x * x + ahead_y * y),
                ahead_x * y - ahead_y * x,
                b.id,
            )
        })
        .collect();
    crucibles.sort_unstable_by_key(|&(distance, along, side, _)| (distance, along, side));
    crucibles
        .into_iter()
        .map(|(.., building)| building)
        .find(|&building| {
            let b = state.building(building).expect("standing Crucible");
            let doorsteps = crate::geometry::rect_adjacent_tiles(b.anchor, b.kind.size());
            reach.reaches(state, id, doorsteps)
                && (!lands || pad_beside(state, id, building).is_some())
        })
}

/// One tick of retirement for `id` at `crucible`.
pub(super) fn retire(
    state: &mut State,
    reach: &mut Reach,
    id: UnitId,
    (crucible, pad): (BuildingId, Option<TilePos>),
    drops: &mut super::logistics::Pending,
    events: &mut Vec<Event>,
) {
    let player = state.unit(id).expect("caller checked").player;
    let (crucible, pad) = if standing_crucible(state, player, crucible) {
        (crucible, pad)
    } else {
        let Some(next) = nearest_crucible(state, reach, id) else {
            stall(state, id, StallReason::NoRoute, events);
            return;
        };
        let unit = state.unit_mut(id).expect("caller checked");
        unit.order = Order::Retire {
            crucible: next,
            pad: None,
        };
        unit.path = None;
        // The meter counted work at the lost Crucible.
        unit.progress = 0;
        (next, None)
    };
    let unit = state.unit(id).expect("caller checked");
    let stats = unit.kind.stats();
    if stats.turn_rate > 0 {
        land_beside(state, id, crucible, pad, events);
        return;
    }
    let building = state.building(crucible).expect("standing Crucible");
    let (anchor, size) = (building.anchor, building.kind.size());
    if unit.pos.dist_sq(building.closest_point_to(unit.pos)) <= RETIRE_REACH * RETIRE_REACH {
        let unit = state.unit_mut(id).expect("caller checked");
        unit.path = None;
        // A loaded transport first sets its riders down beside the
        // Crucible; the meter waits for an empty sling.
        if unit.work_stopped() && !unit.cargo.is_empty() {
            super::logistics::set_down_for_retirement(state, id, drops);
            return;
        }
        dismantle(unit);
        return;
    }
    // Walkers and hovering airframes alike head for a doorstep, chosen in
    // the approach's own frame so mirrored machines take mirrored ones.
    if !approach_rect(state, id, anchor, size) {
        stall(state, id, StallReason::NoRoute, events);
    }
}

/// A turn-limited airframe lands on open ground beside the Crucible and is
/// dismantled where it rests.
fn land_beside(
    state: &mut State,
    id: UnitId,
    crucible: BuildingId,
    pad: Option<TilePos>,
    events: &mut Vec<Event>,
) {
    let unit = state.unit(id).expect("caller checked");
    if unit.landed() {
        // `stays_parked` lifts an airframe off before planning unless it
        // rests on its pad. Parked elsewhere, it lost its Crucible this tick
        // and takes off for the next one.
        if pad == Some(unit.tile()) {
            dismantle(state.unit_mut(id).expect("caller checked"));
        }
        return;
    }
    let pad = if let Some(pad) = pad {
        pad
    } else {
        let Some(pad) = pad_beside(state, id, crucible) else {
            stall(state, id, StallReason::NoOpenGround, events);
            return;
        };
        set_pad(state, id, crucible, pad);
        pad
    };
    match fly_run_in(state, id, pad) {
        Touchdown::Flying | Touchdown::Landed => {}
        Touchdown::Moved(next) => set_pad(state, id, crucible, next),
        Touchdown::Stalled(reason) => stall(state, id, reason, events),
    }
}

/// Open ground beside `crucible` for airframe `id` to land on. The search
/// starts from the doorstep the approach faces, not the footprint's center,
/// which an even footprint floors to one side and so would break mirrored
/// landings.
fn pad_beside(state: &State, id: UnitId, crucible: BuildingId) -> Option<TilePos> {
    let unit = state.unit(id).expect("caller checked");
    let (player, tile) = (unit.player, unit.tile());
    let building = state.building(crucible).expect("standing Crucible");
    let (anchor, size) = (building.anchor, building.kind.size());
    let from = super::super::rect_approach_origin(state, player, tile, anchor, size);
    let doorstep = crate::geometry::rect_adjacent_tiles(anchor, size)
        .min_by_key(|&t| crate::geometry::rect_approach_key_from(tile, from, anchor, size, t))
        .expect("a footprint has doorsteps");
    landing::nearest_landable(
        state,
        unit.kind.stats(),
        id,
        doorstep,
        unit.pos,
        unit.heading,
        crate::stats::GOAL_SNAP_RADIUS,
        None,
        Pick::Nearest,
        Some(player),
    )
}

fn set_pad(state: &mut State, id: UnitId, crucible: BuildingId, pad: TilePos) {
    let unit = state.unit_mut(id).expect("caller checked");
    unit.order = Order::Retire {
        crucible,
        pad: Some(pad),
    };
    unit.path = None;
}

/// Runs the dismantling meter of a machine standing at its Crucible, once
/// its sling, if it has one, is empty.
fn dismantle(unit: &mut Unit) {
    if unit.work_stopped() && unit.cargo.is_empty() {
        unit.progress = (unit.progress + 1).min(RETIRE_TICKS);
    }
}

fn stall(state: &mut State, id: UnitId, reason: StallReason, events: &mut Vec<Event>) {
    let unit = state.unit_mut(id).expect("caller checked");
    let (player, pos) = (unit.player, unit.pos);
    unit.drop_active_order();
    events.push(Event::OrderStalled {
        unit: id,
        player,
        pos,
        reason,
    });
}
