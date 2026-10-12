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

/// The own completed Crucible nearest unit `id` that it can reach. An exact
/// distance tie goes to the bearing from the unit, measured against its
/// heading, a key a half-turn of the scene preserves.
pub(in crate::tick) fn nearest_crucible(
    state: &State,
    reach: &mut Reach,
    id: UnitId,
) -> Option<BuildingId> {
    let unit = state.unit(id)?;
    let (player, pos, heading) = (unit.player, unit.pos, unit.heading);
    let mut crucibles: Vec<_> = state
        .buildings()
        .iter()
        .filter(|b| standing_crucible(state, player, b.id))
        .map(|b| {
            let toward = b.center() - pos;
            (
                pos.dist_sq(b.closest_point_to(pos)),
                chassis::compass::heading_of(toward).wrapping_sub(heading),
                b.id,
                b.anchor,
                b.kind.size(),
            )
        })
        .collect();
    crucibles
        .sort_unstable_by_key(|&(distance, bearing, building, ..)| (distance, bearing, building));
    crucibles
        .into_iter()
        .find(|&(.., anchor, size)| {
            reach.reaches(
                state,
                id,
                crate::geometry::rect_adjacent_tiles(anchor, size),
            )
        })
        .map(|(_, _, building, ..)| building)
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
        // Parked anywhere but its pad, the airframe lifted off before
        // planning: `stays_parked` keeps only a retirement on its pad.
        dismantle(state.unit_mut(id).expect("caller checked"));
        return;
    }
    let pad = if let Some(pad) = pad {
        pad
    } else {
        let (stats, pos, heading, player, tile) = (
            unit.kind.stats(),
            unit.pos,
            unit.heading,
            unit.player,
            unit.tile(),
        );
        // The pad search starts from the doorstep the approach faces, not
        // the footprint's center, which an even footprint floors to one
        // side and so would break mirrored landings.
        let building = state.building(crucible).expect("standing Crucible");
        let (anchor, size) = (building.anchor, building.kind.size());
        let from = super::super::rect_approach_origin(state, player, tile, anchor, size);
        let doorstep = crate::geometry::rect_adjacent_tiles(anchor, size)
            .min_by_key(|&t| crate::geometry::rect_approach_key_from(tile, from, anchor, size, t))
            .expect("a footprint has doorsteps");
        let Some(pad) = landing::nearest_landable(
            state,
            stats,
            id,
            doorstep,
            pos,
            heading,
            crate::stats::GOAL_SNAP_RADIUS,
            None,
            Pick::Nearest,
            Some(player),
        ) else {
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
