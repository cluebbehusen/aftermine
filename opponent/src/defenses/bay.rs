//! Repair Bay siting: where a Bay's aura reaches the most wounds.

use super::guns::{first_placeable, sites_around};
use super::{Guard, STANDOFF};
use crate::frame::{centre_of, doubled, footprint_centre};
use crate::workers;
use aftermine_sim::BuildingKind;
use aftermine_sim::observation::UnitObs;
use aftermine_sim::stats::{FOUNDRY_REPAIR_PRICE, REPAIR_BAY_RADIUS};
use chassis::grid::TilePos;

/// Scrap of missing health a Repair Bay's aura must reach to be worth
/// building.
const BAY_WOUNDS: u64 = 300;

impl Guard<'_> {
    /// The best spot for a Repair Bay beside a guarded building: where its
    /// aura reaches the most missing value among the seat's idle units wounded
    /// enough to go to a Bay and damaged buildings that no Repair Bay treats
    /// yet, as its anchor and that value. A Bay treats the buildings it reaches and as many of the
    /// most wounded units in reach as it has patient slots, after earlier
    /// Bays took theirs.
    pub(super) fn bay(&self) -> Option<(TilePos, u64)> {
        let observation = self.observation;
        let size = BuildingKind::RepairBay.size();
        let anchors: Vec<TilePos> = observation
            .my_buildings
            .iter()
            .filter(|building| building.kind == BuildingKind::RepairBay)
            .map(|building| building.anchor)
            .collect();
        let bays: Vec<Span> = anchors
            .iter()
            .map(|anchor| Span::of(size, *anchor))
            .collect();
        // Only units resting with wounds deep enough to send them to a Bay
        // call for one: a Bay placed by units passing through stands empty
        // once they move on.
        let hurt: Vec<&UnitObs> = observation
            .my_units
            .iter()
            .filter(|unit| {
                let max = u64::from(unit.kind.stats().max_hp);
                unit.idle && u64::from(unit.hp) * 1_000 <= max * (1_000 - workers::UNIT_WELD_DAMAGE)
            })
            .collect();
        let treated = crate::missions::treating(&anchors, &hurt);
        let units = hurt
            .iter()
            .zip(&treated)
            .filter(|(_, treated)| treated.is_none())
            .map(|(unit, _)| {
                let stats = unit.kind.stats();
                (Span::point(unit.tile), stats.cost, unit.hp, stats.max_hp)
            });
        let buildings = observation
            .my_buildings
            .iter()
            .filter(|building| building.built && building.kind != BuildingKind::RepairBay)
            .map(|building| {
                let stats = building.kind.tier_stats(building.tier);
                let price = building
                    .kind
                    .base_stats()
                    .construction
                    .as_ref()
                    .map_or(FOUNDRY_REPAIR_PRICE, |construction| construction.cost);
                let span = Span::of(building.kind.size(), building.anchor);
                (span, price, building.hp, stats.max_hp)
            })
            .filter(|(span, _, _, _)| !bays.iter().any(|bay| bay.aura(*span)));
        let wounds: Vec<(Span, u64)> = units
            .chain(buildings)
            .filter(|(_, _, hp, max)| hp < max)
            .map(|(span, price, hp, max)| {
                let max = u64::from(max.max(1));
                let missing = u64::from(price) * (max - u64::from(hp).min(max)) / max;
                (span, missing)
            })
            .collect();
        if wounds.iter().map(|(_, missing)| missing).sum::<u64>() < BAY_WOUNDS {
            return None;
        }
        let mut sites = Vec::new();
        let margin = 2 * i64::from(STANDOFF[1] + size.0.max(size.1));
        for (member, footprint) in self.assets.iter().flat_map(|asset| &asset.members) {
            // Every site around the building lies within this span.
            let span = Span::of(*footprint, *member);
            let around = Span {
                x: (span.x.0 - margin, span.x.1 + margin),
                y: (span.y.0 - margin, span.y.1 + margin),
            };
            let near: Vec<&(Span, u64)> = wounds
                .iter()
                .filter(|(span, _)| around.aura(*span))
                .collect();
            if near.iter().map(|(_, missing)| missing).sum::<u64>() < BAY_WOUNDS {
                continue;
            }
            let at = centre_of(*member, *footprint);
            for anchor in sites_around(*member, *footprint, size) {
                let bay = Span::of(size, anchor);
                let reached: u64 = near
                    .iter()
                    .filter(|(span, _)| bay.aura(*span))
                    .map(|(_, missing)| missing)
                    .sum();
                if reached >= BAY_WOUNDS {
                    let centre = footprint_centre(BuildingKind::RepairBay, anchor);
                    sites.push((reached, self.frame.rank(at, centre), anchor));
                }
            }
        }
        first_placeable(sites, |anchor| {
            self.placeable(BuildingKind::RepairBay, anchor)
        })
    }
}

/// A footprint's extent, or a unit's centre, in doubled coordinates.
#[derive(Clone, Copy)]
pub(crate) struct Span {
    pub(super) x: (i64, i64),
    pub(super) y: (i64, i64),
}

impl Span {
    /// The centre of `tile`.
    pub(crate) fn point(tile: TilePos) -> Self {
        let (x, y) = doubled(tile);
        Span {
            x: (x, x),
            y: (y, y),
        }
    }

    pub(crate) fn of(size: (i32, i32), anchor: TilePos) -> Self {
        let (x, y) = (2 * i64::from(anchor.x), 2 * i64::from(anchor.y));
        Span {
            x: (x, x + 2 * i64::from(size.0)),
            y: (y, y + 2 * i64::from(size.1)),
        }
    }

    /// Whether a Repair Bay over this span heals `other`: as the simulation
    /// measures, the straight distance between their nearest edges is within
    /// the aura's radius.
    pub(crate) fn aura(self, other: Span) -> bool {
        let apart = |a: (i64, i64), b: (i64, i64)| (a.0 - b.1).max(b.0 - a.1).max(0);
        let (dx, dy) = (apart(self.x, other.x), apart(self.y, other.y));
        let reach = (REPAIR_BAY_RADIUS + REPAIR_BAY_RADIUS).to_num::<i64>();
        dx * dx + dy * dy <= reach * reach
    }
}
