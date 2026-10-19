//! Static defense: Turrets, Bastions and Flak Turrets beside the seat's most
//! valuable buildings, on the side threats come from, Arrays watching the
//! way in, Barricades ahead of the guns, Scuttle Charges on the approach,
//! upgrades for them, and a Repair Bay where the seat's wounded are. Each is
//! an ordinary investment, worth what it adds to the cover of those
//! buildings' approaches. A short defense also buys one defense at once where
//! attackers find a building's approach uncovered.

mod approaches;
mod bay;
mod guns;
mod lattice;
mod obstacles;

use crate::composition;
use crate::decision::Ledger;
use crate::frame::{HomeFrame, centre_of, footprint_centre, gap, tiles_of};
use crate::investments::Investment;
use crate::map::{Gate, MapModel, Shot};
use crate::memory::Memory;
use crate::placement::{self, Layout};
use crate::profile::{PersonalityTraits, ResolvedProfile};
use crate::workers;
pub(crate) use bay::Span;
use chassis::fx::Fx;
use chassis::grid::TilePos;
use guns::{SPLASH_TARGETS, strength, upgrade};
use oxide_sim::observation::ObservationData;
use oxide_sim::stats::{BuildingStats, Domain};
use oxide_sim::{BuildingKind, UnitKind};

/// Tiles a built mast of `kind` at `tier` reaches by radar; zero without one.
fn radar(kind: BuildingKind, tier: u8) -> i64 {
    i64::from(kind.tier_stats(tier).radar.unwrap_or(0))
}

/// Divides an obstacle's worth into investment points.
const OBSTACLE_POINTS: u64 = 16;

/// Empty tiles between a building and a defense guarding it.
const STANDOFF: [i32; 2] = [2, 3];

/// Divides a Repair Bay's worth into investment points.
const BAY_POINTS: u64 = 2;

/// Divides a defense's worth, or an upgrade's, into investment points.
const POINTS: u64 = 48;

/// Scrap a gun's worth is quoted per, so scores per scrap stay on the scale
/// of the seat's other investments.
const QUOTE: u64 = 100;

/// Divides a gun's worth, the army scrap it holds off for each [`QUOTE`] of
/// its price, or a gun upgrade's, measured the same way, into investment
/// points.
const GUN_POINTS: u64 = 5;

/// Per mille of a defense's worth an attack is assumed to bring when it means
/// to beat it, so guns worth a threat divided by it hold that threat off. It
/// models the attackers, not the seat, so it is the same at every difficulty:
/// the seat's own attack margin says how it attacks, not how others do.
const ATTACKER_MARGIN: u64 = 2_000;

/// What a seat's defenses must stand up to: at least an army at the stance's
/// minimum along any approach.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stakes {
    pub(crate) minimum: u64,
}

impl Stakes {
    pub(crate) fn of(profile: &ResolvedProfile) -> Self {
        Stakes {
            minimum: crate::missions::minimum(profile.stance),
        }
    }
}

#[cfg(test)]
impl Default for Stakes {
    /// A Balanced seat's.
    fn default() -> Self {
        Stakes {
            minimum: crate::missions::minimum(oxide_sim::scenario::BotStance::Balanced),
        }
    }
}

/// Tiles from a building inside which visible attackers call for an
/// emergency defense.
const EMERGENCY_TILES: i64 = 12;

/// How sure the seat is of where a building's threat comes from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Evidence {
    /// Only public facts across a chasm: a hostile start or a known enemy
    /// building whose units could reach the building only by landing.
    Landing,
    /// Only public facts: a hostile start or a known enemy building.
    Prior,
    /// Enemies it remembers.
    Remembered,
    /// Enemies in sight now.
    Current,
}

impl Evidence {
    /// How much the evidence counts.
    fn weight(self) -> u64 {
        match self {
            Evidence::Landing | Evidence::Prior => 2,
            Evidence::Remembered => 4,
            Evidence::Current => 6,
        }
    }
}

