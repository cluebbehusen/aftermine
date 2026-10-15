//! Mounted guns: each gun sits on a fixed pivot of its hull, traverses only
//! inside its arc, and picks its own ground target every tick, whatever the
//! hull's order. The helm turns the hull for them only on an attack, an
//! attack-move that meets an enemy, or an idle gun with nothing in its arc.
//! A hull never chases fire from beyond its guns' reach.

use super::*;
use crate::state::{AttackView, IdleFacing};
use crate::stats::{MountStats, MountedGuns};

/// Something one of the hull's guns might shoot, seen this tick.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    target: Target,
    /// Lower first: a hostile that can hit the hull now, then other
    /// machines, then buildings.
    tier: u8,
    /// How far past a small body's its hull reaches.
    excess: Fx,
}

/// The hostiles in reach of a mounted hull's guns, surveyed once before its
/// order runs. Positions hold still while brains run, so the order and the
/// guns read the same survey.
pub(in crate::tick::brain) struct Sight {
    candidates: Vec<Candidate>,
}

impl Sight {
    /// Every hostile its guns could hit from somewhere on the hull: seen
    /// machines in a ground gun's domain, and seen, apparent buildings.
    pub(in crate::tick::brain) fn survey(
        state: &State,
        index: &super::super::super::spatial::UnitIndex,
        id: UnitId,
    ) -> Self {
        let unit = state.unit(id).expect("caller checked");
        let stats = unit.kind.stats();
        let guns = stats.mounts.expect("a mounted hull");
        let (me, pos) = (unit.player, unit.pos);
        let range = stats
            .weapons
            .iter()
            .map(|weapon| weapon.range)
            .max()
            .unwrap_or(Fx::ZERO)
            + guns.reach;
        let home = TilePos::containing(pos);
        let reach = range.floor().to_num::<i32>() + 1 + index.wide_reach();
        let hostiles = index.hostiles_near(state.player(me).team, home, reach + 1);
        let mut candidates = Vec::new();
        let covers = |domain: Domain| stats.weapons.iter().any(|w| w.targets.covers(domain));
        if hostiles.bodies {
            for dy in -reach..=reach {
                for &(_, slot) in index.row_span(home.y + dy, home.x - reach, home.x + reach) {
                    let u = &state.units[slot];
                    if !state.hostile(me, u.player)
                        || u.hp == 0
                        || !covers(u.domain())
                        || !state.sees_unit(me, u)
                    {
                        continue;
                    }
                    let threat = u.kind.stats().weapons.iter().any(|weapon| {
                        weapon.targets.air
                            && within_unit_weapon_reach(
                                state,
                                u.kind,
                                Target::Unit(id),
                                weapon,
                                u.pos.dist_sq(pos),
                            )
                    });
                    candidates.push(Candidate {
                        target: Target::Unit(u.id),
                        tier: u8::from(!threat),
                        excess: crate::stats::hull_excess(u.kind.stats().radius),
                    });
                }
            }
        }
        if covers(Domain::Ground) {
            let buildings = if hostiles.buildings {
                state.buildings()
            } else {
                index.buildings_since_survey(state.buildings())
            };
            let reach_sq = range * range;
            for b in buildings {
                if !state.hostile(me, b.player)
                    || b.hp == 0
                    || b.provisional()
                    || pos.dist_sq(b.closest_point_to(pos)) > reach_sq
                    || !state.building_apparent(me, b)
                    || !b.tiles().any(|tile| state.can_see(me, tile))
                {
                    continue;
                }
                let threat = b.stats().weapons.iter().any(|weapon| {
                    weapon.targets.air
                        && within_weapon_reach(
                            weapon,
                            b.center().dist_sq(pos),
                            crate::stats::hull_excess(stats.radius),
                        )
                });
                candidates.push(Candidate {
                    target: Target::Building(b.id),
                    tier: if threat { 0 } else { 2 },
                    excess: Fx::ZERO,
                });
            }
        }
        Self { candidates }
    }

    /// The candidates in the order a gun at `pivot` ranks them.
    fn ranked(&self, state: &State, pivot: Vec2Fx) -> Vec<(Candidate, Vec2Fx)> {
        let mut ranked: Vec<_> = self
            .candidates
            .iter()
            .map(|candidate| (*candidate, aim_point(state, candidate.target, pivot)))
            .collect();
        ranked.sort_by_key(|(candidate, aim)| {
            (candidate.tier, pivot.dist_sq(*aim), candidate.target)
        });
        ranked
    }
}

