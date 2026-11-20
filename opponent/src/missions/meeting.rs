//! What the seat's units would meet at a place, or on their way to it: the
//! estimate of enemy strength a defense, a local guard and a raid read to go
//! out, wait or give the place up.

use super::defense::at_home;
use super::reserve::LOCAL_REACH;
use super::{against_machines, value};
use crate::frame::{HomeFrame, doubled};
use crate::map::{MapModel, NEIGHBOURS, UNREACHABLE};
use crate::memory::Memory;
use aftermine_sim::UnitKind;
use aftermine_sim::observation::ObservationData;
use aftermine_sim::stats::Domain;
use chassis::fx::Fx;
use chassis::grid::TilePos;

/// What the known armed enemy ground units that could join a fight at
/// `place` are worth against the seat's units: those within local reach of
/// it, or their own reach if longer, in sight by their health and remembered
/// out of sight by how sure the seat is they still are. An enemy holding a
/// line a few tiles behind the one that came near counts, since it joins the
/// fight once the seat's units walk into reach of it.
pub(super) fn opposed(observation: &ObservationData, memory: &Memory, place: TilePos) -> u64 {
    let fights = |kind: UnitKind, tile: TilePos| {
        let reach = crate::defenses::reach(kind);
        kind.stats().domain == Domain::Ground
            && reach > Fx::ZERO
            && tile.chebyshev(place) <= reach.ceil().to_num::<i32>().max(LOCAL_REACH)
    };
    let seen: u64 = observation
        .enemy_units
        .iter()
        .filter(|enemy| fights(enemy.kind, enemy.tile))
        .map(value)
        .sum();
    let remembered: u64 = memory
        .units()
        .iter()
        .filter(|unit| unit.seen < observation.tick && fights(unit.kind, unit.tile))
        .map(|unit| unit.value(observation.tick) * against_machines(unit.kind) / 1_000)
        .sum();
    seen + remembered
}

/// Steps along a way between the places it is judged at, each within local
/// reach of the steps between: a computation bound, not a count.
const STRIDE: u32 = 4;

/// The strongest known enemy force a ground group would meet on its way from
/// the seat's start to `goal`: what [`opposed`] finds every few steps back
/// along a shortest ground way from `goal`, from the first such step, since
/// the guard at the goal itself is the target's, until home, where a fight
/// is a defense's. Nothing when no ground way leads there.
pub(super) fn on_the_way(
    observation: &ObservationData,
    memory: &Memory,
    (map, frame): (&MapModel, HomeFrame),
    goal: TilePos,
) -> u64 {
    let me = observation.me;
    let mut tile = goal;
    let mut most = 0;
    for step in 0.. {
        let here = map.distance(me, tile);
        if here == UNREACHABLE || at_home(observation, map, tile) {
            break;
        }
        if step > 0 && step % STRIDE == 0 {
            most = most.max(opposed(observation, memory, tile));
        }
        // Each step is nearer the start, so the walk ends.
        let Some(next) = NEIGHBOURS
            .iter()
            .map(|(dx, dy)| tile.offset(*dx, *dy))
            .filter(|next| map.distance(me, *next) < here)
            .min_by_key(|next| {
                (
                    map.distance(me, *next),
                    frame.rank(frame.home, doubled(*next)),
                )
            })
        else {
            break;
        };
        tile = next;
    }
    most
}
