//! Gun sites: the best placeable spot for a Turret, Bastion, Flak Turret or
//! Array, and what upgrading a defense adds.

use super::approaches::{ahead, furthest};
use super::{
    Approach, Asset, Cover, Evidence, GUN_POINTS, Guard, POINTS, QUOTE, STANDOFF, distance2,
    domain, points,
};
use crate::frame::{footprint_centre, gap, tiles_of};
use crate::investments::Investment;
use crate::investments::buildable;
use crate::map::{Gate, MapModel};
use chassis::fx::Fx;
use chassis::grid::TilePos;
use oxide_sim::observation::{BuildingObs, ObservationData};
use oxide_sim::stats::{BuildingStats, Domain, WeaponStats};
use oxide_sim::{BuildingKind, PlayerId, UnitKind};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Tiles on its home side inside which defenses holding a gate stand.
const GATE_DEPTH: u16 = 4;

/// Tiles beyond its reach inside which a threat keeps a defense from
/// upgrading: the building is down to a fifth of its health until done.
const UPGRADE_CLEARANCE: i32 = 3;

/// How many enemies a splash shell hits, in thousandths, against a spread
/// enemy and a clustered one. A spread enemy stands too far apart for a
/// shell to hit two; a clustered one, in a column or a clump, counts three.
pub(super) const SPLASH_TARGETS: [u64; 2] = [1_000, 3_000];

impl Guard<'_> {
    /// The best site for `kind` guarding any asset, as its anchor and worth:
    /// asset value times gain times evidence, placeable as far as the seat
    /// knows and not recently refused. Equal sites go to the one nearest a
    /// threat the seat has seen, or nearest the building when the threat is
    /// only a guess from public facts. Sites are tried best first, so few
    /// placement checks run.
    pub(super) fn best(&self, kind: BuildingKind) -> Option<(TilePos, u64)> {
        self.best_where(kind, |_| true)
    }

    /// The best site for `kind` guarding one of the assets `guarded` allows.
    pub(super) fn best_where(
        &self,
        kind: BuildingKind,
        guarded: impl Fn(&Asset) -> bool,
    ) -> Option<(TilePos, u64)> {
        if !buildable(self.observation, kind) {
            return None;
        }
        let reach = Cover::of(kind, 0, TilePos::new(0, 0));
        let indirect = reach.is_some_and(|reach| reach.indirect);
        // Each asset with the most any site beside it could be worth: every
        // sample still open, or every far point nobody sees.
        let mut candidates: Vec<(u64, &Asset, &Approach)> = Vec::new();
        for asset in self.assets.iter().filter(|asset| guarded(asset)) {
            // Radar watches for aircraft as well as ground units, so an
            // Array faces the air approach where no ground one is known.
            let approach = asset
                .approach(domain(kind))
                .or_else(|| (kind == BuildingKind::Array).then(|| asset.approach(Domain::Air))?);
            let Some(approach) = approach else {
                continue;
            };
            let most = if kind == BuildingKind::Array {
                2_000 * Self::watched(asset, |_, seen| !seen)
            } else {
                approach.short(|_, spotted| !indirect || spotted, |_| u64::MAX)
            };
            if !self.usable(approach) || most == 0 {
                continue;
            }
            let bound = asset.value * most * self.weight(approach, kind) / 1_000;
            candidates.push((bound, asset, approach));
        }
        candidates.sort_by_key(|(bound, _, _)| Reverse(*bound));
        // Sites best worth first, then by rank. Once the best placeable one
        // is worth more than any asset still to come could offer, it wins.
        let mut sites: BinaryHeap<Reverse<(Reverse<u64>, _, TilePos)>> = BinaryHeap::new();
        for (bound, asset, approach) in candidates {
            while let Some(Reverse((Reverse(worth), _, anchor))) = sites.peek() {
                if !self.placeable(kind, *anchor) {
                    sites.pop();
                } else if *worth > bound {
                    return Some((*anchor, *worth));
                } else {
                    break;
                }
            }
            for anchor in guard_sites(self.map, self.observation.me, asset, approach, kind) {
                let centre = footprint_centre(kind, anchor);
                let worth = asset.value
                    * Self::gain(self.map, asset, kind, anchor, reach)
                    * self.weight(approach, kind)
                    / 1_000;
                if worth > 0 {
                    let toward = match approach.evidence {
                        Evidence::Landing | Evidence::Prior => asset.centre,
                        Evidence::Remembered | Evidence::Current => approach.source,
                    };
                    let rank = self.frame.rank(toward, centre);
                    sites.push(Reverse((Reverse(worth), rank, anchor)));
                }
            }
        }
        while let Some(Reverse((Reverse(worth), _, anchor))) = sites.pop() {
            if self.placeable(kind, anchor) {
                return Some((anchor, worth));
            }
        }
        None
    }
}

