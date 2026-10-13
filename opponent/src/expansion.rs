//! Where the seat could grow: expansion Foundries at scrap fields away from
//! every start, and Extractors on free frames. Values read the public map for
//! ground distance and starting scrap, and the seat's own knowledge for what is
//! left, what is claimed and what is dangerous.

use crate::frame::gap;
use crate::investments::{ADOPT, Investment};
use crate::map::{MapModel, Site, UNREACHABLE};
use crate::memory::Memory;
use crate::workers::worker;
use chassis::grid::TilePos;
use oxide_sim::geometry::foundry_supports;
use oxide_sim::observation::ObservationData;
use oxide_sim::{BuildingKind, PlayerId, UnitKind};

/// Chebyshev reach around a site or frame that counts toward its danger.
const DANGER_REACH: i32 = 10;

/// A Foundry of the seat's own, or another seat's building, this close to a
/// site's nodes means someone already works it.
const HELD_REACH: i32 = 6;

/// Footprint of Foundries and Extractors, and so of sites and frames.
const WORKS: (i32, i32) = (2, 2);

/// Expansion and Extractor investments with their scores. `greed` is the
/// 0..=100 trait; `depletion` is the per-mille share of home scrap mined.
pub(crate) fn candidates(
    observation: &ObservationData,
    map: &MapModel,
    memory: &Memory,
    greed: u8,
    depletion: u32,
) -> Vec<(Investment, u32)> {
    let mut list = Vec::new();
    for (index, site) in map.sites().iter().enumerate() {
        let Some(value) = value(observation, map, memory, site, greed) else {
            continue;
        };
        if value <= 0 {
            continue;
        }
        let mut score =
            (300 + 4 * i64::from(greed)) * value / 1_000 + 400 * i64::from(depletion) / 1_000;
        let ground = site
            .anchors
            .first()
            .and_then(|anchor| map.component(*anchor));
        if ground
            != map
                .start(observation.me)
                .and_then(|start| map.component(start))
        {
            // Off its home ground, a site is worth adopting in proportion to
            // what it yields per Foundry price, less what landing a builder
            // there still costs.
            let price = i64::from(construction_cost(BuildingKind::Foundry));
            score = score.max(i64::from(ADOPT) * value / price);
            if !settled(observation, map, ground) {
                score = score * price / (price + i64::from(lift_cost(observation)));
            }
        }
        list.push((
            Investment::Expansion(u16::try_from(index).expect("site counts fit in u16")),
            u32::try_from(score.max(0)).unwrap_or(u32::MAX),
        ));
    }
    for frame in &observation.known_frames {
        if held(observation, *frame, -1)
            || memory.failed(BuildingKind::Extractor, *frame, observation.tick)
            || contested(observation, *frame)
        {
            continue;
        }
        let supported = supported(observation, *frame);
        let greed = u64::from(greed);
        let base = if supported {
            500 + 3 * greed
        } else {
            250 + 2 * greed
        };
        let score = base * 1_000 / (1_000 + danger(observation, memory, *frame));
        list.push((
            Investment::Extractor(*frame),
            u32::try_from(score).unwrap_or(u32::MAX),
        ));
    }
    list
}

/// Whether a completed own Foundry supports an Extractor on `frame`, by the
/// simulation's own reach.
pub(crate) fn supported(observation: &ObservationData, frame: TilePos) -> bool {
    observation.my_buildings.iter().any(|building| {
        building.kind == BuildingKind::Foundry
            && building.built
            && foundry_supports(building.anchor, frame)
    })
}

/// Whether the seat already has a built Foundry or a worker on `ground`, so
/// building there needs no lift.
fn settled(observation: &ObservationData, map: &MapModel, ground: Option<u32>) -> bool {
    observation
        .my_buildings
        .iter()
        .any(|b| b.kind == BuildingKind::Foundry && b.built && map.component(b.anchor) == ground)
        || observation
            .my_units
            .iter()
            .any(|u| worker(u.kind) && map.component(u.tile) == ground)
}

