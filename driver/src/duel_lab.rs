//! The duel lab: unit kinds fight each other at equal scrap, so balance
//! questions (does every unit have a counter, is anything dominant) get an
//! answer without a bot's composition choices in the way.
//!
//! Every duel is a staged sandbox scenario with no controller: two armies on a
//! point-symmetric field, mirrored through its center, both hunting toward the
//! other's start. Each pairing runs on every layout and formation and in both
//! seat assignments, so a seat or geometry bias shows up as legs that disagree
//! instead of hiding in an average. The numbers are a report, never a pinned
//! test: balance is meant to move.

use aftermine_sim::scenario::{BuildingSpec, PlayerSpec, ScenarioMode, UnitSpec};
use aftermine_sim::stats::{BuildingStats, Domain};
use aftermine_sim::{
    AttackTarget, BuildingKind, BuildingPhase, Command, Order, PlayerCommand, PlayerId, Scenario,
    State, UnitKind,
};
use anyhow::{Context, Result};
use chassis::grid::{TilePos, as_index};
use serde::Serialize;
use std::fmt::Write as _;
use std::num::NonZeroUsize;

const WIDTH: i32 = 56;
const HEIGHT: i32 = 28;
/// The west army's front column; ranks deepen toward the west edge. The east
/// front mirrors it, 27 tiles away: beyond every weapon's reach and sight.
const FRONT_X: i32 = 14;
/// Rows an army may occupy, inside the rock border.
const FIRST_ROW: i32 = 3;
const LAST_ROW: i32 = HEIGHT - 4;
/// Where an artillery side's spotter starts: just short of the center.
const SPOTTER_X: i32 = WIDTH / 2 - 3;
/// How far short of the nearest enemy a spotter hovers: its 10-tile sight
/// reaches past the target while it stays outside short anti-air reach.
const SPOTTER_STANDOFF: i32 = 6;
/// A matchup score at or beyond this margin counts as a win or a loss when
/// flagging units that win or lose everything.
const DECISIVE: f64 = 0.1;
/// Two kinds whose results against everyone else point the same way at
/// least this closely, and who are near even with each other, play alike.
const ALIKE_SIMILARITY: f64 = 0.95;
const ALIKE_HEAD_TO_HEAD: f64 = 0.25;
/// The same pairing's two seat assignments disagreeing by more than this
/// points at a seat or geometry bias rather than the units.
const SPLIT_LEGS: f64 = 0.5;
/// How often idle armies go looking for the fight again. A hunt ends at its
/// goal, so armies that pass each other (aircraft over ground units) or reach
/// a defense line with targets beyond their reach would otherwise stand idle
/// for the rest of the duel.
const SEEK_PERIOD: u32 = 40;
/// Ticks allowed for refitting a defense line to its tier before the fight:
/// the Bulwark's two upgrades take 800 between them.
const REFIT_LIMIT: u32 = 2_000;

/// The ground the armies meet on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// Open ground between the armies.
    Open,
    /// A rock wall across the middle with a four-tile gap.
    Choke,
    /// Scattered rock blocks across the middle band.
    Cover,
}

impl Layout {
    /// Every layout, in report order.
    pub const ALL: [Self; 3] = [Self::Open, Self::Choke, Self::Cover];

    fn map(self) -> Vec<String> {
        let mut grid = vec![vec!['.'; as_index(WIDTH)]; as_index(HEIGHT)];
        let mut rock = |x: i32, y: i32| {
            grid[as_index(y)][as_index(x)] = '#';
            grid[as_index(HEIGHT - 1 - y)][as_index(WIDTH - 1 - x)] = '#';
        };
        for x in 0..WIDTH {
            rock(x, 0);
        }
        for y in 0..HEIGHT {
            rock(0, y);
        }
        match self {
            Self::Open => {}
            Self::Choke => {
                for y in 1..HEIGHT / 2 - 2 {
                    rock(WIDTH / 2 - 1, y);
                    rock(WIDTH / 2, y);
                }
            }
            Self::Cover => {
                for (x, y) in [(20, 5), (23, 11), (20, 17), (24, 22), (26, 2)] {
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        rock(x + dx, y + dy);
                    }
                }
            }
        }
        grid.into_iter()
            .map(|row| row.into_iter().collect())
            .collect()
    }
}

/// How an army stands at the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Formation {
    /// Spread along the front, filling columns from the front rank back.
    Line,
    /// A square block on the center row.
    Clump,
}

impl Formation {
    /// Every formation, in report order.
    pub const ALL: [Self; 2] = [Self::Line, Self::Clump];

    /// West-side tiles for `count` bodies, front rank first.
    fn tiles(self, count: u32) -> Vec<TilePos> {
        let count = i32::try_from(count).expect("army sizes fit in i32");
        let per_column = match self {
            Self::Line => {
                let rows = LAST_ROW - FIRST_ROW + 1;
                let columns = ((count + rows - 1) / rows).max(1);
                (count + columns - 1) / columns
            }
            Self::Clump => {
                let root = count.isqrt();
                if root * root < count { root + 1 } else { root }
            }
        }
        .max(1);
        let top = (HEIGHT - per_column) / 2;
        (0..count)
            .map(|i| TilePos::new(FRONT_X - i / per_column, top + i % per_column))
            .collect()
    }
}

/// A defense at an upgrade tier. An upgraded line is staged the way a player
/// gets one: placed at tier zero beside the buildings its upgrades require,
/// then refitted by ordinary upgrade commands before the fight starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Defense {
    /// The building kind.
    pub kind: BuildingKind,
    /// Its upgrade tier.
    pub tier: u8,
}

impl Defense {
    /// A base-tier defense.
    pub const fn base(kind: BuildingKind) -> Self {
        Self { kind, tier: 0 }
    }

