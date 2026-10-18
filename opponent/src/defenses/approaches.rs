//! Where each guarded building is attacked from, by what, and how much of
//! each way in own guns leave uncovered.

use super::lattice::{lattice_ring, spire_sources};
use super::{
    ATTACKER_MARGIN, Approach, Asset, Cover, Evidence, Guard, Held, Scope, Stakes, Threat,
    chebyshev, distance2, radar, reach,
};
use crate::composition;
use crate::frame::{HomeFrame, doubled, footprint_centre, gap, ring, tiles_of};
use crate::map::{MapModel, UNREACHABLE};
use crate::memory::{Memory, SeenUnit};
use crate::workers;
use chassis::fx::Fx;
use chassis::grid::TilePos;
use oxide_sim::observation::{BuildingObs, ObservationData};
use oxide_sim::stats::{CHARGE_DAMAGE, Domain};
use oxide_sim::{BuildingKind, UnitKind};
use std::cell::OnceCell;
use std::cmp::Reverse;

/// What one of several bases is worth guarding.
const FOUNDRY_VALUE: u64 = 12;

/// What a lone base is worth guarding.
const LONE_FOUNDRY_VALUE: u64 = 16;

/// What an Extractor out on its own is worth guarding.
const OUTLYING_VALUE: u64 = 6;

/// Empty tiles between an Extractor and the nearest Foundry beyond which it
/// is guarded on its own rather than with that Foundry's base.
const OUTLYING_GAP: i32 = 8;

/// Tiles from a building's centre toward a threat sampled as its approach.
const APPROACH: [i64; 4] = [3, 6, 9, 12];

/// Tiles from a building's centre toward a threat an Array watches.
const FAR: [i64; 4] = [9, 12, 15, 18];

/// Tiles either side of a Lattice's ring, along a known Spire's line, where
/// an Array watches for warheads before they enter it.
const RING_WATCH: [i64; 3] = [-3, 0, 3];

/// Tiles around the nearest known enemy on an approach inside which its
/// companions count toward the threat along it.
const GROUP_TILES: i64 = 10;

impl<'a> Guard<'a> {
    pub(super) fn new(
        observation: &'a ObservationData,
        map: &'a MapModel,
        memory: &'a Memory,
        scope: Scope,
        settled: bool,
        exposed: bool,
        stakes: Stakes,
    ) -> Option<Self> {
        let frame = HomeFrame::of(observation, map)?;
        let foundries: Vec<TilePos> = observation
            .my_buildings
            .iter()
            .filter(|building| building.kind == BuildingKind::Foundry && building.built)
            .map(|building| building.anchor)
            .collect();
        let covers: Vec<Cover> = observation
            .my_buildings
            .iter()
            .filter(|building| !building.provisional)
            .filter_map(|building| Cover::of(building.kind, building.tier, building.anchor))
            .collect();
        let sight = sight(observation);
        // An indirect gun fires only where something sees now, but an Array
        // still going up will watch its ground: counting it keeps the next
        // decision from buying another beside it.
        let sees = |point: (i64, i64), unfinished: bool| {
            sight.iter().any(|(centre, reach2, built)| {
                (*built || unfinished) && distance2(*centre, point) <= *reach2
            })
        };
        let seen = |point| sees(point, false);
        let watched = |point| sees(point, true);
        let current = in_sight(observation, map);
        let mut assets = match scope {
            Scope::Bases => bases(observation, map, frame),
            Scope::Buildings => buildings(observation, frame),
        };
        let known = Known::of(
            observation,
            map,
            memory,
            frame,
            exposed,
            scope,
            &assets,
            &current,
        );
        // Many buildings share a threat's source.
        let mut threats: Vec<(ThreatKey, Threat)> = Vec::new();
        for asset in &mut assets {
            let centre = asset.centre;
            let around = grounds(map, asset.anchor, asset.size);
            asset.ground = approach(&known, asset, Domain::Ground);
            asset.air = approach(&known, asset, Domain::Air);
            for (domain, approach) in [
                (Domain::Ground, asset.ground.as_mut()),
                (Domain::Air, asset.air.as_mut()),
            ] {
                let Some(approach) = approach else {
                    continue;
                };
                // Ground units count only where ground connects them to the
                // building, unless they would come by landing.
                let beside = (domain == Domain::Ground && approach.evidence != Evidence::Landing)
                    .then_some(&around);
                approach.threat = shared(
                    &mut threats,
                    (memory, map, observation.tick),
                    (approach.source, domain),
                    beside,
                    stakes,
                );
                cover(map, approach, domain, centre, &covers, &seen, &watched);
            }
        }
        watch_missiles(observation, frame, &mut assets, &watched);
        let crews = crews(observation, map);
        Some(Guard {
            observation,
            map,
            memory,
            frame,
            assets,
            covers,
            foundries,
            crews,
            settled,
            exposed,
            stakes,
        })
    }
}