/// What the seat still lacks to lift a builder: an Airworks and its
/// Fabricator, and a first carrier.
fn lift_cost(observation: &ObservationData) -> u32 {
    let built = |kind| {
        observation
            .my_buildings
            .iter()
            .any(|b| b.kind == kind && b.built)
    };
    let tech: u32 = [BuildingKind::Fabricator, BuildingKind::Airworks]
        .into_iter()
        .filter(|kind| !built(*kind))
        .map(construction_cost)
        .sum();
    let carrier = observation
        .my_units
        .iter()
        .map(|u| u.kind)
        .chain(observation.my_queues.iter().flatten().copied())
        .any(|kind| kind.stats().transport_capacity > 0);
    tech + if carrier {
        0
    } else {
        UnitKind::Skyhook.stats().cost
    }
}

fn construction_cost(kind: BuildingKind) -> u32 {
    kind.base_stats()
        .construction
        .as_ref()
        .expect("expansion buildings and their prerequisites are constructible")
        .cost
}

/// The site's anchors, best first: those the seat's ground reaches before
/// those it would have to lift a builder to.
pub(crate) fn anchors(map: &MapModel, player: PlayerId, site: &Site) -> Vec<TilePos> {
    let mut anchors = site.anchors.clone();
    anchors.sort_by_key(|anchor| map.distance(player, *anchor) == UNREACHABLE);
    anchors
}

/// A site's value: what it would yield, weighted up by greed, less its
/// distance from home, how much nearer an enemy start is, and the danger
/// around it, weighted down by greed. `None` for a site already held, with
/// no footprint, or failed at every footprint.
pub(crate) fn value(
    observation: &ObservationData,
    map: &MapModel,
    memory: &Memory,
    site: &Site,
    greed: u8,
) -> Option<i64> {
    let me = observation.me;
    let reachable = anchors(map, me, site);
    let anchor = *reachable.first()?;
    if claimed(observation, map, site)
        || reachable
            .iter()
            .all(|anchor| memory.failed(BuildingKind::Foundry, *anchor, observation.tick))
    {
        return None;
    }
    let distance = distance(observation, map, anchor);
    let scrap: i64 = site
        .nodes
        .iter()
        .map(|(node, amount)| {
            if !observation.explored(*node) {
                return i64::from(*amount);
            }
            observation
                .known_scrap
                .binary_search_by_key(&(node.y, node.x), |(tile, _)| (tile.y, tile.x))
                .map_or(0, |index| i64::from(observation.known_scrap[index].1))
        })
        .sum();
    let frames = site
        .frames
        .iter()
        .filter(|frame| {
            observation.known_frames.contains(frame)
                && (!held(observation, **frame, -1)
                    || observation.my_buildings.iter().any(|building| {
                        building.kind == BuildingKind::Extractor
                            && building.anchor == **frame
                            && !supported(observation, **frame)
                    }))
        })
        .count();
    let frames = i64::try_from(frames).expect("frame counts fit in i64");
    // Once a Crucible stands, a Foundry here would let own remote Extractors
    // in its reach become Deep Bores.
    let deepens = if observation
        .my_buildings
        .iter()
        .any(|building| building.kind == BuildingKind::Crucible && building.built)
    {
        observation
            .my_buildings
            .iter()
            .filter(|building| {
                building.kind == BuildingKind::Extractor
                    && building.built
                    && building.tier == 0
                    && !supported(observation, building.anchor)
                    && foundry_supports(anchor, building.anchor)
            })
            .count()
    } else {
        0
    };
    let deepens = i64::try_from(deepens).expect("frame counts fit in i64");
    let resource = scrap.min(3_200) / 4 + 150 * (frames + deepens);
    let distance = i64::from(distance);
    let hostile = map.hostile_distance(me, anchor);
    let contested = if hostile == UNREACHABLE {
        0
    } else {
        (distance - i64::from(hostile) + 100).max(0)
    };
    let penalty = 8 * distance / 10
        + contested
        + i64::try_from(danger(observation, memory, anchor)).unwrap_or(i64::MAX);
    let weight = 750 + 5 * i64::from(greed);
    Some(resource * weight / 1_000 - penalty * (2_000 - weight) / 1_000)
}

