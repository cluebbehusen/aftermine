//! The fighting half of the brain: target acquisition, the weapons
//! matrix (direct, indirect, projectile, sidearms), turret fire, and
//! the retaliation contract. Every shot buffers into the tick's volley;
//! nothing here applies damage directly.

mod blind;
mod drones;
mod mounts;
use super::contact;
pub(super) use blind::{ShotBuffers, attack_known, automatic_radar, normalize_order};
pub(super) use drones::{Fleets, alert_hosts, fly as fly_drone, watch as watch_drones};
pub(super) use mounts::{
    Sight as MountSight, face_idle as face_mounted_idle, fire as fire_mounts, hunt as mounted_hunt,
};

use super::super::fire::{Fire, Landing, ShotEnd, blast_reaches, clear_line, levels_allow};
use super::super::flight;
use super::super::landing;
use super::super::route_for;
use super::PendingHit;
use super::locomotion::{approach_rect, walk};
use crate::event::{Event, StallReason, UnitLaunchPose};
use crate::ids::{PlayerId, Target, UnitId};
use crate::state::{Motor, Order, PathFollow, Resume, State};
use crate::stats::{Domain, WeaponStats};
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;

/// The movement domain a target occupies (buildings sit on the ground).
fn target_domain(state: &State, target: Target) -> Domain {
    match target {
        Target::Unit(uid) => state
            .unit(uid)
            .map_or(Domain::Ground, crate::state::Unit::domain),
        Target::Building(_) => Domain::Ground,
    }
}

/// The bearing of `aim` from `from` against `heading`: the frame an exact
/// distance tie between victims goes round in, which a half turn of the
/// field preserves.
fn bearing_from(from: Vec2Fx, heading: u8, aim: Vec2Fx) -> u8 {
    let toward = aim - from;
    if toward == Vec2Fx::ZERO {
        0
    } else {
        chassis::compass::heading_of(toward).wrapping_sub(heading)
    }
}

/// Whether `weapon` reaches a target `distance_sq` from it whose hull
/// reaches `excess` past a small body's, so a wide machine is hit at its
/// edge rather than only at its centre.
fn within_weapon_reach(weapon: &WeaponStats, distance_sq: chassis::fx::Fx, excess: Fx) -> bool {
    let range = weapon.range + excess;
    distance_sq <= range * range && distance_sq >= weapon.minimum_range * weapon.minimum_range
}

/// Whether a hit on `victim` counts as a hit on a structure: a building, or
/// a machine built like one.
fn struck_as_structure(state: &State, victim: Target) -> bool {
    match victim {
        Target::Building(_) => true,
        Target::Unit(id) => state.unit(id).is_some_and(|u| u.kind.stats().structural),
    }
}

/// Damage one hit of `weapon` deals `victim`: its structure damage against
/// a structure, where it has one.
fn damage_against(state: &State, weapon: &WeaponStats, victim: Target) -> u32 {
    match weapon.building_damage {
        Some(damage) if struck_as_structure(state, victim) => damage,
        _ => weapon.damage,
    }
}

/// How far past a small body's the hull of `target` reaches: zero for a
/// building and for every machine up to the Breaker's radius.
fn target_excess(state: &State, target: Target) -> Fx {
    match target {
        Target::Unit(id) => state.unit(id).map_or(Fx::ZERO, |u| {
            crate::stats::hull_excess(u.kind.stats().radius)
        }),
        Target::Building(_) => Fx::ZERO,
    }
}

/// How far `kind`'s `weapon` reaches `target`, measured to its centre: to
/// a wide hull's edge, and for a contact weapon no further than the two
/// hulls plus its contact reach. `None` is a target known only by where
/// it is, with no hull to measure.
pub(in crate::tick) fn unit_weapon_reach(
    state: &State,
    kind: crate::UnitKind,
    target: Option<Target>,
    weapon: &WeaponStats,
) -> Fx {
    let excess = target.map_or(Fx::ZERO, |target| target_excess(state, target));
    let Some(reach) = kind.stats().contact_reach else {
        return kind.stats().reach(weapon) + excess;
    };
    let target_radius = match target {
        Some(Target::Unit(id)) => state.unit(id).map_or(Fx::ZERO, |u| u.kind.stats().radius),
        Some(Target::Building(_)) | None => Fx::ZERO,
    };
    (weapon.range + excess).min(kind.stats().radius + target_radius + reach)
}

fn within_unit_weapon_reach(
    state: &State,
    kind: crate::UnitKind,
    target: Target,
    weapon: &WeaponStats,
    distance_sq: Fx,
) -> bool {
    let range = unit_weapon_reach(state, kind, Some(target), weapon);
    distance_sq <= range * range && distance_sq >= weapon.minimum_range * weapon.minimum_range
}

/// Flight length in ticks (at least one) for a shell from `from` to `aim`.
/// A projectile flies to one fixed fire-time aim point and resolves against
/// whatever stands there then; predictive artillery chooses that point
/// before launch, and the flight is never guided.
fn shell_flight(from: Vec2Fx, aim: Vec2Fx, speed: Fx) -> u64 {
    (from.dist(aim) / speed).ceil().to_num::<u64>().max(1)
}

/// One tick-boundary sample of the motion a visible body is already showing.
///
/// The sample is captured before any unit brain runs. That keeps artillery
/// independent of the alternating brain order. Ground samples use retained
/// motor speed and heading, including pathless coasting; air samples keep the
/// current steering line. Neither reads an opponent's later A* turns.
pub(super) struct MotionSnapshot {
    velocities: Vec<(UnitId, Vec2Fx)>,
}

impl MotionSnapshot {
    pub(super) fn capture(state: &State) -> Self {
        let velocities = state
            .units
            .iter()
            .filter_map(|unit| {
                if unit.kind.stats().domain == Domain::Ground {
                    return (unit.drive_speed() > Fx::ZERO).then_some((
                        unit.id,
                        chassis::compass::dir(unit.heading) * unit.drive_speed(),
                    ));
                }
                let path = unit.path.as_ref()?;
                let next = path.waypoints.get(path.next as usize)?.center();
                let delta = next - unit.pos;
                let distance = delta.length();
                (distance > Fx::ZERO)
                    .then(|| (unit.id, delta * (unit.kind.stats().speed / distance)))
            })
            .collect();
        Self { velocities }
    }

    fn position_after(&self, target: UnitId, current: Vec2Fx, ticks: u64) -> Option<Vec2Fx> {
        let slot = self
            .velocities
            .binary_search_by_key(&target, |(id, _)| *id)
            .ok()?;
        Some(
            current
                + self.velocities[slot].1 * Fx::from_num(ticks.min(crate::stats::MAX_LEAD_TICKS)),
        )
    }
}

fn position_along_current_motion(
    motion: &MotionSnapshot,
    target: UnitId,
    current: Vec2Fx,
    ticks: u64,
) -> Option<Vec2Fx> {
    motion.position_after(target, current, ticks)
}

fn aim_at_near_splash_edge(current: Vec2Fx, predicted: Vec2Fx, radius: Fx) -> Vec2Fx {
    predicted.move_toward(current, radius)
}

fn visible_hostile_cluster_at(
    state: &State,
    owner: PlayerId,
    current: Vec2Fx,
    weapon: &WeaponStats,
) -> bool {
    let Some(radius) = weapon.splash else {
        return false;
    };
    let radius_sq = radius * radius;
    state
        .units
        .iter()
        .filter(|unit| {
            unit.hp > 0
                && state.hostile(owner, unit.player)
                && weapon.targets.covers(unit.domain())
                && state.sees_unit(owner, unit)
                && unit.pos.dist_sq(current) <= radius_sq
        })
        .take(2)
        .count()
        >= 2
}

#[derive(Clone, Copy)]
struct ProjectileShooter {
    owner: PlayerId,
    domain: Domain,
}

/// Leads a moving unit using its tick-boundary motion sample for the shell's
/// estimated flight. For splash shells, the aim backs toward the current
/// position by one blast radius: a straight commitment remains just inside
/// the footprint, while stopping or turning gets a margin for error. If the
/// current footprint already contains a second visible eligible hostile, the
/// gun keeps that known cluster instead of leading away from it. Later path
/// turns remain private, and the target remains free to change course after
/// launch; shells are still unguided. Buildings keep their closest-footprint
/// aim unchanged.
fn projectile_aim(
    state: &State,
    motion: &MotionSnapshot,
    shooter: ProjectileShooter,
    from: Vec2Fx,
    target: Target,
    current: Vec2Fx,
    weapon: &WeaponStats,
) -> Vec2Fx {
    let Target::Unit(target) = target else {
        return current;
    };
    if visible_hostile_cluster_at(state, shooter.owner, current, weapon) {
        return current;
    }
    let speed = weapon
        .projectile
        .expect("only projectile weapons lead their aim")
        .speed;
    let mut flight = shell_flight(from, current, speed);
    let mut aim = current;
    let mut unhedged_aim = current;
    // Ground artillery can only lead ground units, all slower than a shell.
    // Eight fixed iterations settle even the fastest ground body; the
    // projection itself stays bounded if a balance change breaks that.
    for _ in 0..8 {
        let Some(predicted) = position_along_current_motion(motion, target, current, flight) else {
            break;
        };
        let next_aim = weapon.splash.map_or(predicted, |radius| {
            aim_at_near_splash_edge(current, predicted, radius)
        });
        let distance_sq = from.dist_sq(next_aim);
        if distance_sq < weapon.minimum_range * weapon.minimum_range {
            // There is no legal predicted impact inside a weapon's dead zone.
            // Keep the target's current, already-validated aim instead of
            // inventing a radial boundary point the target never occupies.
            aim = current;
            unhedged_aim = current;
            break;
        }
        unhedged_aim = if from.dist_sq(predicted) > weapon.range * weapon.range {
            from.move_toward(predicted, weapon.range)
        } else {
            predicted
        };
        aim = if distance_sq > weapon.range * weapon.range {
            from.move_toward(next_aim, weapon.range)
        } else {
            next_aim
        };
        let next_flight = shell_flight(from, aim, speed);
        if next_flight == flight {
            break;
        }
        flight = next_flight;
    }
    let victim = target_domain(state, Target::Unit(target));
    let lands = |at: Vec2Fx| {
        clear_line(
            state,
            Fire::of(weapon),
            ShotEnd::new(from, shooter.domain),
            ShotEnd::new(at, victim),
            Landing::Checked,
        )
    };
    if lands(unhedged_aim) && lands(aim) {
        aim
    } else {
        current
    }
}

fn launch_shell(
    state: &State,
    launches: &mut Vec<crate::state::Shell>,
    attacker: Target,
    attacker_owner: PlayerId,
    from: Vec2Fx,
    aim: Vec2Fx,
    weapon: &WeaponStats,
) -> u64 {
    let projectile = weapon.projectile.expect("only projectile weapons launch");
    let flight = shell_flight(from, aim, projectile.speed);
    launches.push(crate::state::Shell {
        kind: projectile.payload,
        shooter: attacker,
        player: attacker_owner,
        launch: from,
        impact: aim,
        launched_at: state.tick,
        arrival: state.tick + flight,
        damage: weapon.damage,
        targets: weapon.targets,
        splash: weapon.splash,
    });
    flight
}

/// The gun a shot leaves: a defense's, or the one in a unit's weapon slot.
#[derive(Debug, Clone, Copy)]
enum Gunner {
    Defense(crate::ids::BuildingId),
    Unit { id: UnitId, slot: usize },
}

impl Gunner {
    fn source(self) -> Target {
        match self {
            Self::Defense(id) => Target::Building(id),
            Self::Unit { id, .. } => Target::Unit(id),
        }
    }

    fn owner(self, state: &State) -> PlayerId {
        match self {
            Self::Defense(id) => state.building(id).expect("a firing gun is live").player,
            Self::Unit { id, .. } => state.unit(id).expect("a firing gun is live").player,
        }
    }

    fn weapon(self, state: &State) -> &'static WeaponStats {
        match self {
            Self::Defense(id) => {
                &state
                    .building(id)
                    .expect("a firing gun is live")
                    .stats()
                    .weapons[0]
            }
            Self::Unit { id, slot } => {
                &state
                    .unit(id)
                    .expect("a firing gun is live")
                    .kind
                    .stats()
                    .weapons[slot]
            }
        }
    }

    fn start_reload(self, state: &mut State) {
        let ticks = self.weapon(state).cooldown_ticks;
        match self {
            Self::Defense(id) => {
                state
                    .building_mut(id)
                    .expect("a firing gun is live")
                    .cooldown = ticks;
            }
            Self::Unit { id, slot } => {
                state.unit_mut(id).expect("a firing gun is live").cooldowns[slot] = ticks;
            }
        }
    }
}

/// What a shot is fired on: a seen victim, or a known place it cannot see into.
#[derive(Debug, Clone, Copy)]
enum Mark {
    Seen(Target),
    Blind(crate::state::AttackView),
}

impl Mark {
    fn target(self) -> Option<Target> {
        match self {
            Self::Seen(target) => Some(target),
            Self::Blind(_) => None,
        }
    }
}