/// Where own and allied buildings, built or with their ground verified, see
/// or pick up by radar once built: each one's centre, its squared doubled
/// reach, and whether it is built.
fn sight(observation: &ObservationData) -> Vec<((i64, i64), i64, bool)> {
    observation
        .my_buildings
        .iter()
        .chain(&observation.ally_buildings)
        .filter(|building| !building.provisional)
        .map(|building| {
            let vision = i64::from(building.kind.tier_stats(building.tier).vision);
            let reach = vision.max(radar(building.kind, building.tier));
            (
                footprint_centre(building.kind, building.anchor),
                4 * reach * reach,
                building.built,
            )
        })
        .collect()
}

/// The threat along a way in from `source` in `domain`, worked out once for
/// all the buildings that share it.
fn shared(
    threats: &mut Vec<(ThreatKey, Threat)>,
    (memory, map, now): (&Memory, &MapModel, u64),
    (source, domain): ((i64, i64), Domain),
    beside: Option<&Vec<u32>>,
    stakes: Stakes,
) -> Threat {
    if let Some((_, known)) = threats
        .iter()
        .find(|((at, of, on), _)| *at == source && *of == domain && on.as_ref() == beside)
    {
        return known.clone();
    }
    let threat = threat(
        memory,
        map,
        now,
        (source, domain),
        beside.map(Vec::as_slice),
        stakes,
    );
    threats.push(((source, domain, beside.cloned()), threat.clone()));
    threat
}

/// How `approach`, a way in to the building at `centre` in `domain`, is
/// covered: the cover that holds its threat off, how far short own `covers`
/// fall at each sample, which samples own buildings see, and the points
/// further along where an Array watches, with those own buildings will
/// watch once built.
fn cover(
    map: &MapModel,
    approach: &mut Approach,
    domain: Domain,
    centre: (i64, i64),
    covers: &[Cover],
    seen: &impl Fn((i64, i64)) -> bool,
    watched: &impl Fn((i64, i64)) -> bool,
) {
    let need = approach.threat.value * 1_000 / ATTACKER_MARGIN;
    approach.need = need;
    let open = approach
        .samples
        .iter()
        .map(|point| {
            let covered: u64 = covers
                .iter()
                .filter(|cover: &&Cover| {
                    cover.covers(map, domain, *point) && (!cover.indirect || seen(*point))
                })
                .map(|cover| approach.held(cover))
                .sum();
            (2_000 * need.saturating_sub(covered))
                .checked_div(need)
                .unwrap_or(0)
        })
        .collect();
    approach.open = open;
    approach.spotted = approach.samples.iter().map(|point| seen(*point)).collect();
    let far = match &approach.cut {
        Some(cut) => cut
            .gates
            .iter()
            .flat_map(|gate| along(gate.centre, approach.source, 0, &FAR))
            .collect(),
        None => along(centre, approach.source, approach.edge, &FAR),
    };
    approach.far = far
        .into_iter()
        .map(|point| (point, watched(point)))
        .collect();
}

/// Marks, for each asset an own Lattice guards, where a warhead from the
/// nearest known Spire would cross into the Lattice's ring, and which of
/// those points own buildings watch, or will once built.
fn watch_missiles(
    observation: &ObservationData,
    frame: HomeFrame,
    assets: &mut [Asset],
    watched: &impl Fn((i64, i64)) -> bool,
) {
    let spires = spire_sources(observation);
    let ring = lattice_ring();
    let lattices: Vec<(i64, i64)> = observation
        .my_buildings
        .iter()
        .filter(|building| building.kind == BuildingKind::Lattice)
        .map(|building| footprint_centre(building.kind, building.anchor))
        .collect();
    for asset in assets {
        let guarded = lattices
            .iter()
            .any(|lattice| distance2(*lattice, asset.centre) <= 4 * ring * ring);
        let nearest = spires.iter().copied().min_by_key(|spire| {
            (
                chebyshev(*spire, asset.centre),
                frame.rank(asset.centre, *spire),
            )
        });
        if let (true, Some(spire)) = (guarded, nearest) {
            let tiles = RING_WATCH.map(|offset| ring + offset);
            asset.missile = along(asset.centre, spire, 0, &tiles)
                .into_iter()
                .map(|point| (point, watched(point)))
                .collect();
        }
    }
}

