//! Firing decisions use observations; only impact resolution reads occupants.

use super::*;
use crate::state::AttackView;
use crate::{AttackTarget, BuildingId};

pub(in crate::tick::brain) struct ShotBuffers<'a> {
    pub(in crate::tick::brain) events: &'a mut Vec<Event>,
    pub(in crate::tick::brain) hits: &'a mut Vec<PendingHit>,
    pub(in crate::tick::brain) launches: &'a mut Vec<crate::state::Shell>,
}

pub(super) fn solution(
    state: &State,
    from: Vec2Fx,
    shooter: Domain,
    view: AttackView,
    weapon: &WeaponStats,
) -> bool {
    let aim = view.aim_from(from);
    weapon.targets.covers(view.domain)
        && within_weapon_reach(weapon, from.dist_sq(aim), Fx::ZERO)
        && clear_line(
            state,
            Fire::of(weapon),
            ShotEnd::new(from, shooter),
            ShotEnd::new(aim, view.domain).on(view.footing()),
            Landing::Checked,
        )
}

pub(super) fn radar_in_range(
    state: &State,
    player: PlayerId,
    from: Vec2Fx,
    domain: Domain,
    weapon: &WeaponStats,
) -> Option<AttackView> {
    // `solution` and the ranking never read velocity, so every track is
    // judged on its still view and only the chosen one is estimated.
    let (chosen, _) = state
        .vision(player)
        .tracks()
        .iter()
        .filter(|track| track.visible_unit.is_none())
        .map(|track| (track.id, AttackView::still_blip(track)))
        .filter(|(_, view)| solution(state, from, domain, *view, weapon))
        .min_by_key(|(id, view)| (from.dist_sq(view.position), *id))?;
    state.attack_view(player, AttackTarget::Contact(chosen))
}

fn predicted_aim(
    state: &State,
    from: Vec2Fx,
    domain: Domain,
    view: AttackView,
    weapon: &WeaponStats,
) -> Vec2Fx {
    let current = view.aim_from(from);
    let Some(projectile) = weapon.projectile.filter(|_| view.footprint.is_none()) else {
        return current;
    };
    let mut aim = current;
    let mut flight = shell_flight(from, aim, projectile.speed);
    for _ in 0..8 {
        let predicted =
            current + view.velocity * Fx::from_num(flight.min(crate::stats::MAX_LEAD_TICKS));
        let next = weapon.splash.map_or(predicted, |radius| {
            aim_at_near_splash_edge(current, predicted, radius)
        });
        if from.dist_sq(next) < weapon.minimum_range * weapon.minimum_range {
            return current;
        }
        aim = if from.dist_sq(next) > weapon.range * weapon.range {
            from.move_toward(next, weapon.range)
        } else {
            next
        };
        let next_flight = shell_flight(from, aim, projectile.speed);
        if next_flight == flight {
            break;
        }
        flight = next_flight;
    }
    if clear_line(
        state,
        Fire::of(weapon),
        ShotEnd::new(from, domain),
        ShotEnd::new(aim, view.domain),
        Landing::Checked,
    ) {
        aim
    } else {
        current
    }
}

pub(super) fn buffer_blind(
    state: &State,
    shooter: (Target, PlayerId),
    from: Vec2Fx,
    aim: Vec2Fx,
    view: AttackView,
    weapon: &WeaponStats,
    hits: &mut Vec<PendingHit>,
) {
    let (shooter, player) = shooter;
    let building = || {
        state
            .buildings()
            .iter()
            .filter(|b| {
                weapon.targets.ground
                    && b.hp > 0
                    && !b.provisional()
                    && state.hostile(player, b.player)
                    && b.closest_point_to(aim).dist_sq(aim) <= const { Fx::lit("0.0001") }
            })
            .min_by_key(|b| b.id)
            .map(|b| Target::Building(b.id))
    };
    let direct = if view.footprint.is_some() {
        building()
    } else {
        let tile = TilePos::containing(view.position);
        state
            .units()
            .iter()
            .filter(|u| {
                u.hp > 0
                    && state.hostile(player, u.player)
                    && weapon.targets.covers(u.domain())
                    && u.tile() == tile
            })
            .min_by_key(|u| (u.pos.dist_sq(tile.center()), u.id))
            .map(|u| Target::Unit(u.id))
            // A radar blip can be a building's footprint tile.
            .or_else(building)
    };
    if let Some(victim) = direct {
        buffer_shot(state, shooter, victim, from, aim, weapon, hits);
    } else if let Some(radius) = weapon.splash {
        let footing = view
            .footprint
            .map_or(TilePos::containing(aim), |(anchor, _)| anchor);
        for u in state.units().iter().filter(|u| {
            u.hp > 0
                && state.hostile(player, u.player)
                && weapon.targets.covers(u.domain())
                && u.pos.dist_sq(aim) <= radius * radius
                && blast_catches(state, aim, footing, u)
        }) {
            hits.push(PendingHit::along(
                state,
                shooter,
                Target::Unit(u.id),
                weapon.damage,
                from,
                aim,
            ));
        }
        for b in state.buildings().iter().filter(|b| {
            b.hp > 0
                && !b.provisional()
                && state.hostile(player, b.player)
                && weapon.targets.ground
                && b.kind.is_stealthy()
                && b.center().dist_sq(aim) <= radius * radius
                && blast_catches_charge(state, aim, footing, b)
        }) {
            hits.push(PendingHit::along(
                state,
                shooter,
                Target::Building(b.id),
                weapon.damage,
                from,
                aim,
            ));
        }
    }
}

