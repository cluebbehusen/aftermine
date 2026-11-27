//! Focus fire: an engaged mission's members that already reach one enemy
//! shoot it together, the one each rung judges weakest, while the rest keep
//! their own orders. Nobody chases an enemy a member already reaches, so
//! focusing never pulls a member out of position or sends it after an enemy
//! it cannot walk to. Members a direct-fire shooter outranges, with nothing
//! else in reach, close on it while it outranges them and stands clear of
//! known guns: left alone, they stand and die to it, or keep on at a
//! building while it shoots them. A defense sent at artillery keeps after
//! it. Members that pick their own targets heaviest first keep their own
//! aim: the weakest enemy would waste their single heavy shot.

use super::attack::fortified;
use super::{Missions, Task, hunt, mine, reached};
use crate::decision::Ledger;
use crate::frame::{HomeFrame, doubled};
use crate::map::{MapModel, Shot};
use aftermine_sim::observation::{ObservationData, UnitObs};
use aftermine_sim::scenario::BotDifficulty;
use aftermine_sim::stats::{Domain, TargetPriority};
use aftermine_sim::{AttackTarget, Command, UnitId};
use chassis::fx::Fx;

/// Tiles from a member inside which an enemy can be focused.
const FOCUS_TILES: i32 = 6;

impl Missions {
    /// Points every engaged mission at an enemy that outranges its members,
    /// else at the reachable enemy it judges weakest, keeping a focus while
    /// it lasts and ordering only when it changes.
    pub(crate) fn focus(
        &mut self,
        observation: &ObservationData,
        map: &MapModel,
        frame: HomeFrame,
        difficulty: BotDifficulty,
        ledger: &mut Ledger,
    ) {
        for mission in &mut self.list {
            let members: Vec<&UnitObs> = mission
                .units
                .iter()
                .filter_map(|id| mine(observation, *id))
                .filter(|unit| unit.kind.stats().target_priority == TargetPriority::Danger)
                // One sent running this decision keeps that order.
                .filter(|unit| !ledger.running(unit.id))
                .collect();
            let defending = matches!(mission.task, Task::Defend { .. });
            let Some(focus) = mission.task.focus() else {
                continue;
            };
            let legal = |enemy: &UnitObs| !shooters(map, &members, enemy).is_empty();
            // A defense sent at artillery closes on it before it can shoot,
            // and one sent at an unarmed intruder, such as a builder raising
            // a site, holds the target it chose. Held past reach, a focus on
            // a shooter that steps back would draw its members after it.
            let kept = focus.and_then(|id| {
                observation
                    .enemy_units
                    .iter()
                    .find(|enemy| enemy.id == id)
                    .filter(|enemy| {
                        legal(enemy)
                            || (defending
                                && (reached(enemy.kind) || enemy.kind.stats().weapons.is_empty()))
                    })
            });
            if kept.is_some() {
                continue;
            }
            let goal = doubled(mission.goal);
            let key = |enemy: &&UnitObs| {
                (
                    health(difficulty, enemy),
                    frame.rank(goal, doubled(enemy.tile)),
                    enemy.id,
                )
            };
            let next = outranger(observation, map, &members, key).or_else(|| {
                observation
                    .enemy_units
                    .iter()
                    .filter(|enemy| threatens(enemy, &members) && legal(enemy))
                    .min_by_key(key)
            });
            match next {
                Some(enemy) => {
                    let mut units = outranged(observation, map, &members, enemy);
                    units.extend(shooters(map, &members, enemy));
                    units.sort_unstable();
                    units.dedup();
                    let attack = Command::Attack {
                        units,
                        target: AttackTarget::Unit(enemy.id),
                        queue: false,
                    };
                    if ledger.order(attack) {
                        *focus = Some(enemy.id);
                    }
                }
                // Members their mission already ordered this decision keep
                // that order.
                None if focus.is_some()
                    && ledger.order(hunt(
                        members
                            .iter()
                            .filter(|unit| !unit.kind.stats().weapons.is_empty())
                            .map(|unit| unit.id)
                            .filter(|id| !ledger.ordered(*id))
                            .collect(),
                        mission.goal,
                    )) =>
                {
                    *focus = None;
                }
                None => {}
            }
        }
    }
}

/// How wounded `enemy` looks when choosing a focus, lowest first: a
/// difficulty limit on judging targets. The upper rungs read its exact
/// health, Standard only the quarter of its health bar it shows, and
/// Scrapheap ignores wounds and takes the enemy nearest the mission's goal.
fn health(difficulty: BotDifficulty, enemy: &UnitObs) -> u32 {
    match difficulty {
        BotDifficulty::Scrapheap => 0,
        BotDifficulty::Standard => (4 * enemy.hp).div_ceil(enemy.kind.stats().max_hp.max(1)),
        BotDifficulty::Veteran | BotDifficulty::Prime => enemy.hp,
    }
}

/// The enemy in sight that outranges members, standing clear of known guns,
/// first by `key`: the one they close on before any they already reach.
pub(super) fn outranger<'a, K: Ord>(
    observation: &'a ObservationData,
    map: &MapModel,
    members: &[&UnitObs],
    key: impl Fn(&&'a UnitObs) -> K,
) -> Option<&'a UnitObs> {
    observation
        .enemy_units
        .iter()
        .filter(|enemy| !fortified(observation, enemy.tile))
        .filter(|enemy| !outranged(observation, map, members, enemy).is_empty())
        .min_by_key(key)
}