/// The grounds the seat's workers stand on, where it can build.
fn crews(observation: &ObservationData, map: &MapModel) -> Vec<u32> {
    let mut crews: Vec<u32> = observation
        .my_units
        .iter()
        .filter(|unit| workers::worker(unit.kind))
        .filter_map(|unit| map.component(unit.tile))
        .collect();
    crews.sort_unstable();
    crews.dedup();
    crews
}

/// Where a threat comes from, the domain it moves in, and, for ground units
/// that must walk, the grounds they count from.
type ThreatKey = ((i64, i64), Domain, Option<Vec<u32>>);

/// The armed enemies in `domain` the seat remembers near `source`, valued by
/// how sure it is they are still there, and at least an army at the stance's
/// minimum. Given `beside`, only those standing beside one of those grounds
/// count.
fn threat(
    memory: &Memory,
    map: &MapModel,
    now: u64,
    (source, domain): ((i64, i64), Domain),
    beside: Option<&[u32]>,
    stakes: Stakes,
) -> Threat {
    let units: Vec<&SeenUnit> = armed_near(memory, source, domain)
        .filter(|unit| {
            beside.is_none_or(|grounds| {
                ring(unit.tile, (1, 1)).any(|tile| {
                    map.component(tile)
                        .is_some_and(|component| grounds.contains(&component))
                })
            })
        })
        .collect();
    let reaches: Vec<(Fx, u64)> = units
        .iter()
        .map(|unit| (reach(unit.kind), unit.value(now)))
        .collect();
    let near: u64 = reaches.iter().map(|(_, value)| value).sum();
    Threat {
        value: near.max(stakes.minimum),
        reaches,
        clustered: composition::clustered(units),
    }
}

/// The health of the armed ground enemies the seat remembers near `source`,
/// each rounded up to whole Scuttle Charge blasts since a blast hits one
/// body, by how sure the seat is they are still there, and at least an army
/// of Sentinels at the stance's minimum.
pub(super) fn health(memory: &Memory, now: u64, source: (i64, i64), stakes: Stakes) -> u64 {
    let near: u64 = armed_near(memory, source, Domain::Ground)
        .map(|unit| {
            let blasts = unit.kind.stats().max_hp.div_ceil(CHARGE_DAMAGE);
            u64::from(blasts * CHARGE_DAMAGE) * u64::from(unit.confidence(now)) / 1_000
        })
        .sum();
    let sentinel = UnitKind::Sentinel.stats();
    near.max(stakes.minimum * u64::from(sentinel.max_hp) / u64::from(sentinel.cost))
}

/// The armed enemies in `domain` the seat remembers near `source`.
fn armed_near(
    memory: &Memory,
    source: (i64, i64),
    domain: Domain,
) -> impl Iterator<Item = &SeenUnit> {
    memory.units().iter().filter(move |unit| {
        let stats = unit.kind.stats();
        stats.can_fight()
            && stats.domain == domain
            && chebyshev(doubled(unit.tile), source) <= 2 * GROUP_TILES
    })
}

/// An enemy unit in sight that fights or carries others.
struct Enemy {
    at: (i64, i64),
    domain: Domain,
    /// Ground beside a ground unit. Few units are in sight, and every guarded
    /// building asks about each of them.
    grounds: Vec<u32>,
}

/// The enemy units in sight that fight or carry others.
fn in_sight(observation: &ObservationData, map: &MapModel) -> Vec<Enemy> {
    observation
        .enemy_units
        .iter()
        .filter(|unit| attacker(unit.kind))
        .map(|unit| {
            let domain = unit.kind.stats().domain;
            let grounds = match domain {
                Domain::Ground => grounds(map, unit.tile, (1, 1)),
                Domain::Air => Vec::new(),
            };
            Enemy {
                at: doubled(unit.tile),
                domain,
                grounds,
            }
        })
        .collect()
}

