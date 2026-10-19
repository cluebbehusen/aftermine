//! Drones in flight. Each fights the nearest seen enemy aircraft within its
//! leash of the host's edge, holds a slot on a ring around the host with
//! none, and flies
//! home to dock once the host is calm. A host stays on alert while a seen
//! enemy aircraft is near it or anything hits it or its drones.

use super::*;
use crate::stats::{DRONE_LEASH, DRONE_ORBIT, LAUNCH_RADIUS, RECALL_TICKS};
use std::collections::BTreeMap;

/// Ticks for the drones' ring to turn one compass step.
const ORBIT_STEP_TICKS: u64 = 2;

/// Every host's flying drones in id order, captured once before the brains
/// run: launches and docks wait until after them, so the fleets hold still.
pub(in crate::tick::brain) struct Fleets {
    fleets: BTreeMap<UnitId, Vec<UnitId>>,
}

impl Fleets {
    pub(in crate::tick::brain) fn capture(state: &State) -> Self {
        let mut fleets: BTreeMap<UnitId, Vec<UnitId>> = BTreeMap::new();
        for unit in &state.units {
            if let Some(host) = unit.host {
                fleets.entry(host).or_default().push(unit.id);
            }
        }
        Self { fleets }
    }

    /// The drone's rank among its host's flying drones, and how many fly.
    fn rank(&self, host: UnitId, drone: UnitId) -> (usize, usize) {
        self.fleets.get(&host).map_or((0, 1), |fleet| {
            (fleet.binary_search(&drone).unwrap_or(0), fleet.len().max(1))
        })
    }
}

/// The nearest seen enemy aircraft to `from` within `radius` of `around`.
/// Equally near ones are told apart by their bearing from `from` turned into
/// the frame of `heading`, then by their rank among their owner's units, so
/// mirrored fleets pick mirrored targets.
fn aircraft_near(
    state: &State,
    index: &super::super::super::spatial::UnitIndex,
    me: PlayerId,
    around: Vec2Fx,
    radius: Fx,
    (from, heading): (Vec2Fx, u8),
) -> Option<(UnitId, Vec2Fx)> {
    let home = TilePos::containing(around);
    let reach = radius.ceil().to_num::<i32>() + 1;
    let team = state.player(me).team;
    if !index.hostiles_near(team, home, reach).bodies {
        return None;
    }
    let reach_sq = radius * radius;
    let candidates = (-reach..=reach)
        .flat_map(|dy| index.row_span(home.y + dy, home.x - reach, home.x + reach))
        .map(|&(_, slot)| &state.units[slot])
        .filter(|u| {
            state.hostile(me, u.player)
                && u.hp > 0
                && u.domain() == Domain::Air
                && around.dist_sq(u.pos) <= reach_sq
                && state.sees_unit(me, u)
        })
        .map(|u| {
            let bearing = chassis::compass::heading_of(u.pos - from).wrapping_sub(heading);
            ((from.dist_sq(u.pos), bearing), u)
        })
        .collect::<Vec<_>>();
    let nearest = candidates.iter().map(|(key, _)| *key).min()?;
    // Ranking walks every unit, so only an exact tie pays for it.
    candidates
        .into_iter()
        .filter(|(key, _)| *key == nearest)
        .map(|(_, u)| u)
        .min_by_key(|u| {
            let rank = crate::ids::owner_local_unit_rank(
                u.id,
                u.player,
                state.units.iter().map(|o| (o.id, o.player)),
            );
            (rank, u.player)
        })
        .map(|u| (u.id, u.pos))
}

/// Keeps `id`'s bay on alert while a seen enemy aircraft is within
/// [`LAUNCH_RADIUS`] of it, and otherwise lets the alert run down.
pub(in crate::tick::brain) fn watch(
    state: &mut State,
    index: &super::super::super::spatial::UnitIndex,
    id: UnitId,
) {
    let host = state.unit(id).expect("caller checked");
    let (me, pos, heading) = (host.player, host.pos, host.heading);
    let near = aircraft_near(state, index, me, pos, LAUNCH_RADIUS, (pos, heading)).is_some();
    let bay = state
        .unit_mut(id)
        .expect("caller checked")
        .bay
        .as_mut()
        .expect("a host");
    bay.alert = if near {
        RECALL_TICKS
    } else {
        bay.alert.saturating_sub(1)
    };
}