    fn stats(self) -> &'static BuildingStats {
        self.kind.tier_stats(self.tier)
    }

    /// Scrap through this tier: the build plus every upgrade.
    fn cost(self) -> u32 {
        (0..=self.tier)
            .filter_map(|tier| self.kind.tier_stats(tier).construction)
            .map(|c| c.cost)
            .sum()
    }

    /// Buildings the upgrades to this tier require.
    fn prerequisites(self) -> Vec<BuildingKind> {
        let mut kinds: Vec<BuildingKind> = (1..=self.tier)
            .filter_map(|tier| self.kind.tier_stats(tier).construction)
            .flat_map(|c| c.requires.iter().map(|need| need.kind))
            .collect();
        kinds.sort_by_key(|kind| BuildingKind::ALL.iter().position(|k| k == kind));
        kinds.dedup();
        kinds
    }
}

/// One side of a duel: a unit kind, or a defense standing in a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Contender {
    /// An army of one unit kind, hunting toward the other side.
    Unit(UnitKind),
    /// A line of one defense, standing where it was built.
    Defense(Defense),
}

impl Contender {
    /// Scrap one body costs.
    pub fn cost(self) -> u32 {
        match self {
            Self::Unit(kind) => kind.stats().cost,
            Self::Defense(defense) => defense.cost(),
        }
    }

    fn domain(self) -> Domain {
        match self {
            Self::Unit(kind) => kind.stats().domain,
            Self::Defense(_) => Domain::Ground,
        }
    }

    /// Whether this side can hurt the other at all.
    pub fn threatens(self, other: Self) -> bool {
        let domain = other.domain();
        match self {
            Self::Unit(kind) => {
                kind.stats().can_target(domain)
                    || (matches!(other, Self::Defense(_)) && kind.stats().demolition.is_some())
            }
            Self::Defense(defense) => defense
                .stats()
                .weapons
                .iter()
                .any(|weapon| weapon.targets.covers(domain)),
        }
    }

    /// A side whose reach outruns its own sight gets a spotter, or its guns
    /// would wait for targets to walk inside its eyes.
    fn needs_spotter(self) -> bool {
        let (vision, reach) = match self {
            Self::Unit(kind) => {
                let stats = kind.stats();
                (stats.vision, stats.weapons.iter().map(|w| w.range).max())
            }
            Self::Defense(defense) => {
                let stats = defense.stats();
                (stats.vision, stats.weapons.iter().map(|w| w.range).max())
            }
        };
        reach.is_some_and(|reach| reach > chassis::fx::Fx::from_num(vision))
    }

    /// The short label the matrix prints.
    pub fn label(self) -> String {
        self.name().chars().filter(|c| *c != ' ').take(5).collect()
    }

    fn name(self) -> &'static str {
        match self {
            Self::Unit(kind) => kind.name(),
            Self::Defense(defense) => defense.kind.tier_name(defense.tier),
        }
    }
}

/// Whether `kind` can be staged on its own: a drone exists only with the
/// host it flies for.
fn stands_alone(kind: UnitKind) -> bool {
    !kind.stats().docks()
}

/// Every unit kind that carries a gun: the duel roster.
pub fn armed_units() -> Vec<UnitKind> {
    UnitKind::ALL
        .iter()
        .copied()
        .filter(|kind| !kind.stats().weapons.is_empty() && stands_alone(*kind))
        .collect()
}

/// Every unarmed kind that supports an army, such as a point defender: it
/// joins mixes in the composition search and never fights alone.
pub fn support_units() -> Vec<UnitKind> {
    UnitKind::ALL
        .iter()
        .copied()
        .filter(|kind| !kind.stats().can_fight() && kind.stats().point_defense.is_some())
        .collect()
}

/// Every unit kind that can attack a defense, Sappers included.
pub fn defense_attackers() -> Vec<UnitKind> {
    UnitKind::ALL
        .iter()
        .copied()
        .filter(|kind| kind.stats().can_fight() && stands_alone(*kind))
        .collect()
}

/// Every buildable defense tier that fires on its own.
pub fn armed_defenses() -> Vec<Defense> {
    BuildingKind::ALL
        .iter()
        .copied()
        .filter(|kind| kind.base_stats().construction.is_some())
        .flat_map(|kind| {
            (0..kind.tiers().len())
                .map(move |tier| Defense {
                    kind,
                    tier: u8::try_from(tier).expect("upgrade ladders are short"),
                })
                .filter(|defense| !defense.stats().weapons.is_empty())
        })
        .collect()
}

/// How many bodies one side fields: the whole number nearest the budget, at
/// least one. The report keeps what was actually spent, since an uneven
/// budget can explain a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Side {
    /// What this side fields.
    pub contender: Contender,
    /// How many.
    pub count: u32,
    /// Scrap actually spent.
    pub spent: u32,
}

impl Side {
    /// Size a pairing: the costlier side rounds to the budget first, then the
    /// cheaper side matches what it actually spent, which keeps coarse kinds
    /// (three Condors are 2,100 scrap) from fighting a 17% smaller army.
    pub fn matched(a: Contender, b: Contender, budget: u32) -> (Self, Self) {
        if a.cost() >= b.cost() {
            let a = Self::sized(a, budget);
            (a, Self::sized(b, a.spent))
        } else {
            let b = Self::sized(b, budget);
            (Self::sized(a, b.spent), b)
        }
    }

    /// Size a side to a budget.
    pub fn sized(contender: Contender, budget: u32) -> Self {
        let cost = contender.cost().max(1);
        let count = ((budget + cost / 2) / cost).max(1);
        Self {
            contender,
            count,
            spent: count * cost,
        }
    }
}

/// One leg: a pairing on one layout and formation with one seat assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Duel {
    /// The side the matrix scores.
    pub a: Side,
    /// Its opponent.
    pub b: Side,
    /// The ground.
    pub layout: Layout,
    /// How both armies stand at the start.
    pub formation: Formation,
    /// Whether side `a` takes the west seat.
    pub a_west: bool,
    /// Whether an army that needs a spotter gets a parked Surveyor instead
    /// of a Kestrel.
    pub surveyor_spotter: bool,
}