/// Whether `kind` fights or carries others.
fn attacker(kind: UnitKind) -> bool {
    let stats = kind.stats();
    stats.can_fight() || stats.transport_capacity > 0
}

/// The ground around a `size` footprint at `anchor`.
fn grounds(map: &MapModel, anchor: TilePos, size: (i32, i32)) -> Vec<u32> {
    let mut grounds: Vec<u32> = ring(anchor, size)
        .filter_map(|tile| map.component(tile))
        .collect();
    grounds.sort_unstable();
    grounds.dedup();
    grounds
}

/// The seat's built buildings worth guarding each on its own, most valuable
/// first: its Foundries, tech and production buildings, Extractors (more
/// beside a Foundry) and Reclaimers.
fn buildings(observation: &ObservationData, frame: HomeFrame) -> Vec<Asset> {
    let foundries: Vec<&BuildingObs> = observation
        .my_buildings
        .iter()
        .filter(|building| building.kind == BuildingKind::Foundry && building.built)
        .collect();
    let mut assets: Vec<Asset> = observation
        .my_buildings
        .iter()
        .filter(|building| building.built)
        .filter_map(|building| {
            let size = building.kind.size();
            let value = match building.kind {
                BuildingKind::Foundry if foundries.len() == 1 => LONE_FOUNDRY_VALUE,
                BuildingKind::Foundry => FOUNDRY_VALUE,
                BuildingKind::Crucible => 10,
                BuildingKind::Airworks => 9,
                BuildingKind::Fabricator => 8,
                BuildingKind::Extractor if building.tier >= 1 => 12,
                BuildingKind::Extractor => {
                    let supported = foundries.iter().any(|foundry| {
                        gap(foundry.anchor, foundry.kind.size(), building.anchor, size)
                            <= OUTLYING_GAP
                    });
                    if supported { 8 } else { OUTLYING_VALUE }
                }
                BuildingKind::Reclaimer => 5,
                _ => return None,
            };
            Some(Asset::new(
                building.anchor,
                size,
                footprint_centre(building.kind, building.anchor),
                value,
            ))
        })
        .collect();
    assets.sort_by_key(|asset| (Reverse(asset.value), frame.rank(frame.home, asset.centre)));
    assets
}

/// What the seat guards, most valuable first: each base, and each Extractor
/// beyond `OUTLYING_GAP` of every Foundry on its own. A base grows from the
/// seat's start Foundry, a Foundry founded on an expansion site, or failing
/// those any Foundry on its ground, and holds every other built building but
/// defenses nearest it on that ground.
fn bases(observation: &ObservationData, map: &MapModel, frame: HomeFrame) -> Vec<Asset> {
    let size = |building: &BuildingObs| building.kind.size();
    let foundries: Vec<&BuildingObs> = observation
        .my_buildings
        .iter()
        .filter(|building| building.kind == BuildingKind::Foundry && building.built)
        .collect();
    let start = map.start(observation.me);
    let founded = |anchor: TilePos| {
        Some(anchor) == start
            || map
                .sites()
                .iter()
                .any(|site| site.anchors.contains(&anchor))
    };
    let mut origins: Vec<&BuildingObs> = foundries
        .iter()
        .copied()
        .filter(|foundry| founded(foundry.anchor))
        .collect();
    for foundry in &foundries {
        let ground = map.component(foundry.anchor);
        if !origins
            .iter()
            .any(|origin| map.component(origin.anchor) == ground)
        {
            origins.push(foundry);
        }
    }
    let value = if foundries.len() == 1 {
        LONE_FOUNDRY_VALUE
    } else {
        FOUNDRY_VALUE
    };
    let mut assets: Vec<Asset> = origins
        .iter()
        .map(|origin| {
            Asset::new(
                origin.anchor,
                size(origin),
                footprint_centre(origin.kind, origin.anchor),
                value,
            )
        })
        .collect();
    for building in observation.my_buildings.iter().filter(|building| {
        building.built
            && !defense(building.kind)
            && !origins.iter().any(|origin| origin.id == building.id)
    }) {
        let (anchor, footprint) = (building.anchor, size(building));
        let outlying = building.kind == BuildingKind::Extractor
            && !foundries.iter().any(|foundry| {
                gap(foundry.anchor, size(foundry), anchor, footprint) <= OUTLYING_GAP
            });
        if outlying {
            assets.push(Asset::new(
                anchor,
                footprint,
                footprint_centre(building.kind, anchor),
                OUTLYING_VALUE,
            ));
            continue;
        }
        let ground = map.component(anchor);
        let centre = footprint_centre(building.kind, anchor);
        let nearest = origins
            .iter()
            .zip(&mut assets)
            .filter(|(origin, _)| map.component(origin.anchor) == ground)
            .min_by_key(|(origin, base)| {
                (
                    gap(origin.anchor, size(origin), anchor, footprint),
                    frame.rank(centre, base.centre),
                )
            });
        if let Some((_, base)) = nearest {
            base.members.push((anchor, footprint));
        }
    }
    assets.sort_by_key(|asset| (Reverse(asset.value), frame.rank(frame.home, asset.centre)));
    assets
}