/// One shot from a gun that bears and has reloaded.
#[derive(Debug, Clone, Copy)]
struct Shot {
    gunner: Gunner,
    from: Vec2Fx,
    aim: Vec2Fx,
    mark: Mark,
}

/// Fires `shot` as its weapon does: a shell if it launches one, else a direct
/// hit in this tick's volley.
fn fire_shot(state: &mut State, shot: Shot, buffers: ShotBuffers<'_>) {
    let ShotBuffers {
        events,
        hits,
        launches,
    } = buffers;
    if shot.gunner.weapon(state).projectile.is_some() {
        fire_shell(state, shot, events, launches);
    } else {
        fire_direct(state, shot, events, hits);
    }
}

fn fire_shell(
    state: &mut State,
    shot: Shot,
    events: &mut Vec<Event>,
    launches: &mut Vec<crate::state::Shell>,
) {
    let Shot {
        gunner,
        from,
        aim,
        mark,
    } = shot;
    let (shooter, player, weapon) = (gunner.source(), gunner.owner(state), gunner.weapon(state));
    gunner.start_reload(state);
    let flight = launch_shell(state, launches, shooter, player, from, aim, weapon);
    let unit_pose = match gunner {
        Gunner::Defense(_) => None,
        Gunner::Unit { id, .. } => Some(UnitLaunchPose::from(
            state.unit(id).expect("a firing gun is live"),
        )),
    };
    events.push(Event::ShellLaunched {
        shooter,
        unit_pose,
        target: mark.target(),
        player,
        from,
        to: aim,
        flight,
    });
}

/// Buffers `shot` as a direct hit, whatever its weapon: a sidearm, a mounted
/// gun and a drone never launch.
fn fire_direct(state: &mut State, shot: Shot, events: &mut Vec<Event>, hits: &mut Vec<PendingHit>) {
    let Shot {
        gunner,
        from,
        aim,
        mark,
    } = shot;
    let (shooter, player, weapon) = (gunner.source(), gunner.owner(state), gunner.weapon(state));
    gunner.start_reload(state);
    match mark {
        Mark::Seen(victim) => buffer_shot(state, shooter, victim, from, aim, weapon, hits),
        Mark::Blind(view) => {
            blind::buffer_blind(state, (shooter, player), from, aim, view, weapon, hits);
        }
    }
    events.push(match gunner {
        Gunner::Defense(id) => {
            let b = state.building(id).expect("a firing gun is live");
            Event::TurretFired {
                turret: id,
                kind: b.kind,
                tier: b.tier,
                target: mark.target(),
                turret_pos: from,
                target_pos: aim,
            }
        }
        Gunner::Unit { id, slot } => Event::AttackHit {
            attacker: id,
            attacker_kind: state.unit(id).expect("a firing gun is live").kind,
            weapon: slot,
            target: mark.target(),
            attacker_pos: from,
            target_pos: aim,
        },
    });
}

/// Arrived shells join this tick's volley, computed against the same
/// start-of-tick world every buffered shot uses. The direct hit lands on
/// the hostile building whose footprint touches the impact point; units
/// take splash only. No fire gate here: the gate cleared at launch.
pub(super) fn land_shells(state: &mut State, hits: &mut Vec<PendingHit>, events: &mut Vec<Event>) {
    let now = state.tick;
    let mut due = Vec::new();
    state.shells.retain(|shell| {
        if shell.arrival <= now {
            due.push(shell.clone());
            false
        } else {
            true
        }
    });
    for shell in due {
        events.push(Event::ShellLanded {
            player: shell.player,
            targets: shell.targets,
            at: shell.impact,
            splash: shell.splash,
        });
        // The direct hit is distance-zero to a footprint, not tile
        // containment: a shell aimed at a building lands on the footprint's
        // closest edge point, whose exact coordinate can floor into the
        // neighboring tile. A scaffold above a buried charge takes the
        // direct hit; the charge still takes splash. Other ties go to the
        // lowest id.
        let direct = state
            .buildings
            .iter()
            .filter(|b| {
                b.hp > 0
                    && !b.provisional()
                    && shell.targets.ground
                    && state.hostile(shell.player, b.player)
                    && b.closest_point_to(shell.impact).dist_sq(shell.impact)
                        <= const { chassis::fx::Fx::lit("0.0001") }
            })
            .min_by_key(|b| (b.kind.is_stealthy() && b.built(), b.id));
        if let Some(b) = direct {
            hits.push(PendingHit::along(
                state,
                shell.shooter,
                Target::Building(b.id),
                shell.damage,
                shell.launch,
                shell.impact,
            ));
        }
        let Some(radius) = shell.splash else { continue };
        let radius_sq = radius * radius;
        // A blast spreads along the level it lands on, never up or down a
        // cliff, and a direct hit's blast stands on the building.
        let footing = direct.map_or_else(|| landing_tile(shell.launch, shell.impact), |b| b.anchor);
        // Buried charges take splash, as in `buffer_shot`.
        if shell.targets.ground {
            for b in &state.buildings {
                if b.hp == 0
                    || b.provisional()
                    || !b.kind.is_stealthy()
                    || !state.hostile(shell.player, b.player)
                    || direct.is_some_and(|d| d.id == b.id)
                    || b.center().dist_sq(shell.impact) > radius_sq
                    || !blast_catches_charge(state, shell.impact, footing, b)
                {
                    continue;
                }
                hits.push(PendingHit::along(
                    state,
                    shell.shooter,
                    Target::Building(b.id),
                    shell.damage,
                    shell.launch,
                    shell.impact,
                ));
            }
        }
        for u in &state.units {
            let reach = radius + crate::stats::hull_excess(u.kind.stats().radius);
            if u.hp == 0
                || !state.hostile(shell.player, u.player)
                || !shell.targets.covers(u.domain())
                || u.pos.dist_sq(shell.impact) > reach * reach
                || !blast_catches(state, shell.impact, footing, u)
            {
                continue;
            }
            hits.push(PendingHit::along(
                state,
                shell.shooter,
                Target::Unit(u.id),
                shell.damage,
                shell.launch,
                shell.impact,
            ));
        }
    }
}

/// The tile a shell's blast stands on when no building takes the direct
/// hit. A shell aimed at a building lands on the footprint's nearest edge,
/// whose coordinate can floor into the tile beyond; reading the point a hair
/// further along the flight keeps the blast on the footprint's side, where
/// it was aimed, even once the building is gone.
fn landing_tile(launch: Vec2Fx, impact: Vec2Fx) -> TilePos {
    let along = |from: chassis::fx::Fx, to: chassis::fx::Fx| match to.cmp(&from) {
        std::cmp::Ordering::Greater => chassis::fx::Fx::DELTA,
        std::cmp::Ordering::Less => -chassis::fx::Fx::DELTA,
        std::cmp::Ordering::Equal => chassis::fx::Fx::ZERO,
    };
    TilePos::containing(Vec2Fx::new(
        impact.x + along(launch.x, impact.x),
        impact.y + along(launch.y, impact.y),
    ))
}

/// Whether a blast bursting at `at` on `footing` catches `u`.
pub(super) fn blast_catches(
    state: &State,
    at: Vec2Fx,
    footing: TilePos,
    u: &crate::state::Unit,
) -> bool {
    blast_reaches(&state.map, at, footing, ShotEnd::new(u.pos, u.domain()))
}

/// Whether a blast bursting at `at` on `footing` catches the buried charge
/// `b`, measured to its center.
pub(super) fn blast_catches_charge(
    state: &State,
    at: Vec2Fx,
    footing: TilePos,
    b: &crate::state::Building,
) -> bool {
    blast_reaches(
        &state.map,
        at,
        footing,
        ShotEnd::new(b.center(), Domain::Ground).on(b.anchor),
    )
}

/// A machine a wide cutter's front goes over: a ground body of a radius
/// under this.
const CRUSHED_RADIUS: Fx = Fx::lit("0.5");

/// Damage a wide cutter's front deals each tick to every small hostile
/// machine under it: a Sentinel lasts about a second.
const CRUSH_DAMAGE: u32 = 25;

/// How far past touching a small machine still counts as under the front.
const CRUSH_SLACK: Fx = Fx::lit("0.1");

/// A wide cutter crushes every small hostile ground machine touching the
/// front half of its hull, whatever its order. As with splash, no sight
/// gate applies: the hull goes over whatever stands before it.
pub(super) fn crush(
    state: &State,
    index: &super::super::spatial::UnitIndex,
    id: UnitId,
    hits: &mut Vec<PendingHit>,
    events: &mut Vec<Event>,
) {
    let Some(cutter) = state
        .unit(id)
        .filter(|u| u.hp > 0 && contact::square_cutter(u.kind))
    else {
        return;
    };
    let forward = chassis::compass::dir(cutter.heading);
    let radius = cutter.kind.stats().radius;
    let reach = (radius + CRUSHED_RADIUS + CRUSH_SLACK)
        .floor()
        .to_num::<i32>()
        + 1;
    let home = cutter.tile();
    for dy in -reach..=reach {
        for &(_, slot) in index.row_span(home.y + dy, home.x - reach, home.x + reach) {
            let u = &state.units[slot];
            let offset = u.pos - cutter.pos;
            let small = u.kind.stats().radius;
            let touch = radius + small + CRUSH_SLACK;
            if u.hp == 0
                || small >= CRUSHED_RADIUS
                || u.domain() != Domain::Ground
                || !state.hostile(cutter.player, u.player)
                || offset.x * forward.x + offset.y * forward.y <= Fx::ZERO
                || offset.length_sq() > touch * touch
            {
                continue;
            }
            let point = cutter.pos + offset * (radius / (radius + small));
            let victim = Target::Unit(u.id);
            hits.push(PendingHit::along(
                state,
                Target::Unit(id),
                victim,
                CRUSH_DAMAGE,
                cutter.pos,
                point,
            ));
            events.push(Event::AttackHit {
                attacker: id,
                attacker_kind: cutter.kind,
                weapon: 0,
                target: Some(victim),
                attacker_pos: cutter.pos,
                target_pos: point,
            });
        }
    }
}

/// A wide cutter's stroke also cuts every other hostile building its drum
/// presses flat against, each at full structure damage. As with splash, no
/// sight gate applies: the drum cuts whatever it presses on.
fn cut_alongside(
    state: &State,
    id: UnitId,
    struck: Target,
    weapon_index: usize,
    hits: &mut Vec<PendingHit>,
    events: &mut Vec<Event>,
) {
    let unit = state.unit(id).expect("a cutter that just struck");
    let weapon = &unit.kind.stats().weapons[weapon_index];
    let span = contact::drum_span(unit.kind);
    for b in state.buildings() {
        let victim = Target::Building(b.id);
        if victim == struck
            || !contact::cuts(state, unit.player, b)
            || unit.pos.dist_sq(b.closest_point_to(unit.pos)) > span * span
        {
            continue;
        }
        let Some((point, _)) = contact::drum_contact(state, unit, b) else {
            continue;
        };
        hits.push(PendingHit::along(
            state,
            Target::Unit(id),
            victim,
            damage_against(state, weapon, victim),
            unit.pos,
            point,
        ));
        events.push(Event::AttackHit {
            attacker: id,
            attacker_kind: unit.kind,
            weapon: weapon_index,
            target: Some(victim),
            attacker_pos: unit.pos,
            target_pos: point,
        });
    }
}

/// Buffers a shot: the direct hit, plus, for splash weapons, one hit on
/// every other hostile unit inside the radius that the weapon can cover and
/// on hostile buried charges. Other buildings only take the direct hit.
///
/// Splash skips the owner-sight fire gate the aimed paths enforce: the gate
/// governs choosing a victim, and splash hits whatever stands in the blast.
/// No information leaks: a bystander's owner learns only that its own body
/// took damage, and retaliation stays gated on the victim seeing the
/// shooter.
fn buffer_shot(
    state: &State,
    attacker: Target,
    victim: Target,
    from: Vec2Fx,
    aim: Vec2Fx,
    weapon: &WeaponStats,
    hits: &mut Vec<PendingHit>,
) {
    let attacker_owner = match attacker {
        Target::Unit(id) => state.unit(id).map(|unit| unit.player),
        Target::Building(id) => state.building(id).map(|building| building.player),
    }
    .expect("a buffered shot has a live source");
    hits.push(PendingHit::along(
        state,
        attacker,
        victim,
        damage_against(state, weapon, victim),
        from,
        aim,
    ));
    let Some(radius) = weapon.splash else { return };
    // The blast stands where the victim does (see `land_shells`).
    let footing = match victim {
        Target::Building(id) => state
            .building(id)
            .map_or(TilePos::containing(aim), |b| b.anchor),
        Target::Unit(_) => TilePos::containing(aim),
    };
    for u in &state.units {
        // A wide hull is caught once the blast reaches its edge.
        let reach = radius + crate::stats::hull_excess(u.kind.stats().radius);
        if u.hp == 0
            || !state.hostile(attacker_owner, u.player)
            || Target::Unit(u.id) == victim
            || !weapon.targets.covers(u.domain())
            || u.pos.dist_sq(aim) > reach * reach
            || !blast_catches(state, aim, footing, u)
        {
            continue;
        }
        hits.push(PendingHit::along(
            state,
            attacker,
            Target::Unit(u.id),
            weapon.damage,
            from,
            aim,
        ));
    }
    // The one building exception to direct-hits-only: a buried charge takes
    // splash, detected or not, so saturation fire can clear a minefield.
    if weapon.targets.ground {
        for b in &state.buildings {
            if b.hp == 0
                || b.provisional()
                || !b.kind.is_stealthy()
                || !state.hostile(attacker_owner, b.player)
                || Target::Building(b.id) == victim
                || b.center().dist_sq(aim) > radius * radius
                || !blast_catches_charge(state, aim, footing, b)
            {
                continue;
            }
            hits.push(PendingHit::along(
                state,
                attacker,
                Target::Building(b.id),
                weapon.damage,
                from,
                aim,
            ));
        }
    }
}

