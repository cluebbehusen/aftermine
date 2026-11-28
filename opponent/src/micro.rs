//! The top rung's handling of its fighting units one at a time: stepping out
//! of an incoming shell's blast, and keeping out of a slower, shorter-reaching
//! enemy's reach while a weapon reloads. Nothing here persists between
//! decisions.

use crate::decision::Ledger;
use crate::frame::{HomeFrame, doubled};
use crate::map::MapModel;
use aftermine_sim::observation::{ObservationData, UnitObs};
use aftermine_sim::scenario::BotDifficulty;
use aftermine_sim::stats::{BuildingKind, Domain, UnitKind};
use aftermine_sim::{AttackTarget, Command, UnitId};
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;

/// Half a tile: observations give tiles, not positions.
const TILE_SLACK: Fx = Fx::lit("0.5");

/// Whether a seat handles its fighting units one at a time: steps them out
/// of shell blasts and keeps its range while reloading. A difficulty limit
/// on execution.
pub(crate) fn handles(difficulty: BotDifficulty) -> bool {
    difficulty == BotDifficulty::Prime
}

/// A unit micro may move: a member of a mission in its fight, with that
/// mission's goal and focus, or an idle unit with neither.
#[derive(Clone, Copy)]
struct Fighter<'a> {
    unit: &'a UnitObs,
    goal: Option<TilePos>,
    focus: Option<UnitId>,
}

/// Steps own fighting units out of incoming shells' blasts, then keeps
/// ranged ones out of slower, shorter-reaching enemies' reach while they
/// reload. A unit given an order this decision keeps it, and one moved here
/// takes no later order this decision.
pub(crate) fn act(
    (observation, map, frame): (&ObservationData, &MapModel, HomeFrame),
    fighting: &[(UnitId, TilePos, Option<UnitId>)],
    interval: u64,
    ledger: &mut Ledger,
) {
    let ordered = ledger.ordered_units();
    let fighters: Vec<Fighter> = observation
        .my_units
        .iter()
        .filter(|unit| !unit.kind.stats().weapons.is_empty() && unit.kind.stats().harvest.is_none())
        .filter(|unit| {
            ordered.binary_search(&unit.id).is_err()
                && !ledger.aside(unit.id)
                && !ledger.stuck(unit.id)
                && !ledger.docked(unit.id)
        })
        .filter_map(
            |unit| match fighting.binary_search_by_key(&unit.id, |(id, ..)| *id) {
                Ok(index) => Some(Fighter {
                    unit,
                    goal: Some(fighting[index].1),
                    focus: fighting[index].2,
                }),
                Err(_) => unit.idle.then_some(Fighter {
                    unit,
                    goal: None,
                    focus: None,
                }),
            },
        )
        .collect();
    dodge((observation, map, frame), &fighters, ledger);
    kite((observation, map, frame), &fighters, interval, ledger);
}

fn centre(tile: TilePos) -> Vec2Fx {
    Vec2Fx::new(
        Fx::from_num(tile.x) + TILE_SLACK,
        Fx::from_num(tile.y) + TILE_SLACK,
    )
}

fn apart(a: TilePos, b: TilePos) -> Fx {
    (centre(a) - centre(b)).length()
}

/// The widest blast of any shell a unit or structure fires.
fn widest_blast() -> Fx {
    let units = UnitKind::ALL.iter().flat_map(|kind| kind.stats().weapons);
    let structures = BuildingKind::ALL
        .iter()
        .flat_map(|kind| kind.tiers())
        .flat_map(|tier| tier.weapons);
    units
        .chain(structures)
        .filter(|weapon| weapon.projectile.is_some())
        .filter_map(|weapon| weapon.splash)
        .max()
        .unwrap_or(Fx::ZERO)
}

/// Tiles within which an enemy in sight that could hit a unit counts as in
/// its fight, a computation bound on whom it must outrun to leave.
const FIGHT_TILES: i32 = 6;

/// Whether `unit` outruns every armed enemy in sight in its fight that could
/// hit it: each strictly slower.
pub(crate) fn outpaces(observation: &ObservationData, unit: &UnitObs) -> bool {
    let domain = unit.body_domain();
    let speed = unit.kind.stats().speed;
    observation.enemy_units.iter().all(|enemy| {
        range_against(enemy.kind, domain).is_none()
            || enemy.tile.chebyshev(unit.tile) > FIGHT_TILES
            || enemy.kind.stats().speed < speed
    })
}

