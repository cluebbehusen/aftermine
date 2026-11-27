//! What the seat's units would meet at a place, or on their way to it: the
//! estimate of enemy strength a defense, a local guard and a raid read to go
//! out, wait or give the place up.

use super::attack::{Force, guards, gun};
use super::defense::{at_home, lets_go};
use super::reserve::LOCAL_REACH;
use super::{against_machines, reached, value};
use crate::counters::{Piece, weigh};
use crate::frame::{HomeFrame, doubled, gap};
use crate::map::{MapModel, NEIGHBOURS, UNREACHABLE};
use crate::memory::{Memory, SeenUnit};
use aftermine_sim::observation::ObservationData;
use aftermine_sim::stats::Domain;
use aftermine_sim::{BuildingKind, UnitId, UnitKind};
use chassis::fx::Fx;
use chassis::grid::TilePos;

/// A known armed enemy ground unit that could join a fight: what it is,
/// where it stands, how near a fight it joins from, and what it is worth
/// against the seat's units.
struct Foe {
    kind: UnitKind,
    tile: TilePos,
    reach: i32,
    worth: u64,
}

/// The known armed enemy ground units, in sight by their health and
/// remembered out of sight by `worth`. Each
/// joins a fight within local reach of it, or its own reach if longer: an
/// enemy holding a line a few tiles behind the one that came near joins the
/// fight once the seat's units walk into reach of it.
fn foes(
    observation: &ObservationData,
    memory: &Memory,
    worth: impl Fn(&SeenUnit) -> u64,
) -> Vec<Foe> {
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
            kind: enemy.kind,
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
                kind: unit.kind,
                tile: unit.tile,
                reach,
                worth: worth(unit),
            })
        });
    seen.chain(remembered).collect()
}

/// What the `foes` that would join a fight at `place` are worth.
fn joining(foes: &[Foe], place: TilePos) -> u64 {
    joining_force(foes, place)
        .iter()
        .map(|(_, worth)| worth)
        .sum()
}

/// The `foes` that could join a fight at `place`, as the chart weighs them.
fn joining_force(foes: &[Foe], place: TilePos) -> Force {
    foes.iter()
        .filter(|foe| foe.tile.chebyshev(place) <= foe.reach)
        .map(|foe| (Piece::Unit(foe.kind), foe.worth))
        .collect()
}

/// The known armed enemy ground units that could join a fight at `place`,
/// each kind with what it is worth against the seat's units, a remembered
/// one at its whole price while the seat remembers where it stood.
pub(super) fn opposed(observation: &ObservationData, memory: &Memory, place: TilePos) -> Force {
    joining_force(&foes(observation, memory, located(observation.tick)), place)
}

/// A remembered unit's whole price while the seat remembers where it stood.
fn located(now: u64) -> impl Fn(&SeenUnit) -> u64 {
    move |unit| super::located(unit, now)
}

/// A remembered unit's price by how sure the seat is it is still there.
fn faded(now: u64) -> impl Fn(&SeenUnit) -> u64 {
    move |unit| unit.value(now) * against_machines(unit.kind) / 1_000
}

/// What the known armed enemy ground units other than the gun standing at
/// `place` that could join a fight at it are worth: its escort, which
/// defenders going at the gun meet on the way. Another gun beside it is
/// escort too.
pub(super) fn escort(observation: &ObservationData, memory: &Memory, place: TilePos) -> u64 {
    let foes: Vec<Foe> = foes(observation, memory, located(observation.tick))
        .into_iter()
        .filter(|foe| !(foe.tile == place && reached(foe.kind)))
        .collect();
    joining(&foes, place)
}

/// Steps along a way between the places it is judged at, each within local
/// reach of the steps between: a computation bound, not a count.
const STRIDE: u32 = 4;

