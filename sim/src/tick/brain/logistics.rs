//! Transport logistics: boarding walks, the unload disgorge, and their
//! deferred resolution.
//!
//! Cargo lives outside the world's unit list (see
//! [`crate::state::Unit::cargo`]), so once a machine embarks nothing can
//! see, target, or command it. The unit list must hold still during the
//! brain phase, or every spatial-index slot behind the acting unit goes
//! stale, so board and unload only buffer intent here, like damage;
//! [`resolve`] mutates the list after the last brain has decided.

use super::super::route_for;
use crate::event::{Event, StallReason};
use crate::ids::UnitId;
use crate::state::{Order, PathFollow, Rider, State, Unit};
use chassis::grid::TilePos;

/// Boardings and unloads one tick's brains asked for, applied by
/// [`resolve`] once the decision loop is over.
#[derive(Default)]
pub(in crate::tick) struct Pending {
    /// (rider, carrier) pairs within reach of a sling with room.
    boardings: Vec<(UnitId, UnitId)>,
    /// Carriers setting their riders down this tick.
    drops: Vec<Drop>,
}

/// A carrier setting every rider down on open ground around a tile.
struct Drop {
    carrier: UnitId,
    /// The center of the drop ring.
    at: TilePos,
    /// Whether the ring scans half-turned.
    reverse: bool,
    /// An unload ends its order once the sling is empty or stuck. A retiring
    /// transport keeps its order: it is dismantled once empty, and riders
    /// with no open tile wait aboard for the next try.
    ends_order: bool,
}

/// Sets every rider of `carrier`, a retiring transport standing at its
/// Crucible, down around where it stands. Riders leave idle.
pub(super) fn set_down_for_retirement(state: &State, carrier: UnitId, pending: &mut Pending) {
    let unit = state.unit(carrier).expect("caller checked");
    // The airframe's facing frames the ring, so mirrored transports set
    // mirrored riders down: half-turned headings fall on opposite halves.
    pending.drops.push(Drop {
        carrier,
        at: unit.tile(),
        reverse: unit.heading >= 128,
        ends_order: false,
    });
}

/// Total sling room a transport's current riders occupy.
fn cargo_load(transport: &Unit) -> u8 {
    transport
        .cargo
        .iter()
        .map(|u| u.kind.stats().transport_size)
        .sum()
}

/// Walk within [`crate::stats::load_reach`] of the carrier and ask to
/// climb aboard. A full sling, a dead carrier, or a carrier that
/// stopped being ours stands the boarder down where it is.
pub(super) fn board(
    state: &mut State,
    id: UnitId,
    transport: UnitId,
    pending: &mut Pending,
    events: &mut Vec<Event>,
) {
    let unit = state.unit(id).expect("caller checked");
    let (pos, kind, player, my_size) = (
        unit.pos,
        unit.kind,
        unit.player,
        unit.kind.stats().transport_size,
    );
    // A transport on its way to be dismantled takes no more riders.
    let Some(carrier) = state
        .unit(transport)
        .filter(|t| t.hp > 0 && t.player == player && !t.retiring())
    else {
        state.unit_mut(id).expect("caller checked").clear_program();
        return;
    };
    let capacity = carrier.kind.stats().transport_capacity;
    let carrier_pos = carrier.pos;
    let reach = crate::stats::load_reach(carrier.kind.stats());
    if cargo_load(carrier) + my_size > capacity {
        let unit = state.unit_mut(id).expect("caller checked");
        unit.clear_program();
        events.push(Event::OrderStalled {
            unit: id,
            player,
            pos,
            reason: StallReason::TransportFull,
        });
        return;
    }
    if pos.dist_sq(carrier_pos) <= reach * reach {
        // In reach: stop walking and ask for the sling. The embark waits
        // for resolution so the unit list holds still under the other
        // brains.
        let unit = state.unit_mut(id).expect("caller checked");
        unit.path = None;
        pending.boardings.push((id, transport));
        return;
    }
    // Chase a standable tile inside boarding reach. Aircraft may hover over a
    // building, scrap, or blocked terrain that a ground rider can never occupy;
    // routing to the carrier's exact tile would reject an otherwise legal Load.
    let stale = state
        .unit(id)
        .expect("caller checked")
        .path
        .as_ref()
        .is_none_or(|p| {
            !state.passable(p.goal) || p.goal.center().dist_sq(carrier_pos) > reach * reach
        });
    if !stale {
        return;
    }
    if let Some((goal, waypoints)) = boarding_route(state, kind, pos, (carrier_pos, reach)) {
        let unit = state.unit_mut(id).expect("caller checked");
        unit.path = Some(PathFollow {
            final_point: None,
            goal,
            waypoints,
            next: 0,
            clearing: false,
        });
    } else {
        let unit = state.unit_mut(id).expect("caller checked");
        unit.drop_active_order();
        events.push(Event::OrderStalled {
            unit: id,
            player,
            pos,
            reason: StallReason::NoRoute,
        });
    }
}

