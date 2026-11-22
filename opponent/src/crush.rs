//! Keeping small machines out of a crusher's way. A crusher, such as the
//! Maw, never turns on a machine, but its front goes over every small one
//! that stands before it. Nothing here persists between decisions.

use crate::decision::Ledger;
use crate::frame::{HomeFrame, doubled};
use crate::map::MapModel;
use aftermine_sim::observation::{ObservationData, UnitObs};
use aftermine_sim::stats::{Domain, UnitKind, crush_touch};
use aftermine_sim::{AttackTarget, Command, UnitId};
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;

/// Half a tile: observations give tiles, not positions.
const TILE_SLACK: Fx = Fx::lit("0.5");

/// Tiles past the width of a crusher's path that a machine stepping out of
/// it looks for somewhere clear, a computation bound on the search.
const SEARCH_SLACK: i32 = 3;

/// The enemy crushers on the ground in sight.
pub(crate) fn crushers(observation: &ObservationData) -> Vec<&UnitObs> {
    observation
        .enemy_units
        .iter()
        .filter(|enemy| enemy.kind.crushes() && enemy.body_domain() == Domain::Ground)
        .collect()
}

fn centre(tile: TilePos) -> Vec2Fx {
    Vec2Fx::new(
        Fx::from_num(tile.x) + Fx::lit("0.5"),
        Fx::from_num(tile.y) + Fx::lit("0.5"),
    )
}

/// How far ahead of `crusher`, and how far to one side of its line, the
/// centre of `tile` lies, along the way it faces.
fn bearing(crusher: &UnitObs, tile: TilePos) -> (Fx, Fx) {
    let forward = chassis::compass::dir(crusher.heading);
    let offset = centre(tile) - centre(crusher.tile);
    (
        offset.x * forward.x + offset.y * forward.y,
        offset.y * forward.x - offset.x * forward.y,
    )
}

/// How far either side of `crusher`'s line a machine of `kind` must stand to
/// pass beside its front.
fn half_width(crusher: &UnitObs, kind: UnitKind) -> Fx {
    crush_touch(crusher.kind, kind) + TILE_SLACK
}

/// Whether a small machine of `kind` at `tile` stands in `crusher`'s path:
/// ahead of it, within its front's reach either side of its line, and near
/// enough that the crusher reaches it before the machine could cross out of
/// the way from where it stands and the seat look again `interval` ticks
/// later.
pub(crate) fn in_path(crusher: &UnitObs, kind: UnitKind, tile: TilePos, interval: u64) -> bool {
    let speed = kind.stats().speed;
    if !kind.crushable() || speed <= Fx::ZERO {
        return false;
    }
    let (ahead, left) = bearing(crusher, tile);
    let width = half_width(crusher, kind);
    if ahead <= Fx::ZERO || left.abs() > width {
        return false;
    }
    let crossing = (width - left.abs()) / speed;
    let length = width + crusher.kind.stats().speed * (crossing + Fx::from_num(interval));
    ahead <= length
}

/// Whether a crusher in sight would reach a footprint of `kind` at `anchor`,
/// or the ring beside it where its crew stands, within `ticks`, going over
/// the crew raising it and cutting it. The crew is judged as Harvesters.
pub(crate) fn in_way(
    observation: &ObservationData,
    kind: aftermine_sim::BuildingKind,
    anchor: TilePos,
    ticks: u64,
) -> bool {
    let (width, height) = kind.size();
    let crushers = crushers(observation);
    !crushers.is_empty()
        && (-1..=height)
            .flat_map(|dy| (-1..=width).map(move |dx| anchor.offset(dx, dy)))
            .any(|tile| !clear_of(&crushers, UnitKind::Harvester, tile, ticks))
}

/// Whether a small machine of `kind` at `tile` stands clear of every
/// crusher's path.
pub(crate) fn clear_of(
    crushers: &[&UnitObs],
    kind: UnitKind,
    tile: TilePos,
    interval: u64,
) -> bool {
    crushers
        .iter()
        .all(|crusher| !in_path(crusher, kind, tile, interval))
}