/// Where a building is attacked from in one domain.
struct Approach {
    /// How far the asset's buildings reach toward the threat beyond its
    /// Foundry or Extractor: a step along the straight way in, in doubled
    /// coordinates, measured as [`along`](approaches::along) measures; none at
    /// a cut.
    edge: i64,
    /// The cut the way in runs through, held instead of the base's edge.
    cut: Option<Held>,
    /// Points along the way in, beyond the edge or from each of the cut's
    /// gates, in doubled coordinates.
    samples: Vec<(i64, i64)>,
    /// At a cut, the gate each sample leads from.
    gate_of: Vec<usize>,
    /// For each sample, in thousandths, how far own weapons covering it fall
    /// short of holding off the threat along the way: two thousand where none
    /// covers it, none once they hold.
    open: Vec<u64>,
    /// For each sample, whether own buildings see it.
    spotted: Vec<bool>,
    /// Points further along the way in, where an Array watches, and whether
    /// own buildings see or pick up each, or will once built.
    far: Vec<((i64, i64), bool)>,
    evidence: Evidence,
    /// The threat the samples lead to.
    source: (i64, i64),
    /// The cover that holds the threat off at a sample, in army scrap.
    need: u64,
    /// The threat along the way, in army scrap, how its known enemies reach
    /// buildings, and whether they gather in clumps.
    threat: Threat,
}

/// A cut on the way in to the seat's start, held by guns on the home side of
/// each of its gates and charges across them.
struct Held {
    gates: Vec<Gate>,
    /// Ground distance from the seat's start to its near side, in tenths of
    /// a tile.
    distance: u16,
}

/// The value of the armed enemies the seat remembers near a source, at least
/// an army at the stance's minimum, their reach against buildings with each
/// one's value, and whether they gather in clumps a shell's splash hits
/// several of.
#[derive(Clone, Default)]
struct Threat {
    value: u64,
    reaches: Vec<(Fx, u64)>,
    clustered: bool,
}

impl Approach {
    /// The army scrap still missing at a sample `open` thousandths short.
    fn shortfall(&self, open: u64) -> u64 {
        self.need * open / 2_000
    }

    /// The shortfall summed over the samples `counts`, given each sample and
    /// whether own buildings see it, each at most what `closes` there.
    fn short(
        &self,
        counts: impl Fn((i64, i64), bool) -> bool,
        closes: impl Fn((i64, i64)) -> u64,
    ) -> u64 {
        self.samples
            .iter()
            .zip(&self.open)
            .zip(&self.spotted)
            .filter(|((point, _), spotted)| counts(**point, **spotted))
            .map(|((point, open), _)| self.shortfall(*open).min(closes(*point)))
            .sum()
    }

    /// What `cover` holds off along this way in: its strength, less the
    /// share of the threat it cannot fire back at.
    fn held(&self, cover: &Cover) -> u64 {
        let unanswered: u64 = self
            .threat
            .reaches
            .iter()
            .filter(|(reach, _)| !cover.answers(*reach))
            .map(|(_, value)| value)
            .sum();
        let value = self.threat.value.max(1);
        cover.value[usize::from(self.threat.clustered)] * (value - unanswered.min(value)) / value
    }
}

/// What the seat guards: a base, grown from its Foundry, or an Extractor out
/// on its own.
struct Asset {
    anchor: TilePos,
    size: (i32, i32),
    centre: (i64, i64),
    value: u64,
    /// The buildings guarded together, as anchors and sizes, the Foundry or
    /// the Extractor first.
    members: Vec<(TilePos, (i32, i32))>,
    ground: Option<Approach>,
    air: Option<Approach>,
    /// Points where a warhead from the nearest known enemy Spire crosses
    /// into the ring of an own Lattice guarding the asset, just outside,
    /// at and just inside it, and whether own buildings see or pick up
    /// each, or will once built: radar there tracks a warhead before the
    /// Lattice must stop it.
    missile: Vec<((i64, i64), bool)>,
}

