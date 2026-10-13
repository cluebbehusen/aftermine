//! The impact ledger: what every unit and building did over a match, valued
//! in scrap. It observes each tick's state and events, so it serves live
//! evaluation and replays alike, and it is omniscient QA evidence that never
//! reaches a controller.
//!
//! The simulation names a shooter on each hit but reports neither damage
//! amounts nor killers, so the ledger diffs every unit's and building's
//! health tick to tick, net of reported repairs, and splits each loss among
//! the sources that hit that body that tick: targeted and blind hits, turret
//! fire, shells matched from launch to landing across their splash, charge
//! detonations and Sapper blasts. A loss is valued at the victim's worth
//! times the fraction of its health lost. Two credits fall outside damage
//! dealt: repair, to the welder or Repair Bay that supplied it, and
//! spotting, to the nearest friendly body whose sight covered a target its
//! shooter could not see, approximated by vision radius. Crew repair of
//! buildings, radar warning and the harvester recovery trickle report
//! nothing the ledger can credit.

use chassis::grid::TilePos;
use oxide_sim::stats::{
    BuildingKind, CHARGE_ARRAY_DETECT_RADIUS, CHARGE_BASE_ARRAY_DETECT_RADIUS, CHARGE_BLAST_RADIUS,
    CRUCIBLE_SMELT_PERIOD, CRUCIBLE_SMELT_RADIUS, FOUNDRY_DRIP_PERIOD, FOUNDRY_DRIP_START_TICK,
    RECLAIMER_PERIOD, REFINERY_PERIOD, UnitKind,
};
use oxide_sim::{
    Building, BuildingId, Event, Order, State, Target, TickReport, UnitId, UnitRepairSource,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Ticks between net-worth samples.
pub const WORTH_PERIOD: u64 = 1_000;

/// Tiles from a seat's buildings, other than mines and Barricades scattered
/// across the map, inside which a fight counts as at its home.
const PLACE_TILES: i32 = 12;

/// Ticks per minute.
const MINUTE: u64 = oxide_sim::TICKS_PER_SECOND as u64 * 60;

/// Minutes at which the spending phases begin, after the first.
const PHASES: [u64; 3] = [5, 10, 20];

/// Damage, spotting and repair accumulate in thousandths of a scrap, so a
/// loss split over many small hits keeps its value, and are rounded to scrap
/// once, in [`ImpactLedger::finish`].
const MILLI: u64 = 1_000;

/// Ticks a launched shell may land late and still match its launch.
const SHELL_SLACK: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Unit(UnitId),
    Building(BuildingId),
}

/// What a body is, for valuing the damage it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Army,
    Worker,
    Other,
    Building,
}

/// A unit or building as the ledger last saw it.
#[derive(Debug, Clone, Copy)]
struct Body {
    key: Key,
    owner: u8,
    team: u8,
    hp: u32,
    max_hp: u32,
    /// Scrap the body is worth whole: a unit's cost, or a building's cost
    /// summed over the tiers it has reached.
    value: u64,
    tile: TilePos,
    vision: i32,
    class: Class,
    unit: Option<UnitKind>,
    building: Option<(BuildingKind, u8, bool)>,
    /// The transport a rider travels in; riders count toward worth and die
    /// with their carrier.
    carrier: Option<Key>,
    /// A building's construction or upgrade progress, the build ticks its
    /// tier takes, and the health salvage has drained from it.
    progress: u32,
    build_ticks: u32,
    salvage: u32,
}

impl Body {
    fn tier(&self) -> Option<u8> {
        self.building.map(|(_, tier, _)| tier)
    }

    /// Health construction or an upgrade added by `post`, which emits no
    /// event: the simulation's ramp from a fifth of full health by build
    /// progress. `None` on the tick work starts, which sets the site up.
    fn built_gain(&self, post: &Body) -> Option<u32> {
        if !matches!(self.building, Some((_, _, false))) || self.build_ticks == 0 {
            return Some(0);
        }
        if self.progress == 0 && post.progress > 0 {
            return None;
        }
        let ramp = self.max_hp - self.max_hp / 5;
        let at = |progress: u32| ramp * progress.min(self.build_ticks) / self.build_ticks;
        Some(at(post.progress).saturating_sub(at(self.progress)))
    }
}

/// Who dealt a loss, as they were when they fired: a shell can land after
/// its gun is gone.
#[derive(Debug, Clone, Copy)]
struct Source {
    key: Key,
    owner: u8,
    team: u8,
    vision: i32,
    /// Where it fired from, for spotting credit.
    from: Option<TilePos>,
}

/// One body's tally over its life.
#[derive(Debug, Clone, Default)]
struct Record {
    owner: u8,
    unit: Option<UnitKind>,
    building: Option<BuildingKind>,
    top_tier: u8,
    starting: bool,
    born: u64,
    gone: Option<u64>,
    paid: u64,
    dealt: Dealt,
    taken: u64,
    enabled: u64,
    repaired: u64,
    harvested: u64,
    received: u64,
    passive: u64,
    produced: u64,
    intercepted: u64,
}

/// Scrap value of damage dealt, by what was hit and where.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dealt {
    /// To armed units and the point defenders that shield them.
    pub army: u64,
    /// To workers.
    pub workers: u64,
    /// To other units: transports and scouts.
    pub other: u64,
    /// To buildings.
    pub buildings: u64,
    /// Near the dealer's own buildings.
    pub home: u64,
    /// Near the victim's buildings.
    pub away: u64,
    /// Near neither.
    pub field: u64,
}

impl Dealt {
    /// Everything dealt.
    pub fn total(&self) -> u64 {
        self.army + self.workers + self.other + self.buildings
    }

    fn add(&mut self, other: &Dealt) {
        self.army += other.army;
        self.workers += other.workers;
        self.other += other.other;
        self.buildings += other.buildings;
        self.home += other.home;
        self.away += other.away;
        self.field += other.field;
    }
}

/// One unit kind's, or one building kind's at its highest tier, totals for
/// a seat.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KindLedger {
    /// Bodies the seat built or trained during the match.
    pub built: u64,
    /// Bodies the seat started with.
    pub starting: u64,
    /// Scrap paid, counting starting bodies at their worth.
    pub paid: u64,
    /// Damage dealt, in scrap.
    pub dealt: Dealt,
    /// Damage taken, in scrap.
    pub taken: u64,
    /// Damage friendly shooters dealt to targets only this kind could see,
    /// and hidden charges destroyed inside an Array's detection.
    pub enabled: u64,
    /// Repair supplied to others, in scrap.
    pub repaired: u64,
    /// Scrap delivered by workers.
    pub harvested: u64,
    /// Scrap delivered to this kind of building.
    pub received: u64,
    /// Scrap credited without deliveries: Reclaimers, Extractors and the
    /// Foundry drip.
    pub passive: u64,
    /// Scrap of units this kind of building trained.
    pub produced: u64,
    /// Bodies destroyed.
    pub deaths: u64,
    /// Ticks the destroyed ones lived, summed.
    pub lifetime: u64,
    /// Hostile shells shot down in flight.
    pub intercepted: u64,
}

impl KindLedger {
    /// Adds `other` to these totals.
    pub fn add(&mut self, other: &KindLedger) {
        self.built += other.built;
        self.starting += other.starting;
        self.paid += other.paid;
        self.dealt.add(&other.dealt);
        self.taken += other.taken;
        self.enabled += other.enabled;
        self.repaired += other.repaired;
        self.harvested += other.harvested;
        self.received += other.received;
        self.passive += other.passive;
        self.produced += other.produced;
        self.deaths += other.deaths;
        self.lifetime += other.lifetime;
        self.intercepted += other.intercepted;
    }
}

