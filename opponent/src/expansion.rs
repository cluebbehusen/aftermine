//! Where the seat could grow: expansion Foundries at scrap fields away from
//! every start, and Extractors on free frames. Values read the public map for
//! ground distance and starting scrap, and the seat's own knowledge for what is
//! left, what is claimed and what is dangerous.

use crate::frame::gap;
use crate::investments::{ADOPT, Investment};
use crate::map::{MapModel, Site, UNREACHABLE};
use crate::memory::Memory;
use crate::workers::worker;
use aftermine_sim::geometry::foundry_supports;
use aftermine_sim::observation::{BuildingObs, ObservationData};
use aftermine_sim::stats;
use aftermine_sim::{BuildingKind, PlayerId, TICKS_PER_SECOND, UnitKind};
use chassis::fx::Fx;
use chassis::grid::TilePos;

/// Chebyshev reach around a site or frame that counts toward its danger.
const DANGER_REACH: i32 = 10;

/// An own or enemy Foundry, or any ally building, this close to a site's
/// nodes means someone already works it.
const HELD_REACH: i32 = 6;

/// Footprint of Foundries and Extractors, and so of sites and frames.
const WORKS: (i32, i32) = (2, 2);

/// Doubles each scrap a minute an investment returns per scrap it costs, so
/// growth that pays for itself in a minute scores 2,000 at middling greed:
/// above any tech building, which returns nothing, while growth that would
/// take longer than about seven minutes to pay back falls under adoption.
const PAYBACK_SCALE: u64 = 2;