/// A defense's gun as its target choice reads it.
#[derive(Debug, Clone, Copy)]
struct Gun {
    center: Vec2Fx,
    heading: u8,
    traverse: u8,
    /// Ticks of reload left after this one.
    reload: u32,
}

impl Gun {
    /// Ticks after this one before the gun could fire at `aim`: the later of
    /// its reload and its turn onto it.
    fn ready(self, aim: Vec2Fx) -> u32 {
        use super::super::movement::{longest_bear, ticks_to_bear};
        // Early in a long reload the gun can bear anywhere before it has
        // reloaded, so the turn need not be measured.
        if self.reload >= longest_bear(self.traverse) {
            return self.reload;
        }
        ticks_to_bear(self.heading, aim - self.center, self.traverse).max(self.reload)
    }
}

/// The candidate with the least `key` among those that pass `test`: the
/// pick filtering first would make, but the costly test runs only on a
/// candidate that would beat the best so far.
fn least_passing<T, K: Ord>(
    candidates: impl Iterator<Item = T>,
    key: impl Fn(&T) -> K,
    test: impl Fn(&T) -> bool,
) -> Option<T> {
    let mut best: Option<(K, T)> = None;
    for candidate in candidates {
        let rank = key(&candidate);
        if best.as_ref().is_none_or(|(least, _)| rank < *least) && test(&candidate) {
            best = Some((rank, candidate));
        }
    }
    best.map(|(_, candidate)| candidate)
}

/// Turns the gun of the defense `id` toward `aim` at its tier's traverse;
/// returns whether it now bears.
fn aim_gun(state: &mut State, id: crate::ids::BuildingId, aim: Vec2Fx) -> bool {
    let b = state.building_mut(id).expect("live defense");
    let (center, traverse) = (b.center(), b.stats().traverse);
    let heading = b
        .gun_heading
        .as_mut()
        .expect("a building that fires carries a gun");
    super::super::movement::steer_bearing(heading, aim - center, traverse)
}

/// Built defenses pick their own fights every tick, in building-id order.
/// A focus the gun can fire on comes first. Otherwise the gun takes the
/// seen enemy unit in range with a clear line that it can fire on soonest,
/// counting both its reload and its turn, the nearest breaking a tie; a
/// ground-capable defense with none falls back to apparent hostile buildings
/// ranked the same way, and then to a radar contact. Out-of-line targets are
/// ignored until they move. The gun turns toward its pick and fires once it
/// bears and has reloaded; with no pick it holds its bearing.
///
/// Right after a shot, every target the gun can reach within its reload
/// ties, so the nearest wins. The pick it turns toward only gets sooner, so
/// the gun never swings back and forth between two targets that stand still.
pub(super) fn turret_fire(
    state: &mut State,
    index: &super::super::spatial::UnitIndex,
    motion: &MotionSnapshot,
    events: &mut Vec<Event>,
    hits: &mut Vec<PendingHit>,
    launches: &mut Vec<crate::state::Shell>,
) {
    let ids: Vec<crate::ids::BuildingId> = state.buildings.iter().map(|b| b.id).collect();
    let cutters: Vec<UnitId> = state
        .units()
        .iter()
        .filter(|u| u.hp > 0 && contact::square_cutter(u.kind))
        .map(|u| u.id)
        .collect();
    for id in ids {
        let Some(b) = state.building(id) else {
            continue;
        };
        let Some(atk) = b.stats().weapons.first() else {
            continue;
        };
        if !b.built() || b.hp == 0 {
            continue;
        }
        // A hostile drum pressed on a building jams its guns: it holds its
        // fire and bearing, though its reload still runs down.
        if cutters.iter().any(|&cutter| {
            let cutter = state.unit(cutter).expect("listed this tick");
            state.hostile(cutter.player, b.player) && contact::jams(state, cutter, b)
        }) {
            let b = state.building_mut(id).expect("just seen");
            b.cooldown = b.cooldown.saturating_sub(1);
            continue;
        }
        let (me, center, focus) = (b.player, b.center(), b.focus);
        let (heading, traverse) = (
            b.gun_heading.expect("a building that fires carries a gun"),
            b.stats().traverse,
        );
        let focused = focus
            .and_then(|target| state.attack_view(me, target))
            .filter(|view| atk.targets.covers(view.domain));
        let b = state.building_mut(id).expect("just seen");
        if focus.is_some() && focused.is_none() {
            b.focus = None;
        }
        b.cooldown = b.cooldown.saturating_sub(1);
        let gun = Gun {
            center,
            heading,
            traverse,
            reload: b.cooldown,
        };
        let focused =
            focused.filter(|view| blind::solution(state, center, Domain::Ground, *view, atk));
        if let Some(view) = focused.filter(|view| view.entity.is_none()) {
            blind::fire_building(state, id, view, events, hits, launches);
            continue;
        }
        let focused_victim =
            focused.and_then(|view| view.entity.map(|target| (target, view.aim_from(center))));
        // A unit in range stands on a tile within `reach` of the turret's;
        // a building in range covers one, or the next where its closest
        // point sits on an edge. Positions hold still through the brain
        // phase, so the phase's index and survey still describe them.
        let home = TilePos::containing(center);
        let reach = atk.range.floor().to_num::<i32>() + 1 + index.wide_reach();
        let hostiles = index.hostiles_near(state.player(me).team, home, reach + 1);
        let unit_victim = (focused_victim.is_none() && hostiles.bodies)
            .then(|| {
                let in_reach = (home.y - reach..=home.y + reach)
                    .flat_map(|y| index.row_span(y, home.x - reach, home.x + reach))
                    .map(|&(_, slot)| &state.units[slot])
                    .filter(|u| {
                        state.hostile(me, u.player) && u.hp > 0 && atk.targets.covers(u.domain())
                    })
                    .filter(|u| state.sees_unit(me, u))
                    .filter(|u| {
                        let excess = crate::stats::hull_excess(u.kind.stats().radius);
                        within_weapon_reach(atk, center.dist_sq(u.pos), excess)
                    })
                    .map(|u| (center.dist_sq(u.pos), u.id, u.pos, u.domain()));
                least_passing(
                    in_reach,
                    |&(d, uid, pos, _)| {
                        (
                            gun.ready(pos),
                            d,
                            bearing_from(center, gun.heading, pos),
                            uid,
                        )
                    },
                    |(_, _, pos, dom)| {
                        clear_line(
                            state,
                            Fire::of(atk),
                            ShotEnd::new(center, Domain::Ground),
                            ShotEnd::new(*pos, *dom),
                            Landing::Unchecked,
                        )
                    },
                )
            })
            .flatten();
        let building_victim = (focused_victim.is_none()
            && unit_victim.is_none()
            && atk.targets.covers(Domain::Ground))
        .then(|| {
            let candidates = if hostiles.buildings {
                state.buildings()
            } else {
                index.buildings_since_survey(state.buildings())
            };
            building_victim(state, me, gun, atk, candidates)
        })
        .flatten();
        let (victim, aim) = if let Some(focused) = focused_victim {
            focused
        } else if let Some((_, uid, position, _)) = unit_victim {
            (Target::Unit(uid), position)
        } else if let Some((target, position)) = building_victim {
            (Target::Building(target), position)
        } else {
            if let Some(view) = blind::radar_in_range(state, me, center, Domain::Ground, atk) {
                blind::fire_building(state, id, view, events, hits, launches);
            }
            continue;
        };
        let aim = if atk.projectile.is_some() {
            projectile_aim(
                state,
                motion,
                ProjectileShooter {
                    owner: me,
                    domain: Domain::Ground,
                },
                center,
                victim,
                aim,
                atk,
            )
        } else {
            aim
        };
        if !aim_gun(state, id, aim) || gun.reload > 0 {
            continue;
        }
        fire_shot(
            state,
            Shot {
                gunner: Gunner::Defense(id),
                from: center,
                aim,
                mark: Mark::Seen(victim),
            },
            ShotBuffers {
                events,
                hits,
                launches,
            },
        );
    }
}

/// The hostile building among `candidates` that `gun` can strike with `atk`
/// soonest, the nearest breaking a tie: one in reach and in true sight with
/// a clear line to its closest point, and its aim point. Every test is pure,
/// so the cheapest runs first.
fn building_victim(
    state: &State,
    me: PlayerId,
    gun: Gun,
    atk: &WeaponStats,
    candidates: &[crate::Building],
) -> Option<(crate::ids::BuildingId, Vec2Fx)> {
    let center = gun.center;
    let in_reach = candidates
        .iter()
        .map(|target| {
            let aim = target.closest_point_to(center);
            (center.dist_sq(aim), target, aim)
        })
        .filter(|(distance, ..)| within_weapon_reach(atk, *distance, Fx::ZERO))
        .filter(|(_, target, _)| state.sees_hostile_building(me, target));
    least_passing(
        in_reach,
        |(distance, target, aim)| {
            (
                gun.ready(*aim),
                *distance,
                bearing_from(center, gun.heading, *aim),
                target.id,
            )
        },
        |(_, target, aim)| {
            clear_line(
                state,
                Fire::of(atk),
                ShotEnd::new(center, Domain::Ground),
                ShotEnd::new(*aim, Domain::Ground).on(target.anchor),
                Landing::Checked,
            )
        },
    )
    .map(|(_, target, aim)| (target.id, aim))
}

/// Firing positions for a chaser around an unstandable victim tile,
/// ring-scanned outward, keeping only tiles the chaser can stand on and
/// whose centres lie within the weapon's Euclidean reach of `aim`, the
/// victim's own position (ring corners sit √2 further out than their
/// Chebyshev radius). Reach runs from the body, not its tile's centre: a
/// body overhanging a footprint's empty margin stands within a short
/// weapon's reach of the tile beside it. Within a ring the scan is
/// row-major, which a half-turn does not preserve, so a caller that picks
/// one must rank them. Empty when the victim sits deeper in blocked ground
/// than any weapon reaches.
fn chase_stand_ins(
    state: &State,
    domain: Domain,
    around: TilePos,
    aim: Vec2Fx,
    range: chassis::fx::Fx,
) -> Vec<TilePos> {
    let mut out = Vec::new();
    for r in 1..=crate::stats::CHASE_STAND_RADIUS {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let t = around.offset(dx, dy);
                if state.passable_for(domain, t) && t.center().dist_sq(aim) <= range * range {
                    out.push(t);
                }
            }
        }
    }
    out
}

/// Routes a mobile artillery unit out of its own dead zone to the nearest
/// reachable tile from which its primary weapon has a clear, legal shot.
/// Keeping a still-valid goal avoids rebuilding the route every brain tick.
fn retreat_to_firing_stand(
    state: &mut State,
    id: UnitId,
    target: Target,
    victim_domain: Domain,
    weapon: &WeaponStats,
) -> bool {
    let domain = state.unit(id).expect("caller checked").kind.stats().domain;
    let legal = |state: &State, tile: TilePos| {
        if !state.passable_for(domain, tile) {
            return false;
        }
        let position = tile.center();
        let aim = match target {
            Target::Unit(target) => state
                .unit(target)
                .filter(|unit| unit.hp > 0)
                .map(|unit| ShotEnd::new(unit.pos, victim_domain)),
            Target::Building(target) => state
                .building(target)
                .filter(|building| building.hp > 0)
                .map(|building| {
                    ShotEnd::new(building.closest_point_to(position), victim_domain)
                        .on(building.anchor)
                }),
        };
        let Some(victim) = aim else { return false };
        let aim = victim.at;
        let distance_sq = position.dist_sq(aim);
        if !within_weapon_reach(weapon, distance_sq, target_excess(state, target)) {
            return false;
        }
        clear_line(
            state,
            Fire::of(weapon),
            ShotEnd::new(position, domain),
            victim,
            Landing::Checked,
        )
    };

    let footprint = match target {
        Target::Unit(target) => (state.unit(target).expect("resolved target").tile(), (1, 1)),
        Target::Building(target) => {
            let building = state.building(target).expect("resolved target");
            (building.anchor, building.kind.size())
        }
    };
    let reach = weapon.range.ceil().to_num::<i32>();
    route_to_stand(state, id, footprint, reach, legal)
}