/// What a seat's units and buildings did over a match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatLedger {
    /// Totals by unit kind.
    pub units: BTreeMap<String, KindLedger>,
    /// Totals by building kind at the highest tier each building reached.
    pub buildings: BTreeMap<String, KindLedger>,
    /// Damage this seat took that no hit accounts for, in scrap.
    pub unattributed: u64,
    /// Scrap spent by `<category>:<kind>`, in the phases starting at minutes
    /// 0, 5, 10 and 20. Categories: `army` and `worker` units, `economy`,
    /// `defense` and `tech` buildings, and `upgrade` to the named tier. Units
    /// count when trained, not when queued and paid, so one still queued
    /// when the match ends is not counted.
    pub spend: BTreeMap<String, [u64; 4]>,
    /// Scrap refunded by cancelled and salvaged buildings.
    pub refunds: u64,
    /// Scrap refunded by units retired at a Crucible: recycling, neither
    /// income nor combat value.
    pub retired: u64,
    /// Net worth every [`WORTH_PERIOD`] ticks after `start`: units, riders
    /// included, and buildings at their worth times their health fraction,
    /// plus the bank.
    pub worth: Vec<u64>,
    /// Tick the ledger began: zero for a match, later for a recording that
    /// starts from a saved world.
    #[serde(default)]
    pub start: u64,
    /// Tick the ledger ended; zero when a serialized row omits it.
    #[serde(default)]
    pub end: u64,
    /// Net worth at `end`.
    #[serde(default)]
    pub final_worth: u64,
}

/// Where a seat's income came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IncomeSource {
    /// Worker deliveries of scrap dug from scrap fields.
    FieldHarvest,
    /// Worker deliveries of scrap stripped from wrecks.
    WreckSalvage,
    /// A Reclaimer's trickle.
    Reclaimer,
    /// A Reclaimer upgraded to a Refinery.
    Refinery,
    /// An Extractor's yield.
    Extractor,
    /// An Extractor upgraded to a Deep Bore.
    DeepBore,
    /// The Foundry drip.
    FoundryDrip,
    /// A Crucible smelting wreck.
    Smelting,
}

impl IncomeSource {
    /// Every source, in report order.
    pub const ALL: [IncomeSource; 8] = [
        IncomeSource::FieldHarvest,
        IncomeSource::WreckSalvage,
        IncomeSource::Reclaimer,
        IncomeSource::Refinery,
        IncomeSource::Extractor,
        IncomeSource::DeepBore,
        IncomeSource::FoundryDrip,
        IncomeSource::Smelting,
    ];

    /// The source's report name.
    pub const fn name(self) -> &'static str {
        match self {
            IncomeSource::FieldHarvest => "field harvest",
            IncomeSource::WreckSalvage => "wreck salvage",
            IncomeSource::Reclaimer => "reclaimer",
            IncomeSource::Refinery => "refinery",
            IncomeSource::Extractor => "extractor",
            IncomeSource::DeepBore => "deep bore",
            IncomeSource::FoundryDrip => "foundry drip",
            IncomeSource::Smelting => "smelting",
        }
    }
}

/// Scrap a seat earned in the last observed tick from one source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Credit {
    /// The earning seat.
    pub seat: u8,
    /// Where it came from.
    pub source: IncomeSource,
    /// Scrap earned.
    pub scrap: u64,
}

/// Health a body lost in the last observed tick and the hostile bodies
/// credited with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The body that lost health.
    pub victim: Target,
    /// Each credited body and its team.
    pub sources: Vec<(Target, u8)>,
}

/// Scrap a worker carries, by where it picked it up.
#[derive(Debug, Clone, Copy, Default)]
struct Cargo {
    field: u32,
    wreck: u32,
}

impl Cargo {
    fn total(self) -> u32 {
        self.field + self.wreck
    }
}

/// A shell in flight, waiting to land.
#[derive(Debug, Clone, Copy)]
struct Shell {
    lands: u64,
    owner: u8,
    to: TilePos,
    source: Source,
}

/// Damage sources reported in one tick.
#[derive(Default)]
struct Sources {
    aimed: BTreeMap<Key, Vec<Source>>,
    blind: Vec<(TilePos, Source)>,
    splash: Vec<(TilePos, i32, Source)>,
    repaired: BTreeMap<Key, u32>,
    /// Charges that fired this tick, spending themselves.
    detonated: Vec<Key>,
    /// Units trained this tick, with their kind and owner.
    born: Vec<(Key, UnitKind, u8)>,
    /// Where each unit that died this tick died.
    died: BTreeMap<Key, TilePos>,
    /// Each unit retired this tick, with its hp as it came apart.
    retired: BTreeMap<Key, u32>,
}

/// The ledger.
pub struct ImpactLedger {
    start: u64,
    teams: Vec<u8>,
    bodies: Vec<Body>,
    records: BTreeMap<Key, Record>,
    unattributed: Vec<u64>,
    spend: Vec<BTreeMap<String, [u64; 4]>>,
    refunds: Vec<u64>,
    retired: Vec<u64>,
    worth: Vec<Vec<u64>>,
    shells: Vec<Shell>,
    /// The Crucibles and wreck in their reach as a smelting tick begins.
    hearths: Hearths,
    /// Extractor payouts as the coming tick begins.
    payouts: Vec<Payout>,
    /// Income credited in the last observed tick.
    credits: Vec<Credit>,
    /// Losses attributed in the last observed tick.
    hits: Vec<Hit>,
    /// Each worker's cargo by origin, kept while it rides a transport.
    cargo: BTreeMap<UnitId, Cargo>,
    /// The tile each harvesting worker was working, as the last observed
    /// tick left it.
    working: BTreeMap<UnitId, TilePos>,
    /// Field scrap on those tiles then.
    field_scrap: BTreeMap<TilePos, u32>,
}

fn snapshot(state: &State) -> Vec<Body> {
    let team = |player: oxide_sim::PlayerId| state.players()[usize::from(player.0)].team;
    let mut bodies: Vec<Body> = state
        .units()
        .iter()
        .map(|unit| {
            let stats = unit.kind.stats();
            Body {
                key: Key::Unit(unit.id),
                owner: unit.player.0,
                team: team(unit.player),
                hp: unit.hp,
                max_hp: stats.max_hp,
                value: u64::from(stats.cost),
                tile: unit.tile(),
                vision: stats.vision,
                class: class(unit.kind),
                unit: Some(unit.kind),
                building: None,
                carrier: None,
                progress: 0,
                build_ticks: 0,
                salvage: 0,
            }
        })
        .collect();
    let riders: Vec<Body> = state
        .units()
        .iter()
        .flat_map(|carrier| {
            carrier.cargo.iter().map(|rider| {
                let stats = rider.kind.stats();
                Body {
                    key: Key::Unit(rider.id),
                    owner: carrier.player.0,
                    team: team(carrier.player),
                    hp: rider.hp,
                    max_hp: stats.max_hp,
                    value: u64::from(stats.cost),
                    tile: carrier.tile(),
                    vision: 0,
                    class: class(rider.kind),
                    unit: Some(rider.kind),
                    building: None,
                    carrier: Some(Key::Unit(carrier.id)),
                    progress: 0,
                    build_ticks: 0,
                    salvage: 0,
                }
            })
        })
        .collect();
    if !riders.is_empty() {
        bodies.extend(riders);
        bodies.sort_unstable_by_key(|body| body.key);
    }
    bodies.extend(state.buildings().iter().map(|building| {
        let stats = building.kind.tier_stats(building.tier);
        Body {
            key: Key::Building(building.id),
            owner: building.player.0,
            team: team(building.player),
            hp: building.hp,
            max_hp: stats.max_hp,
            value: u64::from(building.kind.invested_cost(building.tier)),
            tile: TilePos::containing(building.center()),
            vision: stats.vision,
            class: Class::Building,
            unit: None,
            building: Some((building.kind, building.tier, building.built())),
            carrier: None,
            progress: building.construction_progress().unwrap_or(0),
            build_ticks: stats
                .construction
                .as_ref()
                .map_or(0, |construction| construction.build_ticks),
            salvage: building.salvage_drained,
        }
    }));
    bodies
}