/// Whether `kind` is a defense the seat places to guard others, not one of
/// the buildings it guards.
fn defense(kind: BuildingKind) -> bool {
    matches!(
        kind,
        BuildingKind::Turret
            | BuildingKind::FlakTurret
            | BuildingKind::Bastion
            | BuildingKind::Array
            | BuildingKind::Barricade
            | BuildingKind::ScuttleCharge
            | BuildingKind::RepairBay
            | BuildingKind::Lattice
    )
}

/// What one decision knows about where threats come from.
struct Known<'a> {
    observation: &'a ObservationData,
    map: &'a MapModel,
    memory: &'a Memory,
    frame: HomeFrame,
    /// Whether the seat has no army to speak of.
    exposed: bool,
    /// Whether the start's base holds its ground way in at a gate.
    gates: bool,
    current: &'a [Enemy],
    /// What an approach falls back on, worked out when one first does, since
    /// many buildings share it: known enemy buildings and hostile starts,
    /// where remembered enemy aircraft were, and where remembered enemy
    /// ground units were beside each ground the seat's buildings stand by.
    bases: OnceCell<Vec<Base>>,
    aircraft: OnceCell<Vec<(i64, i64)>>,
    walkers: Vec<Walkers>,
}

/// Where remembered enemy ground units beside `grounds` were last seen.
struct Walkers {
    grounds: Vec<u32>,
    at: OnceCell<Vec<(i64, i64)>>,
}

impl<'a> Known<'a> {
    /// What the guard of `assets` knows of where threats come from, with
    /// `current` the enemies in sight.
    #[expect(
        clippy::too_many_arguments,
        reason = "the decision's inputs a guard's threats are found from"
    )]
    fn of(
        observation: &'a ObservationData,
        map: &'a MapModel,
        memory: &'a Memory,
        frame: HomeFrame,
        exposed: bool,
        scope: Scope,
        assets: &[Asset],
        current: &'a [Enemy],
    ) -> Self {
        let mut walkers: Vec<Walkers> = Vec::new();
        for asset in assets {
            let around = grounds(map, asset.anchor, asset.size);
            if !walkers.iter().any(|walkers| walkers.grounds == around) {
                walkers.push(Walkers {
                    grounds: around,
                    at: OnceCell::new(),
                });
            }
        }
        Known {
            observation,
            map,
            memory,
            frame,
            exposed,
            gates: matches!(scope, Scope::Bases),
            current,
            bases: OnceCell::new(),
            aircraft: OnceCell::new(),
            walkers,
        }
    }
}