/// Routes a unit to the nearest tile within `reach` tiles of a footprint
/// that `legal` accepts and a route reaches, keeping a path whose goal is
/// still legal. Equally near stands rank in the unit's approach frame around
/// the footprint, so mirrored units take mirrored stands. Clears the path
/// and returns `false` when no stand routes.
fn route_to_stand(
    state: &mut State,
    id: UnitId,
    (anchor, size): (TilePos, (i32, i32)),
    reach: i32,
    legal: impl Fn(&State, TilePos) -> bool,
) -> bool {
    let unit = state.unit(id).expect("caller checked");
    if unit
        .path
        .as_ref()
        .is_some_and(|path| legal(state, path.goal))
    {
        return true;
    }
    let (from, from_tile, kind, player) = (unit.pos, unit.tile(), unit.kind, unit.player);
    let frame = crate::tick::rect_approach_origin(state, player, from_tile, anchor, size);
    let (width, height) = size;
    let mut candidates = Vec::new();
    for y in
        (anchor.y - reach).max(0)..=(anchor.y + height - 1 + reach).min(state.map().height() - 1)
    {
        for x in
            (anchor.x - reach).max(0)..=(anchor.x + width - 1 + reach).min(state.map().width() - 1)
        {
            let tile = TilePos::new(x, y);
            if legal(state, tile) {
                candidates.push(tile);
            }
        }
    }
    candidates.sort_unstable_by_key(|&tile| {
        (
            from.dist_sq(tile.center()),
            crate::geometry::rect_approach_key_from(from_tile, frame, anchor, size, tile),
        )
    });
    let path = candidates.into_iter().find_map(|goal| {
        route_for(state, kind, from_tile, goal).map(|waypoints| PathFollow {
            final_point: None,
            goal,
            waypoints,
            next: 0,
        })
    });
    let found = path.is_some();
    state.unit_mut(id).expect("caller checked").path = path;
    found
}

/// The nearest enemy this unit's weapons can cover, in its autonomous
/// acquisition range: units before buildings, ties to the lowest id.
/// `None` for unarmed units, empty horizons, and everything outside the
/// weapon masks.
///
/// Unit candidates come from the phase's spatial `index`: everything within
/// that range of `pos` stands within `floor(range) + 1` tiles Chebyshev of
/// its tile (the extra tile covers both bodies' sub-tile offsets), and the
/// pick is a `min` over a complete [`Pick`], so window visit order cannot
/// move it.
pub(super) fn acquire_target(
    state: &State,
    index: &super::super::spatial::UnitIndex,
    id: UnitId,
) -> Option<Target> {
    let pos = state.unit(id).expect("caller checked").pos;
    acquire_target_from(state, index, id, pos)
}

/// [`acquire_target`] for a unit that sets out after what it picks, passing
/// over every victim the `volley` buffered this tick already kills. A
/// contact weapon passes over a structure it has no reachable stance to
/// meet: it could only set out, find no stance, give the fight up, and pick
/// the same structure again the next tick.
pub(super) fn acquire_reachable_target(
    state: &State,
    index: &super::super::spatial::UnitIndex,
    reach: &mut super::super::reach::Reach,
    id: UnitId,
    volley: &[PendingHit],
) -> Option<Target> {
    let unit = state.unit(id).expect("caller checked");
    let contact = unit.kind.stats().contact_reach.is_some();
    acquire_target_within(state, index, id, unit.pos, None, |target| {
        !volley_kills(state, volley, target)
            && match target {
                Target::Building(b) if contact => state.building(b).is_some_and(|b| {
                    !contact::passes(unit, b)
                        && reach.reaches(state, id, contact::stance_goals(state, unit, b))
                }),
                _ => true,
            }
    })
}

/// What the unit `id`, fighting for its `resume` program, would pick afresh:
/// the pick that program makes, passing over every victim the `volley`
/// already kills.
fn repick(
    state: &State,
    index: &super::super::spatial::UnitIndex,
    reach: &mut super::super::reach::Reach,
    id: UnitId,
    resume: Option<Resume>,
    volley: &[PendingHit],
) -> Option<Target> {
    match resume {
        Some(Resume::Accompany(escorted)) => {
            let center = state.escorted(escorted)?.pos;
            acquire_escort_target(state, index, reach, id, center, volley)
        }
        _ => acquire_reachable_target(state, index, reach, id, volley),
    }
}

/// The same pick judged as if the unit stood at `pos`: what a landing
/// aircraft would find in reach once parked on its tile, not what it
/// happens to pass on the way there.
pub(super) fn acquire_target_from(
    state: &State,
    index: &super::super::spatial::UnitIndex,
    id: UnitId,
    pos: chassis::fx::Vec2Fx,
) -> Option<Target> {
    acquire_target_within(state, index, id, pos, None, |_| true)
}

/// An escort's pick. It chases the threat nearest the followed body at
/// `center`, within [`crate::stats::ESCORT_ENGAGE_RADIUS`] of it and the
/// escort's own aggro range, passing over a victim it can neither shoot from
/// where it stands nor reach: that chase could only stall, return to the
/// slot, and pick the victim again every tick. Failing that, an escort
/// within the engage radius of the body fires without moving at an enemy
/// already in its own weapon reach, so a long gun escorts at full range from
/// its slot. Further out it is catching up: such a fight would be broken off
/// at the break radius before it closed. Like [`acquire_reachable_target`],
/// it passes over every victim the `volley` already kills.
pub(super) fn acquire_escort_target(
    state: &State,
    index: &super::super::spatial::UnitIndex,
    reach: &mut super::super::reach::Reach,
    id: UnitId,
    center: chassis::fx::Vec2Fx,
    volley: &[PendingHit],
) -> Option<Target> {
    let unit = state.unit(id).expect("caller checked");
    let (kind, pos, domain) = (unit.kind, unit.pos, unit.kind.stats().domain);
    let live = |target: Target| !volley_kills(state, volley, target);
    let in_reach_now =
        |target: Target| live(target) && in_weapon_reach_from(state, kind, pos, target);
    let cap = Some(crate::stats::ESCORT_ENGAGE_RADIUS);
    let near = acquire_target_within(state, index, id, center, cap, |target| {
        in_reach_now(target)
            || live(target)
                && match target {
                    Target::Unit(u) => state.unit(u).is_some_and(|victim| {
                        let tile = victim.tile();
                        if state.passable_for(domain, tile) {
                            return reach.reaches(state, id, [tile]);
                        }
                        // A victim over ground the escort cannot stand on is a
                        // fight only if a firing stand around it can be reached.
                        let Some(weapon) = kind
                            .stats()
                            .weapons
                            .iter()
                            .find(|weapon| weapon.targets.covers(victim.domain()))
                        else {
                            return false;
                        };
                        let range = unit_weapon_reach(state, kind, Some(target), weapon);
                        let stands = chase_stand_ins(state, domain, tile, victim.pos, range);
                        reach.reaches(state, id, stands)
                    }),
                    Target::Building(b) => state.building(b).is_some_and(|b| {
                        let (w, h) = b.kind.size();
                        let ring = (-1..=w).flat_map(move |dx| (-1..=h).map(move |dy| (dx, dy)));
                        reach.reaches(state, id, ring.map(|(dx, dy)| b.anchor.offset(dx, dy)))
                    }),
                }
    });
    let engage = crate::stats::ESCORT_ENGAGE_RADIUS;
    near.or_else(|| {
        (pos.dist_sq(center) <= engage * engage)
            .then(|| acquire_target_within(state, index, id, pos, None, in_reach_now))
            .flatten()
    })
}

/// Whether a `kind` machine standing at `pos` has a weapon that reaches
/// `target` now, on a level that weapon may fire at.
fn in_weapon_reach_from(state: &State, kind: crate::UnitKind, pos: Vec2Fx, target: Target) -> bool {
    let victim = target_domain(state, target);
    let aim = match target {
        Target::Unit(u) => state.unit(u).map(|u| ShotEnd::new(u.pos, victim)),
        Target::Building(b) => state
            .building(b)
            .map(|b| ShotEnd::new(b.closest_point_to(pos), victim).on(b.anchor)),
    };
    let shooter = ShotEnd::new(pos, kind.stats().domain);
    aim.is_some_and(|aim| {
        kind.stats().weapons.iter().any(|weapon| {
            weapon.targets.covers(victim)
                && within_unit_weapon_reach(state, kind, target, weapon, pos.dist_sq(aim.at))
                && levels_allow(state, Fire::of(weapon), shooter, aim)
        })
    })
}

/// How a unit ranks a victim it picks for itself, least first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Pick {
    /// The levels let no weapon fire on it from where the unit stands. Such
    /// a victim stays a pick, so a unit shelled from a plateau still climbs
    /// to answer.
    refused: bool,
    /// Machines come first.
    building: bool,
    /// It cannot hit the unit from where it stands.
    harmless: bool,
    /// How fast the unit's fire removes the danger a victim in weapon reach
    /// poses: its damage rate, times the unit's damage rate against it, over
    /// its health. Zero for the rest.
    worth: std::cmp::Reverse<u64>,
    /// A heavy-first gun's measure of a machine: its full health.
    heft: std::cmp::Reverse<u32>,
    distance: Fx,
    /// See [`bearing_from`]: from the unit's nose.
    bearing: u8,
    target: Target,
}

/// How `unit`, picking from `pos`, ranks the live `target` aimed at `aim`.
/// See [`crate::stats::TargetPriority`].
fn pick(
    state: &State,
    unit: &crate::state::Unit,
    pos: Vec2Fx,
    (target, aim): (Target, Vec2Fx),
    refused: bool,
) -> Pick {
    use crate::stats::TargetPriority;
    let stats = unit.kind.stats();
    let mut pick = Pick {
        refused,
        building: matches!(target, Target::Building(_)),
        harmless: false,
        worth: std::cmp::Reverse(0),
        heft: std::cmp::Reverse(0),
        distance: pos.dist_sq(aim),
        bearing: bearing_from(pos, unit.heading, aim),
        target,
    };
    match stats.target_priority {
        TargetPriority::Buildings => return pick,
        TargetPriority::Heaviest => {
            if let Target::Unit(victim) = target {
                pick.heft =
                    std::cmp::Reverse(state.unit(victim).map_or(0, |v| v.kind.stats().max_hp));
            }
            return pick;
        }
        TargetPriority::Danger => {}
    }
    let me = Target::Unit(unit.id);
    let domain = stats.domain;
    // Damage per thousand ticks.
    let rate = |weapon: &WeaponStats, damage: u32| {
        u64::from(damage) * 1_000 / u64::from(weapon.cooldown_ticks.max(1))
    };
    let (weapons, harmless, hp) = match target {
        Target::Unit(id) => {
            let victim = state.unit(id).expect("a ranked victim stands");
            let weapons = victim.kind.stats().weapons;
            let distance = victim.pos.dist_sq(pos);
            let harmless = !weapons.iter().any(|weapon| {
                weapon.targets.covers(domain)
                    && within_unit_weapon_reach(state, victim.kind, me, weapon, distance)
            });
            (weapons, harmless, victim.hp)
        }
        Target::Building(id) => {
            let victim = state.building(id).expect("a ranked victim stands");
            let weapons = if victim.built() {
                victim.stats().weapons
            } else {
                &[]
            };
            let distance = victim.center().dist_sq(pos);
            let excess = crate::stats::hull_excess(stats.radius);
            let harmless = !weapons.iter().any(|weapon| {
                weapon.targets.covers(domain) && within_weapon_reach(weapon, distance, excess)
            });
            (weapons, harmless, victim.hp)
        }
    };
    pick.harmless = harmless;
    let victim_domain = target_domain(state, target);
    if in_weapon_reach_from(state, unit.kind, pos, target)
        && let Some(weapon) = stats
            .weapons
            .iter()
            .find(|weapon| weapon.targets.covers(victim_domain))
    {
        // A victim that cannot hit the unit's kind still counts for half its
        // damage, as a danger to the unit's fellows.
        let against: u64 = weapons
            .iter()
            .filter(|weapon| weapon.targets.covers(domain))
            .map(|weapon| rate(weapon, damage_against(state, weapon, me)))
            .sum();
        let danger = if against > 0 {
            against
        } else {
            weapons
                .iter()
                .map(|weapon| rate(weapon, weapon.damage))
                .sum::<u64>()
                / 2
                + 1
        };
        let shot = rate(weapon, damage_against(state, weapon, target));
        let removed = u128::from(danger) * u128::from(shot) * 1_000 / u128::from(hp.max(1));
        pick.worth = std::cmp::Reverse(u64::try_from(removed).unwrap_or(u64::MAX).max(1));
    }
    pick
}

/// How the unit `id`, from where it stands, ranks a live `target`.
fn pick_of(state: &State, id: UnitId, target: Target) -> Option<Pick> {
    let unit = state.unit(id)?;
    let (aim, victim) = match target {
        Target::Unit(victim) => {
            let victim = state.unit(victim).filter(|victim| victim.hp > 0)?;
            (victim.pos, ShotEnd::new(victim.pos, victim.domain()))
        }
        Target::Building(victim) => {
            let victim = state.building(victim).filter(|victim| victim.hp > 0)?;
            let aim = victim.closest_point_to(unit.pos);
            (aim, ShotEnd::new(aim, Domain::Ground).on(victim.anchor))
        }
    };
    Some(pick(
        state,
        unit,
        unit.pos,
        (target, aim),
        levels_refuse(state, unit, victim),
    ))
}