fn class(kind: UnitKind) -> Class {
    let stats = kind.stats();
    if stats.harvest.is_some() {
        Class::Worker
    } else if stats.can_fight() || stats.point_defense.is_some() {
        Class::Army
    } else {
        Class::Other
    }
}

/// A seat's net worth: its share of `bodies` plus its bank in `state`.
fn worth(state: &State, bodies: &[Body], seat: usize) -> u64 {
    let owned: u64 = bodies
        .iter()
        .filter(|body| usize::from(body.owner) == seat)
        .map(|body| body.value * u64::from(body.hp) / u64::from(body.max_hp.max(1)))
        .sum();
    owned + u64::from(state.players()[seat].scrap)
}

/// Built Crucibles and the wreck within their reach, as a tick begins.
#[derive(Debug, Clone, Default)]
struct Hearths {
    crucibles: Vec<Building>,
    wreck: BTreeMap<TilePos, u32>,
}

/// Tiles a Crucible's smelting reach could cover, before the exact radius
/// test.
fn reach_box(building: &Building) -> impl Iterator<Item = TilePos> {
    let reach = CRUCIBLE_SMELT_RADIUS.to_num::<i32>() + 1;
    let (w, h) = building.kind.size();
    let anchor = building.anchor;
    ((anchor.y - reach)..(anchor.y + h + reach)).flat_map(move |y| {
        ((anchor.x - reach)..(anchor.x + w + reach)).map(move |x| TilePos::new(x, y))
    })
}

fn hearths(state: &State) -> Hearths {
    let crucibles: Vec<Building> = state
        .buildings()
        .iter()
        .filter(|building| {
            building.built() && building.hp > 0 && building.kind == BuildingKind::Crucible
        })
        .cloned()
        .collect();
    let wreck = crucibles
        .iter()
        .flat_map(reach_box)
        .map(|tile| (tile, state.map().wreck_at(tile)))
        .filter(|(_, wreck)| *wreck > 0)
        .collect();
    Hearths { crucibles, wreck }
}

/// The Crucibles of `hearths` that smelt a wreck once workers have stripped
/// `stripped` from it, by the simulation's reach rule and allocation: in id
/// order, each takes one unit of salvage from the nearest wreck, the richer
/// on a tie, so two sharing a last unit do not both smelt it. Exact ties
/// fall to row order here, where the simulation mirrors the order for half
/// the map; that matters only when two Crucibles contest a last unit.
fn smelters(hearths: &Hearths, stripped: &BTreeMap<TilePos, u32>) -> Vec<Key> {
    let radius = CRUCIBLE_SMELT_RADIUS * CRUCIBLE_SMELT_RADIUS;
    let mut taken: BTreeMap<TilePos, u32> = stripped.clone();
    let mut smelting = Vec::new();
    for building in &hearths.crucibles {
        let mut fuel = None;
        for tile in reach_box(building) {
            let left = hearths
                .wreck
                .get(&tile)
                .copied()
                .unwrap_or(0)
                .saturating_sub(taken.get(&tile).copied().unwrap_or(0));
            let center = tile.center();
            let distance = building.closest_point_to(center).dist_sq(center);
            if left == 0 || distance > radius {
                continue;
            }
            let key = (distance, std::cmp::Reverse(left), (tile.y, tile.x));
            if fuel.is_none_or(|best| key < best) {
                fuel = Some(key);
            }
        }
        if let Some((_, _, (y, x))) = fuel {
            *taken.entry(TilePos::new(x, y)).or_default() += 1;
            smelting.push(Key::Building(building.id));
        }
    }
    smelting
}

/// An Extractor that pays on a tick beginning from the state it was read
/// from: the simulation pays before any combat in the tick.
#[derive(Debug, Clone, Copy)]
struct Payout {
    key: Key,
    owner: u8,
    source: IncomeSource,
    amount: u64,
    period: u64,
}

fn payouts(state: &State) -> Vec<Payout> {
    state
        .buildings()
        .iter()
        .filter_map(|building| {
            let income = state.extractor_income(building.id)?;
            let (amount, period) = income.yield_cadence();
            Some(Payout {
                key: Key::Building(building.id),
                owner: building.player.0,
                source: if income.tier() == 0 {
                    IncomeSource::Extractor
                } else {
                    IncomeSource::DeepBore
                },
                amount: u64::from(amount),
                period,
            })
        })
        .collect()
}

fn within(a: TilePos, b: TilePos, radius: i32) -> bool {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    dx * dx + dy * dy <= radius * radius
}

fn tiles(radius: chassis::fx::Fx) -> i32 {
    radius.to_num::<i32>() + 1
}

fn phase(tick: u64) -> usize {
    let minute = tick / MINUTE;
    PHASES.iter().filter(|start| minute >= **start).count()
}

impl ImpactLedger {
    /// A ledger starting from `state`, whose bodies count as the seats'
    /// starting stock.
    pub fn new(state: &State) -> Self {
        let start = state.current_tick();
        let bodies = snapshot(state);
        let mut records = BTreeMap::new();
        for body in &bodies {
            let mut record = record(body, true);
            record.born = start;
            records.insert(body.key, record);
        }
        let seats = state.players().len();
        // Cargo held before the ledger began counts as field scrap.
        let cargo = state
            .units()
            .iter()
            .flat_map(|unit| {
                std::iter::once((unit.id, unit.carrying()))
                    .chain(unit.cargo.iter().map(|rider| (rider.id, rider.carrying)))
            })
            .filter(|(_, carrying)| *carrying > 0)
            .map(|(id, carrying)| {
                (
                    id,
                    Cargo {
                        field: carrying,
                        wreck: 0,
                    },
                )
            })
            .collect();
        let mut ledger = Self {
            start,
            teams: state.players().iter().map(|player| player.team).collect(),
            bodies,
            records,
            unattributed: vec![0; seats],
            spend: vec![BTreeMap::new(); seats],
            refunds: vec![0; seats],
            retired: vec![0; seats],
            worth: vec![Vec::new(); seats],
            shells: Vec::new(),
            hearths: if start.is_multiple_of(CRUCIBLE_SMELT_PERIOD) {
                hearths(state)
            } else {
                Hearths::default()
            },
            payouts: payouts(state),
            credits: Vec::new(),
            hits: Vec::new(),
            cargo,
            working: BTreeMap::new(),
            field_scrap: BTreeMap::new(),
        };
        ledger.note_work(state);
        ledger
    }

    /// Income credited in the last observed tick, by seat and source. A
    /// delivery splits between field harvest and wreck salvage by where its
    /// worker picked each piece up: a piece is field scrap when field scrap
    /// left the tile the worker was working that tick, and workers sharing a
    /// tile take the field scrap that left it first.
    pub fn credits(&self) -> &[Credit] {
        &self.credits
    }

    /// Losses attributed to hostile bodies in the last observed tick. A loss
    /// no hit accounts for is absent.
    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    /// Value of the damage unit `id` has dealt and taken so far, in
    /// thousandths of a scrap.
    pub fn totals(&self, id: UnitId) -> (u64, u64) {
        self.records
            .get(&Key::Unit(id))
            .map_or((0, 0), |record| (record.dealt.total(), record.taken))
    }

    /// Accounts for the tick `report` describes, which left `state`.
    pub fn observe(&mut self, state: &State, report: &TickReport) {
        let now = report.tick;
        self.credits.clear();
        self.hits.clear();
        let after = snapshot(state);
        let sources = self.events(report);
        let stripped = self.load(state, now);
        self.spending(&after, now);
        self.damage(&after, &sources, now);
        self.passive(state, now, &stripped);
        if (now + 1 - self.start).is_multiple_of(WORTH_PERIOD) {
            for (seat, samples) in self.worth.iter_mut().enumerate() {
                samples.push(worth(state, &after, seat));
            }
        }
        self.hearths = if (now + 1).is_multiple_of(CRUCIBLE_SMELT_PERIOD) {
            hearths(state)
        } else {
            Hearths::default()
        };
        self.payouts = payouts(state);
        self.bodies = after;
    }