/// The longest reach of `kind`'s weapons against `domain`, if any hits it.
fn range_against(kind: UnitKind, domain: Domain) -> Option<(usize, Fx)> {
    kind.stats()
        .weapons
        .iter()
        .enumerate()
        .filter(|(_, weapon)| weapon.targets.covers(domain))
        .map(|(slot, weapon)| (slot, weapon.range))
        .max_by_key(|(_, range)| *range)
}

/// Ticks a brace takes to deploy again after its gun moves.
fn redeploy(kind: UnitKind) -> u32 {
    kind.stats()
        .brace
        .map_or(0, |brace| u32::from(brace.deploy_ticks))
}

/// Ticks `unit` takes to walk to `tile`, rounded up.
fn walk_ticks(unit: &UnitObs, tile: TilePos) -> u32 {
    let speed = unit.kind.stats().speed;
    if speed <= Fx::ZERO {
        return u32::MAX;
    }
    (apart(unit.tile, tile) / speed).ceil().to_num::<u32>()
}

/// The tiles within `span` of `unit` it could stand on: on its ground and
/// open as far as the seat knows for a walker, inside the map for a flier.
fn stands<'a>(
    (observation, map): (&'a ObservationData, &'a MapModel),
    unit: &'a UnitObs,
    span: i32,
) -> impl Iterator<Item = TilePos> + 'a {
    let walks = unit.body_domain() == Domain::Ground;
    let ground = map.component(unit.tile);
    (-span..=span)
        .flat_map(move |dy| (-span..=span).map(move |dx| unit.tile.offset(dx, dy)))
        .filter(move |tile| {
            if walks {
                ground.is_some()
                    && map.component(*tile) == ground
                    && crate::crush::open(observation, *tile)
            } else {
                (0..observation.map_width).contains(&tile.x)
                    && (0..observation.map_height).contains(&tile.y)
            }
        })
}

/// Moves `fighter` to `goal`, a turreted gun firing on the way, then back to
/// its fight: at the enemy `back`, else its mission's goal, else where it
/// stood. False once the allowance has no room for both orders.
fn step(fighter: Fighter, goal: TilePos, back: Option<UnitId>, ledger: &mut Ledger) -> bool {
    if ledger.room() < 2 {
        return false;
    }
    let units = vec![fighter.unit.id];
    ledger.order(if fighter.unit.kind.has_ground_turret() {
        Command::Advance {
            units: units.clone(),
            goal,
            queue: false,
        }
    } else {
        Command::Run {
            units: units.clone(),
            goal,
            queue: false,
        }
    });
    ledger.order(match back {
        Some(enemy) => Command::Attack {
            units,
            target: AttackTarget::Unit(enemy),
            queue: true,
        },
        None => Command::Hunt {
            units,
            goal: fighter.goal.unwrap_or(fighter.unit.tile),
            queue: true,
        },
    });
    ledger.step_aside(fighter.unit.id);
    true
}

/// Steps each walking fighter standing within the blast of a shell the seat
/// sees coming down out of every such blast, most urgent first: to the
/// nearest tile clear of them all, preferring one from which it still
/// reaches its focus. A gun with no turret, which cannot fire on the move,
/// stays to shoot unless its reload, and its brace, outlast the step or
/// nothing is in its reach.
fn dodge(
    (observation, map, frame): (&ObservationData, &MapModel, HomeFrame),
    fighters: &[Fighter],
    ledger: &mut Ledger,
) {
    let impacts = &observation.incoming_shells;
    if impacts.is_empty() {
        return;
    }
    let blast = widest_blast();
    let reach = |unit: &UnitObs| blast + unit.kind.stats().radius + TILE_SLACK;
    let mut endangered: Vec<(Fx, TilePos, &Fighter)> = fighters
        .iter()
        .filter(|fighter| fighter.unit.body_domain() == Domain::Ground)
        .filter_map(|fighter| {
            let (nearest, impact) = impacts
                .iter()
                .map(|impact| (apart(fighter.unit.tile, *impact), *impact))
                .min_by_key(|(nearest, impact)| {
                    (
                        *nearest,
                        frame.rank(doubled(fighter.unit.tile), doubled(*impact)),
                    )
                })?;
            (nearest <= reach(fighter.unit)).then_some((nearest, impact, fighter))
        })
        .collect();
    endangered.sort_by_key(|(nearest, impact, fighter)| {
        (
            *nearest,
            frame.rank(doubled(*impact), doubled(fighter.unit.tile)),
        )
    });
    for (.., fighter) in endangered {
        let unit = fighter.unit;
        let clear = reach(unit);
        let focus = fighter
            .focus
            .and_then(|id| observation.enemy_units.iter().find(|enemy| enemy.id == id));
        let reaches_focus = |tile: TilePos| {
            focus.is_none_or(|enemy| {
                range_against(unit.kind, enemy.body_domain())
                    .is_some_and(|(_, range)| apart(tile, enemy.tile) <= range)
            })
        };
        let span = clear.ceil().to_num::<i32>() + 1;
        let Some(refuge) = stands((observation, map), unit, span)
            .filter(|tile| impacts.iter().all(|impact| apart(*tile, *impact) > clear))
            .min_by_key(|tile| {
                (
                    !reaches_focus(*tile),
                    unit.tile.chebyshev(*tile),
                    frame.rank(doubled(unit.tile), doubled(*tile)),
                )
            })
        else {
            continue;
        };
        if !unit.kind.has_ground_turret() {
            let busy = observation.enemy_units.iter().any(|enemy| {
                range_against(unit.kind, enemy.body_domain())
                    .is_some_and(|(_, range)| apart(unit.tile, enemy.tile) <= range)
            });
            let waits = unit.reload[0];
            let away = walk_ticks(unit, refuge).saturating_add(redeploy(unit.kind));
            if busy && waits < away {
                continue;
            }
        }
        if !step(*fighter, refuge, focus.map(|enemy| enemy.id), ledger) {
            return;
        }
    }
}