/// Turns the defense `id` toward a known place it cannot see into and fires
/// there once its gun bears and it has reloaded.
pub(super) fn fire_building(
    state: &mut State,
    id: BuildingId,
    view: AttackView,
    events: &mut Vec<Event>,
    hits: &mut Vec<PendingHit>,
    launches: &mut Vec<crate::state::Shell>,
) {
    let b = state.building(id).expect("live defense");
    let (from, weapon) = (b.center(), b.stats().weapons[0]);
    let aim = predicted_aim(state, from, Domain::Ground, view, &weapon);
    if !aim_gun(state, id, aim) || state.building(id).expect("live defense").cooldown > 0 {
        return;
    }
    fire_shot(
        state,
        Shot {
            gunner: Gunner::Defense(id),
            from,
            aim,
            mark: Mark::Blind(view),
        },
        ShotBuffers {
            events,
            hits,
            launches,
        },
    );
}

fn fire_unit(
    state: &mut State,
    id: UnitId,
    view: AttackView,
    primary: usize,
    moving: bool,
    buffers: ShotBuffers<'_>,
) {
    let u = state.unit(id).expect("live shooter");
    let (from, kind) = (u.pos, u.kind);
    let weapon = &kind.stats().weapons[primary];
    let aim = predicted_aim(state, from, kind.stats().domain, view, weapon);
    let u = state.unit_mut(id).expect("live shooter");
    if !moving {
        u.path = None;
    }
    if moving && kind.stats().turret_turn_rate == 0 {
        if !super::super::super::movement::ground_weapon_aligned(u, aim - from) {
            return;
        }
    } else if !super::super::super::movement::steer_weapon_heading(u, aim - from) {
        return;
    }
    if u.cooldowns[primary] > 0 {
        return;
    }
    fire_shot(
        state,
        Shot {
            gunner: Gunner::Unit { id, slot: primary },
            from,
            aim,
            mark: Mark::Blind(view),
        },
        buffers,
    );
}

/// Fires an automatic unit's weapon at a radar contact in reach when it has
/// no ordinary target. `acquired` receives the ordinary acquisition whenever
/// this judged one, so a brain later in the same tick need not repeat it.
#[allow(clippy::too_many_arguments)]
#[expect(
    clippy::option_option,
    reason = "`None` means no ordinary search ran; `Some(None)` means it found nothing"
)]
pub(in crate::tick::brain) fn automatic_radar(
    state: &mut State,
    index: &super::super::super::spatial::UnitIndex,
    id: UnitId,
    moving: bool,
    events: &mut Vec<Event>,
    hits: &mut Vec<PendingHit>,
    launches: &mut Vec<crate::state::Shell>,
    acquired: &mut Option<Option<Target>>,
) -> bool {
    let u = state.unit(id).expect("live unit");
    let stats = u.kind.stats();
    if u.kind.stats().contact_reach.is_some()
        || stats.mounts.is_some()
        || stats.turn_rate > 0
        || stats.weapons.is_empty()
        || (moving && stats.brace.is_some())
        || {
            let target = acquire_target(state, index, id);
            *acquired = Some(target);
            target.is_some()
        }
    {
        return false;
    }
    let Some((slot, view)) = stats.weapons.iter().enumerate().find_map(|(slot, weapon)| {
        radar_in_range(state, u.player, u.pos, stats.domain, weapon).map(|view| (slot, view))
    }) else {
        return false;
    };
    fire_unit(
        state,
        id,
        view,
        slot,
        moving,
        ShotBuffers {
            events,
            hits,
            launches,
        },
    );
    fire_sidearms(state, index, id, slot, hits, events);
    true
}