/// The members `enemy` hits with direct fire from beyond their reach, by
/// id: in its range with a clear shot, able to hit it themselves only from
/// closer, reaching no other enemy that threatens them, and on its ground
/// unless they fly. They close on it; artillery is answered as it is
/// reached instead.
pub(super) fn outranged(
    observation: &ObservationData,
    map: &MapModel,
    members: &[&UnitObs],
    enemy: &UnitObs,
) -> Vec<UnitId> {
    let stats = enemy.kind.stats();
    let mut closers: Vec<UnitId> = members
        .iter()
        .filter(|unit| unit.tile.chebyshev(enemy.tile) <= FOCUS_TILES)
        .filter(|unit| {
            unit.kind.stats().domain != Domain::Ground
                || map
                    .component(unit.tile)
                    .is_some_and(|ground| map.component(enemy.tile) == Some(ground))
        })
        .filter(|unit| {
            let domain = unit.body_domain();
            let distance_sq = distance_sq(unit, enemy);
            let hit = stats.weapons.iter().filter(|weapon| {
                !weapon.indirect
                    && weapon.targets.covers(domain)
                    && distance_sq <= weapon.range * weapon.range
                    && distance_sq >= weapon.minimum_range * weapon.minimum_range
                    && map.clear_shot(
                        enemy.tile.center(),
                        unit.tile.center(),
                        (enemy.tile, unit.tile),
                        Shot::of(weapon, stats.domain, domain),
                    )
            });
            let Some(range) = hit.map(|weapon| weapon.range).max() else {
                return false;
            };
            let target = enemy.body_domain();
            let own = unit
                .kind
                .stats()
                .weapons
                .iter()
                .filter(|weapon| weapon.targets.covers(target))
                .map(|weapon| weapon.range)
                .max();
            own.is_some_and(|own| own < range)
                && shooters(map, &[*unit], enemy).is_empty()
                && !observation.enemy_units.iter().any(|other| {
                    other.id != enemy.id
                        && threatens(other, &[*unit])
                        && !shooters(map, &[*unit], other).is_empty()
                })
        })
        .map(|unit| unit.id)
        .collect();
    closers.sort_unstable();
    closers
}

/// The members of `units` on an armed `focus` in sight: those that reach it
/// and those it outranges, as the focus orders them. Its mission leaves them
/// on it rather than sending them on again, unless it is `dropping` them or
/// turns to a `gun` other than the focus.
pub(super) fn engaged(
    observation: &ObservationData,
    map: &MapModel,
    units: &[UnitId],
    (focus, gun): (Option<UnitId>, Option<UnitId>),
    dropping: bool,
) -> Vec<UnitId> {
    let Some(enemy) = focus
        .filter(|focus| !dropping && gun.is_none_or(|gun| gun == *focus))
        .and_then(|focus| observation.enemy_units.iter().find(|unit| unit.id == focus))
        .filter(|enemy| !enemy.kind.stats().weapons.is_empty())
    else {
        return Vec::new();
    };
    let members: Vec<&UnitObs> = units
        .iter()
        .filter_map(|id| mine(observation, *id))
        .collect();
    let mut engaged = outranged(observation, map, &members, enemy);
    engaged.extend(shooters(map, &members, enemy));
    engaged.sort_unstable();
    engaged.dedup();
    engaged
}

/// The squared distance between two units' tile centres, as a shot measures
/// it.
fn distance_sq(unit: &UnitObs, enemy: &UnitObs) -> Fx {
    let dx = Fx::from_num(unit.tile.x - enemy.tile.x);
    let dy = Fx::from_num(unit.tile.y - enemy.tile.y);
    dx * dx + dy * dy
}

/// Whether `enemy` stands near a member and can hit one.
fn threatens(enemy: &UnitObs, members: &[&UnitObs]) -> bool {
    let stats = enemy.kind.stats();
    members.iter().any(|unit| {
        let domain = unit.body_domain();
        unit.tile.chebyshev(enemy.tile) <= FOCUS_TILES
            && (stats
                .weapons
                .iter()
                .any(|weapon| weapon.targets.covers(domain))
                || (stats.demolition.is_some() && domain == Domain::Ground))
    })
}

/// The members that already reach `enemy`, by id: in a straight line between
/// tile centres that terrain does not stop, as the simulation measures a
/// shot, from the enemy's ground unless they fly. The rest keep their own
/// orders.
fn shooters(map: &MapModel, members: &[&UnitObs], enemy: &UnitObs) -> Vec<UnitId> {
    let domain = enemy.body_domain();
    let mut shooters = Vec::new();
    for unit in members {
        let mut weapons = unit
            .kind
            .stats()
            .weapons
            .iter()
            .filter(|weapon| weapon.targets.covers(domain))
            .peekable();
        if weapons.peek().is_none() {
            continue;
        }
        let walks = unit.kind.stats().domain == Domain::Ground;
        let ground = map.component(unit.tile);
        if walks && (ground.is_none() || map.component(enemy.tile) != ground) {
            continue;
        }
        let distance_sq = distance_sq(unit, enemy);
        let shooter = unit.kind.stats().domain;
        let reaches = weapons.any(|weapon| {
            distance_sq <= weapon.range * weapon.range
                && distance_sq >= weapon.minimum_range * weapon.minimum_range
                && map.clear_shot(
                    unit.tile.center(),
                    enemy.tile.center(),
                    (unit.tile, enemy.tile),
                    Shot::of(weapon, shooter, domain),
                )
        });
        if reaches {
            shooters.push(unit.id);
        }
    }
    shooters
}

#[cfg(test)]
mod tests;