    /// `key` as a damage source, as it is now; a unit trained this tick,
    /// absent from the snapshot, as its record has it.
    fn source(&self, key: Key, from: Option<TilePos>) -> Option<Source> {
        if let Some(body) = self.body(key) {
            return Some(Source {
                key,
                owner: body.owner,
                team: body.team,
                vision: body.vision,
                from,
            });
        }
        let record = self.records.get(&key)?;
        Some(Source {
            key,
            owner: record.owner,
            team: self.teams[usize::from(record.owner)],
            vision: record.unit?.stats().vision,
            from,
        })
    }

    fn body(&self, key: Key) -> Option<&Body> {
        self.bodies
            .binary_search_by_key(&key, |body| body.key)
            .ok()
            .map(|index| &self.bodies[index])
    }

    /// Records births, deaths, deliveries and repairs, and gathers this
    /// tick's damage sources.
    fn events(&mut self, report: &TickReport) -> Sources {
        let now = report.tick;
        let mut sources = Sources::default();
        for event in &report.events {
            match event {
                Event::UnitTrained {
                    building,
                    unit,
                    kind,
                    player,
                } => {
                    let cost = u64::from(kind.stats().cost);
                    sources.born.push((Key::Unit(*unit), *kind, player.0));
                    self.records.insert(
                        Key::Unit(*unit),
                        Record {
                            owner: player.0,
                            unit: Some(*kind),
                            born: now,
                            paid: cost,
                            ..Record::default()
                        },
                    );
                    if let Some(producer) = self.records.get_mut(&Key::Building(*building)) {
                        producer.produced += cost;
                    }
                    let category = if kind.stats().harvest.is_some() {
                        "worker"
                    } else {
                        "army"
                    };
                    self.spent(player.0, format!("{category}:{}", kind.name()), now, cost);
                }
                Event::UnitDied { unit, pos, .. } => {
                    self.cargo.remove(unit);
                    sources
                        .died
                        .insert(Key::Unit(*unit), TilePos::containing(*pos));
                    self.gone(Key::Unit(*unit), now);
                }
                Event::BuildingDestroyed { building, .. } => {
                    self.gone(Key::Building(*building), now);
                }
                Event::BuildCancelled { player, refund, .. }
                | Event::BuildingSalvaged { player, refund, .. } => {
                    self.refunds[usize::from(player.0)] += u64::from(*refund);
                }
                Event::ScrapDeposited {
                    unit,
                    foundry,
                    player,
                    amount,
                } => {
                    self.delivered(player.0, *unit, *amount);
                    if let Some(record) = self.records.get_mut(&Key::Building(*foundry)) {
                        record.received += u64::from(*amount);
                    }
                }
                // A retirement is no death: the body leaves without `gone`,
                // and only the hp it lost before coming apart is damage
                // taken. The load it carried banks like a delivery.
                Event::UnitRetired {
                    unit,
                    player,
                    crucible,
                    hp,
                    refund,
                    cargo,
                    ..
                } => {
                    sources.retired.insert(Key::Unit(*unit), *hp);
                    self.retired[usize::from(player.0)] += u64::from(*refund);
                    self.delivered(player.0, *unit, *cargo);
                    if let Some(record) = self.records.get_mut(&Key::Building(*crucible)) {
                        record.received += u64::from(*cargo);
                    }
                }
                Event::UnitRepaired {
                    unit,
                    source,
                    amount,
                    ..
                } => {
                    let welder = match source {
                        UnitRepairSource::FieldWelder { unit } => Key::Unit(*unit),
                        UnitRepairSource::RepairBay { building } => Key::Building(*building),
                    };
                    self.repair(Key::Unit(*unit), welder, *amount, &mut sources);
                }
                Event::BuildingRepaired {
                    building,
                    repair_bay,
                    amount,
                    ..
                } => self.repair(
                    Key::Building(*building),
                    Key::Building(*repair_bay),
                    *amount,
                    &mut sources,
                ),
                Event::AttackHit {
                    attacker,
                    attacker_kind,
                    weapon,
                    target,
                    attacker_pos,
                    target_pos,
                } => {
                    let Some(source) = self.source(
                        Key::Unit(*attacker),
                        Some(TilePos::containing(*attacker_pos)),
                    ) else {
                        continue;
                    };
                    let at = TilePos::containing(*target_pos);
                    let stats = attacker_kind.stats();
                    let splash = match stats.demolition {
                        Some(demolition) => Some(demolition.blast_radius),
                        None => stats.weapons.get(*weapon).and_then(|weapon| weapon.splash),
                    };
                    if let Some(radius) = splash {
                        sources.splash.push((at, tiles(radius), source));
                    }
                    aim(&mut sources, *target, at, source);
                }
                Event::TurretFired {
                    turret,
                    kind,
                    tier,
                    target,
                    turret_pos,
                    target_pos,
                } => {
                    if let Some(source) = self.source(
                        Key::Building(*turret),
                        Some(TilePos::containing(*turret_pos)),
                    ) {
                        let at = TilePos::containing(*target_pos);
                        // A building fires its first weapon and nothing else.
                        if let Some(radius) = kind
                            .tiers()
                            .get(usize::from(*tier))
                            .and_then(|stats| stats.weapons.first())
                            .and_then(|weapon| weapon.splash)
                        {
                            sources.splash.push((at, tiles(radius), source));
                        }
                        aim(&mut sources, *target, at, source);
                    }
                }
                Event::ShellLaunched {
                    shooter,
                    player,
                    from,
                    to,
                    flight,
                    ..
                } => {
                    let key = match shooter {
                        Target::Unit(id) => Key::Unit(*id),
                        Target::Building(id) => Key::Building(*id),
                    };
                    if let Some(source) = self.source(key, Some(TilePos::containing(*from))) {
                        self.shells.push(Shell {
                            lands: now + flight,
                            owner: player.0,
                            to: TilePos::containing(*to),
                            source,
                        });
                    }
                }
                Event::ShellLanded {
                    player, at, splash, ..
                } => {
                    let at = TilePos::containing(*at);
                    let landed = |slack: u64| {
                        self.shells.iter().position(|shell| {
                            shell.owner == player.0
                                && shell.to == at
                                && shell.lands.abs_diff(now) <= slack
                        })
                    };
                    if let Some(index) = landed(0).or_else(|| landed(SHELL_SLACK)) {
                        let shell = self.shells.remove(index);
                        let radius = splash.map_or(1, tiles);
                        sources.splash.push((at, radius, shell.source));
                    }
                }
                Event::ShellIntercepted {
                    defender,
                    player,
                    impact,
                    arrival,
                    ..
                } => {
                    let to = TilePos::containing(*impact);
                    if let Some(index) = self.shells.iter().position(|shell| {
                        shell.owner == player.0
                            && shell.to == to
                            && shell.lands.abs_diff(*arrival) <= SHELL_SLACK
                    }) {
                        self.shells.remove(index);
                    }
                    if let Some(record) = self.records.get_mut(&Key::Unit(*defender)) {
                        record.intercepted += 1;
                    }
                }
                Event::AircraftImpacted { crash } => {
                    if let Some(profile) = crash.kind.stats().crash {
                        sources.splash.push((
                            TilePos::containing(crash.impact),
                            tiles(profile.radius),
                            Source {
                                key: Key::Unit(crash.unit),
                                owner: crash.player.0,
                                team: self.teams[usize::from(crash.player.0)],
                                vision: 0,
                                from: None,
                            },
                        ));
                    }
                }
                Event::ChargeDetonated { building, at, .. } => {
                    sources.detonated.push(Key::Building(*building));
                    if let Some(source) = self.source(Key::Building(*building), None) {
                        sources.splash.push((
                            TilePos::containing(*at),
                            tiles(CHARGE_BLAST_RADIUS),
                            source,
                        ));
                    }
                }
                _ => {}
            }
        }
        self.shells.retain(|shell| shell.lands + SHELL_SLACK >= now);
        sources
    }

