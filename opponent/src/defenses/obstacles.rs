//! Obstacles on the way in: Barricades in front of guns and fields of Scuttle
//! Charges across approaches.

use super::approaches::health;
use super::guns::{first_placeable, sites_at};
use super::{ATTACKER_MARGIN, Guard, distance2};
use crate::frame::{doubled, footprint_centre, gap};
use crate::map::Gate;
use aftermine_sim::BuildingKind;
use aftermine_sim::observation::BuildingObs;
use aftermine_sim::stats::{CHARGE_DAMAGE, Domain};
use chassis::grid::TilePos;
use std::cmp::Reverse;

/// Tiles beyond a base's edge along its approach where its Scuttle Charges
/// start.
const MINEFIELD_START: i64 = 3;

/// Tiles either side of a Foundry's straight way in that its Scuttle Charges
/// cover: about a blast wide.
const MINEFIELD_WIDTH: i64 = 2;

/// Empty tiles between a gun and the Barricade in front of it.
const BARRICADE_GAP: i32 = 1;

impl Guard<'_> {
    /// The best spot for a Barricade: a tile's gap in front of an own Turret
    /// or Bastion without one, toward the threat of the building it guards.
    /// Whether it leaves the seat's paths open is checked when it is bought.
    pub(super) fn barricade(&self) -> Option<(TilePos, u64)> {
        let observation = self.observation;
        let barricades: Vec<&BuildingObs> = observation
            .my_buildings
            .iter()
            .filter(|building| building.kind == BuildingKind::Barricade)
            .collect();
        let mut sites = Vec::new();
        for gun in observation.my_buildings.iter().filter(|building| {
            building.built && matches!(building.kind, BuildingKind::Turret | BuildingKind::Bastion)
        }) {
            let size = gun.kind.size();
            let fronted = barricades.iter().any(|barricade| {
                gap(gun.anchor, size, barricade.anchor, (1, 1)) <= BARRICADE_GAP + 1
            });
            if fronted {
                continue;
            }
            let centre = footprint_centre(gun.kind, gun.anchor);
            let Some((asset, approach)) = self
                .assets
                .iter()
                .filter_map(|asset| Some((asset, asset.approach(Domain::Ground)?)))
                .filter(|(_, approach)| self.usable(approach))
                .min_by_key(|(asset, _)| (distance2(asset.centre, centre), asset.centre))
            else {
                continue;
            };
            let worth = asset.value * 2 * self.weight(approach, BuildingKind::Barricade);
            for tile in sites_at(gun.anchor, size, (1, 1), &[BARRICADE_GAP]) {
                let point = doubled(tile);
                let ahead = (point.0 - centre.0) * (approach.source.0 - centre.0)
                    + (point.1 - centre.1) * (approach.source.1 - centre.1);
                if ahead > 0 {
                    sites.push((worth, self.frame.rank(approach.source, point), tile));
                }
            }
        }
        first_placeable(sites, |tile| self.placeable(BuildingKind::Barricade, tile))
    }

    /// The best spot for a Scuttle Charge: on the straight way in to a base or
    /// an Extractor out on its own whose field still falls short of its
    /// threat, nearest its edge. A field holds enough charges to deal the
    /// health of the threat along the way divided by the attacker margin,
    /// one body to a blast, in the share the guns covering the way leave
    /// open; every spot is worth the same until it does.
    pub(super) fn charge(&self) -> Option<(TilePos, u64)> {
        let observation = self.observation;
        let charges: Vec<TilePos> = observation
            .my_buildings
            .iter()
            .filter(|building| building.kind == BuildingKind::ScuttleCharge)
            .map(|building| building.anchor)
            .collect();
        let mut sites = Vec::new();
        for asset in &self.assets {
            let Some(approach) = asset.approach(Domain::Ground) else {
                continue;
            };
            if !self.usable(approach) {
                continue;
            }
            let health = health(self.memory, observation.tick, approach.source, self.stakes);
            // The charges a field needs over the samples `counts`.
            let need = |counts: &dyn Fn(usize) -> bool, share: (u64, u64)| {
                let open: Vec<u64> = approach
                    .open
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| counts(*index))
                    .map(|(_, open)| *open)
                    .collect();
                let open = open.iter().sum::<u64>() / open.len().max(1) as u64;
                (health * 1_000 / ATTACKER_MARGIN * open / 2_000 * share.0 / share.1.max(1))
                    .div_ceil(u64::from(CHARGE_DAMAGE))
            };
            let worth = asset.value * 2 * self.weight(approach, BuildingKind::ScuttleCharge);
            if let Some(cut) = &approach.cut {
                // Each gate holds its share of the threat by width, less what
                // the guns covering its own way on hold.
                let total = cut.gates.iter().map(|gate| gate.width() as u64).sum();
                let rank =
                    |tile: &TilePos| (self.frame.rank(approach.source, doubled(*tile)), *tile);
                let mut short: Vec<(Reverse<u64>, _, &Gate)> = cut
                    .gates
                    .iter()
                    .enumerate()
                    .filter_map(|(index, gate)| {
                        let laid = charges
                            .iter()
                            .filter(|charge| gate.tiles.contains(charge))
                            .count() as u64;
                        let need = need(
                            &|sample| approach.gate_of.get(sample) == Some(&index),
                            (gate.width() as u64, total),
                        );
                        (laid < need).then(|| {
                            (
                                Reverse(need - laid),
                                self.frame.rank(approach.source, gate.centre),
                                gate,
                            )
                        })
                    })
                    .collect();
                short.sort_by_key(|(short, rank, _)| (*short, *rank));
                let tile = short.into_iter().find_map(|(_, _, gate)| {
                    let mut tiles = gate.tiles.clone();
                    tiles.sort_by_key(rank);
                    tiles
                        .into_iter()
                        .find(|tile| self.placeable(BuildingKind::ScuttleCharge, *tile))
                });
                if let Some(tile) = tile {
                    sites.push((Reverse(worth), 0, rank(&tile).0, tile));
                }
                continue;
            }
            let field = Field::of(
                asset.centre,
                approach.source,
                approach.edge + 2 * MINEFIELD_START,
            );
            let laid = charges
                .iter()
                .filter(|charge| field.holds(doubled(**charge)))
                .count() as u64;
            if laid >= need(&|_| true, (1, 1)) {
                continue;
            }
            let Some((row, tile)) = field.first(
                |tile| self.placeable(BuildingKind::ScuttleCharge, tile),
                |tile| self.frame.rank(approach.source, doubled(tile)),
            ) else {
                continue;
            };
            let rank = self.frame.rank(approach.source, doubled(tile));
            sites.push((Reverse(worth), row, rank, tile));
        }
        sites
            .into_iter()
            .min()
            .map(|(Reverse(worth), _, _, tile)| (tile, worth))
    }
}