/// The strongest known enemy force, weighed against the group `against`, a
/// ground group would meet on its way from
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
    against: &[(Piece, u64)],
) -> Force {
    let me = observation.me;
    let foes = foes(observation, memory, faded(observation.tick));
    let mut tile = goal;
    let mut most = Vec::new();
    for step in 0.. {
        let here = map.distance(me, tile);
        if here == UNREACHABLE || at_home(observation, map, tile) {
            break;
        }
        if step > 0 && step % STRIDE == 0 {
            let mut met = joining_force(&foes, tile);
            met.extend(
                observation
                    .enemy_buildings
                    .iter()
                    .filter(|building| guards(building, tile))
                    .map(gun),
            );
            if weigh(&met, against) > weigh(&most, against) {
                most = met;
            }
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

/// Tiles from a site going up within which enemies, and the seat's units,
/// would fight over it: a model of who stands at a site, not a count.
const SITE_TILES: i32 = 8;

/// Whether the known enemy ground force near a defense of `kind` placed at
/// `anchor` outweighs the gun and the seat's units near it that hit ground,
/// aircraft too, each side weighed against the other by the counter chart:
/// a lone Turret raised in front of an army that beats it only feeds it its
/// price and its builder. An enemy counts within its reach of the
/// footprint, or within the site's reach when it can walk there; distances
/// run from the footprint, so mirrored sites are judged alike. Never at
/// home, and beside a built Foundry only as [`lets_go`] weighs it, so never
/// by the seat's last, where a short defense may raise one as its answer.
pub(crate) fn overruns(
    observation: &ObservationData,
    memory: &Memory,
    map: &MapModel,
    kind: BuildingKind,
    anchor: TilePos,
) -> bool {
    let size = kind.size();
    if at_home(observation, map, anchor) {
        return false;
    }
    let foundries = crate::investments::built_foundries(&observation.my_buildings).count();
    if foundries == 0 {
        return false;
    }
    let beside = crate::investments::built_foundries(&observation.my_buildings)
        .any(|foundry| gap(foundry.anchor, foundry.kind.size(), anchor, size) <= LOCAL_REACH);
    let ground = map.component(anchor);
    let near = |kind: UnitKind, tile: TilePos| {
        let reach = crate::defenses::reach(kind);
        let apart = gap(anchor, size, tile, (1, 1));
        kind.stats().domain == Domain::Ground
            && reach > Fx::ZERO
            && (apart <= reach.ceil().to_num::<i32>()
                || (apart <= SITE_TILES
                    && ground.is_some_and(|component| {
                        map.component(tile) == Some(component) || map.touches(tile, component)
                    })))
    };
    // A machine reported standing up out of sight is remembered as seen at
    // this decision, so what is in sight is told apart by id.
    let mut visible: Vec<UnitId> = observation
        .enemy_units
        .iter()
        .map(|enemy| enemy.id)
        .collect();
    visible.sort_unstable();
    let enemy: Force = observation
        .enemy_units
        .iter()
        .filter(|enemy| near(enemy.kind, enemy.tile))
        .map(|enemy| (Piece::Unit(enemy.kind), value(enemy)))
        .chain(
            memory
                .units()
                .iter()
                .filter(|unit| {
                    visible.binary_search(&unit.id).is_err() && near(unit.kind, unit.tile)
                })
                .map(|unit| {
                    (
                        Piece::Unit(unit.kind),
                        unit.value(observation.tick) * against_machines(unit.kind) / 1_000,
                    )
                }),
        )
        .collect();
    let held: Force = std::iter::once((
        Piece::Gun(kind, 0),
        u64::from(kind.base_stats().construction.cost),
    ))
    .chain(
        observation
            .my_units
            .iter()
            .filter(|unit| super::hits(unit, Domain::Ground))
            .filter(|unit| gap(anchor, size, unit.tile, (1, 1)) <= SITE_TILES)
            .map(|unit| (Piece::Unit(unit.kind), value(unit))),
    )
    .collect();
    let (against, held) = (weigh(&enemy, &held), weigh(&held, &enemy));
    if beside {
        lets_go(held, against, foundries)
    } else {
        against > held
    }
}

#[cfg(test)]
mod tests;