impl Asset {
    /// A lone building of `size` at `anchor` worth `value`, its approaches not
    /// yet known.
    fn new(anchor: TilePos, size: (i32, i32), centre: (i64, i64), value: u64) -> Self {
        Asset {
            anchor,
            size,
            centre,
            value,
            members: vec![(anchor, size)],
            ground: None,
            air: None,
            missile: Vec::new(),
        }
    }

    fn approach(&self, domain: Domain) -> Option<&Approach> {
        match domain {
            Domain::Ground => self.ground.as_ref(),
            Domain::Air => self.air.as_ref(),
        }
    }
}

/// Where a weapon reaches, in doubled coordinates.
#[derive(Clone, Copy)]
struct Cover {
    centre: (i64, i64),
    /// What the weapon holds off, in army scrap, against a spread enemy and
    /// a clustered one: see [`strength`].
    value: [u64; 2],
    /// Squared doubled reach, and squared doubled minimum range.
    reach2: i64,
    min2: i64,
    /// Reach and minimum range, from the footprint centre.
    range: Fx,
    minimum: Fx,
    size: (i32, i32),
    ground: bool,
    air: bool,
    /// Fires shells beyond its own sight, so only where something spots
    /// for it.
    indirect: bool,
    /// The anchor the gun stands on, and how its shots at ground travel,
    /// for the levels it reaches on a tiered map.
    footing: TilePos,
    shot: Shot,
}

impl Cover {
    fn of(kind: BuildingKind, tier: u8, anchor: TilePos) -> Option<Self> {
        let stats = kind.tier_stats(tier);
        let mut cover = Cover {
            centre: footprint_centre(kind, anchor),
            value: SPLASH_TARGETS.map(|targets| strength(stats, targets)),
            reach2: 0,
            min2: i64::MAX,
            range: Fx::ZERO,
            minimum: Fx::MAX,
            size: kind.size(),
            ground: false,
            air: false,
            indirect: false,
            footing: anchor,
            shot: Shot::Arc,
        };
        for weapon in stats.weapons {
            let reach = (weapon.range + weapon.range).to_num::<i64>();
            let min = (weapon.minimum_range + weapon.minimum_range).to_num::<i64>();
            cover.reach2 = cover.reach2.max(reach * reach);
            cover.min2 = cover.min2.min(min * min);
            cover.range = cover.range.max(weapon.range);
            cover.minimum = cover.minimum.min(weapon.minimum_range);
            cover.ground |= weapon.targets.ground;
            cover.air |= weapon.targets.air;
            cover.indirect |= weapon.indirect;
            if weapon.targets.ground && !weapon.indirect {
                cover.shot = Shot::Ground;
            }
        }
        (cover.ground || cover.air).then_some(cover)
    }

    /// Whether the gun fires back at an enemy of `reach` attacking it. The
    /// enemy measures to the footprint's nearest edge and the gun from its
    /// centre, so the gun must reach the enemy at a corner, and the enemy
    /// at a flat side must stand clear of the gun's minimum range.
    fn answers(self, reach: Fx) -> bool {
        let (width, height) = (i64::from(self.size.0), i64::from(self.size.1));
        let past = self.range - reach;
        // Four squared half-diagonals, against four squared reach to spare.
        let corner = Fx::from_num(width * width + height * height);
        let side = Fx::from_num(width.min(height)) / 2;
        past >= Fx::ZERO && past * past * 4 >= corner && side + reach >= self.minimum
    }

    /// Whether the gun reaches `point`, in doubled coordinates, in
    /// `domain`: in range, and on a tiered map at a level it fires at.
    fn covers(self, map: &MapModel, domain: Domain, point: (i64, i64)) -> bool {
        let fires = match domain {
            Domain::Ground => self.ground,
            Domain::Air => self.air,
        };
        let distance2 = (self.centre.0 - point.0).pow(2) + (self.centre.1 - point.1).pow(2);
        let tile = TilePos::new(
            i32::try_from(point.0.div_euclid(2)).unwrap_or(i32::MAX),
            i32::try_from(point.1.div_euclid(2)).unwrap_or(i32::MAX),
        );
        fires
            && distance2 <= self.reach2
            && distance2 >= self.min2
            && (domain == Domain::Air || map.levels_allow(self.footing, tile, self.shot))
    }
}

