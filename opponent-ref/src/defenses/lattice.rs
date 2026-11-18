//! Lattices against warheads: how strongly the seat should expect an enemy
//! Spire, and where a Lattice covers the most.

use super::guns::{first_placeable, sites_around};
use super::{Guard, distance2};
use crate::frame::{centre_of, footprint_centre, tiles_of};
use crate::investments::buildable;
use aftermine_sim::BuildingKind;
use aftermine_sim::observation::ObservationData;
use chassis::grid::TilePos;
use std::cmp::Reverse;

/// A Spire's two kinds of tech: a Deep Bore and a Crucible.
const SPIRE_SIGNALS: u64 = 2;

impl Guard<'_> {
    /// The best spot for a Lattice, as its anchor and worth, while the seat
    /// should expect an enemy Spire (see [`spire_threat`]).
    ///
    /// An asset counts as covered by the own Lattices, built or not, whose
    /// ring holds its costliest point, where one warhead would take the
    /// most: that is where a strike lands. Assets holding fewer interceptors
    /// there than the Spires the seat expects are open; a mere capability
    /// leaves only the most valuable one open. A site is worth the stakes of
    /// the open assets its ring would cover, scaled by how much of a Spire
    /// the seat knows the enemy holds, and ties go to the site nearest home,
    /// so the battery stands behind what it guards.
    pub(super) fn lattice(&self) -> Option<(TilePos, u64)> {
        let observation = self.observation;
        let kind = BuildingKind::Lattice;
        let (expected, every_asset, share) = match spire_threat(observation) {
            SpireThreat::None => return None,
            SpireThreat::Capable { signals } => (1, signals >= SPIRE_SIGNALS, signals),
            SpireThreat::Known(spires) => (spires, true, SPIRE_SIGNALS),
        };
        if !buildable(observation, kind) {
            return None;
        }
        let ring = lattice_ring();
        let reach2 = 4 * ring * ring;
        let capacity = kind.magazine().map_or(0, |stats| u64::from(stats.capacity));
        let lattices: Vec<(i64, i64)> = observation
            .my_buildings
            .iter()
            .filter(|building| building.kind == kind)
            .map(|building| footprint_centre(kind, building.anchor))
            .collect();
        let marks = crate::warheads::by_column(
            observation
                .my_buildings
                .iter()
                .filter(|building| building.built)
                .map(crate::warheads::Mark::of)
                .collect(),
        );
        let mut open: Vec<(u64, (i64, i64))> = self
            .assets
            .iter()
            .filter_map(|asset| {
                // A strike there is aimed at its most valuable building.
                let worth = |anchor: TilePos| {
                    observation
                        .my_buildings
                        .iter()
                        .find(|building| building.anchor == anchor)
                        .map_or(0, |building| building.kind.invested_cost(building.tier))
                };
                let (anchor, size) = asset.members.iter().min_by_key(|(anchor, size)| {
                    (
                        Reverse(worth(*anchor)),
                        self.frame.rank(self.frame.home, centre_of(*anchor, *size)),
                    )
                })?;
                let (stake, aim) = tiles_of(*anchor, *size)
                    .map(|tile| {
                        let aim = (2 * i64::from(tile.x) + 1, 2 * i64::from(tile.y) + 1);
                        (crate::warheads::volley(&marks, tile, 1), aim)
                    })
                    .min_by_key(|(stake, aim)| {
                        (Reverse(*stake), self.frame.rank(self.frame.home, *aim))
                    })?;
                (stake > 0).then_some((stake, aim))
            })
            .collect();
        if !every_asset {
            open.sort_by_key(|(stake, aim)| {
                (Reverse(*stake), self.frame.rank(self.frame.home, *aim))
            });
            open.truncate(1);
        }
        open.retain(|(_, aim)| {
            let held = lattices
                .iter()
                .filter(|lattice| distance2(**lattice, *aim) <= reach2)
                .count() as u64
                * capacity;
            held < expected
        });
        for (stake, _) in &mut open {
            *stake = *stake * share / SPIRE_SIGNALS;
        }
        let aimed = |(anchor, (width, height)): &(TilePos, (i32, i32))| {
            let (x, y) = (2 * i64::from(anchor.x), 2 * i64::from(anchor.y));
            open.iter().any(|(_, aim)| {
                (x..x + 2 * i64::from(*width)).contains(&aim.0)
                    && (y..y + 2 * i64::from(*height)).contains(&aim.1)
            })
        };
        // The ring covers an aim from anywhere beside the building it falls
        // on, so the search starts there, and widens to the rest of that
        // base only when nothing beside it can be placed.
        let members: Vec<&(TilePos, (i32, i32))> = self
            .assets
            .iter()
            .filter(|asset| asset.members.iter().any(aimed))
            .flat_map(|asset| &asset.members)
            .collect();
        let (near, rest): (Vec<_>, Vec<_>) = members.into_iter().partition(|member| aimed(member));
        let site_among = |members: &[&(TilePos, (i32, i32))]| {
            let mut sites = Vec::new();
            // Neighbouring members offer many of the same anchors.
            let mut offered = std::collections::BTreeSet::new();
            for (member, footprint) in members {
                for anchor in sites_around(*member, *footprint, kind.size()) {
                    if !offered.insert(anchor) {
                        continue;
                    }
                    let centre = footprint_centre(kind, anchor);
                    let worth: u64 = open
                        .iter()
                        .filter(|(_, aim)| distance2(*aim, centre) <= reach2)
                        .map(|(stake, _)| stake)
                        .sum();
                    if worth > 0 {
                        sites.push((worth, self.frame.rank(self.frame.home, centre), anchor));
                    }
                }
            }
            first_placeable(sites, |anchor| self.placeable(kind, anchor))
        };
        site_among(&near).or_else(|| site_among(&rest))
    }
}