/// One drone's tick: home to dock once its host is calm; otherwise at the
/// nearest seen enemy aircraft within its leash of the host's edge, firing
/// once it bears and reaches; otherwise to its slot on the ring around its
/// host. The leash runs from the edge so nothing that reaches the host
/// stands outside it.
pub(in crate::tick::brain) fn fly(
    state: &mut State,
    index: &super::super::super::spatial::UnitIndex,
    fleets: &Fleets,
    id: UnitId,
    shots: ShotBuffers<'_>,
    docks: &mut Vec<UnitId>,
) {
    let drone = state.unit(id).expect("caller checked");
    let (me, pos, kind) = (drone.player, drone.pos, drone.kind);
    let host_id = drone.host.expect("a drone");
    // A host that fell this tick takes its drones with it in cleanup.
    let Some(host) = state.unit(host_id).filter(|host| host.hp > 0) else {
        return;
    };
    let (hpos, hradius, hheading) = (host.pos, host.kind.stats().radius, host.heading);
    let alert = host.bay.map_or(0, |bay| bay.alert);
    if alert == 0 {
        let dock = hradius + kind.stats().radius + Fx::lit("0.5");
        if pos.dist_sq(hpos) <= dock * dock {
            docks.push(id);
        } else {
            head_for(state, id, hpos);
        }
        return;
    }
    if let Some((victim, aim)) = aircraft_near(
        state,
        index,
        me,
        hpos,
        DRONE_LEASH + hradius,
        (pos, hheading),
    ) {
        engage(state, id, (victim, aim), shots);
        return;
    }
    let (rank, count) = fleets.rank(host_id, id);
    let spin =
        u8::try_from((state.current_tick() / ORBIT_STEP_TICKS) % 256).expect("a compass step");
    let step = u8::try_from(rank * 256 / count).expect("a compass step");
    let slot =
        hpos + chassis::compass::dir(hheading.wrapping_add(step).wrapping_add(spin)) * DRONE_ORBIT;
    head_for(state, id, slot);
}

/// Turns the drone toward `aim` and fires its gun once it bears, reaches,
/// sees a clear line and has reloaded; closes in while it does not reach.
fn engage(state: &mut State, id: UnitId, (victim, aim): (UnitId, Vec2Fx), shots: ShotBuffers<'_>) {
    let ShotBuffers { events, hits, .. } = shots;
    let drone = state.unit(id).expect("caller checked");
    let (pos, kind) = (drone.pos, drone.kind);
    let stats = kind.stats();
    let weapon = &stats.weapons[0];
    let reaches =
        within_unit_weapon_reach(state, kind, Target::Unit(victim), weapon, pos.dist_sq(aim));
    let clear = clear_line(
        state,
        Fire::Direct,
        ShotEnd::new(pos, Domain::Air),
        ShotEnd::new(aim, Domain::Air),
        Landing::Unchecked,
    );
    let unit = state.unit_mut(id).expect("caller checked");
    let aligned = super::super::super::movement::steer_bearing(
        &mut unit.heading,
        aim - pos,
        stats.turret_turn_rate,
    );
    if !reaches || !clear {
        head_for(state, id, aim);
        return;
    }
    let unit = state.unit_mut(id).expect("caller checked");
    unit.path = None;
    if !aligned || unit.cooldowns[0] > 0 {
        return;
    }
    unit.cooldowns[0] = weapon.cooldown_ticks;
    buffer_shot(
        state,
        Target::Unit(id),
        Target::Unit(victim),
        pos,
        aim,
        weapon,
        hits,
    );
    events.push(Event::AttackHit {
        attacker: id,
        attacker_kind: kind,
        weapon: 0,
        target: Some(Target::Unit(victim)),
        attacker_pos: pos,
        target_pos: aim,
    });
}

/// Routes the drone toward `point`, replanning only once its route ends more
/// than a tile away from it.
fn head_for(state: &mut State, id: UnitId, point: Vec2Fx) {
    let unit = state.unit(id).expect("caller checked");
    let goal = TilePos::containing(point);
    let settled = match &unit.path {
        Some(path) => path.goal.chebyshev(goal) <= 1,
        None => unit.tile().chebyshev(goal) <= 1,
    };
    if settled {
        return;
    }
    let (pos, kind) = (unit.pos, unit.kind);
    let path =
        crate::tick::route_for_position(state, kind, pos, goal).map(|waypoints| PathFollow {
            final_point: None,
            goal,
            waypoints,
            next: 0,
        });
    state.unit_mut(id).expect("caller checked").path = path;
}

/// Puts every hit host, and the host of every hit drone, on alert.
pub(in crate::tick::brain) fn alert_hosts(state: &mut State, hit: &[UnitId]) {
    for id in hit {
        let host = state.unit(*id).and_then(|u| u.host).unwrap_or(*id);
        if let Some(bay) = state.unit_mut(host).and_then(|u| u.bay.as_mut()) {
            bay.alert = RECALL_TICKS;
        }
    }
}

#[cfg(test)]
mod tests;