/// Whether someone already works `site`: an own Foundry, or any other seat's
/// building, near its nodes on the same ground, or, for a site of frames
/// alone, any Foundry that already supports one of them.
fn claimed(observation: &ObservationData, map: &MapModel, site: &Site) -> bool {
    let own = observation
        .my_buildings
        .iter()
        .filter(|building| building.kind == BuildingKind::Foundry);
    own.chain(&observation.ally_buildings)
        .chain(&observation.enemy_buildings)
        .any(|building| {
            let size = building.kind.size();
            let local = site
                .anchors
                .iter()
                .any(|anchor| map.component(*anchor) == map.component(building.anchor));
            site.nodes
                .iter()
                .any(|(node, _)| local && gap(*node, (1, 1), building.anchor, size) <= HELD_REACH)
                || (building.kind == BuildingKind::Foundry
                    && site.nodes.is_empty()
                    && site
                        .frames
                        .iter()
                        .any(|frame| foundry_supports(building.anchor, *frame)))
        })
}

/// Ground distance from the seat's start to `anchor`, in tenths of a tile.
/// Ground the start cannot reach measures straight-line from the nearest
/// built Foundry on that ground, else from the start, as a lift would fly.
fn distance(observation: &ObservationData, map: &MapModel, anchor: TilePos) -> u16 {
    let distance = map.distance(observation.me, anchor);
    if distance != UNREACHABLE {
        return distance;
    }
    let from = observation
        .my_buildings
        .iter()
        .filter(|b| {
            b.kind == BuildingKind::Foundry
                && b.built
                && map.component(b.anchor) == map.component(anchor)
        })
        .map(|b| b.anchor.chebyshev(anchor))
        .min()
        .or_else(|| map.start(observation.me).map(|home| home.chebyshev(anchor)));
    from.map_or(UNREACHABLE - 1, |distance| {
        u16::try_from(distance * 10).unwrap_or(UNREACHABLE - 1)
    })
}

/// Whether a known building stands within `reach` empty tiles of the
/// two-by-two footprint at `anchor`; a negative reach asks for overlap.
fn held(observation: &ObservationData, anchor: TilePos, reach: i32) -> bool {
    observation
        .my_buildings
        .iter()
        .chain(&observation.ally_buildings)
        .chain(&observation.enemy_buildings)
        .any(|building| gap(anchor, WORKS, building.anchor, building.kind.size()) <= reach)
}

/// Whether an armed enemy in sight stands near the two-by-two footprint at
/// `anchor`: an Extractor lost there, or a new one, waits until it leaves.
fn contested(observation: &ObservationData, anchor: TilePos) -> bool {
    observation.enemy_units.iter().any(|enemy| {
        enemy.kind.stats().can_fight() && gap(anchor, WORKS, enemy.tile, (1, 1)) <= DANGER_REACH
    })
}

/// Remembered enemy units near the two-by-two footprint at `anchor`, weighted
/// by confidence, plus known armed enemy buildings near it.
fn danger(observation: &ObservationData, memory: &Memory, anchor: TilePos) -> u64 {
    let now = observation.tick;
    let units: u64 = memory
        .units()
        .iter()
        .filter(|unit| {
            unit.kind.stats().can_fight() && gap(anchor, WORKS, unit.tile, (1, 1)) <= DANGER_REACH
        })
        .map(|unit| unit.value(now))
        .sum();
    let buildings: u64 = observation
        .enemy_buildings
        .iter()
        .filter(|building| {
            !building.kind.base_stats().weapons.is_empty()
                && gap(anchor, WORKS, building.anchor, building.kind.size()) <= DANGER_REACH
        })
        .filter_map(|building| building.kind.base_stats().construction.as_ref())
        .map(|construction| u64::from(construction.cost))
        .sum();
    units + buildings
}