/// The first of `sites`, best worth first and then by rank, that
/// `placeable` allows, with its worth. A heap yields them in that order
/// without sorting the many that are never tried.
pub(super) fn first_placeable<R: Ord>(
    sites: Vec<(u64, R, TilePos)>,
    placeable: impl Fn(TilePos) -> bool,
) -> Option<(TilePos, u64)> {
    let mut heap: BinaryHeap<Reverse<(Reverse<u64>, R, TilePos)>> = sites
        .into_iter()
        .map(|(worth, rank, anchor)| Reverse((Reverse(worth), rank, anchor)))
        .collect();
    while let Some(Reverse((Reverse(worth), _, anchor))) = heap.pop() {
        if placeable(anchor) {
            return Some((anchor, worth));
        }
    }
    None
}

/// An upgrade for a built defense, worth what the strength it adds closes at
/// the approach samples the next tier covers, per scrap, or for an Array the
/// far points its radar watches, each by how sure the seat is of the threat,
/// unless an enemy in sight could hit it while it is down, from the
/// defense's reach or its own, or the next tier needs a building the seat
/// has not built.
pub(super) fn upgrade(
    guard: &Guard<'_>,
    building: &BuildingObs,
    weight: u64,
) -> Option<(Investment, u32)> {
    let observation = guard.observation;
    if !building.built {
        return None;
    }
    let stats = building.kind.tier_stats(building.tier);
    let cover = Cover::of(building.kind, building.tier, building.anchor);
    let array = building.kind == BuildingKind::Array;
    if cover.is_none() && !array {
        return None;
    }
    let next = building.kind.upgrade_from(building.tier)?;
    let ready = built_all(observation, next.requires);
    let reach = stats
        .weapons
        .iter()
        .map(|weapon| weapon.range.ceil().to_num::<i32>())
        .max()
        .unwrap_or(0);
    let threatened = observation.enemy_units.iter().any(|enemy| {
        let strike = super::reach(enemy.kind);
        if strike == Fx::ZERO {
            return false;
        }
        let strike = strike.ceil().to_num::<i32>();
        gap(building.anchor, building.kind.size(), enemy.tile, (1, 1))
            < reach.max(strike) + UPGRADE_CLEARANCE
    });
    if !ready || threatened {
        return None;
    }
    let centre = footprint_centre(building.kind, building.anchor);
    // The next tier, where it reaches, and the strength it adds.
    let raised = cover.map(|cover| {
        let next = Cover::of(building.kind, building.tier + 1, building.anchor).unwrap_or(cover);
        (cover, next)
    });
    let worth: u64 = guard
        .assets
        .iter()
        .map(|asset| {
            if let Some((cover, next)) = raised {
                [Domain::Ground, Domain::Air]
                    .into_iter()
                    .filter_map(|domain| Some((domain, asset.approach(domain)?)))
                    .map(|(domain, approach)| {
                        // The army scrap of the shortfall at the samples the next
                        // tier covers, and sees where it fires indirect shells,
                        // that it closes beyond the gun today.
                        let short = approach.short(
                            |_, spotted| !next.indirect || spotted,
                            |point| {
                                raises(guard.map, cover, next, domain, point, |cover| {
                                    approach.held(cover)
                                })
                            },
                        );
                        asset.value * short * approach.evidence.weight() / 1_000
                    })
                    .sum::<u64>()
            } else {
                let evidence = [Domain::Ground, Domain::Air]
                    .into_iter()
                    .filter_map(|domain| asset.approach(domain))
                    .map(|approach| approach.evidence.weight())
                    .max()
                    .unwrap_or(0);
                // Points the next tier brings into sight that this one only
                // watches by radar, if at all.
                let sight = |tier: u8| i64::from(building.kind.tier_stats(tier).vision);
                let (now, then) = (sight(building.tier), sight(building.tier + 1));
                let watched = Guard::watched(asset, |point, _| {
                    let distance = distance2(centre, point);
                    distance <= 4 * then * then && distance > 4 * now * now
                });
                asset.value * watched * evidence
            }
        })
        .sum();
    // A gun's upgrade is weighed per scrap, as guns are; an Array's is not.
    let score = if cover.is_some() {
        worth * weight * QUOTE / (GUN_POINTS * u64::from(next.cost).max(1))
    } else {
        worth * weight / POINTS
    };
    (worth > 0).then(|| {
        (
            Investment::Upgrade {
                building: building.id,
                tier: building.tier + 1,
            },
            points(score),
        )
    })
}

/// Anchors for a `size` footprint standing off a building of `footprint` at
/// `anchor` by the standoff gaps.
pub(super) fn sites_around(
    anchor: TilePos,
    footprint: (i32, i32),
    size: (i32, i32),
) -> impl Iterator<Item = TilePos> {
    sites_at(anchor, footprint, size, &STANDOFF)
}

/// Anchors for a `size` footprint standing off a building of `footprint` at
/// `anchor` by one of `gaps`, the widest last, a row at a time.
pub(super) fn sites_at(
    anchor: TilePos,
    footprint: (i32, i32),
    size: (i32, i32),
    gaps: &'static [i32],
) -> impl Iterator<Item = TilePos> {
    let widest = gaps.last().copied().unwrap_or(0);
    let low = -(widest + size.0.max(size.1));
    let high = widest + footprint.0.max(footprint.1);
    (low..=high)
        .flat_map(move |dy| (low..=high).map(move |dx| anchor.offset(dx, dy)))
        .filter(move |tile| gaps.contains(&gap(anchor, footprint, *tile, size)))
}