/// What to fight and for how long.
#[derive(Debug, Clone)]
pub struct Plan {
    /// Scrap each side spends.
    pub budget: u32,
    /// Unit kinds fighting each other, every pairing and mirror.
    pub units: Vec<UnitKind>,
    /// Unit kinds attacking defenses.
    pub attackers: Vec<UnitKind>,
    /// Defenses standing against the attackers.
    pub defenses: Vec<Defense>,
    /// Tick cap per duel.
    pub tick_limit: u32,
    /// Whether an army that needs a spotter gets a parked Surveyor instead
    /// of a Kestrel; a defense line keeps its Kestrel.
    pub surveyor_spotter: bool,
}

impl Plan {
    /// Every leg to run, in report order. Pairings where neither side can
    /// hurt the other are left out: they would only time out.
    pub fn duels(&self) -> Vec<Duel> {
        let mut pairs = Vec::new();
        for (i, &a) in self.units.iter().enumerate() {
            for &b in &self.units[i..] {
                pairs.push((Contender::Unit(a), Contender::Unit(b)));
            }
        }
        for &a in &self.attackers {
            for &d in &self.defenses {
                pairs.push((Contender::Unit(a), Contender::Defense(d)));
            }
        }
        let mut duels = Vec::new();
        for (a, b) in pairs {
            if !a.threatens(b) && !b.threatens(a) {
                continue;
            }
            let (a, b) = Side::matched(a, b, self.budget);
            for layout in Layout::ALL {
                for formation in Formation::ALL {
                    for a_west in [true, false] {
                        duels.push(Duel {
                            a,
                            b,
                            layout,
                            formation,
                            a_west,
                            surveyor_spotter: self.surveyor_spotter,
                        });
                    }
                }
            }
        }
        duels
    }
}

/// How a duel ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    /// One side lost every body.
    Wipe,
    /// The tick cap arrived first.
    Timeout,
}

/// One side's standing when the duel ended.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Standing {
    /// Bodies still alive that can fight.
    pub alive: u32,
    /// Surviving value: each live body's cost scaled by its health, none
    /// once no fighter is left.
    pub retained: f64,
}

/// One leg's result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Outcome {
    /// The leg.
    pub duel: Duel,
    /// How it ended.
    pub end: End,
    /// Ticks it ran.
    pub ticks: u32,
    /// Side `a` at the end.
    pub a: Standing,
    /// Side `b` at the end.
    pub b: Standing,
    /// Side `a`'s share of its value kept minus side `b`'s: +1 is a clean
    /// win for `a`, -1 a clean loss, 0 an even trade.
    pub score: f64,
}

/// Everything one side fields: unit kinds hunting together, or one defense
/// line. The duel lab fields one kind a side; the composition search fields
/// mixes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Force {
    /// Units hunting together, the shortest-ranged in the front ranks and
    /// support behind every gun, following the nearest fighter.
    Army(Vec<(UnitKind, u32)>),
    /// A line of one defense.
    Line(Defense, u32),
}

impl Force {
    /// The force one duel lab side fields.
    pub fn of(side: Side) -> Self {
        match side.contender {
            Contender::Unit(kind) => Self::Army(vec![(kind, side.count)]),
            Contender::Defense(defense) => Self::Line(defense, side.count),
        }
    }

    /// Scrap the whole force cost.
    pub fn spent(&self) -> u32 {
        match self {
            Self::Army(members) => members
                .iter()
                .map(|&(kind, count)| kind.stats().cost * count)
                .sum(),
            Self::Line(defense, count) => defense.cost() * count,
        }
    }

    fn contenders(&self) -> Vec<Contender> {
        match self {
            Self::Army(members) => members
                .iter()
                .map(|&(kind, _)| Contender::Unit(kind))
                .collect(),
            Self::Line(defense, _) => vec![Contender::Defense(*defense)],
        }
    }

    /// Whether any part of this force can hurt any part of the other.
    pub fn threatens(&self, other: &Self) -> bool {
        let theirs = other.contenders();
        self.contenders()
            .iter()
            .any(|&mine| theirs.iter().any(|&their| mine.threatens(their)))
    }

    fn needs_spotter(&self) -> bool {
        self.contenders().iter().any(|c| c.needs_spotter())
    }

    fn fields(&self, kind: UnitKind) -> bool {
        matches!(self, Self::Army(members) if members.iter().any(|&(k, _)| k == kind))
    }

    /// The force's kinds joined with `+`.
    pub fn name(&self) -> String {
        self.contenders()
            .iter()
            .map(|c| c.name())
            .collect::<Vec<_>>()
            .join("+")
    }
}

fn mirrored(player: u8, tile: TilePos) -> TilePos {
    if player == 0 {
        tile
    } else {
        TilePos::new(WIDTH - 1 - tile.x, HEIGHT - 1 - tile.y)
    }
}