/// Fewest-waypoint deterministic ground route to a standable tile within
/// boarding reach. A route that stays on the rider's current tile cannot help
/// when the exact rider and carrier positions are still too far apart, so it
/// is not a boarding route.
///
/// Waypoint count wins, then the tile nearest the rider, then the one nearest
/// the carrier; tiles still tied mirror each other across the rider's
/// approach, and the side of that approach decides. Every key is relative to
/// the rider and carrier, so half-turned boardings choose half-turned tiles.
/// A truly sealed carrier still reports `NoRoute` through the caller.
fn boarding_route(
    state: &State,
    kind: crate::stats::UnitKind,
    rider_pos: chassis::fx::Vec2Fx,
    carrier: (chassis::fx::Vec2Fx, chassis::fx::Fx),
) -> Option<(TilePos, Vec<TilePos>)> {
    let (carrier_pos, reach) = carrier;
    let from = TilePos::containing(rider_pos);
    let center = TilePos::containing(carrier_pos);
    let reach_sq = reach * reach;
    let span = reach.ceil().to_num::<i32>();
    let approach = carrier_pos - rider_pos;
    let (approach_x, approach_y) = (
        i128::from(approach.x.to_bits()),
        i128::from(approach.y.to_bits()),
    );
    (center.y - span..=center.y + span)
        .flat_map(|y| (center.x - span..=center.x + span).map(move |x| TilePos::new(x, y)))
        .filter(|goal| state.passable(*goal) && goal.center().dist_sq(carrier_pos) <= reach_sq)
        .filter_map(|goal| {
            let waypoints = route_for(state, kind, from, goal)?;
            (!waypoints.is_empty()).then_some((goal, waypoints))
        })
        .min_by_key(|(goal, waypoints)| {
            let offset = goal.center() - carrier_pos;
            let side = approach_x * i128::from(offset.y.to_bits())
                - approach_y * i128::from(offset.x.to_bits());
            (
                waypoints.len(),
                goal.center().dist_sq(rider_pos),
                goal.center().dist_sq(carrier_pos),
                side,
            )
        })
}

/// Fly to the drop point; standing on it, ask to set the riders down. A drop
/// point out of reach sets them down where the flight ended instead and
/// reports the shortfall.
pub(super) fn unload(
    state: &mut State,
    index: &super::super::spatial::UnitIndex,
    reach: &mut super::super::reach::Reach,
    id: UnitId,
    pending: &mut Pending,
    events: &mut Vec<Event>,
) {
    use super::locomotion::{Approach, Steer, steer};
    let Steer::Done { short } = steer(state, index, reach, id, Approach::Exact) else {
        return;
    };
    let unit = state.unit(id).expect("caller checked");
    let Order::Unload { reverse, .. } = unit.order else {
        unreachable!("only an unload order steers here");
    };
    let (pos, tile, player, looping) = (unit.pos, unit.tile(), unit.player, unit.looping);
    pending.drops.push(Drop {
        carrier: id,
        at: tile,
        reverse,
        ends_order: true,
    });
    if short && !looping {
        events.push(Event::OrderStalled {
            unit: id,
            player,
            pos,
            reason: StallReason::NoRoute,
        });
    }
}