/// What one decision knows about guarding the seat's buildings.
struct Guard<'a> {
    observation: &'a ObservationData,
    map: &'a MapModel,
    memory: &'a Memory,
    frame: HomeFrame,
    assets: Vec<Asset>,
    /// The seat's own armed buildings, built or not.
    covers: Vec<Cover>,
    /// The seat's built Foundries, whose layouts keep their lanes clear.
    foundries: Vec<TilePos>,
    /// Ground the seat's Harvesters stand on: only there can it build.
    crews: Vec<u32>,
    /// Whether the opening economy is up. Before then, a defense against a
    /// threat known only from public facts would take scrap and a Harvester
    /// the economy still needs.
    settled: bool,
    /// Whether the seat's army is too small to hold on its own, so a threat
    /// known only from public facts weighs as much as a remembered one.
    exposed: bool,
    stakes: Stakes,
}

impl Guard<'_> {
    /// How much a threat's evidence counts for a defense of `kind`. A seat
    /// without an army to speak of guards against the hostile start almost as
    /// if it had seen enemies, and against a landing with a Turret.
    fn weight(&self, approach: &Approach, kind: BuildingKind) -> u64 {
        match approach.evidence {
            Evidence::Prior if self.exposed => 5,
            Evidence::Landing if kind == BuildingKind::Turret => 5,
            evidence => evidence.weight(),
        }
    }

    /// Own weapons covering `point` in `domain`.
    fn covered(&self, domain: Domain, point: (i64, i64)) -> usize {
        self.covers
            .iter()
            .filter(|cover| cover.covers(self.map, domain, point))
            .count()
    }

    /// What a defense of `kind` at `anchor` adds to `asset`'s approach: for a
    /// gun, the army scrap it closes of the shortfall against the threat
    /// along the way at each sample it covers, at most its strength. A gun
    /// firing indirect shells counts only samples its owner's or an ally's
    /// buildings see, since it fires no further than something spots for it.
    /// An Array adds
    /// two thousand for each point further along the way in that nothing
    /// sees yet, nor will once built.
    fn gain(
        map: &MapModel,
        asset: &Asset,
        kind: BuildingKind,
        anchor: TilePos,
        reach: Option<Cover>,
    ) -> u64 {
        if kind == BuildingKind::Array {
            let centre = footprint_centre(kind, anchor);
            let radar = radar(kind, 0);
            return 2_000
                * Self::watched(asset, |point, seen| {
                    !seen && distance2(centre, point) <= 4 * radar * radar
                });
        }
        let Some(reach) = reach else {
            return 0;
        };
        let cover = Cover {
            centre: footprint_centre(kind, anchor),
            footing: anchor,
            ..reach
        };
        let domain = domain(kind);
        let Some(approach) = asset.approach(domain) else {
            return 0;
        };
        approach.short(
            |point, spotted| cover.covers(map, domain, point) && (!cover.indirect || spotted),
            |_| approach.held(&cover),
        )
    }

    /// Points far along `asset`'s ways in, from the ground and the air, that
    /// `counts`, given each point and whether own buildings see it.
    fn watched(asset: &Asset, counts: impl Fn((i64, i64), bool) -> bool) -> u64 {
        [Domain::Ground, Domain::Air]
            .into_iter()
            .filter_map(|domain| asset.approach(domain))
            .flat_map(|approach| &approach.far)
            .chain(&asset.missile)
            .filter(|(point, seen)| counts(*point, *seen))
            .count() as u64
    }

    /// Whether prior evidence may count yet.
    fn usable(&self, approach: &Approach) -> bool {
        !matches!(approach.evidence, Evidence::Landing | Evidence::Prior) || self.settled
    }

    /// Whether `kind` may go at `anchor` as far as the seat knows, on ground
    /// a Harvester of the seat stands on, and was not refused there lately:
    /// off the lanes of the base's layout and the seat's gates, or for a
    /// buried Scuttle Charge, which blocks nothing, only on them, clear of the
    /// blocks' slots, unless no Foundry lays out its ground.
    fn placeable(&self, kind: BuildingKind, anchor: TilePos) -> bool {
        let crewed = self
            .map
            .component(anchor)
            .is_some_and(|ground| self.crews.contains(&ground));
        let laned =
            || tiles_of(anchor, kind.size()).any(|tile| self.map.lane_of(&self.foundries, tile));
        let me = self.observation.me;
        crewed
            && !self.memory.failed(kind, anchor, self.observation.tick)
            && if kind == BuildingKind::ScuttleCharge {
                self.map.gated(me, anchor) || !self.map.laid_out(&self.foundries, anchor) || laned()
            } else {
                !laned() && tiles_of(anchor, kind.size()).all(|tile| !self.map.gated(me, tile))
            }
            && placement::check(self.map, self.observation, kind, anchor, &[], Layout::Apart)
                .is_ok()
    }
}

