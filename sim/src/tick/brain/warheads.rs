//! Silo decks, warhead interception, and warhead impact.
//!
//! A warhead launched this tick joins the world after these passes, so
//! no battery can stop it on its launch tick and it lands no sooner than
//! the next one. Interception reads the team tracking recorded at the end
//! of the previous tick and runs before landing, so a warhead is stoppable
//! up to its arrival tick.

use super::PendingHit;
use crate::event::Event;
use crate::ids::Target;
use crate::state::{State, Warhead};
use crate::stats::WARHEAD;

/// Each built silo with an ordered strike turns its deck toward the tile
/// and, once aligned, launches into `launches`.
pub(super) fn aim_and_launch(
    state: &mut State,
    launches: &mut Vec<Warhead>,
    events: &mut Vec<Event>,
) {
    for index in 0..state.buildings.len() {
        let b = &state.buildings[index];
        let (Some(silo), Some(launcher)) = (b.kind.silo(), b.launcher) else {
            continue;
        };
        let Some(target) = launcher.target.filter(|_| b.built()) else {
            continue;
        };
        let (from, to) = (b.center(), target.center());
        let mut heading = launcher.heading;
        let aligned = crate::tick::movement::steer_bearing(&mut heading, to - from, silo.turn_rate);
        let b = &mut state.buildings[index];
        let launcher = b.launcher.as_mut().expect("checked above");
        launcher.heading = heading;
        if !aligned {
            continue;
        }
        launcher.target = None;
        b.magazine
            .as_mut()
            .expect("a strike is held only while loaded")
            .loaded -= 1;
        let (player, silo) = (b.player, b.id);
        let warhead = state.mint_warhead_id();
        let arrival = state.tick + WARHEAD.flight_ticks(from, to);
        launches.push(Warhead {
            id: warhead,
            player,
            silo,
            launch: from,
            impact: to,
            launched_at: state.tick,
            arrival,
        });
        events.push(Event::WarheadLaunched {
            warhead,
            silo,
            player,
            from,
            to,
            arrival,
        });
    }
}

/// Loaded batteries spend interceptors on the hostile warheads their team
/// has tracked, once a warhead whose impact lies in a ring stands inside
/// it: the ring crossing, or the tick tracking completes inside. Several
/// batteries and warheads resolve as one ordering (see
/// [`crate::strike::assign`]).
pub(super) fn intercept(state: &mut State, events: &mut Vec<Event>) {
    if state.warheads.is_empty() {
        return;
    }
    let engagements = crate::strike::assign(state, state.tick, crate::strike::Scope::Now);
    let mut stopped = vec![false; state.warheads.len()];
    for engagement in engagements {
        stopped[engagement.warhead] = true;
        let b = &mut state.buildings[engagement.battery];
        let magazine = b.magazine.as_mut().expect("batteries keep a magazine");
        magazine.loaded -= 1;
        let warhead = &state.warheads[engagement.warhead];
        events.push(Event::WarheadIntercepted {
            warhead: warhead.id,
            player: warhead.player,
            battery: b.id,
            battery_player: b.player,
            pod: magazine.loaded,
            at: engagement.at,
            impact: warhead.impact,
        });
    }
    let mut flags = stopped.into_iter();
    state
        .warheads
        .retain(|_| !flags.next().expect("one flag per warhead"));
}

/// Arrived warheads burst: every hostile building, measured to its
/// footprint, and every hostile unit, in the air or on the ground, takes
/// the falloff damage at its distance, buildings after their resistance.
pub(super) fn land(state: &mut State, hits: &mut Vec<PendingHit>, events: &mut Vec<Event>) {
    let now = state.tick;
    let (due, flying): (Vec<Warhead>, Vec<Warhead>) = std::mem::take(&mut state.warheads)
        .into_iter()
        .partition(|warhead| warhead.arrival <= now);
    state.warheads = flying;
    for warhead in due {
        events.push(Event::WarheadLanded {
            warhead: warhead.id,
            player: warhead.player,
            at: warhead.impact,
        });
        let attacker = Target::Building(warhead.silo);
        for b in &state.buildings {
            if b.hp == 0 || b.provisional() || !state.hostile(warhead.player, b.player) {
                continue;
            }
            let distance = b.closest_point_to(warhead.impact).dist(warhead.impact);
            let damage = b.kind.warhead_taken(WARHEAD.damage_at(distance));
            if damage > 0 {
                hits.push(PendingHit::along(
                    state,
                    attacker,
                    Target::Building(b.id),
                    damage,
                    warhead.launch,
                    warhead.impact,
                ));
            }
        }
        for u in &state.units {
            if u.hp == 0 || !state.hostile(warhead.player, u.player) {
                continue;
            }
            let damage = WARHEAD.damage_at(u.pos.dist(warhead.impact));
            if damage > 0 {
                hits.push(PendingHit::along(
                    state,
                    attacker,
                    Target::Unit(u.id),
                    damage,
                    warhead.launch,
                    warhead.impact,
                ));
            }
        }
    }
}