/// Applies the tick's buffered boardings and unloads, after every brain has
/// decided and before anything moves. Buffers are re-sorted by id: the
/// brain loop alternates direction by tick parity, and the list mutations
/// here must not inherit that swing.
pub(in crate::tick) fn resolve(state: &mut State, mut pending: Pending, events: &mut Vec<Event>) {
    pending.boardings.sort_unstable_by_key(|&(rider, _)| rider);
    for (rider_id, transport) in pending.boardings {
        let Some(carrier) = state.unit(transport).filter(|t| t.hp > 0 && !t.retiring()) else {
            continue; // the sling died or is retiring; the order retries next tick
        };
        let capacity = carrier.kind.stats().transport_capacity;
        let held = cargo_load(carrier);
        // hp > 0 mirrors the carrier filter above: a rider dealt lethal
        // damage this same tick must die in cleanup, not ride in the sling
        // as a zero-hp corpse the death pass can no longer see.
        let Some(rider) = state.unit(rider_id).filter(|r| r.hp > 0) else {
            continue;
        };
        // Two boarders can clear the same last slot in one decision
        // pass; the first (by id) takes it and the other stalls out
        // through the ordinary full-sling arm next tick.
        if held + rider.kind.stats().transport_size > capacity {
            continue;
        }
        let slot = state
            .units
            .iter()
            .position(|u| u.id == rider_id)
            .expect("just seen");
        let walker = state.units.remove(slot);
        let player = walker.player;
        let carrier = state.unit_mut(transport).expect("just seen");
        carrier.cargo.push(Rider::board(&walker));
        events.push(Event::UnitBoarded {
            transport,
            unit: rider_id,
            player,
        });
    }

    pending.drops.sort_unstable_by_key(|drop| drop.carrier);
    for Drop {
        carrier: id,
        at,
        reverse,
        ends_order,
    } in pending.drops
    {
        let Some(carrier) = state.unit(id).filter(|t| t.hp > 0) else {
            continue;
        };
        let (pos, player) = (carrier.pos, carrier.player);
        // Claim drop tiles in square rings outward from the drop point,
        // center first, in the order's approach frame.
        let open: Vec<TilePos> =
            super::super::goals::ring_scan(at, crate::stats::UNLOAD_SCAN_RADIUS, reverse)
                .filter(|&t| state.passable(t))
                .collect();
        let mut placed = 0usize;
        while placed < open.len() {
            let carrier = state.unit_mut(id).expect("just seen");
            if carrier.cargo.is_empty() {
                break;
            }
            let spot = open[placed];
            let rider = carrier.cargo.remove(0).disembark(player, spot.center());
            let rider_id = rider.id;
            // Reinsert in id order to keep the unit list sorted.
            let slot = state
                .units
                .iter()
                .position(|u| u.id > rider_id)
                .unwrap_or(state.units.len());
            state.units.insert(slot, rider);
            placed += 1;
            events.push(Event::UnitUnloaded {
                transport: id,
                unit: rider_id,
                player,
                at: spot,
            });
        }
        if !ends_order {
            continue;
        }
        let carrier = state.unit_mut(id).expect("just seen");
        let stranded = !carrier.cargo.is_empty();
        carrier.advance_queue();
        if stranded {
            events.push(Event::OrderStalled {
                unit: id,
                player,
                pos,
                reason: StallReason::NoOpenGround,
            });
        }
    }
}

#[cfg(test)]
mod tests;