/// Stage an engagement as a sandbox scenario: `west` takes seat 0, `east`
/// seat 1, mirrored through the field's center.
pub fn stage(
    west: &Force,
    east: &Force,
    layout: Layout,
    formation: Formation,
    surveyor_spotter: bool,
) -> Scenario {
    let mut units = Vec::new();
    let mut buildings = Vec::new();
    let mut scrap = [0u32; 2];
    for (player, force) in [(0u8, west), (1u8, east)] {
        match force {
            Force::Army(members) => {
                let mut ranks: Vec<(UnitKind, u32)> = members.clone();
                ranks.sort_by_key(|&(kind, _)| {
                    let stats = kind.stats();
                    (
                        !stats.can_fight(),
                        stats.weapons.iter().map(|w| w.range).max(),
                    )
                });
                let bodies: Vec<UnitKind> = ranks
                    .iter()
                    .flat_map(|&(kind, count)| std::iter::repeat_n(kind, count as usize))
                    .collect();
                let count = u32::try_from(bodies.len()).expect("army sizes fit in u32");
                for (kind, tile) in bodies.into_iter().zip(formation.tiles(count)) {
                    let tile = mirrored(player, tile);
                    units.push(UnitSpec {
                        player,
                        kind,
                        x: tile.x,
                        y: tile.y,
                    });
                }
            }
            Force::Line(defense, count) => {
                let kind = defense.kind;
                let (w, h) = kind.size();
                let per_column = (LAST_ROW - FIRST_ROW + 2) / (h + 1);
                let count_i = i32::try_from(*count).expect("defense counts fit in i32");
                for i in 0..count_i {
                    let anchor = TilePos::new(
                        FRONT_X - (w - 1) - (i / per_column) * (w + 1),
                        FIRST_ROW + (i % per_column) * (h + 1),
                    );
                    let anchor = if player == 0 {
                        anchor
                    } else {
                        TilePos::new(WIDTH - anchor.x - w, HEIGHT - anchor.y - h)
                    };
                    buildings.push(BuildingSpec {
                        player,
                        kind,
                        x: anchor.x,
                        y: anchor.y,
                    });
                }
                // The upgrades' prerequisites stand in the back corners,
                // and the seat holds exactly the scrap its refits cost.
                for (slot, prerequisite) in defense.prerequisites().into_iter().enumerate() {
                    let (pw, ph) = prerequisite.size();
                    let anchor = if slot == 0 {
                        TilePos::new(1, 1)
                    } else {
                        TilePos::new(1, HEIGHT - 1 - ph)
                    };
                    let anchor = if player == 0 {
                        anchor
                    } else {
                        TilePos::new(WIDTH - anchor.x - pw, HEIGHT - anchor.y - ph)
                    };
                    buildings.push(BuildingSpec {
                        player,
                        kind: prerequisite,
                        x: anchor.x,
                        y: anchor.y,
                    });
                }
                scrap[usize::from(player)] = count * (defense.cost() - Defense::base(kind).cost());
            }
        }
        if force.needs_spotter() && !(surveyor_spotter && matches!(force, Force::Army(_))) {
            let tile = mirrored(player, TilePos::new(SPOTTER_X, HEIGHT / 2));
            units.push(UnitSpec {
                player,
                kind: UnitKind::Kestrel,
                x: tile.x,
                y: tile.y,
            });
        }
    }
    if surveyor_spotter {
        for (player, force, foe) in [(0u8, west, east), (1u8, east, west)] {
            if let (Force::Army(_), true) = (force, force.needs_spotter()) {
                let tile = parked_spot(&layout.map(), player, foe, &buildings);
                units.push(UnitSpec {
                    player,
                    kind: UnitKind::Surveyor,
                    x: tile.x,
                    y: tile.y,
                });
            }
        }
    }
    let seat = |name: &str, scrap: u32| PlayerSpec {
        name: name.into(),
        team: None,
        scrap,
        bot: false,
        bot_config: None,
    };
    Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Sandbox,
        name: format!("duel-{}-vs-{}", west.name(), east.name()),
        map: layout.map(),
        players: vec![seat("West", scrap[0]), seat("East", scrap[1])],
        units,
        buildings,
        meta: None,
    }
}

/// Where a parked Surveyor spots for `player`'s guns along the centre row:
/// the open tile nearest the enemy that stands beyond every enemy gun's
/// reach plus a tile yet keeps an enemy footprint inside its radar. A gun
/// that outreaches the radar leaves no such tile; the scout then takes the
/// first open tile whose radar reaches an enemy footprint.
fn parked_spot(map: &[String], player: u8, foe: &Force, buildings: &[BuildingSpec]) -> TilePos {
    let radar = UnitKind::Surveyor
        .stats()
        .radar
        .expect("the Surveyor carries a radar")
        .radius;
    let reach = match foe {
        Force::Line(defense, _) => defense
            .stats()
            .weapons
            .iter()
            .map(|weapon| weapon.range.to_num::<f64>())
            .fold(0.0, f64::max),
        Force::Army(_) => 0.0,
    };
    let enemy: Vec<&BuildingSpec> = buildings.iter().filter(|b| b.player != player).collect();
    let open = |tile: TilePos| {
        map.get(as_index(tile.y))
            .and_then(|row| row.as_bytes().get(as_index(tile.x)))
            == Some(&b'.')
    };
    let gap = |tile: TilePos, b: &BuildingSpec| {
        let (w, h) = b.kind.size();
        let (cx, cy) = (f64::from(tile.x) + 0.5, f64::from(tile.y) + 0.5);
        let dx = (f64::from(b.x) - cx).max(cx - f64::from(b.x + w)).max(0.0);
        let dy = (f64::from(b.y) - cy).max(cy - f64::from(b.y + h)).max(0.0);
        (dx * dx + dy * dy).sqrt()
    };
    let covers = |tile: TilePos| {
        enemy.iter().any(|b| {
            let (w, h) = b.kind.size();
            (0..w).any(|dx| {
                (0..h).any(|dy| {
                    let (ex, ey) = (b.x + dx - tile.x, b.y + dy - tile.y);
                    ex * ex + ey * ey <= radar * radar
                })
            })
        })
    };
    let row: Vec<TilePos> = (1..WIDTH - 1)
        .map(|x| mirrored(player, TilePos::new(x, HEIGHT / 2)))
        .filter(|&tile| open(tile))
        .collect();
    row.iter()
        .rev()
        .copied()
        .find(|&tile| covers(tile) && enemy.iter().all(|b| gap(tile, b) > reach + 1.0))
        .or_else(|| row.iter().copied().find(|&tile| covers(tile)))
        .unwrap_or_else(|| mirrored(player, TilePos::new(SPOTTER_X, HEIGHT / 2)))
}

/// Stage one duel lab leg.
pub fn scenario(duel: &Duel) -> Scenario {
    let (west, east) = seated(duel);
    stage(
        &west,
        &east,
        duel.layout,
        duel.formation,
        duel.surveyor_spotter,
    )
}

fn seated(duel: &Duel) -> (Force, Force) {
    let (a, b) = (Force::of(duel.a), Force::of(duel.b));
    if duel.a_west { (a, b) } else { (b, a) }
}