/// Steps each ranged fighter reloading for longer than the seat takes to
/// look again back out of the reach of every armed enemy in sight that
/// could hit it by then, when each of them is no faster and reaches less
/// far: to the nearest tile clear of them from which it still reaches the
/// nearest, it then turns on that one again.
fn kite(
    (observation, map, frame): (&ObservationData, &MapModel, HomeFrame),
    fighters: &[Fighter],
    interval: u64,
    ledger: &mut Ledger,
) {
    let look = Fx::from_num(interval);
    for fighter in fighters {
        let unit = fighter.unit;
        // A badly wounded unit is going home, not back into the fight.
        if ledger.aside(unit.id) || ledger.convalescing(unit.id) {
            continue;
        }
        let stats = unit.kind.stats();
        let domain = unit.body_domain();
        // Each enemy that could hit it by the next look, with how far it
        // reaches then.
        let threats: Vec<(&UnitObs, Fx)> = observation
            .enemy_units
            .iter()
            .filter_map(|enemy| {
                let (_, range) = range_against(enemy.kind, domain)?;
                let closes = range + enemy.kind.stats().speed * look + TILE_SLACK;
                (apart(unit.tile, enemy.tile) <= closes).then_some((enemy, closes))
            })
            .collect();
        let Some(&(nearest, _)) = threats.iter().min_by_key(|(enemy, _)| {
            (
                apart(unit.tile, enemy.tile),
                frame.rank(doubled(unit.tile), doubled(enemy.tile)),
            )
        }) else {
            continue;
        };
        let Some((slot, range)) = range_against(unit.kind, nearest.body_domain()) else {
            continue;
        };
        let outclassed = threats.iter().all(|(enemy, _)| {
            enemy.kind.stats().speed <= stats.speed
                && range_against(enemy.kind, domain).is_some_and(|(_, theirs)| {
                    range_against(unit.kind, enemy.body_domain())
                        .is_some_and(|(_, ours)| theirs < ours)
                })
        });
        let waits = unit.reload[slot];
        if !outclassed || u64::from(waits) <= interval {
            continue;
        }
        // A braced gun pays its brace again once it stops.
        let braced = redeploy(unit.kind);
        let span = range.ceil().to_num::<i32>() + 1;
        let Some(goal) = stands((observation, map), unit, span)
            .filter(|tile| {
                threats
                    .iter()
                    .all(|(enemy, closes)| apart(*tile, enemy.tile) > *closes)
            })
            .min_by_key(|tile| {
                (
                    apart(*tile, nearest.tile) > range - TILE_SLACK,
                    walk_ticks(unit, *tile) > waits,
                    unit.tile.chebyshev(*tile),
                    frame.rank(doubled(unit.tile), doubled(*tile)),
                )
            })
        else {
            continue;
        };
        if braced > 0 && walk_ticks(unit, goal).saturating_add(braced) > waits {
            continue;
        }
        if !step(*fighter, goal, Some(nearest.id), ledger) {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
