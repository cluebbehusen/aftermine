//! Buried-charge triggers and the lifecycle of construction over hidden mines.

use super::fire::{ShotEnd, blast_reaches};
use crate::stats::{
    BuildingKind, CHARGE_BLAST_RADIUS, CHARGE_DAMAGE, CHARGE_TRIGGER_RADIUS, Domain,
};
use crate::{BuildingId, Event, Order, PlayerId, State};
use chassis::fx::Vec2Fx;
use chassis::grid::TilePos;

fn known_charge_in(state: &State, player: PlayerId, kind: BuildingKind, anchor: TilePos) -> bool {
    let (w, h) = kind.size();
    (0..h).any(|dy| (0..w).any(|dx| state.known_charge_at(player, anchor.offset(dx, dy))))
}

pub(super) fn cancel_discovered(state: &mut State, events: &mut Vec<Event>) -> bool {
    let sites: Vec<_> = state
        .buildings
        .iter()
        .filter(|b| b.hp > 0 && b.unstarted() && known_charge_in(state, b.player, b.kind, b.anchor))
        .map(|b| (b.id, b.player, b.stats().construction.expect("site").cost))
        .collect();
    let removed = !sites.is_empty();
    for (site, player, cost) in sites {
        super::commands::cancel_site(state, player, site, cost, events);
    }
    let mut claims = Vec::new();
    for u in state.units.iter().filter(|u| u.hp > 0) {
        for order in std::iter::once(&u.order).chain(&u.queue) {
            if let Order::Found { kind, anchor } = *order
                && known_charge_in(state, u.player, kind, anchor)
                && !claims.contains(&(u.player, kind, anchor))
            {
                claims.push((u.player, kind, anchor));
            }
        }
    }
    for (player, kind, anchor) in claims {
        let _ = super::commands::apply_cancel_found(state, player, kind, anchor, events);
    }
    removed
}

/// Called after the volley and before any hp work is applied. A mine killed
/// by that volley or an earlier blast is destroyed without firing.
pub(super) fn detonate_under_construction(
    state: &mut State,
    starts: &[BuildingId],
    events: &mut Vec<Event>,
) {
    if starts.is_empty() {
        return;
    }
    let triggers: Vec<_> = state
        .buildings
        .iter()
        .enumerate()
        .filter(|(_, mine)| mine.kind == BuildingKind::ScuttleCharge && mine.built() && mine.hp > 0)
        .filter_map(|(slot, mine)| {
            let sites: Vec<_> = starts
                .iter()
                .filter_map(|&id| state.building(id))
                .filter(|b| {
                    b.hp > 0 && state.hostile(mine.player, b.player) && b.contains(mine.anchor)
                })
                .map(|b| (b.id, b.player))
                .collect();
            (!sites.is_empty()).then_some((slot, sites))
        })
        .collect();
    for (slot, sites) in triggers {
        if state.buildings[slot].hp == 0 {
            continue;
        }
        detonate(state, slot, events);
        for (id, player) in sites {
            let site = state.building_mut(id).expect("site survives until cleanup");
            super::damage::building(site, site.hp, events);
            super::commands::clear_site_orders(state, player, id);
        }
    }
}

pub(super) fn detonate_under_units(state: &mut State, events: &mut Vec<Event>) {
    // A blast only lowers hp: nothing moves, lands or changes sides. So the
    // ground bodies gathered before the first charge fires hold every body
    // that can trip any charge later in this pass.
    let mut grounded: Option<Vec<(TilePos, usize)>> = None;
    for slot in 0..state.buildings.len() {
        let b = &state.buildings[slot];
        if b.kind != BuildingKind::ScuttleCharge || !b.built() || b.hp == 0 {
            continue;
        }
        let grounded = grounded.get_or_insert_with(|| ground_bodies(state));
        let (low, high) = trigger_window(b.center());
        let start = grounded.partition_point(|(t, _)| (t.y, t.x) < (low.y, low.x));
        let tripped = grounded[start..]
            .iter()
            .take_while(|(t, _)| t.y <= high.y)
            .filter(|(t, _)| (low.x..=high.x).contains(&t.x))
            .any(|&(_, unit)| trips(state, b, &state.units[unit]));
        if tripped {
            detonate(state, slot, events);
        }
    }
}