/// Bodies a force still fields that can fight, and the health-scaled value
/// of everything it fields. Support with no fighter left is lost with the
/// army it served.
fn standing(state: &State, player: PlayerId, force: &Force) -> Standing {
    let mut alive = 0;
    let mut retained = 0.0;
    match force {
        Force::Army(_) => {
            for u in state
                .units()
                .iter()
                .filter(|u| u.player == player && force.fields(u.kind) && u.hp > 0)
            {
                let stats = u.kind.stats();
                if stats.can_fight() {
                    alive += 1;
                }
                retained += f64::from(stats.cost) * f64::from(u.hp) / f64::from(stats.max_hp);
            }
            if alive == 0 {
                retained = 0.0;
            }
        }
        Force::Line(defense, _) => {
            for b in state
                .buildings()
                .iter()
                .filter(|b| b.player == player && b.kind == defense.kind && b.hp > 0)
            {
                alive += 1;
                retained +=
                    f64::from(defense.cost()) * f64::from(b.hp) / f64::from(defense.stats().max_hp);
            }
        }
    }
    Standing { alive, retained }
}

/// Where a force's nearest live body stands, measured from `from`: a unit's
/// tile, or a building's nearest footprint tile with its id and anchor.
fn nearest(
    state: &State,
    player: PlayerId,
    force: &Force,
    from: chassis::fx::Vec2Fx,
) -> Option<(TilePos, Option<(aftermine_sim::BuildingId, TilePos)>)> {
    match force {
        Force::Army(_) => state
            .units()
            .iter()
            .filter(|u| u.player == player && force.fields(u.kind) && u.hp > 0)
            .min_by_key(|u| (from.dist_sq(u.pos), u.id))
            .map(|u| (u.tile(), None)),
        Force::Line(defense, _) => state
            .buildings()
            .iter()
            .filter(|b| b.player == player && b.kind == defense.kind && b.hp > 0)
            .min_by_key(|b| (from.dist_sq(b.closest_point_to(from)), b.id))
            .map(|b| {
                let tile = nearest_tile(b.anchor, defense.kind.size(), from, player);
                (tile, Some((b.id, b.anchor)))
            }),
    }
}

/// The footprint tile nearest `from`. A building's anchor is its top-left
/// tile, and the mirror building's anchor is not that tile's mirror, so a goal
/// taken from the anchor would differ by seat. The nearest tile mirrors
/// exactly; an exact tie falls to the owner's own reading order, which mirrors
/// too.
fn nearest_tile(
    anchor: TilePos,
    (w, h): (i32, i32),
    from: chassis::fx::Vec2Fx,
    owner: PlayerId,
) -> TilePos {
    let reading = if owner == PlayerId(0) { 1 } else { -1 };
    (0..h)
        .flat_map(|dy| (0..w).map(move |dx| anchor.offset(dx, dy)))
        .min_by_key(|tile| {
            (
                from.dist_sq(tile.center()),
                tile.y * reading,
                tile.x * reading,
            )
        })
        .expect("every footprint has a tile")
}

/// Send a side's idle bodies after the nearest live enemy. Sappers only ever
/// attack buildings by order, so they get one against the nearest defense
/// they can see. The harness may look through fog: it scripts both sides the
/// same way and never feeds a controller.
fn seek(
    state: &State,
    me: PlayerId,
    mine: &Force,
    foe: PlayerId,
    theirs: &Force,
) -> Option<PlayerCommand> {
    let idle: Vec<_> = state
        .units()
        .iter()
        .filter(|u| {
            u.player == me
                && mine.fields(u.kind)
                && u.kind.stats().can_fight()
                && u.hp > 0
                && matches!(u.order, Order::Idle)
        })
        .collect();
    let from = idle.first()?.pos;
    let (goal, building) = nearest(state, foe, theirs, from)?;
    let units = idle.iter().map(|u| u.id).collect();
    let demolition = idle.iter().any(|u| u.kind.stats().demolition.is_some());
    let command = match (building, theirs) {
        (Some((id, anchor)), Force::Line(defense, _)) if demolition => {
            let (w, h) = defense.kind.size();
            let seen = (0..w).any(|dx| (0..h).any(|dy| state.can_see(me, anchor.offset(dx, dy))));
            if seen {
                Command::Attack {
                    units,
                    target: AttackTarget::Building(id),
                    queue: false,
                }
            } else {
                Command::Hunt {
                    units,
                    goal,
                    queue: false,
                }
            }
        }
        _ => Command::Hunt {
            units,
            goal,
            queue: false,
        },
    };
    Some(PlayerCommand {
        player: me,
        command,
    })
}

/// Send a side's support bodies that have drifted from their army to its
/// nearest fighter: farther than half a point defender's reach, its
/// middle would leave the cover.
fn escort(state: &State, me: PlayerId, mine: &Force) -> Vec<PlayerCommand> {
    let fighters: Vec<_> = state
        .units()
        .iter()
        .filter(|u| u.player == me && mine.fields(u.kind) && u.kind.stats().can_fight() && u.hp > 0)
        .collect();
    state
        .units()
        .iter()
        .filter(|u| u.player == me && mine.fields(u.kind) && !u.kind.stats().can_fight())
        .filter_map(|support| {
            let nearest = fighters
                .iter()
                .min_by_key(|fighter| (support.pos.dist_sq(fighter.pos), fighter.id))?;
            let gap = support
                .kind
                .stats()
                .point_defense
                .map_or(chassis::fx::Fx::ONE, |defense| defense.radius / 2);
            (support.pos.dist_sq(nearest.pos) > gap * gap).then(|| PlayerCommand {
                player: me,
                command: Command::Run {
                    units: vec![support.id],
                    goal: nearest.tile(),
                    queue: false,
                },
            })
        })
        .collect()
}

/// Keep a side's spotter hovering short of the nearest enemy, on its own
/// side, so the guns it serves keep seeing what they can reach.
fn spot(
    state: &State,
    me: PlayerId,
    mine: &Force,
    foe: PlayerId,
    theirs: &Force,
) -> Option<PlayerCommand> {
    if !mine.needs_spotter() {
        return None;
    }
    let spotter = state
        .units()
        .iter()
        .find(|u| u.player == me && u.kind == UnitKind::Kestrel && u.hp > 0)?;
    let (nearest, _) = nearest(state, foe, theirs, spotter.pos)?;
    let toward_home = if me == PlayerId(0) {
        -SPOTTER_STANDOFF
    } else {
        SPOTTER_STANDOFF
    };
    let goal = TilePos::new((nearest.x + toward_home).clamp(1, WIDTH - 2), nearest.y);
    Some(PlayerCommand {
        player: me,
        command: Command::Run {
            units: vec![spotter.id],
            goal,
            queue: false,
        },
    })
}

