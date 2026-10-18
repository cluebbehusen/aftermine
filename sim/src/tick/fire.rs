//! Whether a shot has a clear line. Every gate that decides a shot asks
//! here: unit attacks and opportunist fire, turrets, sidearms, mounted guns,
//! drones, blind fire, firing stands, and the frontage a crowd holds. A rule
//! about what stops a shot therefore lives in one place.

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

/// One end of a shot: the point it leaves or lands at, and the domain of
/// the body there.
#[derive(Clone, Copy, Debug)]
pub(in crate::tick) struct ShotEnd {
    pub(in crate::tick) at: Vec2Fx,
    pub(in crate::tick) domain: Domain,
}

impl ShotEnd {
    pub(in crate::tick) fn new(at: Vec2Fx, domain: Domain) -> Self {
        Self { at, domain }
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

/// Whether a shot's line may pass over `t`. Peaks wall every pairing and
/// every arc; full-cover tracing additionally respects terrain that grants
/// cover. Pits block neither.
fn crosses(state: &State, t: TilePos, full: bool) -> bool {
    state.map.tile(t).is_some_and(|tile| {
        !tile.terrain.blocks_all_fire() && (!full || !tile.terrain.blocks_direct_fire())
    })
}

/// Whether a shot of `fire` from `from` reaches `to` unblocked.
pub(in crate::tick) fn clear_line(
    state: &State,
    fire: Fire,
    from: ShotEnd,
    to: ShotEnd,
    landing: Landing,
) -> bool {
    let full = traces_terrain(fire, from.domain, to.domain);
    let open = |t: TilePos| crosses(state, t, full);
    (landing == Landing::Unchecked || open(TilePos::containing(to.at)))
        && !chassis::path::line_blocked(from.at, to.at, open)
}

#[cfg(test)]
mod tests;