    /// Splits what each worker picked up in tick `now` between field scrap
    /// and wreck, by how much field scrap left the tile it was working, and
    /// returns the wreck stripped from each tile.
    fn load(&mut self, state: &State, now: u64) -> BTreeMap<TilePos, u32> {
        let mut picked: BTreeMap<Option<TilePos>, Vec<(UnitId, u32)>> = BTreeMap::new();
        for unit in state.units() {
            if unit.kind.stats().harvest.is_none() {
                continue;
            }
            // Only a delivery empties a load, and it settles the cargo.
            let held = self.cargo.get(&unit.id).map_or(0, |cargo| cargo.total());
            if unit.carrying() > held {
                picked
                    .entry(self.working.get(&unit.id).copied())
                    .or_default()
                    .push((unit.id, unit.carrying() - held));
            }
        }
        let mut stripped: BTreeMap<TilePos, u32> = BTreeMap::new();
        for (tile, mut workers) in picked {
            // Workers act in id order on even ticks and in reverse on odd
            // ones, so the first to act takes a tile's last field scrap.
            if now % 2 == 1 {
                workers.reverse();
            }
            // A worker with no tile on record picked up field scrap.
            let mut field = tile.map_or(u32::MAX, |tile| {
                self.field_scrap
                    .get(&tile)
                    .copied()
                    .unwrap_or(0)
                    .saturating_sub(state.map().scrap_at(tile))
            });
            for (id, gained) in workers {
                let from_field = gained.min(field);
                field -= from_field;
                let cargo = self.cargo.entry(id).or_default();
                cargo.field += from_field;
                cargo.wreck += gained - from_field;
                if let Some(tile) = tile {
                    *stripped.entry(tile).or_default() += gained - from_field;
                }
            }
        }
        self.note_work(state);
        stripped
    }

    /// Records the tile each harvesting worker in `state` works and its
    /// field scrap.
    fn note_work(&mut self, state: &State) {
        self.working.clear();
        self.field_scrap.clear();
        for unit in state.units() {
            if let Order::Harvest { node, .. } = unit.order {
                self.working.insert(unit.id, node);
                self.field_scrap.insert(node, state.map().scrap_at(node));
            }
        }
    }

    fn repair(&mut self, patient: Key, welder: Key, amount: u32, sources: &mut Sources) {
        *sources.repaired.entry(patient).or_default() += amount;
        let Some(body) = self.body(patient).copied() else {
            return;
        };
        let value = u64::from(amount) * body.value * MILLI / u64::from(body.max_hp.max(1));
        if let Some(record) = self.records.get_mut(&welder) {
            record.repaired += value;
        }
    }

    /// Credits `amount` of `unit`'s cargo to `seat` as harvest income, split
    /// by how much of the load was wreck.
    fn delivered(&mut self, seat: u8, unit: UnitId, amount: u32) {
        let cargo = self.cargo.remove(&unit).unwrap_or_default();
        let wreck = cargo.wreck.min(amount);
        for (source, scrap) in [
            (IncomeSource::FieldHarvest, amount - wreck),
            (IncomeSource::WreckSalvage, wreck),
        ] {
            if scrap > 0 {
                self.credits.push(Credit {
                    seat,
                    source,
                    scrap: u64::from(scrap),
                });
            }
        }
        if let Some(record) = self.records.get_mut(&Key::Unit(unit)) {
            record.harvested += u64::from(amount);
        }
    }

    fn gone(&mut self, key: Key, now: u64) {
        if let Some(record) = self.records.get_mut(&key) {
            record.gone = Some(now);
        }
    }

    fn spent(&mut self, seat: u8, what: String, now: u64, scrap: u64) {
        self.spend[usize::from(seat)].entry(what).or_default()[phase(now)] += scrap;
    }

    /// Buildings placed and upgraded this tick, and bodies that appeared
    /// without being trained.
    fn spending(&mut self, after: &[Body], now: u64) {
        for body in after {
            let before = self.body(body.key).copied();
            match (before, body.building) {
                (None, _) if !self.records.contains_key(&body.key) => {
                    self.records.insert(body.key, record(body, false));
                    if let Some((kind, tier, _)) = body.building {
                        let record = self.records.get_mut(&body.key).expect("just inserted");
                        record.born = now;
                        record.paid = body.value;
                        record.top_tier = tier;
                        self.spent(
                            body.owner,
                            format!("{}:{}", category(kind), kind.name()),
                            now,
                            body.value,
                        );
                    }
                }
                (Some(before), Some((kind, tier, _))) => {
                    let Some((_, old, _)) = before.building else {
                        continue;
                    };
                    if tier > old {
                        let cost = body.value.saturating_sub(before.value);
                        if let Some(record) = self.records.get_mut(&body.key) {
                            record.paid += cost;
                            record.top_tier = record.top_tier.max(tier);
                        }
                        self.spent(
                            body.owner,
                            format!("upgrade:{}", kind.tier_name(tier)),
                            now,
                            cost,
                        );
                    }
                }
                _ => {}
            }
        }
    }

    /// Splits each body's health lost this tick among the sources that hit
    /// it, including units trained and killed within the tick.
    fn damage(&mut self, after: &[Body], sources: &Sources, now: u64) {
        let mut cursor = 0;
        let before = std::mem::take(&mut self.bodies);
        for pre in &before {
            while cursor < after.len() && after[cursor].key < pre.key {
                cursor += 1;
            }
            // A charge spending itself in its own blast is no one's damage.
            if sources.detonated.contains(&pre.key) {
                continue;
            }
            let gained = sources.repaired.get(&pre.key).copied().unwrap_or(0);
            // A retired machine has no body after the tick; what it lost
            // that tick is the hp between its last body and its retirement.
            if let Some(&hp) = sources.retired.get(&pre.key) {
                self.attribute(
                    &before,
                    pre,
                    (pre.hp + gained).saturating_sub(hp),
                    false,
                    sources,
                );
                continue;
            }
            let lost = match after.get(cursor) {
                // An upgrade rebuilds the building; what it loses to that is
                // no one's damage.
                Some(post) if post.key == pre.key && post.tier() != pre.tier() => continue,
                Some(post) if post.key == pre.key => {
                    // Construction and salvage move health without events.
                    let Some(built) = pre.built_gain(post) else {
                        continue;
                    };
                    let drained = post.salvage.saturating_sub(pre.salvage);
                    (pre.hp + gained + built).saturating_sub(post.hp + drained)
                }
                _ if self
                    .records
                    .get(&pre.key)
                    .is_some_and(|record| record.gone == Some(now)) =>
                {
                    pre.hp + gained
                }
                _ => continue,
            };
            let destroyed = !matches!(after.get(cursor), Some(post) if post.key == pre.key);
            self.attribute(&before, pre, lost, destroyed, sources);
        }
        // A unit trained this tick starts at full health, whether it lived
        // through the tick or died in it.
        for &(key, kind, owner) in &sources.born {
            if find(&before, key).is_some() {
                continue;
            }
            let stats = kind.stats();
            let (tile, hp, destroyed) = match (find(after, key), sources.died.get(&key)) {
                (Some(post), _) => (post.tile, post.hp, false),
                (None, Some(&tile)) => (tile, 0, true),
                (None, None) => continue,
            };
            let body = Body {
                key,
                owner,
                team: self.teams[usize::from(owner)],
                hp: stats.max_hp,
                max_hp: stats.max_hp,
                value: u64::from(stats.cost),
                tile,
                vision: stats.vision,
                class: class(kind),
                unit: Some(kind),
                building: None,
                carrier: None,
                progress: 0,
                build_ticks: 0,
                salvage: 0,
            };
            self.attribute(&before, &body, stats.max_hp - hp, destroyed, sources);
        }
        self.bodies = before;
    }

