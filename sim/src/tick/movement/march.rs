//! Marches: the walkers one Advance or Hunt command sent off together keep
//! pace on the way.
//!
//! As movement begins, every march's members still walking under it are
//! gathered per movement domain. The slowest top speed among them is the
//! march's pace. Each member works out when it would arrive at its own top
//! speed, and the latest of those is when the march arrives. A member drives
//! at the speed that lands it then, held between the pace and its own top
//! speed: nobody outruns the slowest, a member behind the others hurries to
//! arrive with them, and nobody waits. Members that are fighting, braced,
//! without a route, or already there drop out of the count, so the pace
//! follows whoever is actually on the way.
//!
//! Nothing here is stored. The caps are derived from orders and routes each
//! tick, in unit id order, so they cost one walk along each marching route.

use crate::ids::MarchId;
use crate::state::{Order, PathFollow, State, Unit};
use crate::stats::{Domain, UnitStats};
use chassis::fx::Fx;
use std::collections::BTreeMap;

/// The length of a diagonal step between tile centres.
const DIAGONAL: Fx = Fx::lit("1.41421356237");

/// Whether a unit of `stats` can slow down to keep a march's pace. A
/// turn-limited airframe flies its committed runs at full speed.
pub(in crate::tick) fn paces(stats: &UnitStats) -> bool {
    stats.turn_rate == 0
}

/// The march `unit` walks with this tick: its active walk's, while it has a
/// route to follow and a motor free to follow it.
fn marching(unit: &Unit) -> Option<(MarchId, &PathFollow)> {
    if unit.hp == 0 || unit.braces() > 0 || !paces(unit.kind.stats()) {
        return None;
    }
    let march = match &unit.order {
        Order::Run { goal } | Order::Hunt { goal } | Order::Advance { goal } => goal.march?,
        _ => return None,
    };
    Some((march, unit.path.as_ref()?))
}

/// How far `unit` still has to go along `path`: to the waypoint it is
/// making for, then from tile centre to tile centre.
fn remaining(unit: &Unit, path: &PathFollow) -> Fx {
    let next = path.next as usize;
    if next >= path.waypoints.len() {
        return Fx::ZERO;
    }
    let lead = unit.pos.dist(super::ground::path_point(path, next));
    path.waypoints[next..].windows(2).fold(lead, |total, step| {
        let (dx, dy) = ((step[1].x - step[0].x).abs(), (step[1].y - step[0].y).abs());
        total
            + match (dx, dy) {
                (0, 0) => Fx::ZERO,
                (0, 1) | (1, 0) => Fx::ONE,
                (1, 1) => DIAGONAL,
                _ => step[0].center().dist(step[1].center()),
            }
    })
}

/// One marching member: its unit slot, the distance it has left, and its
/// own top speed.
#[derive(Clone, Copy)]
struct Walker {
    slot: usize,
    left: Fx,
    top: Fx,
}

/// The speed each unit may drive at this tick, by unit slot. `None` drives
/// at its own top speed.
pub(super) fn caps(state: &State) -> Vec<Option<Fx>> {
    let mut marches: BTreeMap<(MarchId, Domain), Vec<Walker>> = BTreeMap::new();
    for (slot, unit) in state.units.iter().enumerate() {
        if let Some((march, path)) = marching(unit) {
            let stats = unit.kind.stats();
            marches
                .entry((march, stats.domain))
                .or_default()
                .push(Walker {
                    slot,
                    left: remaining(unit, path),
                    top: stats.speed,
                });
        }
    }
    let mut caps = vec![None; state.units.len()];
    for walkers in marches.values() {
        for (slot, cap) in pace(walkers) {
            caps[slot] = Some(cap);
        }
    }
    caps
}

/// The cap of every member of one march in one domain that is faster than
/// its pace, so the march lands together: the speed that arrives with its
/// last member, held between the slowest top speed and the member's own.
fn pace(walkers: &[Walker]) -> impl Iterator<Item = (usize, Fx)> + '_ {
    let pace = walkers.iter().map(|w| w.top).min().unwrap_or(Fx::ZERO);
    // The member that would arrive last at its own top speed: the greatest
    // distance over speed, compared without dividing.
    let last = walkers.iter().copied().reduce(|a, b| {
        if b.left * a.top > a.left * b.top {
            b
        } else {
            a
        }
    });
    walkers.iter().filter(move |w| w.top > pace).map(move |w| {
        let on_time = match last {
            Some(last) if last.left > Fx::ZERO => w.left * last.top / last.left,
            _ => pace,
        };
        (w.slot, on_time.clamp(pace, w.top))
    })
}

#[cfg(test)]
mod tests;