/// Whether a machine can stand on `tile` as far as the seat knows: no rock,
/// scrap node or building there. A run to anywhere else ends on a tile the
/// seat never checked.
fn open(observation: &ObservationData, tile: TilePos) -> bool {
    !observation.known_rock_at(tile)
        && !observation.known_scrap_at(tile)
        && observation
            .my_buildings
            .iter()
            .chain(&observation.ally_buildings)
            .chain(&observation.enemy_buildings)
            .all(|building| {
                let (width, height) = building.kind.size();
                let (x, y) = (tile.x - building.anchor.x, tile.y - building.anchor.y);
                !(0..width).contains(&x) || !(0..height).contains(&y)
            })
}

/// Where `unit`, in `crusher`'s path, goes to get out of it: the nearest tile
/// on its ground clear of every crusher's path, an armed machine preferring
/// one from which it still reaches `crusher`. The nearest clear tile lies on
/// the side of the path it already stands on.
fn refuge(
    (observation, map, frame): (&ObservationData, &MapModel, HomeFrame),
    (crushers, crusher): (&[&UnitObs], &UnitObs),
    unit: &UnitObs,
    interval: u64,
) -> Option<TilePos> {
    let ground = map.component(unit.tile)?;
    let reach = crate::defenses::reach(unit.kind);
    let within = crusher.kind.stats().radius + reach - TILE_SLACK;
    let from_crusher = |tile: TilePos| (centre(tile) - centre(crusher.tile)).length_sq();
    let fires = |tile: TilePos| reach > Fx::ZERO && from_crusher(tile) <= within * within;
    // Not beside its hull either, where a turn would bring its front round.
    let touch = half_width(crusher, unit.kind);
    let span = (half_width(crusher, unit.kind) * 2).ceil().to_num::<i32>() + SEARCH_SLACK;
    let from = doubled(unit.tile);
    (-span..=span)
        .flat_map(|dy| (-span..=span).map(move |dx| unit.tile.offset(dx, dy)))
        .filter(|tile| {
            map.component(*tile) == Some(ground)
                && open(observation, *tile)
                && from_crusher(*tile) > touch * touch
                && clear_of(crushers, unit.kind, *tile, interval)
        })
        .min_by_key(|tile| {
            (
                !fires(*tile),
                unit.tile.chebyshev(*tile),
                frame.rank(from, doubled(*tile)),
            )
        })
}

/// Sends every own small machine on the ground standing in a crusher's path
/// running out of it, an armed one to the nearest place beside or behind the
/// crusher it fires from, and leaves it out of every later order this
/// decision. A machine with nowhere clear to go keeps its orders.
pub(crate) fn step_aside(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    interval: u64,
    ledger: &mut Ledger,
) {
    let crushers = crushers(observation);
    if crushers.is_empty() {
        return;
    }
    for unit in observation
        .my_units
        .iter()
        .filter(|unit| unit.body_domain() == Domain::Ground)
    {
        let Some(crusher) = crushers
            .iter()
            .filter(|crusher| in_path(crusher, unit.kind, unit.tile, interval))
            .min_by_key(|crusher| {
                (
                    crusher.tile.chebyshev(unit.tile),
                    frame.rank(doubled(unit.tile), doubled(crusher.tile)),
                    crusher.id,
                )
            })
        else {
            continue;
        };
        let Some(goal) = refuge(
            (observation, map, frame),
            (&crushers, crusher),
            unit,
            interval,
        ) else {
            continue;
        };
        let id: UnitId = unit.id;
        if !ledger.order(Command::Run {
            units: vec![id],
            goal,
            queue: false,
        }) {
            return;
        }
        if crate::defenses::reach(unit.kind) > Fx::ZERO {
            ledger.order(Command::Attack {
                units: vec![id],
                target: AttackTarget::Unit(crusher.id),
                queue: true,
            });
        }
        ledger.step_aside(id);
    }
}

#[cfg(test)]
mod tests;