/// Upgrade every defense line to its tier with ordinary commands, one rung
/// at a time, while the armies wait out of reach.
fn refit(state: &mut State, sides: [(PlayerId, &Force); 2]) -> Result<()> {
    for _ in 0..REFIT_LIMIT {
        let mut orders = Vec::new();
        let mut done = true;
        for (player, force) in sides {
            let Force::Line(defense, _) = force else {
                continue;
            };
            for b in state
                .buildings()
                .iter()
                .filter(|b| b.player == player && b.kind == defense.kind)
            {
                let built = matches!(b.phase, BuildingPhase::Built { .. });
                if b.tier < defense.tier || !built {
                    done = false;
                }
                if b.tier < defense.tier && built {
                    orders.push(PlayerCommand {
                        player,
                        command: Command::UpgradeBuilding { building: b.id },
                    });
                }
            }
        }
        if done {
            return Ok(());
        }
        state.tick(&orders);
    }
    anyhow::bail!("defense refits did not finish in {REFIT_LIMIT} ticks")
}

/// How an engagement ended, by seat.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Fought {
    /// How it ended.
    pub end: End,
    /// Ticks it ran, refits excluded.
    pub ticks: u32,
    /// The west force at the end.
    pub west: Standing,
    /// The east force at the end.
    pub east: Standing,
}

/// Stage and play one engagement to a wipe or the tick cap.
pub fn fight(
    west: &Force,
    east: &Force,
    layout: Layout,
    formation: Formation,
    tick_limit: u32,
    surveyor_spotter: bool,
) -> Result<Fought> {
    let sc = stage(west, east, layout, formation, surveyor_spotter);
    let mut state = sc.build().with_context(|| format!("staging {}", sc.name))?;
    refit(&mut state, [(PlayerId(0), west), (PlayerId(1), east)])
        .with_context(|| format!("refitting {}", sc.name))?;
    let front = |player: u8| mirrored(player, TilePos::new(FRONT_X, HEIGHT / 2));
    let mut orders = Vec::new();
    for (player, force, goal) in [(0u8, west, front(1)), (1u8, east, front(0))] {
        if !matches!(force, Force::Army(_)) {
            continue;
        }
        let units = state
            .units()
            .iter()
            .filter(|u| {
                u.player == PlayerId(player) && force.fields(u.kind) && u.kind.stats().can_fight()
            })
            .map(|u| u.id)
            .collect();
        orders.push(PlayerCommand {
            player: PlayerId(player),
            command: Command::Hunt {
                units,
                goal,
                queue: false,
            },
        });
    }
    state.tick(&orders);
    let mut ticks = 1;
    let sides = [
        (PlayerId(0), west, PlayerId(1), east),
        (PlayerId(1), east, PlayerId(0), west),
    ];
    let end = loop {
        let (w, e) = (
            standing(&state, PlayerId(0), west),
            standing(&state, PlayerId(1), east),
        );
        if w.alive == 0 || e.alive == 0 {
            break End::Wipe;
        }
        if ticks >= tick_limit {
            break End::Timeout;
        }
        let orders = if ticks % SEEK_PERIOD == 0 {
            sides
                .iter()
                .flat_map(|&(me, mine, foe, theirs)| {
                    [
                        seek(&state, me, mine, foe, theirs),
                        spot(&state, me, mine, foe, theirs),
                    ]
                    .into_iter()
                    .flatten()
                    .chain(escort(&state, me, mine))
                })
                .collect()
        } else {
            Vec::new()
        };
        state.tick(&orders);
        ticks += 1;
    };
    Ok(Fought {
        end,
        ticks,
        west: standing(&state, PlayerId(0), west),
        east: standing(&state, PlayerId(1), east),
    })
}

/// Run one duel lab leg to a wipe or the tick cap.
pub fn run(duel: &Duel, tick_limit: u32) -> Result<Outcome> {
    let (west, east) = seated(duel);
    let fought = fight(
        &west,
        &east,
        duel.layout,
        duel.formation,
        tick_limit,
        duel.surveyor_spotter,
    )?;
    let (a, b) = if duel.a_west {
        (fought.west, fought.east)
    } else {
        (fought.east, fought.west)
    };
    Ok(Outcome {
        duel: *duel,
        end: fought.end,
        ticks: fought.ticks,
        a,
        b,
        score: a.retained / f64::from(duel.a.spent) - b.retained / f64::from(duel.b.spent),
    })
}

/// Run every leg of a plan across at most `jobs` workers.
pub fn run_plan(plan: &Plan, jobs: NonZeroUsize) -> Result<Vec<Outcome>> {
    let duels = plan.duels();
    crate::pool::fan_out_bounded(&duels, jobs, |duel| run(duel, plan.tick_limit))
}

/// A row kind's mean score against a column, over every layout, formation
/// and seat assignment; `None` where neither side can hurt the other.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Matrix {
    /// Row contenders.
    pub rows: Vec<Contender>,
    /// Column contenders.
    pub columns: Vec<Contender>,
    /// `cells[row][column]`.
    pub cells: Vec<Vec<Option<f64>>>,
}

impl Matrix {
    fn build(rows: Vec<Contender>, columns: Vec<Contender>, outcomes: &[Outcome]) -> Self {
        let cells = rows
            .iter()
            .map(|&row| {
                columns
                    .iter()
                    .map(|&column| {
                        let scores: Vec<f64> = outcomes
                            .iter()
                            .filter_map(|o| {
                                let (a, b) = (o.duel.a.contender, o.duel.b.contender);
                                if (a, b) == (row, column) {
                                    Some(o.score)
                                } else if (b, a) == (row, column) {
                                    Some(-o.score)
                                } else {
                                    None
                                }
                            })
                            .collect();
                        (!scores.is_empty())
                            .then(|| scores.iter().sum::<f64>() / scores.len() as f64)
                    })
                    .collect()
            })
            .collect();
        Self {
            rows,
            columns,
            cells,
        }
    }