    /// Credits `lost` health of `pre`, which ended the tick destroyed when
    /// `destroyed`, to the sources that hit it this tick.
    fn attribute(
        &mut self,
        before: &[Body],
        pre: &Body,
        lost: u32,
        destroyed: bool,
        sources: &Sources,
    ) {
        if lost == 0 {
            return;
        }
        let value = u64::from(lost) * pre.value * MILLI / u64::from(pre.max_hp.max(1));
        let mut from: Vec<Source> = sources.aimed.get(&pre.key).cloned().unwrap_or_default();
        // A rider dies to whatever brought its carrier down.
        if let Some(carrier) = pre.carrier {
            from.extend(sources.aimed.get(&carrier).into_iter().flatten().copied());
        }
        from.extend(
            sources
                .splash
                .iter()
                .filter(|(at, radius, _)| within(*at, pre.tile, *radius))
                .map(|(_, _, source)| *source),
        );
        from.extend(
            sources
                .blind
                .iter()
                .filter(|(at, _)| at.chebyshev(pre.tile) <= 1)
                .map(|(_, source)| *source),
        );
        from.retain(|source| source.team != pre.team);
        // A source counts once per victim per tick: a Sapper's blast reports
        // its target both as aimed and inside its splash.
        from.sort_by_key(|source| source.key);
        from.dedup_by_key(|source| source.key);
        if from.is_empty() {
            // An unfinished site that loses health with no one firing is
            // decaying, not under attack.
            if !matches!(pre.building, Some((_, _, false))) {
                self.unattributed[usize::from(pre.owner)] += value;
                if let Some(record) = self.records.get_mut(&pre.key) {
                    record.taken += value;
                }
            }
            return;
        }
        if let Some(record) = self.records.get_mut(&pre.key) {
            record.taken += value;
        }
        self.hits.push(Hit {
            victim: target(pre.key),
            sources: from
                .iter()
                .map(|source| (target(source.key), source.team))
                .collect(),
        });
        let share = value / from.len() as u64;
        for source in &from {
            let place = place(before, source.owner, pre.owner, pre.tile);
            if let Some(record) = self.records.get_mut(&source.key) {
                let dealt = &mut record.dealt;
                *match pre.class {
                    Class::Army => &mut dealt.army,
                    Class::Worker => &mut dealt.workers,
                    Class::Other => &mut dealt.other,
                    Class::Building => &mut dealt.buildings,
                } += share;
                *match place {
                    Place::Home => &mut dealt.home,
                    Place::Away => &mut dealt.away,
                    Place::Field => &mut dealt.field,
                } += share;
            }
            if let Some(origin) = source.from
                && !within(origin, pre.tile, source.vision)
                && let Some(spotter) = before
                    .iter()
                    .filter(|body| {
                        body.team == source.team
                            && body.key != source.key
                            && within(body.tile, pre.tile, body.vision)
                    })
                    .min_by_key(|body| {
                        let (dx, dy) = (body.tile.x - pre.tile.x, body.tile.y - pre.tile.y);
                        (dx * dx + dy * dy, body.key)
                    })
                && let Some(record) = self.records.get_mut(&spotter.key)
            {
                record.enabled += share;
            }
        }
        let charge = matches!(pre.building, Some((BuildingKind::ScuttleCharge, _, _)));
        if charge && destroyed {
            let team = from[0].team;
            let array = before
                .iter()
                .filter(|body| body.team == team)
                .filter(|body| match body.building {
                    Some((BuildingKind::Array, tier, true)) => within(
                        body.tile,
                        pre.tile,
                        if tier >= 1 {
                            CHARGE_ARRAY_DETECT_RADIUS
                        } else {
                            CHARGE_BASE_ARRAY_DETECT_RADIUS
                        },
                    ),
                    _ => false,
                })
                .min_by_key(|body| body.key);
            if let Some(array) = array
                && let Some(record) = self.records.get_mut(&array.key)
            {
                record.enabled += pre.value * MILLI;
            }
        }
    }

    /// Credits the income the simulation pays without an event, by the same
    /// rules and cadence: Reclaimers, Extractors and the Foundry drip as
    /// buildings stood when the tick began, before any combat in it, and
    /// Crucibles with wreck still in reach once workers had stripped
    /// `stripped` from it.
    fn passive(&mut self, state: &State, now: u64, stripped: &BTreeMap<TilePos, u32>) {
        let completed = now + 1;
        let mut credits: Vec<(Key, IncomeSource, u64)> = Vec::new();
        for body in &self.bodies {
            let Some((kind, tier, true)) = body.building else {
                continue;
            };
            if body.hp == 0 {
                continue;
            }
            // Commands run before the tick's income, so an upgrade one
            // started keeps the building out of service all tick.
            if let Key::Building(id) = body.key
                && state.building(id).is_some_and(|building| !building.built())
            {
                continue;
            }
            let resigned = state.players()[usize::from(body.owner)].resigned;
            let credit = match kind {
                BuildingKind::Reclaimer if tier == 0 => now
                    .is_multiple_of(RECLAIMER_PERIOD)
                    .then_some(IncomeSource::Reclaimer),
                BuildingKind::Reclaimer if tier == 1 => now
                    .is_multiple_of(REFINERY_PERIOD)
                    .then_some(IncomeSource::Refinery),
                BuildingKind::Foundry
                    if !resigned
                        && completed >= FOUNDRY_DRIP_START_TICK
                        && completed.is_multiple_of(FOUNDRY_DRIP_PERIOD) =>
                {
                    Some(IncomeSource::FoundryDrip)
                }
                _ => None,
            };
            if let Some(source) = credit {
                credits.push((body.key, source, 1));
            }
        }
        // A resignation is a command, applied before the tick's income, and
        // so is an upgrade that takes an Extractor out of service.
        credits.extend(
            self.payouts
                .iter()
                .filter(|payout| {
                    completed.is_multiple_of(payout.period)
                        && !state.players()[usize::from(payout.owner)].resigned
                        && !matches!(payout.key, Key::Building(id)
                            if state.building(id).is_some_and(|building| !building.built()))
                })
                .map(|payout| (payout.key, payout.source, payout.amount)),
        );
        if now.is_multiple_of(CRUCIBLE_SMELT_PERIOD) {
            credits.extend(
                smelters(&self.hearths, stripped)
                    .into_iter()
                    .map(|key| (key, IncomeSource::Smelting, 1)),
            );
        }
        for (key, source, scrap) in credits {
            if let Some(record) = self.records.get_mut(&key) {
                record.passive += scrap;
                self.credits.push(Credit {
                    seat: record.owner,
                    source,
                    scrap,
                });
            }
        }
    }

    /// Each seat's ledger, ending with `state`, the state the last observed
    /// tick left.
    pub fn finish(self, state: &State) -> Vec<SeatLedger> {
        let end = state.current_tick();
        let mut seats: Vec<SeatLedger> = (0..self.spend.len())
            .map(|seat| SeatLedger {
                unattributed: scrap(self.unattributed[seat]),
                spend: self.spend[seat].clone(),
                refunds: self.refunds[seat],
                retired: self.retired[seat],
                worth: self.worth[seat].clone(),
                start: self.start,
                end,
                final_worth: worth(state, &self.bodies, seat),
                ..SeatLedger::default()
            })
            .collect();
        for record in self.records.values() {
            let Some(seat) = seats.get_mut(usize::from(record.owner)) else {
                continue;
            };
            let (table, name) = match (record.unit, record.building) {
                (Some(kind), _) => (&mut seat.units, kind.name()),
                (None, Some(kind)) => (&mut seat.buildings, kind.tier_name(record.top_tier)),
                (None, None) => continue,
            };
            let entry = table.entry(name.to_owned()).or_default();
            if record.starting {
                entry.starting += 1;
            } else {
                entry.built += 1;
            }
            entry.paid += record.paid;
            entry.dealt.add(&record.dealt);
            entry.taken += record.taken;
            entry.enabled += record.enabled;
            entry.repaired += record.repaired;
            entry.harvested += record.harvested;
            entry.received += record.received;
            entry.passive += record.passive;
            entry.produced += record.produced;
            entry.intercepted += record.intercepted;
            if let Some(gone) = record.gone {
                entry.deaths += 1;
                entry.lifetime += gone.min(end).saturating_sub(record.born);
            }
        }
        for seat in &mut seats {
            for totals in seat.units.values_mut().chain(seat.buildings.values_mut()) {
                let dealt = &mut totals.dealt;
                for value in [
                    &mut dealt.army,
                    &mut dealt.workers,
                    &mut dealt.other,
                    &mut dealt.buildings,
                    &mut dealt.home,
                    &mut dealt.away,
                    &mut dealt.field,
                    &mut totals.taken,
                    &mut totals.enabled,
                    &mut totals.repaired,
                ] {
                    *value = scrap(*value);
                }
            }
        }
        seats
    }
}

