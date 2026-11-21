//! What the seat's units would meet at a place, or on their way to it: the
//! estimate of enemy strength a defense, a local guard and a raid read to go
//! out, wait or give the place up.

use super::attack::{building_value, guards};
use super::defense::at_home;
use super::reserve::LOCAL_REACH;
use super::{against_machines, value};
use crate::frame::{HomeFrame, doubled};
use crate::map::{MapModel, NEIGHBOURS, UNREACHABLE};
use crate::memory::Memory;
use aftermine_sim::observation::ObservationData;
use aftermine_sim::stats::Domain;
use aftermine_sim::{UnitId, UnitKind};
use chassis::fx::Fx;
use chassis::grid::TilePos;

/// A known armed enemy ground unit that could join a fight: where it stands,
/// how near a fight it joins from, and what it is worth against the seat's
/// units.
struct Foe {
    tile: TilePos,
    reach: i32,
    worth: u64,
}

/// The known armed enemy ground units, in sight by their health and
/// remembered out of sight by how sure the seat is they still are. Each
/// joins a fight within local reach of it, or its own reach if longer: an
/// enemy holding a line a few tiles behind the one that came near joins the
/// fight once the seat's units walk into reach of it.
fn foes(observation: &ObservationData, memory: &Memory) -> Vec<Foe> {
    let joins = |kind: UnitKind| {
        let reach = crate::defenses::reach(kind);
        (kind.stats().domain == Domain::Ground && reach > Fx::ZERO)
            .then(|| reach.ceil().to_num::<i32>().max(LOCAL_REACH))
    };
    // A machine reported standing up out of sight is remembered as seen at
    // this decision, so what is in sight is told apart by id.
    let mut visible: Vec<UnitId> = observation
        .enemy_units
        .iter()
        .map(|enemy| enemy.id)
        .collect();
    visible.sort_unstable();
    let seen = observation.enemy_units.iter().filter_map(|enemy| {
        joins(enemy.kind).map(|reach| Foe {
            tile: enemy.tile,
            reach,
            worth: value(enemy),
        })
    });
    let remembered = memory
        .units()
        .iter()
        .filter(|unit| visible.binary_search(&unit.id).is_err())
        .filter_map(|unit| {
            joins(unit.kind).map(|reach| Foe {
                tile: unit.tile,
                reach,
                worth: unit.value(observation.tick) * against_machines(unit.kind) / 1_000,
            })
        });
    seen.chain(remembered).collect()
}

/// What the `foes` that would join a fight at `place` are worth.
fn joining(foes: &[Foe], place: TilePos) -> u64 {
    foes.iter()
        .filter(|foe| foe.tile.chebyshev(place) <= foe.reach)
        .map(|foe| foe.worth)
        .sum()
}

/// What the known armed enemy ground units that could join a fight at
/// `place` are worth against the seat's units.
pub(super) fn opposed(observation: &ObservationData, memory: &Memory, place: TilePos) -> u64 {
    joining(&foes(observation, memory), place)
}

/// Steps along a way between the places it is judged at, each within local
/// reach of the steps between: a computation bound, not a count.
const STRIDE: u32 = 4;

/// The strongest known enemy force a ground group would meet on its way from
/// the seat's start to `goal`: what [`opposed`] finds every few steps back
/// along a shortest ground way from `goal`, with the known enemy buildings
/// that fire on ground there, from the first such step, since the guard at
/// the goal itself is the target's, until home, where a fight is a
/// defense's. Nothing when no ground way leads there.
pub(super) fn on_the_way(
    observation: &ObservationData,
    memory: &Memory,
    (map, frame): (&MapModel, HomeFrame),
    goal: TilePos,
) -> u64 {
    let me = observation.me;
    let foes = foes(observation, memory);
    let mut tile = goal;
    let mut most = 0;
    for step in 0.. {
        let here = map.distance(me, tile);
        if here == UNREACHABLE || at_home(observation, map, tile) {
            break;
        }
        if step > 0 && step % STRIDE == 0 {
            let guns: u64 = observation
                .enemy_buildings
                .iter()
                .filter(|building| guards(building, tile))
                .map(building_value)
                .sum();
            most = most.max(joining(&foes, tile) + guns);
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

#[cfg(test)]
mod tests;