/// Expansion and Extractor investments with their scores. `greed` is the
/// 0..=100 trait; `secure` whether the army holds what the seat knows of the
/// enemy's, so an expansion may count what its support pays back;
/// `depletion` is the per-mille share of home scrap mined.
pub(crate) fn candidates(
    observation: &ObservationData,
    map: &MapModel,
    memory: &Memory,
    (greed, secure, peril): (u8, bool, u64),
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
        // Scored as other growth is, by what it pays back, this already counts
        // a lift's cost.
        let lift = if settled(observation, map, ground) {
            0
        } else {
            lift_cost(observation)
        };
        // Scored at the anchor a Foundry would go up at: the first not failed.
        let open = anchors(map, observation.me, site)
            .into_iter()
            .find(|anchor| !memory.failed(BuildingKind::Foundry, *anchor, observation.tick));
        if secure && let Some(anchor) = open.as_ref() {
            score = score.max(i64::from(support_payback(
                observation,
                *anchor,
                lift,
                greed,
            )));
        }
        // A seat standing on one Foundry that a known army outweighs would
        // lose its income with it: a second Foundry near frames insures
        // against that, the more the nearer it lies to the seat's start than
        // to any hostile one, where the enemy reaches it last.
        if peril > 0
            && let Some(anchor) = open.as_ref()
            && known_frames(observation, site) > 0
        {
            let distance = u64::from(distance(observation, map, *anchor));
            let hostile = map.hostile_distance(observation.me, *anchor);
            let share = if hostile == UNREACHABLE {
                1_000
            } else {
                u64::from(hostile) * 1_000 / (distance + u64::from(hostile)).max(1)
            };
            let price = u64::from(construction_cost(BuildingKind::Foundry)) + u64::from(lift);
            score += i64::from(payback(peril * share / 1_000, price, greed));
        }
        list.push((
            Investment::Expansion(u16::try_from(index).expect("site counts fit in u16")),
            u32::try_from(score.max(0)).unwrap_or(u32::MAX),
        ));
    }
    for frame in &observation.known_frames {
        let me = observation.me;
        // Measured to the nearest tile of the frame, so mirrored frames
        // measure alike. A seat that walks to no tile of it holds it from
        // the nearest ground its ground units shoot it from.
        let walk = |player: PlayerId| {
            crate::frame::tiles_of(*frame, WORKS)
                .map(|tile| map.distance(player, tile))
                .min()
                .unwrap_or(UNREACHABLE)
        };
        let reach = |player: PlayerId| match walk(player) {
            UNREACHABLE => map.firing_distance(player, *frame),
            walked => walked,
        };
        // Ground the seat or an ally already holds with a Foundry is home.
        let settled = walk(me) == UNREACHABLE
            && observation
                .my_buildings
                .iter()
                .chain(&observation.ally_buildings)
                .any(|building| {
                    building.kind == BuildingKind::Foundry
                        && building.built
                        && map.component(building.anchor) == map.component(*frame)
                });
        let distance = if settled { 0 } else { reach(me) };
        let hostile = map.hostiles(me).map(reach).min().unwrap_or(UNREACHABLE);
        let walked = map.hostiles(me).any(|other| walk(other) != UNREACHABLE);
        // A frame deep in enemy ground, at least twice as near an enemy as
        // home, is theirs to hold: a scout that finds one there gives the
        // seat nothing to build. One its home ground does not reach is left
        // to the rules for other ground, unless only an enemy's guns reach it.
        if hostile != UNREACHABLE
            && if distance == UNREACHABLE {
                !walked
            } else {
                hostile < distance && u64::from(hostile) * 2 <= u64::from(distance)
            }
        {
            continue;
        }
        if held(observation, *frame, -1)
            || memory.failed(BuildingKind::Extractor, *frame, observation.tick)
            || contested(observation, *frame)
        {
            continue;
        }
        let gain = yield_per_minute(0, supported(observation, *frame));
        let mut score = u64::from(payback(
            gain,
            u64::from(construction_cost(BuildingKind::Extractor)),
            greed,
        ));
        // An enemy's ground reaches this frame no later than the seat's, so
        // it stands to be lost sooner than one on the seat's own side: worth
        // half on the line between them, and less the nearer the enemy, so a
        // frame a scout finds at an enemy's door is not worth saving for.
        if hostile != UNREACHABLE && distance != UNREACHABLE && distance >= hostile {
            score = score * u64::from(hostile.max(1)) / (2 * u64::from(distance).max(1));
        }
        let score = score * 1_000 / (1_000 + danger(observation, map, memory, *frame));
        list.push((
            Investment::Extractor(*frame),
            u32::try_from(score).unwrap_or(u32::MAX),
        ));
    }
    list
}

/// What a Foundry at `anchor` would pay back through the seat's remote
/// Extractors and Deep Bores in its reach, scored as other growth is: what
/// they would yield supported over what they yield now, and once a Crucible
/// stands, what the Deep Bores the Extractors could then become yield over
/// that, at their refit's price; over the Foundry's price and `lift`, what
/// landing a builder still costs. Free frames are left to the Extractor investments, so a Foundry
/// comes for Extractors stranded without support rather than as an opener.
fn support_payback(observation: &ObservationData, anchor: TilePos, lift: u32, greed: u8) -> u32 {
    let stranded = |bored: bool| -> u64 {
        let count = observation
            .my_buildings
            .iter()
            .filter(|building| {
                building.kind == BuildingKind::Extractor
                    && building.built
                    && (building.tier > 0) == bored
                    && !supported(observation, building.anchor)
                    && foundry_supports(anchor, building.anchor)
                    // A Foundry already going up in reach will support it.
                    && !observation.my_buildings.iter().any(|site| {
                        site.kind == BuildingKind::Foundry
                            && !site.built
                            && foundry_supports(site.anchor, building.anchor)
                    })
            })
            .count();
        u64::try_from(count).expect("building counts fit in u64")
    };
    let (remote, bores) = (stranded(false), stranded(true));
    let support = |tier: u8| yield_per_minute(tier, true) - yield_per_minute(tier, false);
    let mut gain = support(0) * remote + support(1) * bores;
    let mut price = u64::from(construction_cost(BuildingKind::Foundry) + lift);
    if crate::investments::built(observation, BuildingKind::Crucible) {
        gain += (yield_per_minute(1, true) - yield_per_minute(0, true)) * remote;
        price += u64::from(BuildingKind::Extractor.tier_stats(1).construction.cost) * remote;
    }
    if gain == 0 {
        return 0;
    }
    payback(gain, price, greed)
}