/// Thousandths of a scrap rounded to whole scrap.
pub fn scrap(milli: u64) -> u64 {
    (milli + MILLI / 2) / MILLI
}

/// Where a fight happened, from one side's view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    /// Near that side's own buildings.
    Home,
    /// Near the other side's buildings.
    Away,
    /// Near neither.
    Field,
}

impl Place {
    /// The place of a fight `own` tiles (Chebyshev) from the nearest
    /// building of one side that [`Place::marks`] and `theirs` from the
    /// other side's: home wins a tie.
    pub fn from_distances(own: i32, theirs: i32) -> Place {
        if own <= PLACE_TILES && own <= theirs {
            Place::Home
        } else if theirs <= PLACE_TILES {
            Place::Away
        } else {
            Place::Field
        }
    }

    /// Whether a building of `kind` marks its owner's ground: mines and
    /// Barricades are scattered across the map and do not.
    pub fn marks(kind: BuildingKind) -> bool {
        !matches!(kind, BuildingKind::ScuttleCharge | BuildingKind::Barricade)
    }
}

/// Where a loss to `victim`'s body at `tile` happened, from `dealer`'s view.
fn place(bodies: &[Body], dealer: u8, victim: u8, tile: TilePos) -> Place {
    let near = |seat: u8| {
        bodies
            .iter()
            .filter(|body| {
                body.owner == seat && body.building.is_some_and(|(kind, _, _)| Place::marks(kind))
            })
            .map(|body| body.tile.chebyshev(tile))
            .min()
            .unwrap_or(i32::MAX)
    };
    Place::from_distances(near(dealer), near(victim))
}

/// The spending category of a building kind.
fn category(kind: BuildingKind) -> &'static str {
    match kind {
        BuildingKind::Foundry | BuildingKind::Extractor | BuildingKind::Reclaimer => "economy",
        BuildingKind::Turret
        | BuildingKind::FlakTurret
        | BuildingKind::Bastion
        | BuildingKind::Barricade
        | BuildingKind::ScuttleCharge => "defense",
        _ => "tech",
    }
}

fn target(key: Key) -> Target {
    match key {
        Key::Unit(id) => Target::Unit(id),
        Key::Building(id) => Target::Building(id),
    }
}

fn find(bodies: &[Body], key: Key) -> Option<&Body> {
    bodies
        .binary_search_by_key(&key, |body| body.key)
        .ok()
        .map(|index| &bodies[index])
}

fn aim(sources: &mut Sources, target: Option<Target>, at: TilePos, source: Source) {
    match target {
        Some(Target::Unit(id)) => sources.aimed.entry(Key::Unit(id)).or_default().push(source),
        Some(Target::Building(id)) => sources
            .aimed
            .entry(Key::Building(id))
            .or_default()
            .push(source),
        None => sources.blind.push((at, source)),
    }
}

fn record(body: &Body, starting: bool) -> Record {
    Record {
        owner: body.owner,
        unit: body.unit,
        building: body.building.map(|(kind, _, _)| kind),
        top_tier: body.building.map_or(0, |(_, tier, _)| tier),
        starting,
        paid: if starting { body.value } else { 0 },
        ..Record::default()
    }
}

/// Phase names for spending, by the minute each begins.
const PHASE_NAMES: [&str; 4] = ["0-5", "5-10", "10-20", "20+"];

/// Ledgers of several seats, pooled.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LedgerPool {
    /// Seat ledgers pooled.
    pub seats: u64,
    /// Totals by unit kind.
    pub units: BTreeMap<String, KindLedger>,
    /// Totals by building kind at its highest tier.
    pub buildings: BTreeMap<String, KindLedger>,
    /// Damage the seats took that no hit accounts for, in scrap.
    pub unattributed: u64,
    /// Scrap spent by `<category>:<kind>` and phase.
    pub spend: BTreeMap<String, [u64; 4]>,
}

impl LedgerPool {
    /// Adds one seat's ledger.
    pub fn add(&mut self, ledger: &SeatLedger) {
        self.seats += 1;
        for (kind, totals) in &ledger.units {
            self.units.entry(kind.clone()).or_default().add(totals);
        }
        for (kind, totals) in &ledger.buildings {
            self.buildings.entry(kind.clone()).or_default().add(totals);
        }
        self.unattributed += ledger.unattributed;
        for (what, phases) in &ledger.spend {
            let entry = self.spend.entry(what.clone()).or_default();
            for (sum, scrap) in entry.iter_mut().zip(phases) {
                *sum += scrap;
            }
        }
    }

    /// Scrap earned by source: worker deliveries, then each building kind's
    /// passive credits, most first.
    pub fn income(&self) -> Vec<(String, u64)> {
        let harvest: u64 = self.units.values().map(|totals| totals.harvested).sum();
        let mut income: Vec<(String, u64)> = vec![("harvest".to_owned(), harvest)];
        income.extend(
            self.buildings
                .iter()
                .filter(|(_, totals)| totals.passive > 0)
                .map(|(kind, totals)| (kind.clone(), totals.passive)),
        );
        income.sort_by_key(|(name, scrap)| (std::cmp::Reverse(*scrap), name.clone()));
        income
    }