pub(super) fn sidearm_radar(
    state: &mut State,
    id: UnitId,
    slot: usize,
    hits: &mut Vec<PendingHit>,
    events: &mut Vec<Event>,
) {
    let unit = state.unit(id).expect("live unit");
    let (player, pos, kind) = (unit.player, unit.pos, unit.kind);
    let weapon = &kind.stats().weapons[slot];
    let Some(view) = radar_in_range(state, player, pos, unit.domain(), weapon) else {
        return;
    };
    if !super::super::super::movement::ground_weapon_aligned(unit, view.position - pos) {
        return;
    }
    fire_direct(
        state,
        Shot {
            gunner: Gunner::Unit { id, slot },
            from: pos,
            aim: view.position,
            mark: Mark::Blind(view),
        },
        events,
        hits,
    );
}

fn complete(state: &mut State, id: UnitId, resume: Option<Resume>) {
    state
        .unit_mut(id)
        .expect("live unit")
        .complete_attack(resume);
}

/// Runs the `Attack` order of the unit `id`.
pub(in crate::tick::brain) fn attack_known(
    state: &mut State,
    index: &super::super::super::spatial::UnitIndex,
    reach: &mut super::super::super::reach::Reach,
    motion: &MotionSnapshot,
    id: UnitId,
    buffers: ShotBuffers<'_>,
) {
    let Order::Attack {
        target,
        resume,
        pursue,
    } = state.unit(id).expect("live unit").order
    else {
        return;
    };
    let ShotBuffers {
        events,
        hits,
        launches,
    } = buffers;
    let u = state.unit(id).expect("live unit");
    let (player, pos, tile, kind) = (u.player, u.pos, u.tile(), u.kind);
    let Some(target) = state.attack_objective(player, target) else {
        complete(state, id, resume);
        return;
    };
    let Some(view) = state.attack_view(player, target) else {
        complete(state, id, resume);
        return;
    };
    // Escort duty bounds the fight by the escorted unit. A victim within
    // the break radius of it is chased wherever the escort is. One past it is
    // fought only from where the escort stands, in its own reach, on a level
    // its weapon may fire at, and while the escort is within the break radius
    // itself. An escorted unit that has left play sends the escort back to
    // its slot too.
    if let Some(Resume::Accompany(escorted)) = resume {
        let bound = crate::stats::ESCORT_BREAK_RADIUS * crate::stats::ESCORT_BREAK_RADIUS;
        let aim = view.aim_from(pos);
        let shooter = ShotEnd::new(pos, kind.stats().domain);
        let victim = ShotEnd::new(aim, view.domain).on(view.footing());
        let in_reach = kind.stats().weapons.iter().any(|weapon| {
            weapon.targets.covers(view.domain)
                && within_weapon_reach(
                    weapon,
                    pos.dist_sq(aim),
                    view.entity
                        .map_or(Fx::ZERO, |target| target_excess(state, target)),
                )
                && levels_allow(state, Fire::of(weapon), shooter, victim)
        });
        if state.escorted(escorted).is_none_or(|body| {
            view.position.dist_sq(body.pos) > bound && (!in_reach || pos.dist_sq(body.pos) > bound)
        }) {
            complete(state, id, resume);
            return;
        }
    }
    state.unit_mut(id).expect("live unit").order = Order::Attack {
        target,
        resume,
        pursue,
    };
    // Mounted guns fire after the order, at whatever bears; the order only
    // flies and turns the hull.
    if kind.stats().mounts.is_some() {
        super::mounts::attack(state, id, view, resume, events);
        return;
    }
    // A victim the unit chose itself gives way to a seen one once only radar
    // holds it, to any other once the tick's volley already kills it, and
    // to a clearly better one at each look. A breacher keeps to its
    // structures.
    let switch = if pursue {
        None
    } else if view.is_blip() {
        repick(state, index, reach, id, resume, hits)
    } else if let Some(current) = view.entity
        && kind.stats().target_priority != crate::stats::TargetPriority::Buildings
    {
        if volley_kills(state, hits, current) {
            repick(state, index, reach, id, resume, hits).filter(|&next| next != current)
        } else if state
            .current_tick()
            .is_multiple_of(crate::stats::TARGET_RECHECK_TICKS)
        {
            repick(state, index, reach, id, resume, hits)
                .filter(|&next| next != current && outranks(state, id, next, current))
        } else {
            None
        }
    } else {
        None
    };
    if let Some(next) = switch {
        state.unit_mut(id).expect("live unit").order = Order::Attack {
            target: next.into(),
            resume,
            pursue: false,
        };
        attack(
            state, index, motion, id, next, resume, events, hits, launches,
        );
        normalize_order(state, id);
        return;
    }
    if let Some(entity) = view.entity {
        // A breacher strikes a machine it found itself only while in
        // contact, and lets it go rather than follow it.
        if !pursue
            && matches!(entity, Target::Unit(_))
            && kind.stats().target_priority == crate::stats::TargetPriority::Buildings
            && !in_weapon_reach_from(state, kind, pos, entity)
        {
            complete(state, id, resume);
            return;
        }
        attack(
            state, index, motion, id, entity, resume, events, hits, launches,
        );
        normalize_order(state, id);
        return;
    }
    let stats = kind.stats();
    let primary = stats
        .weapons
        .iter()
        .position(|weapon| weapon.targets.covers(view.domain));
    if let Some(primary) = primary {
        let weapon = &stats.weapons[primary];
        if kind.stats().contact_reach.is_none() && solution(state, pos, stats.domain, view, weapon)
        {
            // Turn-limited bombers obtain their own sight before release range.
            if stats.turn_rate == 0 {
                // Only an order fires on a building nobody sees. One destroyed
                // out of sight leaves its memory standing, and an automatic
                // engagement shelling that memory would never move on.
                if !pursue && matches!(target, AttackTarget::RememberedBuilding(_)) {
                    complete(state, id, resume);
                    return;
                }
                fire_unit(
                    state,
                    id,
                    view,
                    primary,
                    false,
                    ShotBuffers {
                        events: &mut *events,
                        hits: &mut *hits,
                        launches: &mut *launches,
                    },
                );
                fire_sidearms(state, index, id, primary, hits, events);
                return;
            }
        }
    } else if stats.demolition.is_none() {
        complete(state, id, resume);
        return;
    }
    if !pursue && view.footprint.is_none() {
        complete(state, id, resume);
        return;
    }
    if let Some(primary) = primary {
        fire_sidearms(state, index, id, primary, hits, events);
    }
    state.unit_mut(id).expect("live unit").retract_braces();
    let aim = view.aim_from(pos);
    let goal = TilePos::containing(view.position);
    let inside_dead_zone = primary.is_some_and(|i| {
        pos.dist_sq(aim) < stats.weapons[i].minimum_range * stats.weapons[i].minimum_range
    });
    if inside_dead_zone {
        if !route_to_firing_stand(state, id, view, &stats.weapons[primary.expect("weapon")]) {
            stall_attack(state, id, resume, StallReason::NoRoute, events);
        }
        return;
    }
    let routed = if let Some((anchor, size)) = view.footprint
        && stats.domain == Domain::Ground
    {
        approach_rect(state, id, anchor, size)
    } else if state
        .unit(id)
        .expect("live unit")
        .path
        .as_ref()
        .is_some_and(|p| p.goal == goal)
    {
        true
    } else if let Some(waypoints) = route_for(state, kind, tile, goal) {
        state.unit_mut(id).expect("live unit").path = Some(PathFollow {
            final_point: None,
            goal,
            waypoints,
            next: 0,
            clearing: false,
        });
        true
    } else {
        false
    };
    if !routed
        && !primary
            .is_some_and(|primary| route_to_firing_stand(state, id, view, &stats.weapons[primary]))
    {
        stall_attack(state, id, resume, StallReason::NoRoute, events);
    }
}

fn route_to_firing_stand(
    state: &mut State,
    id: UnitId,
    view: AttackView,
    weapon: &WeaponStats,
) -> bool {
    let domain = state.unit(id).expect("live unit").kind.stats().domain;
    let legal = |state: &State, tile: TilePos| {
        state.passable_for(domain, tile) && solution(state, tile.center(), domain, view, weapon)
    };
    let footprint = view
        .footprint
        .unwrap_or((TilePos::containing(view.position), (1, 1)));
    let reach = weapon.range.ceil().to_num::<i32>();
    route_to_stand(state, id, footprint, reach, legal)
}

pub(in crate::tick::brain) fn normalize_order(state: &mut State, id: UnitId) {
    let u = state.unit(id).expect("live unit");
    if let Order::Attack {
        target,
        resume,
        pursue,
    } = u.order
        && let Some(target) = state.attack_objective(u.player, target)
    {
        state.unit_mut(id).expect("live unit").order = Order::Attack {
            target,
            resume,
            pursue,
        };
    }
}

#[cfg(test)]
mod tests;