impl Known<'_> {
    /// Where remembered enemies that fight or carry others in `domain` were
    /// last seen, in doubled coordinates: aircraft anywhere, ground units
    /// beside `grounds`.
    fn remembered(&self, domain: Domain, grounds: &[u32]) -> &[(i64, i64)] {
        let attackers = |keep: &dyn Fn(TilePos) -> bool| {
            self.memory
                .units()
                .iter()
                .filter(|unit| {
                    attacker(unit.kind) && unit.kind.stats().domain == domain && keep(unit.tile)
                })
                .map(|unit| doubled(unit.tile))
                .collect()
        };
        match domain {
            Domain::Air => self.aircraft.get_or_init(|| attackers(&|_| true)),
            Domain::Ground => self
                .walkers
                .iter()
                .find(|walkers| walkers.grounds == grounds)
                .map_or(&[], |walkers| {
                    walkers.at.get_or_init(|| {
                        attackers(&|tile| {
                            ring(tile, (1, 1)).any(|tile| {
                                self.map
                                    .component(tile)
                                    .is_some_and(|component| grounds.contains(&component))
                            })
                        })
                    })
                }),
        }
    }

    fn bases(&self) -> &[Base] {
        self.bases.get_or_init(|| {
            self.observation
                .enemy_buildings
                .iter()
                .map(|building| (building.kind, building.anchor))
                .chain(
                    self.map
                        .hostiles(self.observation.me)
                        .filter_map(|owner| self.map.start(owner))
                        .map(|anchor| (BuildingKind::Foundry, anchor)),
                )
                .map(|(kind, anchor)| Base {
                    centre: footprint_centre(kind, anchor),
                    grounds: grounds(self.map, anchor, kind.size()),
                })
                .collect()
        })
    }
}

/// A known enemy building or hostile start.
struct Base {
    centre: (i64, i64),
    grounds: Vec<u32>,
}

/// Where `asset` is attacked from in `domain`, and the samples along the
/// way: the nearest visible enemy that fights in that domain, else the
/// nearest remembered one, else for ground the nearest known enemy building
/// or hostile start, and for air the nearest known enemy Airworks. Enemy
/// ground units count only on or beside the asset's ground: across a chasm,
/// they arrive by air if at all, and land as a threat in sight. With no
/// enemy building or start on the asset's ground, one across a chasm counts
/// as a landing only while the seat is exposed, with no army to meet it. A
/// threat too close for any sample on the way is its own sample.
fn approach(known: &Known<'_>, asset: &Asset, domain: Domain) -> Option<Approach> {
    let Known {
        observation,
        map,
        frame,
        exposed,
        current,
        ..
    } = *known;
    let centre = asset.centre;
    let grounds = grounds(map, asset.anchor, asset.size);
    let beside = |others: &[u32]| others.iter().any(|component| grounds.contains(component));
    let nearest = |points: &mut dyn Iterator<Item = (i64, i64)>| {
        points.min_by_key(|point| (chebyshev(*point, centre), frame.rank(centre, *point)))
    };
    let current = nearest(
        &mut current
            .iter()
            .filter(|enemy| {
                enemy.domain == domain && (domain == Domain::Air || beside(&enemy.grounds))
            })
            .map(|enemy| enemy.at),
    );
    let remembered = || nearest(&mut known.remembered(domain, &grounds).iter().copied());
    let prior = || match domain {
        Domain::Ground => {
            let centres = |walk: bool| {
                known
                    .bases()
                    .iter()
                    .filter(move |base| !walk || beside(&base.grounds))
                    .map(|base| base.centre)
            };
            nearest(&mut centres(true))
                .map(|source| (source, Evidence::Prior))
                .or_else(|| {
                    exposed
                        .then(|| nearest(&mut centres(false)))
                        .flatten()
                        .map(|source| (source, Evidence::Landing))
                })
        }
        Domain::Air => nearest(
            &mut observation
                .enemy_buildings
                .iter()
                .filter(|building| building.kind == BuildingKind::Airworks)
                .map(|building| footprint_centre(building.kind, building.anchor)),
        )
        .map(|source| (source, Evidence::Prior)),
    };
    let (source, evidence) = current
        .map(|source| (source, Evidence::Current))
        .or_else(|| remembered().map(|source| (source, Evidence::Remembered)))
        .or_else(prior)?;
    let cut = (known.gates
        && domain == Domain::Ground
        && evidence != Evidence::Landing
        && map.start(observation.me) == Some(asset.anchor))
    .then(|| held(known, asset, source))
    .flatten();
    let (edge, mut samples, gate_of): (i64, Vec<(i64, i64)>, Vec<usize>) = if let Some(cut) = &cut {
        // Each gate, and the way on from it.
        let (samples, gate_of) = cut
            .gates
            .iter()
            .enumerate()
            .flat_map(|(index, gate)| {
                let mut samples = vec![gate.centre];
                samples.extend(along(gate.centre, source, 0, &APPROACH));
                samples.truncate(APPROACH.len());
                samples.into_iter().map(move |sample| (sample, index))
            })
            .unzip();
        (0, samples, gate_of)
    } else {
        let edge = edge(asset, source);
        (edge, along(centre, source, edge, &APPROACH), Vec::new())
    };
    if samples.is_empty() {
        samples.push(source);
    }
    Some(Approach {
        edge,
        cut,
        samples,
        gate_of,
        open: Vec::new(),
        spotted: Vec::new(),
        far: Vec::new(),
        evidence,
        source,
        need: 0,
        threat: Threat::default(),
    })
}