    fn cell(&self, row: usize, column: usize) -> Option<f64> {
        self.cells[row][column]
    }
}

/// Two kinds whose results against everyone else look the same.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Alike {
    /// One kind.
    pub a: UnitKind,
    /// The other.
    pub b: UnitKind,
    /// Cosine similarity of their scores against the kinds both met.
    pub similarity: f64,
    /// Their head-to-head score, from `a`'s side.
    pub head_to_head: f64,
}

/// How a tier fares against a lower one at equal scrap.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TierMargin {
    /// The higher tier.
    pub higher: u8,
    /// The lower tier.
    pub lower: u8,
    /// Mean score of every higher-tier kind against every lower-tier kind
    /// it can trade fire with.
    pub mean: f64,
    /// How many matchups the mean covers.
    pub matchups: usize,
}

/// A pairing whose two seat assignments disagree.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SplitLeg {
    /// Side `a`.
    pub a: Contender,
    /// Side `b`.
    pub b: Contender,
    /// The ground.
    pub layout: Layout,
    /// The start.
    pub formation: Formation,
    /// Side `a`'s score from the west seat.
    pub west: f64,
    /// Side `a`'s score from the east seat.
    pub east: f64,
}

/// What the matrix says, flagged.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Flags {
    /// Kinds that win every matchup they can fight.
    pub dominant: Vec<UnitKind>,
    /// Kinds that lose every matchup they can fight.
    pub dominated: Vec<UnitKind>,
    /// Kinds that play alike.
    pub alike: Vec<Alike>,
    /// Kinds whose results elsewhere match another's but who lose to it head
    /// to head (`a` is the winner): the loser adds nothing the winner lacks.
    pub outclassed: Vec<Alike>,
    /// Tier against tier.
    pub tiers: Vec<TierMargin>,
    /// Legs that depend on the seat.
    pub split_legs: Vec<SplitLeg>,
}

/// The whole lab report.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    /// Scrap each side spent, before rounding to whole bodies.
    pub budget: u32,
    /// Tick cap per duel.
    pub tick_limit: u32,
    /// Unit against unit.
    pub units: Matrix,
    /// Attacker against defense line; empty without defenses.
    pub defenses: Matrix,
    /// The unit matrix, flagged.
    pub flags: Flags,
    /// Every leg.
    pub outcomes: Vec<Outcome>,
}

/// Which tier a kind belongs to: what its producer and prerequisites ask.
pub fn tier(kind: UnitKind) -> u8 {
    let producer = BuildingKind::ALL
        .iter()
        .copied()
        .find(|b| b.base_stats().produces.contains(&kind));
    let needs_crucible = |needs: &[aftermine_sim::stats::Prerequisite]| {
        needs.iter().any(|need| need.kind == BuildingKind::Crucible)
    };
    let cradle_needs_crucible = BuildingKind::ALL.iter().any(|b| {
        b.assembles() == Some(kind)
            && b.base_stats()
                .construction
                .is_some_and(|construction| needs_crucible(construction.requires))
    });
    if producer == Some(BuildingKind::Crucible)
        || needs_crucible(kind.stats().requires)
        || cradle_needs_crucible
    {
        3
    } else if producer == Some(BuildingKind::Foundry) {
        1
    } else {
        2
    }
}

fn kinds_of(matrix: &Matrix) -> Vec<UnitKind> {
    matrix
        .rows
        .iter()
        .filter_map(|c| match c {
            Contender::Unit(kind) => Some(*kind),
            Contender::Defense(_) => None,
        })
        .collect()
}

fn flags(matrix: &Matrix, outcomes: &[Outcome]) -> Flags {
    let kinds = kinds_of(matrix);
    let n = kinds.len();
    let against_others = |i: usize| -> Vec<f64> {
        (0..n)
            .filter(|&j| j != i)
            .filter_map(|j| matrix.cell(i, j))
            .collect()
    };
    let dominant = (0..n)
        .filter(|&i| {
            let scores = against_others(i);
            !scores.is_empty() && scores.iter().all(|&s| s >= DECISIVE)
        })
        .map(|i| kinds[i])
        .collect();
    let dominated = (0..n)
        .filter(|&i| {
            let scores = against_others(i);
            !scores.is_empty() && scores.iter().all(|&s| s <= -DECISIVE)
        })
        .map(|i| kinds[i])
        .collect();

    let mut alike = Vec::new();
    let mut outclassed = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            let Some(head_to_head) = matrix.cell(i, j) else {
                continue;
            };
            let shared: Vec<(f64, f64)> = (0..n)
                .filter(|&k| k != i && k != j)
                .filter_map(|k| Some((matrix.cell(i, k)?, matrix.cell(j, k)?)))
                .collect();
            if shared.len() < 3 {
                continue;
            }
            let dot: f64 = shared.iter().map(|(x, y)| x * y).sum();
            let norm = |pick: fn(&(f64, f64)) -> f64| {
                shared.iter().map(|p| pick(p).powi(2)).sum::<f64>().sqrt()
            };
            let (nx, ny) = (norm(|p| p.0), norm(|p| p.1));
            if nx == 0.0 || ny == 0.0 {
                continue;
            }
            let similarity = dot / (nx * ny);
            if similarity < ALIKE_SIMILARITY {
                continue;
            }
            if head_to_head.abs() <= ALIKE_HEAD_TO_HEAD {
                alike.push(Alike {
                    a: kinds[i],
                    b: kinds[j],
                    similarity,
                    head_to_head,
                });
            } else {
                let (winner, loser) = if head_to_head > 0.0 { (i, j) } else { (j, i) };
                outclassed.push(Alike {
                    a: kinds[winner],
                    b: kinds[loser],
                    similarity,
                    head_to_head: head_to_head.abs(),
                });
            }
        }
    }

    // A tier margin compares machines that can trade fire. An anti-air
    // fighter losing to ground units it cannot shoot says nothing about its
    // tier, and those one-sided losses would drown the rest.
    let kinds: &[UnitKind] = &kinds;
    let mutual = |i: usize, j: usize| {
        let (a, b) = (Contender::Unit(kinds[i]), Contender::Unit(kinds[j]));
        a.threatens(b) && b.threatens(a)
    };
    let mut tiers = Vec::new();
    for (higher, lower) in [(3, 2), (3, 1), (2, 1)] {
        let scores: Vec<f64> = (0..n)
            .filter(|&i| tier(kinds[i]) == higher)
            .flat_map(|i| {
                (0..n)
                    .filter(move |&j| tier(kinds[j]) == lower && mutual(i, j))
                    .filter_map(move |j| matrix.cell(i, j))
            })
            .collect();
        if !scores.is_empty() {
            tiers.push(TierMargin {
                higher,
                lower,
                mean: scores.iter().sum::<f64>() / scores.len() as f64,
                matchups: scores.len(),
            });
        }
    }

    let split_legs = outcomes
        .iter()
        .filter(|o| o.duel.a_west)
        .filter_map(|west| {
            let east = outcomes.iter().find(|o| {
                !o.duel.a_west
                    && o.duel.a == west.duel.a
                    && o.duel.b == west.duel.b
                    && o.duel.layout == west.duel.layout
                    && o.duel.formation == west.duel.formation
            })?;
            ((west.score - east.score).abs() > SPLIT_LEGS).then_some(SplitLeg {
                a: west.duel.a.contender,
                b: west.duel.b.contender,
                layout: west.duel.layout,
                formation: west.duel.formation,
                west: west.score,
                east: east.score,
            })
        })
        .collect();

    Flags {
        dominant,
        dominated,
        alike,
        outclassed,
        tiers,
        split_legs,
    }
}