    /// Renders unit and building tables, the first `limit` kinds of each by
    /// scrap paid, then income sources, spending by phase and the
    /// unattributed share, each line led by `indent`.
    pub fn render(&self, out: &mut String, indent: &str, limit: Option<usize>) {
        let limit = limit.unwrap_or(usize::MAX);
        let unit_spend: u64 = self.units.values().map(|totals| totals.paid).sum();
        let _ = writeln!(
            out,
            "{indent}{:<12} {:>6} {:>6} {:>6} | {:>5} {:>5} {:>5} {:>5} | {:>5} {:>5} {:>5} | {:>7} {:>8} {:>5} {:>6} {:>7}",
            "unit",
            "built",
            "spend",
            "dealt",
            "army",
            "wrkr",
            "other",
            "bldg",
            "home",
            "field",
            "away",
            "enabled",
            "repaired",
            "died",
            "life s",
            "stopped"
        );
        let mut units: Vec<(&String, &KindLedger)> = self.units.iter().collect();
        units.sort_by_key(|(kind, totals)| (std::cmp::Reverse(totals.paid), (*kind).clone()));
        for (kind, totals) in units.into_iter().take(limit) {
            let dealt = totals.dealt.total();
            let _ = writeln!(
                out,
                "{indent}{:<12} {:>6} {:>6} {:>6} | {:>5} {:>5} {:>5} {:>5} | {:>5} {:>5} {:>5} | {:>7} {:>8} {:>5} {:>6} {:>7}",
                kind,
                totals.built + totals.starting,
                share(totals.paid, unit_spend),
                ratio(dealt, totals.paid),
                share(totals.dealt.army, dealt),
                share(totals.dealt.workers, dealt),
                share(totals.dealt.other, dealt),
                share(totals.dealt.buildings, dealt),
                share(totals.dealt.home, dealt),
                share(totals.dealt.field, dealt),
                share(totals.dealt.away, dealt),
                ratio(totals.enabled, totals.paid),
                ratio(totals.repaired, totals.paid),
                share(totals.deaths, totals.built + totals.starting),
                life(totals),
                totals.intercepted,
            );
        }
        let _ = writeln!(
            out,
            "{indent}{:<14} {:>6} {:>8} {:>6} {:>7} {:>8} {:>8} {:>8} {:>9}",
            "building",
            "built",
            "paid",
            "dealt",
            "income",
            "produced",
            "enabled",
            "repaired",
            "destroyed"
        );
        let mut buildings: Vec<(&String, &KindLedger)> = self.buildings.iter().collect();
        buildings.sort_by_key(|(kind, totals)| (std::cmp::Reverse(totals.paid), (*kind).clone()));
        for (kind, totals) in buildings.into_iter().take(limit) {
            let _ = writeln!(
                out,
                "{indent}{:<14} {:>6} {:>8} {:>6} {:>7} {:>8} {:>8} {:>8} {:>9}",
                kind,
                totals.built + totals.starting,
                totals.paid,
                ratio(totals.dealt.total(), totals.paid),
                ratio(totals.passive + totals.received, totals.paid),
                ratio(totals.produced, totals.paid),
                ratio(totals.enabled, totals.paid),
                ratio(totals.repaired, totals.paid),
                share(totals.deaths, totals.built + totals.starting),
            );
        }
        let income = self.income();
        let earned: u64 = income.iter().map(|(_, scrap)| scrap).sum();
        let _ = writeln!(
            out,
            "{indent}income {:.0} per seat-game: {}",
            earned as f64 / self.seats.max(1) as f64,
            income
                .iter()
                .filter(|(_, scrap)| *scrap > 0)
                .map(|(name, scrap)| format!("{name} {}", share(*scrap, earned)))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let mut categories: BTreeMap<&str, [u64; 4]> = BTreeMap::new();
        for (what, phases) in &self.spend {
            let category = what.split(':').next().unwrap_or(what);
            let entry = categories.entry(category).or_default();
            for (sum, scrap) in entry.iter_mut().zip(phases) {
                *sum += scrap;
            }
        }
        for (index, phase) in PHASE_NAMES.iter().enumerate() {
            let total: u64 = categories.values().map(|phases| phases[index]).sum();
            if total == 0 {
                continue;
            }
            let mut parts: Vec<(&str, u64)> = categories
                .iter()
                .map(|(category, phases)| (*category, phases[index]))
                .filter(|(_, scrap)| *scrap > 0)
                .collect();
            parts.sort_by_key(|(category, scrap)| (std::cmp::Reverse(*scrap), *category));
            let _ = writeln!(
                out,
                "{indent}spend, minutes {phase:<5}: {}",
                parts
                    .iter()
                    .map(|(category, scrap)| format!("{category} {}", share(*scrap, total)))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        let taken: u64 = self
            .units
            .values()
            .chain(self.buildings.values())
            .map(|totals| totals.taken)
            .sum();
        let _ = writeln!(
            out,
            "{indent}damage taken with no shooter found: {}",
            share(self.unattributed, taken)
        );
    }
}

/// A seat's net worth at `tick`: its final worth once the ledger has ended,
/// else the last sample at or before it; `None` before the first. A row with
/// no recorded end carries its last sample forward instead.
pub fn worth_at(ledger: &SeatLedger, tick: u64) -> Option<u64> {
    if ledger.end > 0 && tick >= ledger.end {
        return Some(ledger.final_worth);
    }
    let samples = usize::try_from(tick.checked_sub(ledger.start)? / WORTH_PERIOD).ok()?;
    if samples == 0 {
        return None;
    }
    let worth = &ledger.worth;
    worth.get(samples - 1).or_else(|| worth.last()).copied()
}

fn ratio(numerator: u64, denominator: u64) -> String {
    if denominator == 0 {
        "-".to_owned()
    } else {
        format!("{:.2}", numerator as f64 / denominator as f64)
    }
}

/// `part` as a whole percentage of `whole`, or `-` when `whole` is zero.
pub fn share(part: u64, whole: u64) -> String {
    if whole == 0 {
        "-".to_owned()
    } else {
        format!("{:.0}%", part as f64 * 100.0 / whole as f64)
    }
}

fn life(totals: &KindLedger) -> String {
    if totals.deaths == 0 {
        "-".to_owned()
    } else {
        format!(
            "{:.0}",
            totals.lifetime as f64 / totals.deaths as f64 / f64::from(oxide_sim::TICKS_PER_SECOND)
        )
    }
}

/// Ticks at which reports compare seats' net worth.
pub const WORTH_TICKS: [u64; 4] = [6_000, 12_000, 18_000, 24_000];

/// Per mille of all net worth that `team` held at each of [`WORTH_TICKS`],
/// from each seat's ledger and team; `None` unless every seat has a ledger.
pub fn team_shares(
    ledgers: &[Option<&SeatLedger>],
    teams: &[u8],
    team: u8,
) -> Option<[Option<u32>; 4]> {
    let ledgers: Vec<&SeatLedger> = ledgers.iter().copied().collect::<Option<_>>()?;
    Some(WORTH_TICKS.map(|tick| {
        let (mut own, mut all) = (0_u64, 0_u64);
        for (ledger, seat_team) in ledgers.iter().zip(teams) {
            let worth = worth_at(ledger, tick)?;
            all += worth;
            if *seat_team == team {
                own += worth;
            }
        }
        (all > 0).then(|| u32::try_from(own * 1_000 / all).unwrap_or(1_000))
    }))
}

/// A side's mean share of net worth at one tick over seat-swapped pairs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorthShare {
    /// Tick compared.
    pub tick: u64,
    /// Pairs with a share at that tick.
    pub pairs: u32,
    /// Mean share, 0 to 1.
    pub mean: f64,
    /// 95% interval of the mean over pairs, absent with fewer than two.
    pub interval: Option<[f64; 2]>,
}

/// Each pair's legs' shares at [`WORTH_TICKS`], from [`team_shares`], keyed
/// by pair.
pub type PairShares = BTreeMap<String, Vec<[Option<u32>; 4]>>;

/// Mean shares at each of [`WORTH_TICKS`] over pairs: each pair's legs are
/// averaged first, so a pair counts once.
pub fn worth_shares(pairs: &PairShares) -> Vec<WorthShare> {
    WORTH_TICKS
        .iter()
        .enumerate()
        .filter_map(|(index, tick)| {
            let means: Vec<f64> = pairs
                .values()
                .filter_map(|legs| {
                    let shares: Vec<f64> = legs
                        .iter()
                        .filter_map(|leg| leg[index])
                        .map(|share| f64::from(share) / 1_000.0)
                        .collect();
                    (!shares.is_empty()).then(|| shares.iter().sum::<f64>() / shares.len() as f64)
                })
                .collect();
            if means.is_empty() {
                return None;
            }
            let n = means.len() as f64;
            let mean = means.iter().sum::<f64>() / n;
            let interval = (means.len() >= 2).then(|| {
                let variance = means
                    .iter()
                    .map(|value| (value - mean).powi(2))
                    .sum::<f64>()
                    / (n - 1.0);
                let half = 1.96 * (variance / n).sqrt();
                [mean - half, mean + half]
            });
            Some(WorthShare {
                tick: *tick,
                pairs: u32::try_from(means.len()).expect("lengths fit in u32"),
                mean,
                interval,
            })
        })
        .collect()
}

/// The shares as one line: `6k 52% [48-56%]`, and so on.
pub fn render_shares(shares: &[WorthShare]) -> String {
    shares
        .iter()
        .map(|share| {
            let interval = share.interval.map_or_else(String::new, |[low, high]| {
                format!(" [{:.0}-{:.0}%]", low * 100.0, high * 100.0)
            });
            format!(
                "{}k {:.0}%{interval}",
                share.tick / 1_000,
                share.mean * 100.0
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests;