/// The score of growth that adds `gain` scrap a minute for `price` scrap:
/// what each scrap spent returns a minute, weighted by greed as a site's
/// resources are.
pub(crate) fn payback(gain: u64, price: u64, greed: u8) -> u32 {
    let weight = 750 + 5 * u64::from(greed);
    let score = gain * 1_000 * PAYBACK_SCALE / price.max(1) * weight / 1_000;
    u32::try_from(score).unwrap_or(u32::MAX)
}

/// Scrap an Extractor at `tier` earns over `ticks`, `supported` or remote.
pub(crate) fn yields(tier: u8, supported: bool, ticks: u64) -> u64 {
    let (amount, period) = match (tier > 0, supported) {
        (false, false) => stats::EXTRACTOR_REMOTE_YIELD,
        (false, true) => stats::EXTRACTOR_SUPPORTED_YIELD,
        (true, false) => stats::DEEP_BORE_REMOTE_YIELD,
        (true, true) => stats::DEEP_BORE_SUPPORTED_YIELD,
    };
    u64::from(amount) * ticks / period
}

/// Scrap an Extractor at `tier` earns a minute, `supported` or remote.
pub(crate) fn yield_per_minute(tier: u8, supported: bool) -> u64 {
    yields(tier, supported, 60 * u64::from(TICKS_PER_SECOND))
}

/// Whether a completed own Foundry supports an Extractor on `frame`, by the
/// simulation's own reach.
pub(crate) fn supported(observation: &ObservationData, frame: TilePos) -> bool {
    crate::investments::built_foundries(&observation.my_buildings)
        .any(|foundry| foundry_supports(foundry.anchor, frame))
}

/// Whether the seat holds a Deep Bore, standing or still refitting, or an
/// Extractor it could refit into one. With none, a Spire's or cradle's
/// prerequisites have no next step and saving for one would hold the seat's
/// scrap for nothing.
pub(crate) fn bore_within_reach(observation: &ObservationData) -> bool {
    observation.my_buildings.iter().any(|building| {
        building.kind == BuildingKind::Extractor
            && (building.tier > 0 || (building.built && supported(observation, building.anchor)))
    })
}

/// Whether the seat already has a built Foundry or a worker on `ground`, so
/// building there needs no lift.
fn settled(observation: &ObservationData, map: &MapModel, ground: Option<u32>) -> bool {
    crate::investments::built_foundries(&observation.my_buildings)
        .any(|b| map.component(b.anchor) == ground)
        || observation
            .my_units
            .iter()
            .any(|u| worker(u.kind) && map.component(u.tile) == ground)
}

