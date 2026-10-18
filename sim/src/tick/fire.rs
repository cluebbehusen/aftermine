//! Whether a shot has a clear line. Every gate that decides a shot asks
//! here: unit attacks and opportunist fire, turrets, sidearms, mounted guns,
//! drones, blind fire, firing stands, and the frontage a crowd holds. A rule
//! about what stops a shot therefore lives in one place.
//!
//! On a tiered map, levels gate ground-to-ground fire. Direct fire needs ends
//! that share a level, which a ramp shares with the levels above and below
//! it, and passes over nothing higher than both ends. Indirect fire arcs over
//! anything but never lands above the shooter's highest level. Shots
//! involving aircraft ignore levels.

use crate::state::State;
use crate::stats::{Domain, WeaponStats};
use chassis::fx::Vec2Fx;
use chassis::grid::TilePos;

/// How a weapon's shot travels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tick) enum Fire {
    /// A straight line from muzzle to target.
    Direct,
    /// An arcing shell that passes over cover.
    Indirect,
}

impl Fire {
    pub(in crate::tick) fn of(weapon: &WeaponStats) -> Self {
        if weapon.indirect {
            Self::Indirect
        } else {
            Self::Direct
        }
    }
}

/// One end of a shot: the point it leaves or lands at, the domain of the
/// body there, and the tile whose level that body stands at.
#[derive(Clone, Copy, Debug)]
pub(in crate::tick) struct ShotEnd {
    pub(in crate::tick) at: Vec2Fx,
    pub(in crate::tick) domain: Domain,
    footing: TilePos,
}

impl ShotEnd {
    /// A body at `at`, standing at the level of the tile there.
    pub(in crate::tick) fn new(at: Vec2Fx, domain: Domain) -> Self {
        Self {
            at,
            domain,
            footing: TilePos::containing(at),
        }
    }

    /// The same end, standing at the level of `tile`. A building's aim point
    /// sits on its footprint's edge and can floor into the tile beside it,
    /// so a building stands on its anchor.
    pub(in crate::tick) fn on(self, tile: TilePos) -> Self {
        Self {
            footing: tile,
            ..self
        }
    }
}

/// Whether the tile a shot lands on must be open as well as its line.
///
/// The line trace skips both end tiles. A unit's aim point is its own
/// standable tile, but a building's closest-footprint aim is an exact edge
/// coordinate that can floor into the tile beside it, which flush against a
/// peak is the mountain itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tick) enum Landing {
    Checked,
    Unchecked,
}

/// Whether full terrain cover applies to a shot: only direct fire between
/// two ground parties traces rock; shots involving aircraft and indirect
/// shells pass over it. Buildings never block fire. Peaks block every shot
/// regardless.
fn traces_terrain(fire: Fire, shooter: Domain, victim: Domain) -> bool {
    fire == Fire::Direct && shooter == Domain::Ground && victim == Domain::Ground
}

/// A ceiling no level reaches.
const OPEN_SKY: u8 = u8::MAX;

/// The highest level a shot may pass over, or `None` when the levels at its
/// ends refuse it outright (see the module docs).
fn ceiling(state: &State, fire: Fire, from: ShotEnd, to: ShotEnd) -> Option<u8> {
    if from.domain != Domain::Ground || to.domain != Domain::Ground {
        return Some(OPEN_SKY);
    }
    let (Some(shooter), Some(target)) = (
        state.map.elevation(from.footing),
        state.map.elevation(to.footing),
    ) else {
        return Some(OPEN_SKY);
    };
    let ((shooter_low, shooter_high), (target_low, target_high)) = (shooter.span(), target.span());
    match fire {
        Fire::Indirect => (target_low <= shooter_high).then_some(OPEN_SKY),
        Fire::Direct => (shooter_low <= target_high && target_low <= shooter_high)
            .then_some(shooter_high.max(target_high)),
    }
}

/// Whether the levels at a shot's ends allow it, wherever the line between
/// them runs: the verdict a unit weighs before it moves.
pub(in crate::tick) fn levels_allow(state: &State, fire: Fire, from: ShotEnd, to: ShotEnd) -> bool {
    ceiling(state, fire, from, to).is_some()
}

/// Whether a shot's line may pass over `t`. Peaks wall every pairing and
/// every arc; full-cover tracing additionally respects terrain that grants
/// cover. Pits block neither. Ground above `ceiling` blocks too.
fn crosses(state: &State, t: TilePos, full: bool, ceiling: u8) -> bool {
    state.map.tile(t).is_some_and(|tile| {
        !tile.terrain.blocks_all_fire() && (!full || !tile.terrain.blocks_direct_fire())
    }) && (ceiling == OPEN_SKY
        || state
            .map
            .elevation(t)
            .is_none_or(|level| level.span().0 <= ceiling))
}

/// Whether a shot of `fire` from `from` reaches `to` unblocked.
pub(in crate::tick) fn clear_line(
    state: &State,
    fire: Fire,
    from: ShotEnd,
    to: ShotEnd,
    landing: Landing,
) -> bool {
    let Some(ceiling) = ceiling(state, fire, from, to) else {
        return false;
    };
    let full = traces_terrain(fire, from.domain, to.domain);
    let open = |t: TilePos| crosses(state, t, full, ceiling);
    (landing == Landing::Unchecked || open(TilePos::containing(to.at)))
        && !chassis::path::line_blocked(from.at, to.at, open)
}

#[cfg(test)]
mod tests;