/// A minefield: a band about a blast wide either side of an asset's straight
/// way in, from a step along it toward the threat, in doubled coordinates.
struct Field {
    centre: (i64, i64),
    /// From the asset's centre to the threat.
    toward: (i64, i64),
    /// The step along the way in where the field starts, as
    /// [`along`](super::approaches::along) measures.
    start: i64,
}

impl Field {
    fn of(centre: (i64, i64), source: (i64, i64), start: i64) -> Self {
        Field {
            centre,
            toward: (source.0 - centre.0, source.1 - centre.1),
            start,
        }
    }

    /// Whether `point` lies past the field's start, short of the threat and
    /// within the band: among the tiles `first` tries, allowing for their
    /// rounding to tile centres.
    fn holds(&self, point: (i64, i64)) -> bool {
        let (dx, dy) = self.toward;
        let (px, py) = (point.0 - self.centre.0, point.1 - self.centre.1);
        let length = dx.abs().max(dy.abs());
        let squared = dx * dx + dy * dy;
        let slack = 2 * (dx.abs() + dy.abs());
        let ahead = px * dx + py * dy;
        let across = (px * dy - py * dx).abs();
        ahead > 0
            && ahead * length >= (self.start - 1) * squared
            && ahead <= squared + slack
            && across * length <= 2 * MINEFIELD_WIDTH * squared + slack * length
    }

    /// The first tile that `fits`, a row at a time from the field's start
    /// outward, as its step and the tile; within a row, the least by `rank`.
    fn first<R: Ord>(
        &self,
        fits: impl Fn(TilePos) -> bool,
        rank: impl Fn(TilePos) -> R,
    ) -> Option<(i64, TilePos)> {
        let (dx, dy) = self.toward;
        let length = dx.abs().max(dy.abs());
        let mut row = self.start;
        while row < length {
            let (ax, ay) = (
                self.centre.0 + dx * row / length,
                self.centre.1 + dy * row / length,
            );
            let mut tiles: Vec<TilePos> = (-MINEFIELD_WIDTH..=MINEFIELD_WIDTH)
                .flat_map(|side| {
                    tiles_at((ax - dy * 2 * side / length, ay + dx * 2 * side / length))
                })
                .collect();
            tiles.sort_by_key(|tile| (rank(*tile), *tile));
            tiles.dedup();
            if let Some(tile) = tiles.into_iter().find(|tile| fits(*tile)) {
                return Some((row, tile));
            }
            row += 2;
        }
        None
    }
}

/// The tiles whose centres lie within half a tile of `point`: one when it
/// falls on a centre, up to four when it falls between them. Choosing among
/// them is left to the mirror-safe ranking.
fn tiles_at(point: (i64, i64)) -> Vec<TilePos> {
    let axis = |value: i64| {
        let half = value.div_euclid(2);
        if value.rem_euclid(2) == 1 {
            vec![half]
        } else {
            vec![half - 1, half]
        }
    };
    let ys = axis(point.1);
    axis(point.0)
        .into_iter()
        .flat_map(|x| {
            ys.iter().map(move |y| {
                TilePos::new(
                    i32::try_from(x).expect("map coordinates fit in i32"),
                    i32::try_from(*y).expect("map coordinates fit in i32"),
                )
            })
        })
        .collect()
}