/// Centres of the enemy Spires the seat knows, seen or remembered, in
/// doubled coordinates.
fn enemy_spires(observation: &ObservationData) -> Vec<(i64, i64)> {
    observation
        .enemy_buildings
        .iter()
        .filter(|building| building.kind == BuildingKind::Spire)
        .map(|building| footprint_centre(building.kind, building.anchor))
        .collect()
}

/// How strongly the seat should expect an enemy Spire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpireThreat {
    /// No enemy is known to hold any of a Spire's tech.
    None,
    /// No Spire is known, but one enemy is known to hold `signals` of the
    /// [`SPIRE_SIGNALS`] kinds of tech a Spire needs.
    Capable { signals: u64 },
    /// Enemy Spires the seat knows, seen or given away by a tracked
    /// warhead.
    Known(u64),
}

/// Every seat sees each Deep Bore, so this needs no scouting for the Deep
/// Bore; a Crucible must have been seen.
fn spire_threat(observation: &ObservationData) -> SpireThreat {
    let known = enemy_spires(observation).len() as u64;
    if known > 0 {
        return SpireThreat::Known(known);
    }
    // Each enemy's known Deep Bore and Crucible, by player index.
    let mut held: Vec<(bool, bool)> = Vec::new();
    let mut note = |owner: aftermine_sim::PlayerId, deep_bore: bool, crucible: bool| {
        let index = usize::from(owner.0);
        if held.len() <= index {
            held.resize(index + 1, (false, false));
        }
        held[index].0 |= deep_bore;
        held[index].1 |= crucible;
    };
    for public in &observation.public_buildings {
        note(public.owner, deep_bore(public.kind, public.tier), false);
    }
    for building in &observation.enemy_buildings {
        note(
            building.player,
            deep_bore(building.kind, building.tier),
            building.kind == BuildingKind::Crucible,
        );
    }
    let signals = held
        .iter()
        .map(|&(deep_bore, crucible)| u64::from(deep_bore) + u64::from(crucible))
        .max()
        .unwrap_or(0);
    if signals == 0 {
        SpireThreat::None
    } else {
        SpireThreat::Capable { signals }
    }
}

/// Where the seat expects warheads to come from: its known enemy Spires,
/// or failing those, the Spire tech it knows enemies hold.
pub(super) fn spire_sources(observation: &ObservationData) -> Vec<(i64, i64)> {
    let spires = enemy_spires(observation);
    if !spires.is_empty() {
        return spires;
    }
    observation
        .enemy_buildings
        .iter()
        .filter(|building| {
            building.kind == BuildingKind::Crucible || deep_bore(building.kind, building.tier)
        })
        .map(|building| footprint_centre(building.kind, building.anchor))
        .chain(
            observation
                .public_buildings
                .iter()
                .filter(|public| deep_bore(public.kind, public.tier))
                .map(|public| footprint_centre(public.kind, public.anchor)),
        )
        .collect()
}

/// Whether a building of `kind` at `tier` is a Deep Bore.
fn deep_bore(kind: BuildingKind, tier: u8) -> bool {
    kind == BuildingKind::Extractor && tier > 0
}

/// A Lattice's ring, in whole tiles.
pub(super) fn lattice_ring() -> i64 {
    BuildingKind::Lattice
        .battery()
        .map_or(0, |battery| battery.ring.to_num::<i64>())
}