/// Where a gun at `from` aims at `target`: a machine's centre, or a
/// building's closest point.
fn aim_point(state: &State, target: Target, from: Vec2Fx) -> Vec2Fx {
    match target {
        Target::Unit(id) => state.unit(id).map_or(from, |u| u.pos),
        Target::Building(id) => state
            .building(id)
            .map_or(from, |b| b.closest_point_to(from)),
    }
}

/// The bearing from `pivot` to `aim` against a hull facing `heading`. A
/// target under the pivot reads as dead ahead, which every arc covers.
fn relative(pivot: Vec2Fx, aim: Vec2Fx, heading: u8) -> u8 {
    if aim == pivot {
        return 0;
    }
    chassis::compass::heading_of(aim - pivot).wrapping_sub(heading)
}

/// Whether a gun at `pivot` has a clear shot at `aim`: only peaks block the
/// fire of an aircraft.
fn clear(state: &State, pivot: Vec2Fx, aim: Vec2Fx) -> bool {
    !chassis::path::line_blocked(pivot, aim, |t| shot_crosses(state, t, false))
}

/// Whether `candidate`, aimed at `aim`, is in `weapon`'s reach from `pivot`.
fn in_reach(weapon: &WeaponStats, pivot: Vec2Fx, aim: Vec2Fx, excess: Fx) -> bool {
    within_weapon_reach(weapon, pivot.dist_sq(aim), excess)
}

/// Whether some gun would bear on a target at `aim` if the hull at `pos`
/// pointed its nose at it. A target under the rear of the hull never is:
/// no turn brings it into an arc.
fn feedable(guns: MountedGuns, pos: Vec2Fx, aim: Vec2Fx) -> bool {
    if aim == pos {
        return false;
    }
    let heading = chassis::compass::heading_of(aim - pos);
    guns.mounts.iter().any(|mount| {
        let pivot = mount.pivot(pos, heading);
        mount.covers(relative(pivot, aim, heading))
    })
}

/// What one gun took this tick.
#[derive(Debug, Clone, Copy)]
pub(in crate::tick::brain) struct Pick {
    aim: Vec2Fx,
}

/// The ordered target, as a gun at `pivot` would fire on it: a seen victim,
/// or a known place to fire on blind.
enum Ordered {
    Seen(Target),
    Blind(AttackView),
}

fn ordered(state: &State, id: UnitId) -> Option<Ordered> {
    let unit = state.unit(id).expect("caller checked");
    let Order::Attack { target, .. } = unit.order else {
        return None;
    };
    let view = state.attack_view(unit.player, target)?;
    Some(match view.entity {
        Some(entity) => Ordered::Seen(entity),
        None => Ordered::Blind(view),
    })
}

/// Every gun turns toward and fires on its own pick: the ordered target if
/// it bears, else a hostile that can hit the hull, else the nearest machine,
/// then the nearest building, each in its arc, reach and line. A gun with
/// nothing to shoot holds its bearing. Returns each slot's pick.
pub(in crate::tick::brain) fn fire(
    state: &mut State,
    id: UnitId,
    sight: &Sight,
    buffers: ShotBuffers<'_>,
) -> [Option<Pick>; crate::stats::MAX_WEAPONS] {
    let ShotBuffers { events, hits, .. } = buffers;
    let unit = state.unit(id).expect("caller checked");
    let (pos, heading, kind, player) = (unit.pos, unit.heading, unit.kind, unit.player);
    let stats = kind.stats();
    let guns = stats.mounts.expect("a mounted hull");
    let ordered = ordered(state, id);
    let mut picks = [None; crate::stats::MAX_WEAPONS];
    for (slot, (mount, weapon)) in guns.mounts.iter().zip(stats.weapons).enumerate() {
        let pivot = mount.pivot(pos, heading);
        let ranked = sight.ranked(state, pivot);
        let seen = |target: Target, excess: Fx| {
            let aim = aim_point(state, target, pivot);
            (mount.covers(relative(pivot, aim, heading))
                && in_reach(weapon, pivot, aim, excess)
                && clear(state, pivot, aim))
            .then_some(aim)
        };
        let order_pick = match &ordered {
            Some(Ordered::Seen(target)) => {
                seen(*target, target_excess(state, *target)).map(|aim| (Some(*target), aim, None))
            }
            Some(Ordered::Blind(view)) => {
                let aim = view.aim_from(pivot);
                (weapon.targets.covers(view.domain)
                    && mount.covers(relative(pivot, aim, heading))
                    && blind::solution(state, pivot, Domain::Air, *view, weapon))
                .then_some((None, aim, Some(*view)))
            }
            None => None,
        };
        let pick = order_pick.or_else(|| {
            ranked.iter().find_map(|(candidate, _)| {
                seen(candidate.target, candidate.excess)
                    .map(|aim| (Some(candidate.target), aim, None))
            })
        });
        let Some((victim, aim, blind_view)) = pick else {
            continue;
        };
        picks[slot] = Some(Pick { aim });
        let aligned = traverse(state, id, slot, mount, guns.traverse, pivot, aim);
        let unit = state.unit_mut(id).expect("caller checked");
        if !aligned || unit.cooldowns[slot] > 0 {
            continue;
        }
        unit.cooldowns[slot] = weapon.cooldown_ticks;
        match (victim, blind_view) {
            (Some(victim), _) => {
                buffer_shot(state, Target::Unit(id), victim, pivot, aim, weapon, hits);
            }
            (None, Some(view)) => {
                blind::buffer_blind(
                    state,
                    (Target::Unit(id), player),
                    pivot,
                    aim,
                    view,
                    weapon,
                    hits,
                );
            }
            (None, None) => unreachable!("a pick names a victim or a blind aim"),
        }
        events.push(Event::AttackHit {
            attacker: id,
            attacker_kind: kind,
            weapon: slot,
            target: victim,
            attacker_pos: pivot,
            target_pos: aim,
        });
    }
    picks
}

