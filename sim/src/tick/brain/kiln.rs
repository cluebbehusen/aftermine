//! Kilns: machines that heal only by eating wreck salvage around their hulls.

use super::PendingUnitHeal;
use crate::ids::UnitId;
use crate::state::State;
use crate::stats::{KILN_BITE_PERIOD, Mend};
use chassis::grid::TilePos;

impl State {
    /// The wreck tile a wounded kiln machine bites next, if any: within its
    /// intake (the tiles under its hull and its kiln's reach past them), the
    /// nearest wreck, then the richest, then tile order in the machine's
    /// half-turn frame so mirrored machines eat mirrored tiles. `None` for a
    /// machine at full health, a dead one, or one without a kiln.
    pub fn kiln_fuel(&self, unit: UnitId) -> Option<TilePos> {
        let unit = self.unit(unit).filter(|u| u.hp > 0)?;
        let stats = unit.kind.stats();
        let Mend::Kiln(kiln) = stats.mend else {
            return None;
        };
        if unit.hp >= stats.max_hp {
            return None;
        }
        let rotated = super::super::movement::uses_rotated_map_frame(self, unit.pos);
        let (mut low, mut high) = (unit.tile(), unit.tile());
        for tile in crate::geometry::body_tiles(unit.pos, stats.radius) {
            low = TilePos::new(low.x.min(tile.x), low.y.min(tile.y));
            high = TilePos::new(high.x.max(tile.x), high.y.max(tile.y));
        }
        super::first_fuel(
            self,
            (
                low.offset(-kiln.reach, -kiln.reach),
                high.offset(kiln.reach, kiln.reach),
            ),
            rotated,
            |tile| Some(unit.pos.dist_sq(tile.center())),
        )
    }
}

/// Every [`KILN_BITE_PERIOD`] ticks, every wounded kiln machine bites one
/// unit of wreck from [`State::kiln_fuel`] and buffers the heal it buys.
/// The wreck leaves the salvage pool at once, so a machine the volley kills
/// still consumed it, as a welder's prepaid scrap is spent. Machines bite in
/// unit order, reversed on alternate pulses so neither of two machines
/// sharing a wreck holds a standing first bite: with an even period, tick
/// parity would never alternate.
pub(super) fn feed(state: &mut State, heals: &mut Vec<PendingUnitHeal>) {
    let tick = state.current_tick();
    if !tick.is_multiple_of(KILN_BITE_PERIOD) {
        return;
    }
    let mut eaters: Vec<(UnitId, u32)> = state
        .units
        .iter()
        .filter_map(|u| match u.kind.stats().mend {
            Mend::Kiln(kiln) => Some((u.id, kiln.hp_per_wreck)),
            Mend::Repair | Mend::Dock => None,
        })
        .collect();
    if (tick / KILN_BITE_PERIOD) % 2 == 1 {
        eaters.reverse();
    }
    for (id, hp_per_wreck) in eaters {
        let Some(tile) = state.kiln_fuel(id) else {
            continue;
        };
        if state.map.extract_wreck(tile).is_none() {
            continue;
        }
        let player = state.unit(id).expect("fed machine").player;
        heals.push(PendingUnitHeal {
            unit: id,
            step: hp_per_wreck,
            player,
            paid: 0,
            source: crate::event::UnitRepairSource::Kiln { tile },
        });
    }
}