/// A charge trips only on a body its blast would reach.
fn trips(state: &State, charge: &crate::Building, u: &crate::state::Unit) -> bool {
    u.hp > 0
        && state.hostile(charge.player, u.player)
        && u.domain() == Domain::Ground
        && u.pos.dist_sq(charge.center()) <= {
            let trigger = CHARGE_TRIGGER_RADIUS + crate::stats::hull_excess(u.kind.stats().radius);
            trigger * trigger
        }
        && blast_reaches(
            &state.map,
            charge.center(),
            charge.anchor,
            ShotEnd::new(u.pos, Domain::Ground),
        )
}

/// Every live ground body's tile and unit slot, sorted by tile.
fn ground_bodies(state: &State) -> Vec<(TilePos, usize)> {
    let mut bodies: Vec<(TilePos, usize)> = state
        .units
        .iter()
        .enumerate()
        .filter(|(_, u)| u.hp > 0 && u.domain() == Domain::Ground)
        .map(|(slot, u)| (u.tile(), slot))
        .collect();
    bodies.sort_unstable_by_key(|&(t, slot)| (t.y, t.x, slot));
    bodies
}

/// The tiles, inclusive, that can hold a body trip-close to `center`: the
/// widest hull's trigger reach plus a tile against fixed-point rounding.
fn trigger_window(center: Vec2Fx) -> (TilePos, TilePos) {
    let reach = CHARGE_TRIGGER_RADIUS + crate::stats::hull_excess(crate::stats::MAX_UNIT_RADIUS);
    let reach = Vec2Fx::new(reach, reach);
    (
        TilePos::containing(center - reach).offset(-1, -1),
        TilePos::containing(center + reach).offset(1, 1),
    )
}

fn detonate(state: &mut State, slot: usize, events: &mut Vec<Event>) {
    let b = &state.buildings[slot];
    let (id, owner, center, anchor) = (b.id, b.player, b.center(), b.anchor);
    let blast_sq = CHARGE_BLAST_RADIUS * CHARGE_BLAST_RADIUS;
    let now = state.current_tick();
    let units_blast = |u: &crate::state::Unit| {
        let blast = CHARGE_BLAST_RADIUS + crate::stats::hull_excess(u.kind.stats().radius);
        u.pos.dist_sq(center) <= blast * blast
    };
    crate::vision::forget_observed_building(state, id, true);
    state.buildings[slot].hp = 0;
    events.push(Event::ChargeDetonated {
        building: id,
        player: owner,
        at: center,
    });
    let map = &state.map;
    for u in &mut state.units {
        if u.hp > 0
            && state.players[owner.0 as usize].team != state.players[u.player.0 as usize].team
            && u.domain() == Domain::Ground
            && units_blast(u)
            && blast_reaches(map, center, anchor, ShotEnd::new(u.pos, Domain::Ground))
        {
            super::damage::unit(u, CHARGE_DAMAGE, now, events);
        }
    }
    for other in &mut state.buildings {
        if other.hp > 0
            && !other.provisional()
            && other.kind.is_stealthy()
            && state.players[owner.0 as usize].team != state.players[other.player.0 as usize].team
            && other.center().dist_sq(center) <= blast_sq
            && blast_reaches(
                map,
                center,
                anchor,
                ShotEnd::new(other.center(), Domain::Ground).on(other.anchor),
            )
        {
            super::damage::building(other, CHARGE_DAMAGE, events);
        }
    }
}

#[cfg(test)]
mod tests;