/// Voluntary defenses, defense upgrades and Repair Bays worth investing in,
/// with their scores. Personality weighs each kind: fortification for
/// Turrets, Bastions and Barricades, fortification and support for Flak
/// Turrets, fortification and guile for Arrays and Scuttle Charges,
/// fortification and greed for upgrades, support for Repair Bays.
pub(crate) fn investments(
    observation: &ObservationData,
    map: &MapModel,
    memory: &Memory,
    traits: PersonalityTraits,
    settled: bool,
    exposed: bool,
    stakes: Stakes,
) -> Vec<(Investment, u32)> {
    let Some(guard) = Guard::new(
        observation,
        map,
        memory,
        Scope::Bases,
        settled,
        exposed,
        stakes,
    ) else {
        return Vec::new();
    };
    let fortification = u64::from(traits.fortification);
    let cunning = u64::midpoint(fortification, u64::from(traits.guile));
    let weights = [
        (BuildingKind::Turret, fortification),
        (BuildingKind::Bastion, fortification),
        (
            BuildingKind::FlakTurret,
            u64::midpoint(fortification, u64::from(traits.support)),
        ),
        (BuildingKind::Array, cunning / 2),
    ];
    let mut list: Vec<(Investment, u32)> = weights
        .into_iter()
        .filter_map(|(kind, weight)| {
            let (anchor, worth) = guard.best(kind)?;
            // Guns are weighed by what they hold off per scrap; an Array's
            // radar is not a gun.
            let score = match kind {
                BuildingKind::Array => worth * weight / POINTS,
                kind => worth * weight * QUOTE / (GUN_POINTS * price(kind.base_stats())),
            };
            Some((Investment::Defense { kind, anchor }, points(score)))
        })
        .collect();
    if let Some((anchor, worth)) = guard.barricade() {
        let kind = BuildingKind::Barricade;
        list.push((
            Investment::Defense { kind, anchor },
            points(worth * fortification / OBSTACLE_POINTS),
        ));
    }
    let fabricator = observation
        .my_buildings
        .iter()
        .any(|building| building.kind == BuildingKind::Fabricator && building.built);
    if fabricator && let Some((anchor, worth)) = guard.charge() {
        let kind = BuildingKind::ScuttleCharge;
        list.push((
            Investment::Defense { kind, anchor },
            points(worth * cunning / OBSTACLE_POINTS),
        ));
    }
    if let Some((anchor, stake)) = guard.lattice() {
        let kind = BuildingKind::Lattice;
        list.push((
            Investment::Defense { kind, anchor },
            points(stake * composition::weight(traits.fortification) / 1_000),
        ));
    }
    if let Some((anchor, wounds)) = guard.bay() {
        let kind = BuildingKind::RepairBay;
        list.push((
            Investment::Defense { kind, anchor },
            points(wounds * composition::weight(traits.support) / 1_000 / BAY_POINTS),
        ));
    }
    let upgrade_weight = u64::midpoint(fortification, u64::from(traits.greed));
    list.extend(
        observation
            .my_buildings
            .iter()
            .filter_map(|building| upgrade(&guard, building, upgrade_weight)),
    );
    list
}

