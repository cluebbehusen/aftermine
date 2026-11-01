//! Finds where a match's fighting is, to choose what a viewer is shown.

use aftermine_sim::State;
use std::collections::BTreeMap;

/// The ground one screen of a match holds, in tiles.
const VIEW: (i32, i32) = (44, 26);
/// Tiles between the views tried.
const STEP: i32 = 4;

/// The busiest view of a fight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fight {
    /// The view's center tile.
    pub center: (i32, i32),
    /// Armed units on the side with the second most in view: how many a
    /// fight there could actually bring against the largest side.
    pub opposed: u32,
}

/// The view with the most opposed armed units, or `None` when no view holds
/// armed units of two teams.
#[must_use]
pub fn fiercest(state: &State) -> Option<Fight> {
    let armed: Vec<_> = state
        .units()
        .iter()
        .filter(|unit| unit.kind.stats().can_fight())
        .map(|unit| {
            let tile = unit.tile();
            (tile.x, tile.y, state.player(unit.player).team)
        })
        .collect();
    let map = state.map();
    let mut best: Option<Fight> = None;
    for y in (0..map.height()).step_by(STEP as usize) {
        for x in (0..map.width()).step_by(STEP as usize) {
            let mut teams = BTreeMap::new();
            for (ux, uy, team) in &armed {
                if (ux - x).abs() <= VIEW.0 / 2 && (uy - y).abs() <= VIEW.1 / 2 {
                    *teams.entry(*team).or_insert(0u32) += 1;
                }
            }
            let mut counts: Vec<u32> = teams.into_values().collect();
            counts.sort_unstable_by(|a, b| b.cmp(a));
            let opposed = counts.get(1).copied().unwrap_or(0);
            if opposed > best.map_or(0, |fight| fight.opposed) {
                best = Some(Fight {
                    center: (x, y),
                    opposed,
                });
            }
        }
    }
    best
}

#[cfg(test)]
mod tests;