/// Anchors for a `size` defense holding `gate` from its home side: within
/// [`GATE_DEPTH`] tiles short of it, off it.
fn gate_sites(
    map: &MapModel,
    me: PlayerId,
    gate: &Gate,
    distance: u16,
    size: (i32, i32),
) -> Vec<TilePos> {
    let (low, high) =
        gate.tiles
            .iter()
            .fold((gate.tiles[0], gate.tiles[0]), |(low, high), tile| {
                (
                    TilePos::new(low.x.min(tile.x), low.y.min(tile.y)),
                    TilePos::new(high.x.max(tile.x), high.y.max(tile.y)),
                )
            });
    let margin = i32::from(GATE_DEPTH) + size.0.max(size.1);
    let inside = distance.saturating_sub(10 * GATE_DEPTH)..distance;
    (low.y - margin..=high.y + margin)
        .flat_map(|y| (low.x - margin..=high.x + margin).map(move |x| TilePos::new(x, y)))
        .filter(|anchor| {
            tiles_of(*anchor, size)
                .all(|tile| inside.contains(&map.distance(me, tile)) && !map.gated(me, tile))
        })
        .collect()
}

/// Anchors for a defense of `kind` standing off one of `asset`'s buildings,
/// on the side `approach` comes from and no nearer than the asset's edge
/// along it, so a defense stands in front of a whole base rather than among
/// its buildings.
fn guard_sites(
    map: &MapModel,
    me: PlayerId,
    asset: &Asset,
    approach: &Approach,
    kind: BuildingKind,
) -> Vec<TilePos> {
    let size = kind.size();
    if let Some(cut) = &approach.cut {
        let mut sites: Vec<TilePos> = cut
            .gates
            .iter()
            .flat_map(|gate| gate_sites(map, me, gate, cut.distance, size))
            .collect();
        sites.sort_unstable();
        sites.dedup();
        return sites;
    }
    // A site stands at most this far, as a step, beyond its building.
    let beyond = 3 * i64::from(STANDOFF[1] + size.0.max(size.1) + 1);
    let mut sites: Vec<TilePos> = asset
        .members
        .iter()
        .filter(|(anchor, footprint)| {
            furthest(asset.centre, approach.source, *anchor, *footprint) + beyond >= approach.edge
        })
        .flat_map(|(anchor, footprint)| sites_around(*anchor, *footprint, size))
        .filter(|anchor| {
            let step = ahead(
                asset.centre,
                approach.source,
                footprint_centre(kind, *anchor),
            );
            step > 0 && step >= approach.edge
        })
        .collect();
    sites.sort_unstable();
    sites.dedup();
    sites
}

/// What a gun holds off, in army scrap: the price of the Sentinels that would
/// match it under Lanchester's square law, from the product of its damage per
/// tick and health against a Sentinel's, with a splash shell counting
/// `targets` thousandths of hits.
pub(super) fn strength(stats: &BuildingStats, targets: u64) -> u64 {
    // Damage per tick, in thousandths.
    fn rate<'a>(weapons: impl Iterator<Item = &'a WeaponStats>, targets: u64) -> u64 {
        weapons
            .map(|weapon| {
                let hits = if weapon.splash.is_some() {
                    targets
                } else {
                    1_000
                };
                u64::from(weapon.damage) * hits / u64::from(weapon.cooldown_ticks.max(1))
            })
            .sum()
    }
    let sentinel = UnitKind::Sentinel.stats();
    let reference = rate(
        sentinel
            .weapons
            .iter()
            .filter(|weapon| weapon.targets.ground),
        1_000,
    ) * u64::from(sentinel.max_hp);
    let gun = rate(stats.weapons.iter(), targets) * u64::from(stats.max_hp);
    let ratio = (gun * 1_000_000).checked_div(reference).unwrap_or(0);
    u64::from(sentinel.cost) * ratio.isqrt() / 1_000
}

/// What a gun's next tier `next` adds at `point` over the gun today `cover`,
/// each holding off what `held` says: all of it where the gun does not reach
/// yet, the difference where it does, none beyond the next tier's reach.
pub(super) fn raises(
    map: &MapModel,
    cover: Cover,
    next: Cover,
    domain: Domain,
    point: (i64, i64),
    held: impl Fn(&Cover) -> u64,
) -> u64 {
    if !next.covers(map, domain, point) {
        0
    } else if cover.covers(map, domain, point) {
        held(&next).saturating_sub(held(&cover))
    } else {
        held(&next)
    }
}

/// Whether the seat meets every one of `required`.
fn built_all(observation: &ObservationData, required: &[oxide_sim::stats::Prerequisite]) -> bool {
    required
        .iter()
        .all(|need| crate::investments::met(observation, *need))
}