/// Whether the levels let none of `unit`'s weapons fire on `victim` from
/// where it stands.
fn levels_refuse(state: &State, unit: &crate::state::Unit, victim: ShotEnd) -> bool {
    let stats = unit.kind.stats();
    let shooter = ShotEnd::new(unit.pos, stats.domain);
    state.map.tiered()
        && !stats.weapons.iter().any(|weapon| {
            weapon.targets.covers(victim.domain)
                && levels_allow(state, Fire::of(weapon), shooter, victim)
        })
}

/// Whether the unit `id`, fighting `current`, should take `next` instead:
/// a better group, or in the same group a victim in reach worth half again
/// as much, so equal victims never trade places.
fn outranks(state: &State, id: UnitId, next: Target, current: Target) -> bool {
    let (Some(next), Some(current)) = (pick_of(state, id, next), pick_of(state, id, current))
    else {
        return false;
    };
    let group = |pick: Pick| (pick.refused, pick.building, pick.harmless);
    if group(next) != group(current) {
        return group(next) < group(current);
    }
    let (std::cmp::Reverse(next), std::cmp::Reverse(current)) = (next.worth, current.worth);
    next > 0 && (current == 0 || u128::from(next) * 2 > u128::from(current) * 3)
}

/// Whether the hits this tick has buffered so far already kill `target`.
/// Shells in flight never count, so a miss cannot leave a victim standing.
pub(super) fn volley_kills(state: &State, hits: &[PendingHit], target: Target) -> bool {
    let hp = match target {
        Target::Unit(id) => state.unit(id).map_or(0, |unit| unit.hp),
        Target::Building(id) => state.building(id).map_or(0, |building| building.hp),
    };
    let mut damage = 0_u32;
    hits.iter().filter(|hit| hit.victim == target).any(|hit| {
        damage = damage.saturating_add(hit.damage);
        damage >= hp
    })
}

/// [`acquire_target_from`] around `pos` within the unit's aggro range,
/// capped at `cap`, skipping every candidate `accept` refuses.
fn acquire_target_within(
    state: &State,
    index: &super::super::spatial::UnitIndex,
    id: UnitId,
    pos: chassis::fx::Vec2Fx,
    cap: Option<chassis::fx::Fx>,
    mut accept: impl FnMut(Target) -> bool,
) -> Option<Target> {
    let unit = state.unit(id).expect("caller checked");
    let stats = unit.kind.stats();
    if !stats.can_fight() {
        return None;
    }
    let me = unit.player;
    let acquisition_range = cap.map_or(stats.aggro_range, |cap| stats.aggro_range.min(cap));
    // Every pick needs current team sight of the victim, even within the
    // unit's own vision radius: range is measured between positions while
    // sight covers whole tiles around the unit's tile, so a victim just past
    // the sight disc's edge can still be in range. An attack order cannot
    // keep an unseen victim, and the unit would re-acquire it every tick
    // without ever moving or firing.
    let aggro_sq = acquisition_range * acquisition_range;

    let home = TilePos::containing(pos);
    let reach = acquisition_range.floor().to_num::<i32>() + 1 + index.wide_reach();
    let team = state.player(me).team;
    let mut best: Option<Pick> = None;
    let hostiles = index.hostiles_near(team, home, reach + 1);
    // A breacher takes only structures: it never sets out after a machine,
    // and crushes the small ones it drives into instead. An army at rest far
    // from any enemy is the common case: the survey proves the window holds
    // no hostile body before any row is walked.
    if hostiles.bodies && stats.target_priority != crate::stats::TargetPriority::Buildings {
        for dy in -reach..=reach {
            for &(_, slot) in index.row_span(home.y + dy, home.x - reach, home.x + reach) {
                let u = &state.units[slot];
                if !state.hostile(me, u.player)
                    || u.hp == 0
                    || !stats.can_target(u.domain())
                    || !state.sees_unit(me, u)
                {
                    continue;
                }
                let d = pos.dist_sq(u.pos);
                let outside_dead_zone = stats.weapons.iter().any(|weapon| {
                    weapon.targets.covers(u.domain())
                        && d >= weapon.minimum_range * weapon.minimum_range
                });
                // A wide hull is in aggro range once its edge is.
                let aggro = acquisition_range + crate::stats::hull_excess(u.kind.stats().radius);
                if !(d <= aggro * aggro && outside_dead_zone) {
                    continue;
                }
                let target = Target::Unit(u.id);
                let refused = levels_refuse(state, unit, ShotEnd::new(u.pos, u.domain()));
                let rank = pick(state, unit, pos, (target, u.pos), refused);
                if best.is_some_and(|best| rank >= best) {
                    continue;
                }
                // A victim on ground the chaser cannot stand on needs a
                // firing position to exist, the same test the chase applies
                // one tick later. Without it the unit would acquire an order
                // it can only stall, clear it, and re-acquire every tick.
                let victim_tile = u.tile();
                if !state.passable_for(stats.domain, victim_tile) {
                    let range = stats
                        .weapons
                        .iter()
                        .find(|w| w.targets.covers(u.domain()))
                        .map_or(chassis::fx::Fx::ZERO, |w| {
                            unit_weapon_reach(state, unit.kind, Some(target), w)
                        });
                    if chase_stand_ins(state, stats.domain, victim_tile, u.pos, range).is_empty() {
                        continue;
                    }
                }
                if !accept(target) {
                    continue;
                }
                best = Some(rank);
            }
        }
    }
    // Every machine the levels allow outranks every building. A building in
    // acquisition range covers a tile within `reach` of the unit's tile, or
    // the next one where its closest point sits on an edge.
    if stats.can_target(Domain::Ground) && best.is_none_or(|best| best.refused) {
        let candidates = if hostiles.buildings {
            state.buildings()
        } else {
            index.buildings_since_survey(state.buildings())
        };
        for b in candidates {
            if !state.hostile(me, b.player) || b.hp == 0 {
                continue;
            }
            let aim = b.closest_point_to(pos);
            let d = pos.dist_sq(aim);
            if !(d <= aggro_sq
                && stats.weapons.iter().any(|weapon| {
                    weapon.targets.covers(Domain::Ground)
                        && d >= weapon.minimum_range * weapon.minimum_range
                }))
            {
                continue;
            }
            let target = Target::Building(b.id);
            let footing = ShotEnd::new(b.closest_point_to(unit.pos), Domain::Ground).on(b.anchor);
            let rank = pick(
                state,
                unit,
                pos,
                (target, aim),
                levels_refuse(state, unit, footing),
            );
            // Range and rank gates run before the knowledge gates:
            // apparency for a buried charge scans every friendly scout and
            // radar mast. The filters commute, so ordering only saves work.
            // An undetected buried charge must never be auto-attacked: that
            // would leak its existence to the attacker's owner.
            if best.is_some_and(|best| rank >= best)
                || !state.building_apparent(me, b)
                || !b.tiles().any(|tile| state.can_see(me, tile))
                || !accept(target)
            {
                continue;
            }
            best = Some(rank);
        }
    }
    best.map(|best| best.target)
}

/// Keeps an advance moving while the primary weapon takes a shot that is
/// already available. There is deliberately no acquisition transition:
/// the order and path remain `Advance`, blocked and out-of-range targets
/// are ignored, and retaliation does not recognize this stance. Units
/// therefore never spend movement chasing a pot-shot.
#[allow(clippy::too_many_arguments)]
pub(super) fn advance(
    state: &mut State,
    index: &super::super::spatial::UnitIndex,
    reach: &mut super::super::reach::Reach,
    motion: &MotionSnapshot,
    id: UnitId,
    goal: crate::state::Goal,
    events: &mut Vec<Event>,
    hits: &mut Vec<PendingHit>,
    launches: &mut Vec<crate::state::Shell>,
) {
    if super::locomotion::land_at_destination(state, index, reach, id, events) {
        return;
    }
    walk(state, index, reach, id, events);
    if !state.unit(id).is_some_and(
        |u| matches!(u.order, Order::Advance { goal: current } if current.tile() == goal.tile()),
    ) {
        return;
    }

    let unit = state.unit(id).expect("caller checked");
    let stats = unit.kind.stats();
    let Some(weapon) = stats.weapons.first().copied() else {
        return;
    };
    let cooldown = unit.cooldowns[0];
    // A braced gun fires only once its spades are down, never on the move.
    if stats.brace.is_some() {
        return;
    }
    if cooldown > 0 && stats.turret_turn_rate == 0 {
        return;
    }
    let (pos, home, me, kind) = (unit.pos, unit.tile(), unit.player, unit.kind);
    let reach = weapon.range.floor().to_num::<i32>() + 1 + index.wide_reach();

    let mut unit_target: Option<((chassis::fx::Fx, u8, UnitId), Vec2Fx)> = None;
    for dy in -reach..=reach {
        for &(_, slot) in index.row_span(home.y + dy, home.x - reach, home.x + reach) {
            let target = &state.units[slot];
            let domain = target.domain();
            if target.hp == 0
                || !state.hostile(me, target.player)
                || !weapon.targets.covers(domain)
                || !state.sees_unit(me, target)
                || !super::super::movement::advancing_weapon_aligned(unit, target.pos - pos)
            {
                continue;
            }
            let dist = pos.dist_sq(target.pos);
            if !within_unit_weapon_reach(state, kind, Target::Unit(target.id), &weapon, dist)
                || !clear_line(
                    state,
                    Fire::of(&weapon),
                    ShotEnd::new(pos, stats.domain),
                    ShotEnd::new(target.pos, domain),
                    Landing::Checked,
                )
            {
                continue;
            }
            let rank = (dist, bearing_from(pos, unit.heading, target.pos), target.id);
            if unit_target.is_none_or(|(best, _)| rank < best) {
                unit_target = Some((rank, target.pos));
            }
        }
    }

    let target = unit_target
        .map(|((_, _, uid), aim)| (Target::Unit(uid), aim))
        .or_else(|| {
            if !weapon.targets.covers(Domain::Ground) {
                return None;
            }
            state
                .buildings
                .iter()
                .filter(|b| b.hp > 0 && state.hostile(me, b.player))
                .filter(|b| b.tiles().any(|tile| state.can_see(me, tile)))
                .filter(|b| state.building_apparent(me, b))
                .map(|b| {
                    let aim = b.closest_point_to(pos);
                    (pos.dist_sq(aim), b.id, aim, b.anchor)
                })
                .filter(|(dist, bid, aim, anchor)| {
                    within_unit_weapon_reach(state, kind, Target::Building(*bid), &weapon, *dist)
                        && super::super::movement::advancing_weapon_aligned(unit, *aim - pos)
                        && clear_line(
                            state,
                            Fire::of(&weapon),
                            ShotEnd::new(pos, stats.domain),
                            ShotEnd::new(*aim, Domain::Ground).on(*anchor),
                            Landing::Checked,
                        )
                })
                .min_by_key(|(dist, bid, aim, _)| {
                    (*dist, bearing_from(pos, unit.heading, *aim), *bid)
                })
                .map(|(_, bid, aim, _)| (Target::Building(bid), aim))
        });
    let Some((target, aim)) = target else {
        automatic_radar(state, index, id, true, events, hits, launches, &mut None);
        return;
    };

    let projectile_aim = weapon.projectile.is_some().then(|| {
        projectile_aim(
            state,
            motion,
            ProjectileShooter {
                owner: me,
                domain: stats.domain,
            },
            pos,
            target,
            aim,
            &weapon,
        )
    });
    if !super::super::movement::advancing_weapon_aligned(unit, projectile_aim.unwrap_or(aim) - pos)
    {
        return;
    }
    if kind.stats().turret_turn_rate > 0
        && !super::super::movement::steer_weapon_heading(
            state.unit_mut(id).expect("caller checked"),
            projectile_aim.unwrap_or(aim) - pos,
        )
    {
        return;
    }
    if cooldown > 0 {
        return;
    }
    fire_shot(
        state,
        Shot {
            gunner: Gunner::Unit { id, slot: 0 },
            from: pos,
            aim: projectile_aim.unwrap_or(aim),
            mark: Mark::Seen(target),
        },
        ShotBuffers {
            events,
            hits,
            launches,
        },
    );
}