/// Turns the gun in `slot` toward `aim`, within its arc and at its traverse
/// rate; returns whether it bears. A gun swings through its arc's middle,
/// never through its blind quarter.
fn traverse(
    state: &mut State,
    id: UnitId,
    slot: usize,
    mount: &MountStats,
    rate: u8,
    pivot: Vec2Fx,
    aim: Vec2Fx,
) -> bool {
    let unit = state.unit_mut(id).expect("caller checked");
    let heading = unit.heading;
    let mounts = unit.mounts.as_mut().expect("a mounted hull");
    let current = mount.arc_offset(mounts.bearings[slot]);
    let wanted = mount.arc_offset(relative(pivot, aim, heading));
    let step = (wanted - current).clamp(-i16::from(rate), i16::from(rate));
    let next = current + step;
    let next_byte = i8::try_from(next).expect("an arc offset stays inside the compass");
    mounts.bearings[slot] = mount
        .arc_centre
        .cast_unsigned()
        .wrapping_add_signed(next_byte);
    (wanted - next).abs() <= 2
}

/// An attack: the hull closes until one of its guns reaches the target,
/// then turns nose-on and holds station so both guns bear, following a
/// target that moves. A target under the rear of the hull, which no turn
/// brings into an arc, sends the hull forward a couple of tiles first.
pub(super) fn attack(
    state: &mut State,
    id: UnitId,
    view: AttackView,
    resume: Option<Resume>,
    events: &mut Vec<Event>,
) {
    let unit = state.unit(id).expect("caller checked");
    let (pos, heading, kind, tile) = (unit.pos, unit.heading, unit.kind, unit.tile());
    let stats = kind.stats();
    let guns = stats.mounts.expect("a mounted hull");
    if !stats.can_target(view.domain) {
        complete(state, id, resume);
        return;
    }
    let excess = view
        .entity
        .map_or(Fx::ZERO, |target| target_excess(state, target));
    let reaches = guns
        .mounts
        .iter()
        .zip(stats.weapons)
        .any(|(mount, weapon)| {
            let pivot = mount.pivot(pos, heading);
            let aim = view.aim_from(pivot);
            in_reach(weapon, pivot, aim, excess) && clear(state, pivot, aim)
        });
    let centre_aim = view.aim_from(pos);
    if reaches && feedable(guns, pos, centre_aim) {
        let unit = state.unit_mut(id).expect("caller checked");
        unit.path = None;
        super::super::super::movement::steer_bearing(
            &mut unit.heading,
            centre_aim - pos,
            stats.cruise_turn_rate,
        );
        return;
    }
    let goal = if reaches {
        TilePos::containing(pos + chassis::compass::dir(heading) * Fx::from_num(2))
    } else {
        TilePos::containing(view.position)
    };
    if state
        .unit(id)
        .expect("caller checked")
        .path
        .as_ref()
        .is_some_and(|path| path.goal == goal)
    {
        return;
    }
    match route_for(state, kind, tile, goal) {
        Some(waypoints) => {
            state.unit_mut(id).expect("caller checked").path = Some(PathFollow {
                final_point: None,
                goal,
                waypoints,
                next: 0,
            });
        }
        None => stall_attack(state, id, resume, StallReason::NoRoute, events),
    }
}