/// What the seat still lacks to lift a builder: an Airworks and its
/// Fabricator, and a first carrier.
fn lift_cost(observation: &ObservationData) -> u32 {
    let tech: u32 = [BuildingKind::Fabricator, BuildingKind::Airworks]
        .into_iter()
        .filter(|kind| !crate::investments::built(observation, *kind))
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

pub(crate) fn construction_cost(kind: BuildingKind) -> u32 {
    kind.base_stats().construction.cost
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
    let frames = i64::try_from(known_frames(observation, site)).expect("frame counts fit in i64");
    // Once a Crucible stands, a Foundry here would let own remote Extractors
    // in its reach become Deep Bores.
    let deepens = if crate::investments::built(observation, BuildingKind::Crucible) {
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
        + i64::try_from(danger(observation, map, memory, anchor)).unwrap_or(i64::MAX);
    let weight = 750 + 5 * i64::from(greed);
    Some(resource * weight / 1_000 - penalty * (2_000 - weight) / 1_000)
}

/// The known frames of `site` a Foundry there would let the seat work: free,
/// or holding an own Extractor no Foundry supports yet.
fn known_frames(observation: &ObservationData, site: &Site) -> usize {
    site.frames
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
        .count()
}

/// Whether someone already works `site`: an own or enemy Foundry, or any
/// ally building, near its nodes on the same ground. Another enemy building
/// there is danger, which the site's value weighs, not a claim. A site of
/// frames alone is worked once an ally's Foundry supports one of its frames,
/// so teammates do not crowd one expansion, or once own Foundries support
/// every frame no other seat holds. An enemy's Foundry supports only its own
/// Extractors.
fn claimed(observation: &ObservationData, map: &MapModel, site: &Site) -> bool {
    if site.nodes.is_empty() {
        let foundries = |buildings: &[BuildingObs]| {
            buildings
                .iter()
                .filter(|building| building.kind == BuildingKind::Foundry)
                .map(|building| building.anchor)
                .collect::<Vec<_>>()
        };
        let allied = foundries(&observation.ally_buildings);
        let own = foundries(&observation.my_buildings);
        let others = |frame: TilePos| {
            observation
                .ally_buildings
                .iter()
                .chain(&observation.enemy_buildings)
                .any(|building| gap(frame, WORKS, building.anchor, building.kind.size()) < 0)
        };
        return site.frames.iter().any(|frame| {
            allied
                .iter()
                .any(|anchor| foundry_supports(*anchor, *frame))
        }) || site.frames.iter().all(|frame| {
            others(*frame) || own.iter().any(|anchor| foundry_supports(*anchor, *frame))
        });
    }
    let foundry = |building: &&BuildingObs| building.kind == BuildingKind::Foundry;
    let own = observation.my_buildings.iter().filter(foundry);
    let enemy = observation.enemy_buildings.iter().filter(foundry);
    own.chain(&observation.ally_buildings)
        .chain(enemy)
        .any(|building| {
            let size = building.kind.size();
            let local = site
                .anchors
                .iter()
                .any(|anchor| map.component(*anchor) == map.component(building.anchor));
            site.nodes
                .iter()
                .any(|(node, _)| local && gap(*node, (1, 1), building.anchor, size) <= HELD_REACH)
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
    let from = crate::investments::built_foundries(&observation.my_buildings)
        .filter(|b| map.component(b.anchor) == map.component(anchor))
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

/// Whether an enemy in sight that could hurt a building stands near the
/// two-by-two footprint at `anchor`: an Extractor lost there, or a new one,
/// waits until it leaves.
fn contested(observation: &ObservationData, anchor: TilePos) -> bool {
    observation.enemy_units.iter().any(|enemy| {
        crate::defenses::reach(enemy.kind) > Fx::ZERO
            && gap(anchor, WORKS, enemy.tile, (1, 1)) <= DANGER_REACH
    })
}

/// Remembered enemy units that could hurt a building near the two-by-two
/// footprint at `anchor`, weighted by confidence, plus known enemy buildings
/// whose guns reach it.
fn danger(observation: &ObservationData, map: &MapModel, memory: &Memory, anchor: TilePos) -> u64 {
    let now = observation.tick;
    let units: u64 = memory
        .units()
        .iter()
        .filter(|unit| {
            crate::defenses::reach(unit.kind) > Fx::ZERO
                && gap(anchor, WORKS, unit.tile, (1, 1)) <= DANGER_REACH
        })
        .map(|unit| unit.value(now))
        .sum();
    let buildings: u64 = observation
        .enemy_buildings
        .iter()
        .filter(|building| {
            crate::placement::gun_hits(map, building, BuildingKind::Extractor, anchor)
        })
        .map(|building| u64::from(building.kind.base_stats().construction.cost))
        .sum();
    units + buildings
}