/// The walking charge: chase the ordered target to contact and detonate.
/// Damage is buffered like every shot, so the sapper's own death lands in
/// the same resolution as its blast. Structures take the full charge,
/// every hostile ground machine in the ring (the victim included) takes
/// the splash, and the sapper itself is always consumed.
fn sapper_attack(
    state: &mut State,
    id: UnitId,
    target: Target,
    resume: Option<Resume>,
    events: &mut Vec<Event>,
    hits: &mut Vec<PendingHit>,
) {
    let unit = state.unit(id).expect("caller checked");
    let (pos, tile, me, kind) = (unit.pos, unit.tile(), unit.player, unit.kind);
    let demolition = kind
        .stats()
        .demolition
        .expect("only demolition machines press their charge");
    let target_info: Option<(Vec2Fx, TilePos)> = match target {
        Target::Unit(uid) => state
            .unit(uid)
            .filter(|t| t.hp > 0)
            .map(|t| (t.pos, t.tile())),
        Target::Building(bid) => state
            .building(bid)
            .filter(|b| b.hp > 0)
            .map(|b| (b.closest_point_to(pos), b.anchor)),
    };
    let victim_domain = target_domain(state, target);
    let Some((aim_point, target_tile)) = target_info else {
        state
            .unit_mut(id)
            .expect("caller checked")
            .complete_attack(resume);
        return;
    };
    // A charge only ever presses on ground: air victims are out of reach.
    if victim_domain != Domain::Ground {
        state.unit_mut(id).expect("caller checked").clear_program();
        return;
    }
    let reach = demolition.contact_range + target_excess(state, target);
    if pos.dist_sq(aim_point) <= reach * reach {
        let unit = state.unit_mut(id).expect("caller checked");
        unit.path = None;
        if !super::super::movement::steer_weapon_heading(unit, aim_point - pos) {
            return;
        }
        let direct = if struck_as_structure(state, target) {
            demolition.structure_damage
        } else {
            demolition.splash_damage
        };
        hits.push(PendingHit::along(
            state,
            Target::Unit(id),
            target,
            direct,
            pos,
            aim_point,
        ));
        for u in &state.units {
            let ring = demolition.blast_radius + crate::stats::hull_excess(u.kind.stats().radius);
            if u.hp == 0
                || !state.hostile(me, u.player)
                || Target::Unit(u.id) == target
                || u.domain() != Domain::Ground
                || u.pos.dist_sq(aim_point) > ring * ring
                || !blast_catches(state, aim_point, tile, u)
            {
                continue;
            }
            hits.push(PendingHit::along(
                state,
                Target::Unit(id),
                Target::Unit(u.id),
                demolition.splash_damage,
                pos,
                aim_point,
            ));
        }
        // The charge consumes its carrier, through the same buffer, so
        // a simultaneous kill on the sapper changes nothing.
        let own_hp = state.unit(id).expect("caller checked").hp;
        hits.push(PendingHit::along(
            state,
            Target::Unit(id),
            Target::Unit(id),
            own_hp,
            pos,
            aim_point,
        ));
        events.push(Event::AttackHit {
            attacker: id,
            attacker_kind: kind,
            weapon: 0,
            target: Some(target),
            attacker_pos: pos,
            target_pos: aim_point,
        });
        let unit = state.unit_mut(id).expect("caller checked");
        unit.path = None;
        return;
    }
    // Not there yet: chase without a firing stance. A path still pressing
    // the victim's tile or any doorstep around its footprint stays.
    let size = match target {
        Target::Unit(_) => (1, 1),
        Target::Building(bid) => state.building(bid).expect("resolved target").kind.size(),
    };
    let stale = state
        .unit(id)
        .expect("caller checked")
        .path
        .as_ref()
        .is_none_or(|p| {
            p.goal != target_tile && !super::super::tile_adjacent_to_rect(p.goal, target_tile, size)
        });
    if !stale {
        return;
    }
    let routed = if state.passable_for(Domain::Ground, target_tile) {
        route_for(state, kind, tile, target_tile).map(|w| (target_tile, w))
    } else if let Target::Building(_) = target {
        // A building's tiles are closed ground: press to the nearest open
        // doorstep that routes, equally near ones ranked in the Sapper's
        // approach frame so mirrored Sappers take mirrored doorsteps.
        let frame = crate::tick::rect_approach_origin(state, me, tile, target_tile, size);
        let mut doorsteps: Vec<TilePos> = super::super::rect_adjacent_tiles(target_tile, size)
            .filter(|&t| state.passable_for(Domain::Ground, t))
            .collect();
        doorsteps.sort_unstable_by_key(|&t| {
            (
                pos.dist_sq(t.center()),
                crate::geometry::rect_approach_key_from(tile, frame, target_tile, size, t),
            )
        });
        doorsteps
            .into_iter()
            .find_map(|t| route_for(state, kind, tile, t).map(|w| (t, w)))
    } else {
        None
    };
    if let Some((goal, waypoints)) = routed {
        let unit = state.unit_mut(id).expect("caller checked");
        unit.path = Some(PathFollow {
            final_point: None,
            goal,
            waypoints,
            next: 0,
        });
    } else {
        let unit = state.unit_mut(id).expect("caller checked");
        let (player, upos) = (unit.player, unit.pos);
        unit.clear_program();
        events.push(Event::OrderStalled {
            unit: id,
            player,
            pos: upos,
            reason: StallReason::NoRoute,
        });
    }
}

/// The attack run: a turn-limited flier never takes a firing stance. It
/// keeps a live route on its victim, steers there on a bounded arc, and
/// releases only when the bay is cold, the victim is inside release range,
/// and the victim sits in the forward cone (the geometry a straight pass
/// produces and a tight orbit cannot). Each release drops one bomb on the
/// victim and rolls the bomber onto an egress leg past the target, so the
/// loop back covers the reload.
fn bomber_attack(
    state: &mut State,
    motion: &MotionSnapshot,
    id: UnitId,
    target: Target,
    resume: Option<Resume>,
    events: &mut Vec<Event>,
    launches: &mut Vec<crate::state::Shell>,
) {
    let unit = state.unit(id).expect("caller checked");
    let stats = unit.kind.stats();
    let (pos, me, kind, heading, cooldowns) = (
        unit.pos,
        unit.player,
        unit.kind,
        unit.heading,
        unit.cooldowns,
    );

    // Vanished or uncoverable target: same hand-back as the ground arm.
    let target_info: Option<(Vec2Fx, TilePos)> = match target {
        Target::Unit(uid) => state
            .unit(uid)
            .filter(|t| t.hp > 0)
            .map(|t| (t.pos, t.tile())),
        Target::Building(bid) => state
            .building(bid)
            .filter(|b| b.hp > 0)
            .map(|b| (b.closest_point_to(pos), b.anchor)),
    };
    let victim_domain = target_domain(state, target);
    let primary = stats
        .weapons
        .iter()
        .position(|w| w.targets.covers(victim_domain));
    let (Some((aim_point, target_tile)), Some(pi)) = (target_info, primary) else {
        state
            .unit_mut(id)
            .expect("caller checked")
            .complete_attack(resume);
        return;
    };
    let weapon = &stats.weapons[pi];

    // Release gate: cold bay, in range, spotted, clear arc, and the
    // victim dead ahead. The sight and trace rules are the shared ones.
    let seen = match target {
        Target::Unit(uid) => state.unit(uid).is_some_and(|u| state.sees_unit(me, u)),
        Target::Building(bid) => state.building(bid).is_some_and(|b| {
            b.tiles().any(|t| state.can_see(me, t)) && state.building_apparent(me, b)
        }),
    };
    let to_target = aim_point - pos;
    let in_range =
        within_weapon_reach(weapon, pos.dist_sq(aim_point), target_excess(state, target));
    let hv = chassis::compass::dir(heading);
    let ahead = hv.x * to_target.x + hv.y * to_target.y
        >= to_target.length() * crate::stats::BOMBER_CONE_DOT;
    if cooldowns[pi] == 0
        && in_range
        && seen
        && ahead
        && clear_line(
            state,
            Fire::of(weapon),
            ShotEnd::new(pos, stats.domain),
            ShotEnd::new(aim_point, victim_domain).on(target_tile),
            Landing::Checked,
        )
    {
        let center = projectile_aim(
            state,
            motion,
            ProjectileShooter {
                owner: me,
                domain: stats.domain,
            },
            pos,
            target,
            aim_point,
            weapon,
        );
        // A bomb falls on the building, not on the edge point that range
        // is measured to: a footprint's closest point can be a corner it
        // shares with a neighbour, and a shell landing exactly there is
        // credited to the lowest-id footprint touching it.
        let center = match target {
            Target::Building(bid) => state
                .building(bid)
                .map_or(center, crate::state::Building::center),
            Target::Unit(_) => center,
        };
        let impact = state.map().clamp_to_envelope(center);
        fire_shell(
            state,
            Shot {
                gunner: Gunner::Unit { id, slot: pi },
                from: pos,
                aim: impact,
                mark: Mark::Seen(target),
            },
            events,
            launches,
        );
        // Egress: fly through and past the release point. The leg is a
        // plain path goal, so steering, arrival, and repath all reuse
        // the ordinary machinery; when it completes (or goes stale) the
        // chase below lines up the next run.
        let egress = egress_goal(state, pos, heading, stats);
        let unit = state.unit_mut(id).expect("caller checked");
        unit.path = egress.map(|goal| PathFollow {
            final_point: None,
            goal,
            waypoints: vec![goal],
            next: 0,
        });
        return;
    }

    // A hot bay keeps flying. Until the reload is half done the bomber
    // never turns back toward its victim: it finishes the egress leg, then
    // keeps extending the run straight ahead. Hovering over the target to
    // re-bomb point-blank is the stop-and-strafe this chassis forbids.
    if cooldowns[pi] > weapon.cooldown_ticks / 2 {
        if state.unit(id).expect("caller checked").path.is_none()
            && let Some(goal) = egress_goal(state, pos, heading, stats)
        {
            let unit = state.unit_mut(id).expect("caller checked");
            unit.path = Some(PathFollow {
                final_point: None,
                goal,
                waypoints: vec![goal],
                next: 0,
            });
        }
        return;
    }

    // A bomber already inside release range, or inside its own turn
    // acceptance ring of the attack tile, cannot line up a pass from where
    // it is: the victim may sit outside the forward cone, and any route to
    // the tile would complete without a tick of flight. Once its approach
    // path completes, send it onward along a clear departure leg instead.
    // This covers the second half of a reload as well as a cold bay;
    // otherwise a warm bomber would plan an instantly accepted route every
    // tick and hang over the target. Keep the attack tile as PathFollow's
    // semantic goal so the chase retains this leg; when it completes, the
    // next exact-position route lines up another pass.
    let accept = stats.turn_acceptance();
    let inside_ring = pos.dist_sq(target_tile.center()) <= accept * accept;
    if (in_range || inside_ring) && state.unit(id).expect("caller checked").path.is_none() {
        let departure = egress_goal(state, pos, heading, stats);
        let unit = state.unit_mut(id).expect("caller checked");
        unit.path = departure.map(|waypoint| PathFollow {
            final_point: None,
            goal: target_tile,
            waypoints: vec![waypoint],
            next: 0,
        });
        return;
    }

    // Chase: keep a live route on the victim, repathing when it drifts.
    let stale = state
        .unit(id)
        .expect("caller checked")
        .path
        .as_ref()
        .is_none_or(|p| p.goal != target_tile && p.goal.chebyshev(target_tile) > 1);
    if stale {
        if let Some(waypoints) = landing::run_in_route(
            state,
            stats,
            kind,
            pos,
            heading,
            target_tile,
            landing::RunIn::Attack,
        ) {
            let unit = state.unit_mut(id).expect("caller checked");
            unit.path = Some(PathFollow {
                final_point: None,
                goal: target_tile,
                waypoints,
                next: 0,
            });
        } else {
            let unit = state.unit_mut(id).expect("caller checked");
            let (player, upos) = (unit.player, unit.pos);
            unit.clear_program();
            events.push(Event::OrderStalled {
                unit: id,
                player,
                pos: upos,
                reason: StallReason::NoRoute,
            });
        }
    }
}

/// Where a bomber rolls out after a release: straight ahead along its
/// heading when the sky is open, bending progressively further, up to a
/// full reversal, when a wall, mesa, or map corner closes the line.
/// Every candidate must sit beyond the aircraft's own acceptance ring (a
/// goal inside it completes without a single tick of flight) and must be a
/// state the airframe can still be flown out of on arrival, so a roll-out
/// never ends pressed against the world's edge. `None` only in a closed
/// pocket, where the airframe orbits until the brain replans.
fn egress_goal(
    state: &State,
    pos: Vec2Fx,
    heading: u8,
    stats: &crate::stats::UnitStats,
) -> Option<TilePos> {
    let accept = stats.turn_acceptance();
    let accept_sq = accept * accept;
    let radius = stats.turn_radius();
    for offset in [0u8, 32, 224, 64, 192, 96, 160, 128] {
        let leg_heading = heading.wrapping_add(offset);
        let hv = chassis::compass::dir(leg_heading);
        for reach in [5i64, 4, 3] {
            let probe = pos + hv * Fx::from_num(reach);
            let tile = TilePos::containing(probe);
            if tile.x >= 0
                && tile.y >= 0
                && tile.x < state.map().width()
                && tile.y < state.map().height()
                && state.passable_for(Domain::Air, tile)
                && tile.center().dist_sq(pos) > accept_sq
                && flight::escapable(state.map(), tile.center(), leg_heading, radius)
                && !chassis::path::line_blocked(pos, tile.center(), |crossed| {
                    state
                        .map()
                        .tile(crossed)
                        .is_some_and(|terrain| !terrain.terrain.blocks_air())
                })
            {
                return Some(tile);
            }
        }
    }
    None
}