/// Buys a defense now beside each valuable building whose approach visible
/// attackers near it find uncovered: a Turret against ground attackers, a
/// Flak Turret against aircraft. Each such building has one standing
/// unfinished at a time.
pub(crate) fn emergency(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    memory: &Memory,
    stakes: Stakes,
    ledger: &mut Ledger,
) {
    let Some(guard) = Guard::new(
        observation,
        map,
        memory,
        Scope::Buildings,
        true,
        true,
        stakes,
    ) else {
        return;
    };
    for (domain, kind) in [
        (Domain::Ground, BuildingKind::Turret),
        (Domain::Air, BuildingKind::FlakTurret),
    ] {
        let size = kind.size();
        // An unfinished one beside its buildings already answers it.
        let unanswered = |asset: &Asset| {
            !observation.my_buildings.iter().any(|building| {
                building.kind == kind
                    && !building.built
                    && asset.members.iter().any(|(anchor, footprint)| {
                        gap(*anchor, *footprint, building.anchor, size) <= STANDOFF[1]
                    })
            })
        };
        let pressed = |asset: &Asset| {
            asset.approach(domain).is_some_and(|approach| {
                approach.evidence == Evidence::Current
                    && asset.members.iter().any(|(anchor, footprint)| {
                        chebyshev(approach.source, centre_of(*anchor, *footprint))
                            <= 2 * EMERGENCY_TILES
                    })
                    && approach
                        .samples
                        .iter()
                        .all(|point| guard.covered(domain, *point) == 0)
            })
        };
        let price = kind
            .base_stats()
            .construction
            .as_ref()
            .map_or(u32::MAX, |construction| construction.cost);
        for asset in guard
            .assets
            .iter()
            .filter(|asset| pressed(asset) && unanswered(asset))
        {
            let Some((anchor, _)) = guard.best_where(kind, |other| other.anchor == asset.anchor)
            else {
                continue;
            };
            let Ok(allowed) = placement::check(
                guard.map,
                observation,
                kind,
                anchor,
                ledger.planned(),
                Layout::Apart,
            ) else {
                continue;
            };
            let centre = footprint_centre(kind, anchor);
            if ledger.spendable() < price {
                break;
            }
            if let Some(builder) = workers::builder(observation, map, frame, anchor, centre, ledger)
            {
                ledger.build(builder, kind, anchor, allowed.defer, price);
            }
        }
    }
}

/// How far `kind` strikes buildings, a demolition charge's contact
/// included; zero when it cannot.
pub(crate) fn reach(kind: UnitKind) -> Fx {
    let stats = kind.stats();
    let contact = stats.demolition.map(|charge| charge.contact_range);
    stats
        .weapons
        .iter()
        .filter(|weapon| weapon.targets.ground)
        .map(|weapon| weapon.range)
        .chain(contact)
        .max()
        .unwrap_or(Fx::ZERO)
}

/// What a guard looks after.
#[derive(Clone, Copy)]
enum Scope {
    /// Bases and Extractors out on their own, defended in front of their
    /// edges.
    Bases,
    /// Each building on its own, as an attack on one is answered beside it.
    Buildings,
}

fn distance2(a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.0 - b.0).pow(2) + (a.1 - b.1).pow(2)
}

/// The domain a defense of `kind` fires at: air for a gun that hits only
/// aircraft, ground otherwise, an unarmed work such as the Array included.
fn domain(kind: BuildingKind) -> Domain {
    let weapons = kind.base_stats().weapons;
    if !weapons.is_empty() && weapons.iter().all(|weapon| !weapon.targets.ground) {
        Domain::Air
    } else {
        Domain::Ground
    }
}

fn chebyshev(a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

/// What a building costs to place.
fn price(stats: &BuildingStats) -> u64 {
    stats
        .construction
        .as_ref()
        .map_or(QUOTE, |construction| u64::from(construction.cost).max(1))
}

fn points(worth: u64) -> u32 {
    u32::try_from(worth).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;
