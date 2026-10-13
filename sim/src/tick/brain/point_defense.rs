//! Point defense: charged defenders shoot down hostile projectiles in flight.
//!
//! The pass runs against the same start-of-tick world the volley uses, just
//! before arrivals land, so a shell landing this tick can still be stopped
//! on its final tick. Shells launched this tick are not yet in flight.

use crate::event::Event;
use crate::state::State;
use chassis::fx::Fx;

/// Restores spent charges, then lets every charged defender shoot down the
/// hostile shells inside its reach.
pub(super) fn run(state: &mut State, events: &mut Vec<Event>) {
    recharge(state);
    intercept(state, events);
}

/// Each live defender below full counts toward its next charge, one charge
/// at a time; a full defender holds its counter at zero.
fn recharge(state: &mut State) {
    for unit in &mut state.units {
        let Some(stats) = unit.kind.stats().point_defense else {
            continue;
        };
        let Some(held) = &mut unit.point_defense else {
            continue;
        };
        if unit.hp == 0 || held.charges >= stats.charges {
            continue;
        }
        held.recharge += 1;
        if held.recharge >= stats.recharge_ticks {
            held.charges += 1;
            held.recharge = 0;
        }
    }
}

/// A defender that can still spend a charge this tick.
struct Defender {
    slot: usize,
    pos: chassis::fx::Vec2Fx,
    radius_sq: Fx,
    rotated_frame: bool,
}

/// Assigns charges to shells greedily in one complete order: earliest launch
/// first, then the pair whose shell is nearest the defender, then whose aim
/// point is nearest it. Remaining geometric ties compare the shell's offset in
/// the defender's own map frame, so point-mirrored scenes choose mirrored
/// pairs. Heavier shells come before lighter ones, and only exactly
/// coincident pairs fall back to list order, which within one seat is launch
/// order for shells and id order for units. The greedy pass is not a maximal
/// matching: a defender may spend its charge on a shell a second defender
/// could also have stopped.
fn intercept(state: &mut State, events: &mut Vec<Event>) {
    let now = state.tick;
    let defenders: Vec<Defender> = state
        .units
        .iter()
        .enumerate()
        .filter_map(|(slot, unit)| {
            let stats = unit.kind.stats().point_defense?;
            let held = unit.point_defense?;
            (unit.hp > 0 && held.charges > 0).then(|| Defender {
                slot,
                pos: unit.pos,
                radius_sq: stats.radius * stats.radius,
                rotated_frame: crate::tick::movement::uses_rotated_map_frame(state, unit.pos),
            })
        })
        .collect();
    if defenders.is_empty() || state.shells.is_empty() {
        return;
    }
    let mut pairs = Vec::new();
    for (shell_slot, shell) in state.shells.iter().enumerate() {
        let flight = shell.flight_position(now);
        for (defender_slot, defender) in defenders.iter().enumerate() {
            let owner = state.units[defender.slot].player;
            let flight_sq = defender.pos.dist_sq(flight);
            let aim_sq = defender.pos.dist_sq(shell.impact);
            if !state.hostile(shell.player, owner)
                || flight_sq > defender.radius_sq
                || aim_sq > defender.radius_sq
            {
                continue;
            }
            let offset = if defender.rotated_frame {
                defender.pos - flight
            } else {
                flight - defender.pos
            };
            pairs.push((
                (
                    shell.launched_at,
                    flight_sq,
                    aim_sq,
                    offset.x,
                    offset.y,
                    std::cmp::Reverse(shell.damage),
                    shell_slot,
                    defender_slot,
                ),
                shell_slot,
                defender_slot,
            ));
        }
    }
    pairs.sort_unstable_by_key(|(key, _, _)| *key);
    let mut stopped = vec![false; state.shells.len()];
    for (_, shell_slot, defender_slot) in pairs {
        let slot = defenders[defender_slot].slot;
        let unit = &mut state.units[slot];
        let held = unit
            .point_defense
            .as_mut()
            .expect("defenders carry point defense");
        if stopped[shell_slot] || held.charges == 0 {
            continue;
        }
        held.charges -= 1;
        stopped[shell_slot] = true;
        let shell = &state.shells[shell_slot];
        events.push(Event::ShellIntercepted {
            defender: unit.id,
            defender_player: unit.player,
            defender_pos: unit.pos,
            player: shell.player,
            shooter: shell.shooter,
            kind: shell.kind,
            at: shell.flight_position(now),
            impact: shell.impact,
            arrival: shell.arrival,
        });
    }
    let mut slot = 0;
    state.shells.retain(|_| {
        let keep = !stopped[slot];
        slot += 1;
        keep
    });
}

#[cfg(test)]
mod tests;