/// Chase-and-hit. Range is measured to the target's closest point and
/// shots are buffered. A vanished target — or one no carried weapon can
/// cover — hands control back to the remembered hunt (or idle,
/// where auto-acquire finds the next fight).
#[allow(clippy::too_many_arguments)]
#[expect(
    clippy::too_many_lines,
    reason = "the whole chase-and-hit decision for one unit"
)]
pub(super) fn attack(
    state: &mut State,
    index: &super::super::spatial::UnitIndex,
    motion: &MotionSnapshot,
    id: UnitId,
    target: Target,
    resume: Option<Resume>,
    events: &mut Vec<Event>,
    hits: &mut Vec<PendingHit>,
    launches: &mut Vec<crate::state::Shell>,
) {
    let stance = state.unit(id).expect("caller checked").motor;
    state.unit_mut(id).expect("caller checked").retract_braces();
    let unit = state.unit(id).expect("caller checked");
    let stats = unit.kind.stats();
    if !stats.can_fight() {
        state.unit_mut(id).expect("caller checked").clear_program();
        return;
    }
    if stats.turn_rate > 0 {
        bomber_attack(state, motion, id, target, resume, events, launches);
        return;
    }
    if stats.demolition.is_some() {
        sapper_attack(state, id, target, resume, events, hits);
        return;
    }
    let (pos, tile, me, kind, cooldowns, routed) = (
        unit.pos,
        unit.tile(),
        unit.player,
        unit.kind,
        unit.cooldowns,
        unit.path.is_some(),
    );

    // A hunter attacking a building stays alert: an enemy unit wandering
    // into aggro takes priority (acquisition prefers units), so marching
    // armies fight back.
    if resume.is_some()
        && matches!(target, Target::Building(_))
        && let Some(better @ Target::Unit(_)) = acquire_target(state, index, id)
    {
        let unit = state.unit_mut(id).expect("caller checked");
        unit.order = Order::Attack {
            pursue: false,
            target: better.into(),
            resume,
        };
        unit.path = None;
        return;
    }

    // A wide cutter whose way to its target runs through hostile buildings
    // turns to cut the first of them out of its way, unless its drum already
    // presses on the target.
    let ordered = target;
    let target = if contact::square_cutter(kind) {
        let unit = state.unit(id).expect("caller checked");
        match target {
            Target::Unit(uid) => state
                .unit(uid)
                .filter(|u| u.hp > 0)
                .map(crate::state::Unit::tile),
            Target::Building(bid) => state
                .building(bid)
                .filter(|b| b.hp > 0 && contact::drum_contact(state, unit, b).is_none())
                .map(|b| b.anchor),
        }
        .and_then(|goal| contact::plough_blocker(state, unit, goal, false))
        .map_or(target, Target::Building)
    } else {
        target
    };

    // Resolve the target's current position; None means it is gone. A
    // target outside every weapon mask ends the engagement the same way.
    let target_info: Option<(Vec2Fx, TilePos)> = match target {
        Target::Unit(uid) => state
            .unit(uid)
            .filter(|t| t.hp > 0)
            .map(|t| (t.pos, t.tile())),
        Target::Building(bid) => state.building(bid).filter(|b| b.hp > 0).map(|b| {
            (
                if stats.contact_reach.is_some() {
                    state.contact_surface(b).closest(pos)
                } else {
                    b.closest_point_to(pos)
                },
                b.anchor,
            )
        }),
    };
    let victim_domain = target_domain(state, target);
    let primary = stats
        .weapons
        .iter()
        .position(|w| w.targets.covers(victim_domain));
    let (Some((aim_point, target_tile)), Some(pi)) = (target_info, primary) else {
        state
            .unit_mut(id)
            .expect("caller checked")
            .complete_attack(resume);
        return;
    };
    let weapon = &stats.weapons[pi];

    // In range only counts with a clear line (see `crate::tick::fire`) and
    // with sight. The owner must currently see the victim's tile, so a gun
    // that outranges its own vision fires on a spotter's sight. Scrap piles
    // do not block fire. With no shot, keep approaching.
    // Sight of any footprint tile serves for a building (matching attack
    // validation); a unit is seen at its own tile. The line trace runs
    // only once in range — it is not built for cross-map endpoints.
    let seen = match target {
        Target::Unit(uid) => state.unit(uid).is_some_and(|u| state.sees_unit(me, u)),
        Target::Building(bid) => state.building(bid).is_some_and(|b| {
            b.tiles().any(|t| state.can_see(me, t)) && state.building_apparent(me, b)
        }),
    };
    // A wide cutter cuts a building where its drum presses flat against one
    // of its sides: its target's, or short of that, any other it cuts and
    // sees. The stroke lands where the drum meets the side.
    let drum = contact::square_cutter(kind)
        .then(|| {
            let unit = state.unit(id).expect("caller checked");
            let pressed = |b: &crate::state::Building| {
                contact::drum_contact(state, unit, b).map(|(point, outward)| {
                    let rank = contact::nearest(unit, b, point);
                    (rank, Target::Building(b.id), b.anchor, point, outward)
                })
            };
            let span = contact::drum_span(kind);
            match target {
                Target::Building(bid) => state.building(bid).and_then(pressed),
                Target::Unit(_) => None,
            }
            .or_else(|| {
                state
                    .buildings()
                    .iter()
                    .filter(|b| {
                        Target::Building(b.id) != target
                            && pos.dist_sq(b.closest_point_to(pos)) <= span * span
                            && contact::ploughs(state, me, b)
                    })
                    .filter_map(pressed)
                    .min_by_key(|&(rank, ..)| rank)
            })
            .map(|(_, stroke, anchor, point, outward)| (stroke, anchor, point, outward))
        })
        .flatten();
    let (stroke, aim_point, target_tile, seen) = match drum {
        Some((stroke, anchor, point, _)) => (stroke, point, anchor, seen || stroke != target),
        None => (target, aim_point, target_tile, seen),
    };
    // Stopped within reach of its target but not pressed flat against it, a
    // wide cutter turns square to the side it stands out from. It cuts only
    // once its drum presses flat: squared up off a corner, short of every
    // side, it drives on to meet one.
    if drum.is_none()
        && !routed
        && seen
        && contact::square_cutter(kind)
        && let Target::Building(bid) = target
        && within_unit_weapon_reach(state, kind, target, weapon, pos.dist_sq(aim_point))
        && let Some(outward) = state.building(bid).and_then(|b| {
            let forward = chassis::compass::dir(state.unit(id).expect("caller checked").heading);
            state.contact_surface(b).side_facing(pos, forward)
        })
    {
        let unit = state.unit_mut(id).expect("caller checked");
        if !super::super::movement::steer_weapon_heading(unit, -outward) {
            return;
        }
    }
    let in_range = drum.is_some()
        || (within_unit_weapon_reach(state, kind, target, weapon, pos.dist_sq(aim_point))
            && !(contact::square_cutter(kind) && matches!(target, Target::Building(_))));
    if in_range
        && seen
        && clear_line(
            state,
            Fire::of(weapon),
            ShotEnd::new(pos, stats.domain),
            ShotEnd::new(aim_point, victim_domain).on(target_tile),
            Landing::Checked,
        )
    {
        let projectile_aim = weapon.projectile.is_some().then(|| {
            projectile_aim(
                state,
                motion,
                ProjectileShooter {
                    owner: me,
                    domain: stats.domain,
                },
                pos,
                target,
                aim_point,
                weapon,
            )
        });
        let unit = state.unit_mut(id).expect("caller checked");
        unit.path = None;
        // Reaching the firing stance refreshes the leash patience window,
        // so a guard can finish a wounded runner past the radius without
        // licensing a cross-map dive. A target that never comes in reach
        // grants no window, and its chaser breaks at the radius line.
        if resume.is_none()
            && let Some(leash) = unit.leash.as_mut()
        {
            leash.patience = crate::stats::LEASH_PATIENCE;
        }
        // A gun keeping its firing stance keeps its spades and its swing as
        // they stood; the retraction above was for a gun leaving it.
        if matches!(stance, Motor::Braced { .. }) {
            unit.motor = stance;
        }
        let facing = match drum {
            Some((.., outward)) => -outward,
            None => projectile_aim.unwrap_or(aim_point) - pos,
        };
        if !super::super::movement::steer_weapon_heading(unit, facing) {
            return;
        }
        if cooldowns[pi] == 0 {
            fire_shot(
                state,
                Shot {
                    gunner: Gunner::Unit { id, slot: pi },
                    from: pos,
                    aim: projectile_aim.unwrap_or(aim_point),
                    mark: Mark::Seen(stroke),
                },
                ShotBuffers {
                    events,
                    hits,
                    launches,
                },
            );
            if contact::square_cutter(kind) {
                cut_alongside(state, id, stroke, pi, hits, events);
            }
        }
        fire_sidearms(state, index, id, pi, hits, events);
        return;
    }
    // Opportunist guns don't wait for the march to end.
    fire_sidearms(state, index, id, pi, hits, events);

    // A mobile long gun inside its own dead zone must create space; the
    // generic "not in range" chase would close in further. If terrain offers
    // no legal stand, hold and retry instead of making the geometry worse.
    if pos.dist_sq(aim_point) < weapon.minimum_range * weapon.minimum_range {
        if let Target::Building(building) = target {
            approach_firing_area(state, id, building, weapon);
        } else {
            retreat_to_firing_stand(state, id, target, victim_domain, weapon);
        }
        return;
    }

    // The tether binds the chase, not the trigger or the firing stand above.
    // It measures the guard's own distance from its anchor, never the
    // target's: a stationed machine travels at most the radius from its post
    // plus the patience window, independent of weapon range or speed.
    // Inside the radius the guard hunts freely; beyond it every chase tick
    // spends patience, and an empty window sends the guard walking home,
    // leash kept so the homecoming arms the post cooldown.
    if resume.is_none()
        && let Some(leash) = state.unit(id).expect("caller checked").leash
    {
        let radius_sq = crate::stats::LEASH_RADIUS * crate::stats::LEASH_RADIUS;
        if leash.anchor.center().dist_sq(pos) > radius_sq {
            if leash.patience == 0 {
                let unit = state.unit_mut(id).expect("caller checked");
                unit.order = Order::Run {
                    goal: leash.anchor.into(),
                };
                unit.path = None;
                return;
            }
            state
                .unit_mut(id)
                .expect("caller checked")
                .leash
                .as_mut()
                .expect("just seen")
                .patience -= 1;
        }
    }

    // Out of range (or blind, or blocked): chase.
    let reached: Result<(), StallReason> = match target {
        Target::Unit(_) => {
            // A ground chaser cannot stand where a flyer hovers over rock or
            // a roof, so it marches to a tile it can stand on and shoot from
            // instead. Air chasers and standable victim tiles keep the
            // direct goal.
            let direct = state.passable_for(stats.domain, target_tile);
            // A stand-in must lie within the reach the trigger checks, or
            // the chaser parks where it can never fire.
            let reach = unit_weapon_reach(state, kind, Some(target), weapon);
            // Repath when the target has drifted a tile from the
            // path's goal — cheap pursuit without per-tick A*. A path
            // already aimed at a firing position for this victim stays
            // fresh while it parks, or a grounded chaser would repath
            // forever.
            let stale = state
                .unit(id)
                .expect("caller checked")
                .path
                .as_ref()
                .is_none_or(|p| {
                    if direct {
                        p.goal != target_tile && p.goal.chebyshev(target_tile) > 1
                    } else {
                        !(state.passable_for(stats.domain, p.goal)
                            && p.goal.center().dist_sq(aim_point) <= reach * reach)
                    }
                });
            if stale {
                let unit = state.unit(id).expect("caller checked");
                let routed = if direct {
                    contact::route(state, unit, tile.center(), target_tile)
                        .map(|w| (target_tile, w))
                } else {
                    // Nearest ring first, then ranked in the chaser's
                    // approach frame so a mirrored chaser tries the mirrored
                    // stand-in; the first that routes wins, so an isolated
                    // pocket next to the victim must not stall a chaser that
                    // could fire from the far side.
                    let mut stand_ins =
                        chase_stand_ins(state, stats.domain, target_tile, aim_point, reach);
                    stand_ins.sort_by_key(|goal| {
                        (
                            goal.chebyshev(target_tile),
                            crate::geometry::rect_approach_key(tile, target_tile, (1, 1), *goal),
                        )
                    });
                    stand_ins.into_iter().find_map(|goal| {
                        contact::route(state, unit, tile.center(), goal).map(|w| (goal, w))
                    })
                };
                match routed {
                    Some((goal, waypoints)) => {
                        let unit = state.unit_mut(id).expect("caller checked");
                        unit.path = Some(PathFollow {
                            final_point: None,
                            goal,
                            waypoints,
                            next: 0,
                        });
                        Ok(())
                    }
                    // NoFiringPosition derives from the victim's footing;
                    // report it only while the team sees that ground, or the
                    // stall would leak where a fogged flyer parked. Unseen,
                    // report that no route worked.
                    None if !direct
                        && state.can_see(me, target_tile)
                        && chase_stand_ins(state, stats.domain, target_tile, aim_point, reach)
                            .is_empty() =>
                    {
                        Err(StallReason::NoFiringPosition)
                    }
                    None => Err(StallReason::NoRoute),
                }
            } else {
                Ok(())
            }
        }
        Target::Building(bid) => {
            let approached = if stats.contact_reach.is_some() {
                // A building in the way that the cutter cannot reach leaves
                // the way to the building it was sent for.
                contact::approach(state, id, bid)
                    || matches!(ordered, Target::Building(sent) if sent != bid
                        && state.building(sent).is_some_and(|b| b.hp > 0)
                        && contact::approach(state, id, sent))
            } else {
                approach_firing_area(state, id, bid, weapon)
            };
            if approached {
                Ok(())
            } else {
                Err(StallReason::NoRoute)
            }
        }
    };
    if let Err(reason) = reached {
        stall_attack(state, id, resume, reason, events);
    }
}