/// An attack-move: the hull flies as on a move until an enemy comes within
/// some gun's reach that a turn could bring into an arc, then stops and
/// faces it until nothing is left in reach, and flies on.
pub(in crate::tick::brain) fn hunt(
    state: &mut State,
    index: &super::super::super::spatial::UnitIndex,
    reach: &mut super::super::super::reach::Reach,
    id: UnitId,
    sight: &Sight,
    events: &mut Vec<Event>,
) {
    let unit = state.unit(id).expect("caller checked");
    let (pos, heading) = (unit.pos, unit.heading);
    let stats = unit.kind.stats();
    let guns = stats.mounts.expect("a mounted hull");
    let lead = sight
        .ranked(state, pos)
        .into_iter()
        .filter(|(candidate, centre_aim)| {
            feedable(guns, pos, *centre_aim)
                && guns
                    .mounts
                    .iter()
                    .zip(stats.weapons)
                    .any(|(mount, weapon)| {
                        let pivot = mount.pivot(pos, heading);
                        let aim = aim_point(state, candidate.target, pivot);
                        in_reach(weapon, pivot, aim, candidate.excess) && clear(state, pivot, aim)
                    })
        })
        .map(|(_, centre_aim)| centre_aim)
        .next();
    match lead {
        Some(aim) => {
            let unit = state.unit_mut(id).expect("caller checked");
            unit.path = None;
            super::super::super::movement::steer_bearing(
                &mut unit.heading,
                aim - pos,
                stats.cruise_turn_rate,
            );
        }
        None => walk(state, index, reach, id, events),
    }
}

/// An idle hull turns only to feed a gun that has nothing to shoot: when an
/// enemy within that gun's reach and line sits in its blind quarter, the
/// hull yaws toward the gun's blind side until the gun bears, never turning
/// away from what another gun is firing on. It then keeps the facing it
/// turned to for a while, so a crowd cannot keep it spinning. Any other
/// order frees the facing.
pub(in crate::tick::brain) fn face_idle(
    state: &mut State,
    id: UnitId,
    sight: &Sight,
    picks: [Option<Pick>; crate::stats::MAX_WEAPONS],
) {
    let unit = state.unit(id).expect("caller checked");
    let (pos, heading, order) = (unit.pos, unit.heading, unit.order);
    let stats = unit.kind.stats();
    let guns = stats.mounts.expect("a mounted hull");
    let facing = unit.mounts.expect("a mounted hull").facing;
    let set = |state: &mut State, facing: IdleFacing| {
        state
            .unit_mut(id)
            .expect("caller checked")
            .mounts
            .as_mut()
            .expect("a mounted hull")
            .facing = facing;
    };
    if !matches!(order, Order::Idle) {
        set(state, IdleFacing::Free);
        return;
    }
    if let IdleFacing::Holding { ticks } = facing {
        set(
            state,
            core::num::NonZeroU8::new(ticks.get() - 1)
                .map_or(IdleFacing::Free, |ticks| IdleFacing::Holding { ticks }),
        );
        return;
    }
    let starved = guns
        .mounts
        .iter()
        .zip(stats.weapons)
        .enumerate()
        .find(|(slot, (mount, weapon))| {
            picks[*slot].is_none() && {
                let pivot = mount.pivot(pos, heading);
                sight.ranked(state, pivot).iter().any(|(candidate, aim)| {
                    !mount.covers(relative(pivot, *aim, heading))
                        && in_reach(weapon, pivot, *aim, candidate.excess)
                        && feedable(guns, pos, aim_point(state, candidate.target, pos))
                        && clear(state, pivot, *aim)
                })
            }
        })
        .map(|(_, (mount, _))| mount);
    let hold = || IdleFacing::Holding {
        ticks: core::num::NonZeroU8::new(guns.facing_hold).expect("a mounted hull holds"),
    };
    let Some(mount) = starved else {
        set(
            state,
            if facing == IdleFacing::Turning {
                hold()
            } else {
                IdleFacing::Free
            },
        );
        return;
    };
    // The blind quarter lies behind the far wing, so a gun's target there is
    // brought round by turning toward that wing.
    let rate = stats.cruise_turn_rate.cast_signed();
    let step = if mount.arc_centre < 0 { rate } else { -rate };
    let next = heading.wrapping_add_signed(step);
    let keeps_picks = guns.mounts.iter().zip(picks).all(|(mount, pick)| {
        pick.is_none_or(|pick| {
            let pivot = mount.pivot(pos, next);
            mount.covers(relative(pivot, pick.aim, next))
        })
    });
    if keeps_picks {
        state.unit_mut(id).expect("caller checked").heading = next;
        set(state, IdleFacing::Turning);
    } else {
        set(
            state,
            if facing == IdleFacing::Turning {
                hold()
            } else {
                IdleFacing::Free
            },
        );
    }
}

fn complete(state: &mut State, id: UnitId, resume: Option<Resume>) {
    state
        .unit_mut(id)
        .expect("caller checked")
        .complete_attack(resume);
}

#[cfg(test)]
mod tests;
