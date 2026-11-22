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

/// Tiles past the edge of a crusher's path a machine stepping aside aims
/// beyond the first clear one, a computation bound on the search.
const ASIDE_SEARCH: i32 = 3;

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

/// How far ahead of `crusher`, and how far to its left, the centre of
/// `tile` lies, along the way it faces.
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

/// Whether a crusher in sight would reach any tile of a footprint of `kind`
/// at `anchor` within `ticks`, going over the crew raising it and cutting it.
pub(crate) fn in_way(
    observation: &ObservationData,
    kind: aftermine_sim::BuildingKind,
    anchor: TilePos,
    ticks: u64,
) -> bool {
    let (width, height) = kind.size();
    let crushers = crushers(observation);
    !crushers.is_empty()
        && (0..height)
            .flat_map(|dy| (0..width).map(move |dx| anchor.offset(dx, dy)))
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

/// The nearest tile on `unit`'s ground just past the side of `crusher`'s path
/// it stands on, clear of every crusher's path; either side when it stands
/// on the line.
fn aside(
    map: &MapModel,
    frame: HomeFrame,
    (crushers, crusher): (&[&UnitObs], &UnitObs),
    unit: &UnitObs,
    interval: u64,
) -> Option<TilePos> {
    let ground = map.component(unit.tile)?;
    let forward = chassis::compass::dir(crusher.heading);
    let left = Vec2Fx::new(forward.y, -forward.x);
    let (_, across) = bearing(crusher, unit.tile);
    let past = half_width(crusher, unit.kind) - across.abs() + Fx::lit("1");
    let sides: &[Fx] = match across.cmp(&Fx::ZERO) {
        std::cmp::Ordering::Greater => &[Fx::lit("1")],
        std::cmp::Ordering::Less => &[Fx::lit("-1")],
        std::cmp::Ordering::Equal => &[Fx::lit("1"), Fx::lit("-1")],
    };
    let from = centre(unit.tile);
    sides
        .iter()
        .filter_map(|side| {
            (0..=ASIDE_SEARCH)
                .map(|extra| {
                    let step = (past + Fx::from_num(extra)) * *side;
                    TilePos::containing(from + left * step)
                })
                .find(|tile| {
                    map.component(*tile) == Some(ground)
                        && clear_of(crushers, unit.kind, *tile, interval)
                })
        })
        .min_by_key(|tile| {
            (
                unit.tile.chebyshev(*tile),
                frame.rank(doubled(unit.tile), doubled(*tile)),
            )
        })
}

/// The nearest tile on `unit`'s ground clear of every crusher's path from
/// which it reaches `crusher` with its weapons, beside or behind it; none for
/// an unarmed machine or when no such tile is.
fn firing_post(
    map: &MapModel,
    frame: HomeFrame,
    (crushers, crusher): (&[&UnitObs], &UnitObs),
    unit: &UnitObs,
    interval: u64,
) -> Option<TilePos> {
    let reach = crate::defenses::reach(unit.kind);
    if reach <= Fx::ZERO {
        return None;
    }
    let ground = map.component(unit.tile)?;
    let within = crusher.kind.stats().radius + reach - TILE_SLACK;
    let span = within.ceil().to_num::<i32>();
    let from = centre(crusher.tile);
    (-span..=span)
        .flat_map(|dy| (-span..=span).map(move |dx| crusher.tile.offset(dx, dy)))
        .filter(|tile| {
            let offset = centre(*tile) - from;
            offset.length_sq() <= within * within
                && map.component(*tile) == Some(ground)
                && clear_of(crushers, unit.kind, *tile, interval)
        })
        .min_by_key(|tile| {
            (
                unit.tile.chebyshev(*tile),
                frame.rank(doubled(unit.tile), doubled(*tile)),
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
            .min_by_key(|crusher| (crusher.tile.chebyshev(unit.tile), crusher.id))
        else {
            continue;
        };
        let Some(goal) = firing_post(map, frame, (&crushers, crusher), unit, interval)
            .or_else(|| aside(map, frame, (&crushers, crusher), unit, interval))
        else {
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