fn stall_attack(
    state: &mut State,
    id: UnitId,
    resume: Option<Resume>,
    reason: StallReason,
    events: &mut Vec<Event>,
) {
    let unit = state.unit_mut(id).expect("caller checked");
    // A fight escort duty picked for itself is dropped as quietly: the
    // escort goes back to its slot, and acquisition never picks a victim it
    // can neither shoot nor reach.
    if let Some(Resume::Accompany(_)) = resume {
        unit.complete_attack(resume);
        return;
    }
    // A tethered chase that cannot route breaks off home quietly: it
    // abandons its own acquisition, not a player order, so no stall event.
    if resume.is_none()
        && let Some(leash) = unit.leash
    {
        unit.order = Order::Run {
            goal: leash.anchor.into(),
        };
        unit.path = None;
        return;
    }
    let (player, pos) = (unit.player, unit.pos);
    // A chase that could not be prosecuted leaves the unit stationed,
    // so its next idle acquisition tethers and the leash cooldown
    // paces re-acquisition instead of the same refused chase firing
    // every tick.
    unit.settled = crate::stats::LEASH_STATION_TICKS;
    unit.clear_program();
    events.push(Event::OrderStalled {
        unit: id,
        player,
        pos,
        reason,
    });
}

/// The sidearm's opportunist victim: the nearest hostile unit the weapon
/// can cover, in range, seen by the owner, and clear, chosen through the
/// spatial index's reach window. Selection is keyed `(distance, bearing,
/// id)`, the bearing taken against the shooter's `heading`, so window visit
/// order cannot change the answer.
fn sidearm_victim(
    state: &State,
    index: &super::super::spatial::UnitIndex,
    (shooter_pos, heading): (Vec2Fx, u8),
    me: crate::ids::PlayerId,
    shooter_domain: Domain,
    weapon: &WeaponStats,
) -> Option<(chassis::fx::Fx, UnitId, Vec2Fx, Domain)> {
    // |Δpos| <= range on either axis puts the victim's tile within
    // floor(range) + 1 of the shooter's — the acquire_target bound.
    let home = TilePos::containing(shooter_pos);
    let reach = weapon.range.floor().to_num::<i32>() + 1 + index.wide_reach();
    let mut victim: Option<((chassis::fx::Fx, u8, UnitId), Vec2Fx, Domain)> = None;
    for dy in -reach..=reach {
        for &(_, slot) in index.row_span(home.y + dy, home.x - reach, home.x + reach) {
            let u = &state.units[slot];
            let domain = u.domain();
            if !state.hostile(me, u.player)
                || u.hp == 0
                || !weapon.targets.covers(domain)
                || !state.sees_unit(me, u)
            {
                continue;
            }
            let d = shooter_pos.dist_sq(u.pos);
            if !within_weapon_reach(weapon, d, crate::stats::hull_excess(u.kind.stats().radius)) {
                continue;
            }
            // Pay for the line trace only when the candidate would win;
            // a blocked better candidate never shadows a clear worse one.
            let rank = (d, bearing_from(shooter_pos, heading, u.pos), u.id);
            if victim.is_some_and(|(best, ..)| best <= rank) {
                continue;
            }
            if !clear_line(
                state,
                Fire::of(weapon),
                ShotEnd::new(shooter_pos, shooter_domain),
                ShotEnd::new(u.pos, domain),
                Landing::Unchecked,
            ) {
                continue;
            }
            victim = Some((rank, u.pos, domain));
        }
    }
    victim.map(|((d, _, id), pos, domain)| (d, id, pos, domain))
}

/// Weapons other than the one engaging the ordered target pick their own
/// fights: the nearest hostile unit each can cover, in range, seen by the
/// owner, and clear — opportunist fire that never steers the chassis.
fn fire_sidearms(
    state: &mut State,
    index: &super::super::spatial::UnitIndex,
    id: UnitId,
    primary: usize,
    hits: &mut Vec<PendingHit>,
    events: &mut Vec<Event>,
) {
    let unit = state.unit(id).expect("caller checked");
    let stats = unit.kind.stats();
    if stats.weapons.len() < 2 {
        return;
    }
    let (pos, me, cooldowns) = (unit.pos, unit.player, unit.cooldowns);
    let heading = unit.heading;
    for (wi, weapon) in stats.weapons.iter().enumerate() {
        if wi == primary || cooldowns[wi] > 0 {
            continue;
        }
        let victim = sidearm_victim(state, index, (pos, heading), me, stats.domain, weapon);
        let Some((_, uid, upos, _)) = victim else {
            blind::sidearm_radar(state, id, wi, hits, events);
            continue;
        };
        if !super::super::movement::ground_weapon_aligned(
            state.unit(id).expect("caller checked"),
            upos - pos,
        ) {
            continue;
        }
        fire_direct(
            state,
            Shot {
                gunner: Gunner::Unit { id, slot: wi },
                from: pos,
                aim: upos,
                mark: Mark::Seen(Target::Unit(uid)),
            },
            events,
            hits,
        );
    }
}

/// Damage answers back: a hit unit that can fight and isn't already
/// fighting turns on its attacker, which counters weapons that outrange
/// aggro. A hunter keeps its destination as the resume point. Hits resolve
/// in decision order, so the first answerable attacker of a tick wins.
pub(super) fn retaliate(state: &mut State, victim: UnitId, attacker: Target) {
    let Some(unit) = state.unit(victim) else {
        return;
    };
    let stats = unit.kind.stats();
    let attacker_domain = target_domain(state, attacker);
    // A breacher answers a building that hits it, which is its work, but
    // never turns on a machine. Mounted guns already answer whatever
    // reaches the hull, which never chases what does not, and a drone flies
    // for its host alone.
    if stats.mounts.is_some() || unit.host.is_some() {
        return;
    }
    if unit.hp == 0
        || !stats.can_target(attacker_domain)
        || (stats.target_priority == crate::stats::TargetPriority::Buildings
            && matches!(attacker, Target::Unit(_)))
    {
        return;
    }
    // Answering fire needs sight: chasing a shell lobbed from beyond every
    // friendly sight line would leak the shooter's position.
    let seen = match attacker {
        Target::Unit(uid) => state
            .unit(uid)
            .is_some_and(|a| state.sees_unit(unit.player, a)),
        Target::Building(bid) => state
            .building(bid)
            .is_some_and(|b| b.tiles().any(|t| state.can_see(unit.player, t))),
    };
    if !seen {
        return;
    }
    let resume = match unit.order {
        Order::Idle => None,
        Order::Hunt { goal } => Some(Resume::Hunt(goal)),
        // An escort answers fire from near the unit it guards, or, while it
        // stands within the engage radius itself, from anything its own guns
        // reach; any other shooter would draw it away only to be broken off.
        Order::Accompany { unit: escorted, .. } => {
            let engage = crate::stats::ESCORT_ENGAGE_RADIUS * crate::stats::ESCORT_ENGAGE_RADIUS;
            let answers = state.escorted(escorted).is_some_and(|body| {
                let from = match attacker {
                    Target::Unit(uid) => state.unit(uid).map(|a| a.pos),
                    Target::Building(bid) => {
                        state.building(bid).map(|b| b.closest_point_to(body.pos))
                    }
                };
                from.is_some_and(|from| from.dist_sq(body.pos) <= engage)
                    || (unit.pos.dist_sq(body.pos) <= engage
                        && in_weapon_reach_from(state, unit.kind, unit.pos, attacker))
            });
            if !answers {
                return;
            }
            Some(Resume::Accompany(escorted))
        }
        // A tethered homecoming answers fire: the walk home resumes through
        // the leash once the attacker falls, so no resume goal is carried. A
        // plain Run is the player's recall verb and never auto-engages.
        Order::Run { .. } if unit.leash.is_some() => None,
        // An attack aimed at something that just died in resolution is no
        // engagement; without this arm a surviving out-of-aggro shooter
        // would fire unanswered for another full cooldown. Attacks on live
        // targets are not interrupted.
        Order::Attack { target, resume, .. }
            if state.attack_view(unit.player, target).is_none() =>
        {
            resume
        }
        _ => return, // already busy fighting or working
    };
    let target = state
        .attack_objective(unit.player, attacker.into())
        .unwrap_or(attacker.into());
    let unit = state.unit_mut(victim).expect("checked above");
    unit.order = Order::Attack {
        pursue: false,
        target,
        resume,
    };
    unit.path = None;
    // Answering a hit refreshes the patience window, so the answer can
    // reach an attacker just past the radius (such as a repositioning
    // Bombard) without opening a cross-map dive. An answer that resumes a
    // march stays untethered. An answer with no resume is self-acquisition:
    // an existing tether keeps its anchor, a stationed machine gets a fresh
    // one where it stood, and an unsettled machine answers unleashed.
    if resume.is_none() {
        let stationed = unit.settled >= crate::stats::LEASH_STATION_TICKS;
        unit.settled = 0;
        match unit.leash.as_mut() {
            Some(leash) => {
                leash.patience = crate::stats::LEASH_PATIENCE;
                leash.cooldown = 0;
            }
            None if stationed => {
                let anchor = unit.tile();
                unit.leash = Some(crate::state::Leash {
                    anchor,
                    patience: crate::stats::LEASH_PATIENCE,
                    cooldown: 0,
                });
            }
            None => {}
        }
    }
}

/// Whether a target is still on the field with hit points.
pub(super) fn target_standing(state: &State, target: Target) -> bool {
    match target {
        Target::Unit(u) => state.unit(u).is_some_and(|u| u.hp > 0),
        Target::Building(b) => state.building(b).is_some_and(|b| b.hp > 0),
    }
}

fn approach_firing_area(
    state: &mut State,
    id: UnitId,
    building: crate::BuildingId,
    weapon: &WeaponStats,
) -> bool {
    let unit = state.unit(id).expect("attacker");
    let b = state.building(building).expect("visible building");
    let (from, kind, player, pos) = (unit.tile(), unit.kind, unit.player, unit.pos);
    let domain = unit.domain();
    let legal = |point: Vec2Fx| {
        let aim = b.closest_point_to(point);
        state.passable_for(domain, TilePos::containing(point))
            && crate::tick::crowding::standable(state, unit, point)
            && within_weapon_reach(weapon, point.dist_sq(aim), Fx::ZERO)
            && clear_line(
                state,
                Fire::of(weapon),
                ShotEnd::new(point, domain),
                ShotEnd::new(aim, Domain::Ground).on(b.anchor),
                Landing::Checked,
            )
    };
    if unit.path.as_ref().is_some_and(|path| {
        let point = path.final_point.unwrap_or(path.goal.center());
        legal(point) && !crate::tick::crowding::claimed(state, id, point, true)
    }) {
        return true;
    }
    let r = weapon.range.ceil().to_num::<i32>();
    let (w, h) = b.kind.size();
    let frame = crate::tick::rect_approach_origin(state, player, from, b.anchor, (w, h));
    let mut points = Vec::new();
    for y in ((b.anchor.y - r).max(0) * 2)..((b.anchor.y + h + r).min(state.map.height()) * 2) {
        for x in ((b.anchor.x - r).max(0) * 2)..((b.anchor.x + w + r).min(state.map.width()) * 2) {
            let point = Vec2Fx::new(
                Fx::from_num(x) / 2 + const { Fx::lit("0.25") },
                Fx::from_num(y) / 2 + const { Fx::lit("0.25") },
            );
            if legal(point) {
                points.push(point);
            }
        }
    }
    let aim = b.closest_point_to(pos);
    let distance = pos.dist(aim);
    if distance > weapon.range {
        let point = pos.move_toward(aim, distance - weapon.range + const { Fx::lit("0.02") });
        if legal(point) {
            points.push(point);
        }
    }
    points.sort_by_key(|&point| {
        (
            pos.dist_sq(point),
            crate::geometry::rect_approach_key_from(
                from,
                frame,
                b.anchor,
                (w, h),
                TilePos::containing(point),
            ),
            (point.x - pos.x) * (frame.center().y - pos.y)
                - (point.y - pos.y) * (frame.center().x - pos.x),
        )
    });
    let candidates = points
        .into_iter()
        .map(|point| crate::tick::crowding::Position {
            goal: TilePos::containing(point),
            point,
        })
        .collect();
    let path = crate::tick::crowding::choose(
        state,
        id,
        candidates,
        b.center(),
        |tile| state.passable_for(domain, tile),
        |goal| super::super::route_for_position(state, kind, pos, goal),
    );
    let reachable = path.is_some();
    state.unit_mut(id).expect("attacker").path = path;
    reachable
}

#[cfg(test)]
mod tests;