/// Assemble the report from every leg's outcome.
pub fn report(plan: &Plan, outcomes: Vec<Outcome>) -> Report {
    let units: Vec<Contender> = plan.units.iter().copied().map(Contender::Unit).collect();
    let unit_matrix = Matrix::build(units.clone(), units, &outcomes);
    let defense_matrix = Matrix::build(
        plan.attackers
            .iter()
            .copied()
            .map(Contender::Unit)
            .collect(),
        plan.defenses
            .iter()
            .copied()
            .map(Contender::Defense)
            .collect(),
        &outcomes,
    );
    let flags = flags(&unit_matrix, &outcomes);
    Report {
        budget: plan.budget,
        tick_limit: plan.tick_limit,
        units: unit_matrix,
        defenses: defense_matrix,
        flags,
        outcomes,
    }
}

fn render_matrix(out: &mut String, title: &str, matrix: &Matrix) {
    if matrix.rows.is_empty() || matrix.columns.is_empty() {
        return;
    }
    let _ = writeln!(out, "{title} (row's score vs column x100; -- = no contest)");
    let _ = write!(out, "{:>6}", "");
    for column in &matrix.columns {
        let _ = write!(out, "{:>6}", column.label());
    }
    let _ = writeln!(out);
    for (i, row) in matrix.rows.iter().enumerate() {
        let _ = write!(out, "{:>6}", row.label());
        for j in 0..matrix.columns.len() {
            match matrix.cell(i, j) {
                Some(score) => {
                    // Adding zero turns a rounded -0 into 0.
                    let _ = write!(out, "{:>6.0}", (score * 100.0).round() + 0.0);
                }
                None => {
                    let _ = write!(out, "{:>6}", "--");
                }
            }
        }
        let _ = writeln!(out);
    }
    let _ = writeln!(out);
}

/// The human-readable table: both matrices and the flags.
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    let timeouts = report
        .outcomes
        .iter()
        .filter(|o| o.end == End::Timeout)
        .count();
    let _ = writeln!(
        out,
        "duel lab: {} legs at {} scrap a side, {} timed out at {} ticks\n",
        report.outcomes.len(),
        report.budget,
        timeouts,
        report.tick_limit
    );
    render_matrix(&mut out, "units", &report.units);
    render_matrix(&mut out, "defenses", &report.defenses);
    let names = |kinds: &[UnitKind]| {
        if kinds.is_empty() {
            "none".to_string()
        } else {
            kinds
                .iter()
                .map(|k| k.name())
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    let flags = &report.flags;
    let _ = writeln!(out, "wins every matchup: {}", names(&flags.dominant));
    let _ = writeln!(out, "loses every matchup: {}", names(&flags.dominated));
    for pair in &flags.alike {
        let _ = writeln!(
            out,
            "plays alike: {} and {} (similarity {:.2}, head to head {:+.2})",
            pair.a.name(),
            pair.b.name(),
            pair.similarity,
            pair.head_to_head
        );
    }
    for pair in &flags.outclassed {
        let _ = writeln!(
            out,
            "outclassed: {} by {} (similarity {:.2}, loses head to head by {:.2})",
            pair.b.name(),
            pair.a.name(),
            pair.similarity,
            pair.head_to_head
        );
    }
    for tier in &flags.tiers {
        let _ = writeln!(
            out,
            "tier {} vs tier {}: {:+.2} over {} matchups where both can fire",
            tier.higher, tier.lower, tier.mean, tier.matchups
        );
    }
    let _ = writeln!(
        out,
        "legs split by seat (over {SPLIT_LEGS}): {}",
        flags.split_legs.len()
    );
    let mut timed_out: Vec<(&'static str, &'static str, usize)> = Vec::new();
    for o in report.outcomes.iter().filter(|o| o.end == End::Timeout) {
        let pair = (o.duel.a.contender.name(), o.duel.b.contender.name());
        match timed_out.iter_mut().find(|(a, b, _)| (*a, *b) == pair) {
            Some(entry) => entry.2 += 1,
            None => timed_out.push((pair.0, pair.1, 1)),
        }
    }
    for (a, b, legs) in timed_out {
        let _ = writeln!(out, "timed out: {a} vs {b} ({legs} legs)");
    }
    out
}

#[cfg(test)]
mod tests;