/// The cut the start's base holds on its way in from `source`: the narrowest
/// beyond the reach of its buildings across the way to the hostile start
/// nearest `source`, while the threat stands beyond it, so that its way in
/// crosses it.
fn held(known: &Known<'_>, asset: &Asset, source: (i64, i64)) -> Option<Held> {
    let (map, me) = (known.map, known.observation.me);
    let reach = asset
        .members
        .iter()
        .flat_map(|(anchor, size)| tiles_of(*anchor, *size))
        .map(|tile| map.distance(me, tile))
        .filter(|distance| *distance != UNREACHABLE)
        .max()?;
    let hostile = map
        .hostiles(me)
        .filter_map(|hostile| {
            let centre = footprint_centre(BuildingKind::Foundry, map.start(hostile)?);
            Some((
                chebyshev(centre, source),
                known.frame.rank(source, centre),
                hostile,
            ))
        })
        .min_by_key(|(distance, rank, _)| (*distance, *rank))?
        .2;
    let cut = map.cut(me, hostile, reach.saturating_add(20))?;
    let at = TilePos::new(
        i32::try_from(source.0.div_euclid(2)).ok()?,
        i32::try_from(source.1.div_euclid(2)).ok()?,
    );
    (!cut.holds(at)).then(|| Held {
        gates: cut.gates.clone(),
        distance: cut.distance,
    })
}

/// Points `tiles` tiles beyond the step `edge` from `centre` toward
/// `source`, short of it, in doubled coordinates. Steps are measured along
/// the straight line by its larger axis. Integer division truncates toward
/// zero, so a mirrored seat's points mirror these.
pub(super) fn along(
    centre: (i64, i64),
    source: (i64, i64),
    edge: i64,
    tiles: &[i64],
) -> Vec<(i64, i64)> {
    let (dx, dy) = (source.0 - centre.0, source.1 - centre.1);
    let length = dx.abs().max(dy.abs());
    tiles
        .iter()
        .map(|tiles| edge + 2 * tiles)
        .filter(|step| *step < length)
        .map(|step| (centre.0 + dx * step / length, centre.1 + dy * step / length))
        .collect()
}

/// How far along the straight line from `centre` toward `source` `point`
/// lies: its projection, as a step [`along`] measures.
pub(super) fn ahead(centre: (i64, i64), source: (i64, i64), point: (i64, i64)) -> i64 {
    let (dx, dy) = (source.0 - centre.0, source.1 - centre.1);
    let length = dx.abs().max(dy.abs());
    let squared = dx * dx + dy * dy;
    let dot = (point.0 - centre.0) * dx + (point.1 - centre.1) * dy;
    (dot * length).checked_div(squared).unwrap_or(0)
}

/// How far a footprint of `size` at `anchor` reaches along the straight line
/// from `centre` toward `source`: its furthest corner's projection.
pub(super) fn furthest(
    centre: (i64, i64),
    source: (i64, i64),
    anchor: TilePos,
    size: (i32, i32),
) -> i64 {
    let (x, y) = (2 * i64::from(anchor.x), 2 * i64::from(anchor.y));
    let (w, h) = (2 * i64::from(size.0), 2 * i64::from(size.1));
    [(x, y), (x + w, y), (x, y + h), (x + w, y + h)]
        .into_iter()
        .map(|corner| ahead(centre, source, corner))
        .max()
        .unwrap_or(0)
}

/// How far `asset`'s buildings besides its first reach toward `source`, as a
/// step [`along`] measures; none for an asset of one building.
fn edge(asset: &Asset, source: (i64, i64)) -> i64 {
    asset
        .members
        .iter()
        .skip(1)
        .map(|(anchor, size)| furthest(asset.centre, source, *anchor, *size))
        .max()
        .unwrap_or(0)
        .max(0)
}
