//! The fight lab: armies meet on open ground, and the report gives what
//! each destroyed of the other's value per value it lost. The matrix pits
//! one kind against another at even prices, and each kind against a block of
//! the structures a seat raises to defend; the presets mix kinds the way
//! armies fight in games: artillery behind a front line with a spotter,
//! snipers behind a screen, a Mantle over a push. Each fight plays twice,
//! the sides trading seats, and sums both, so the seats' turn order favors
//! neither. Armies hunt the other side's middle unit, or its middle
//! structure once it has no units, with no terrain or micro: the lab reads
//! how kinds trade, not how a match goes.

use crate::pool::fan_out_bounded;
use aftermine_sim::scenario::{BuildingSpec, PlayerSpec, Scenario, ScenarioMode, UnitSpec};
use aftermine_sim::stats::{BuildingKind, Domain, UnitKind};
use aftermine_sim::{Command, PlayerCommand, PlayerId, State};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fmt::Write as _;
use std::num::NonZeroUsize;

/// The kinds a matrix covers unless told otherwise: every unit that fights
/// but the cradle machines, whose price no shared budget fits.
pub const COMBAT: [UnitKind; 12] = [
    UnitKind::Scuttler,
    UnitKind::Sentinel,
    UnitKind::Lancer,
    UnitKind::Bombard,
    UnitKind::Warden,
    UnitKind::Piledriver,
    UnitKind::Avalanche,
    UnitKind::Breaker,
    UnitKind::Flakhound,
    UnitKind::Buzzard,
    UnitKind::Talon,
    UnitKind::Condor,
];

/// Scrap a side spends on each matrix fight unless told otherwise: two sizes,
/// because a matchup can turn on how many arrive at once.
pub const BUDGETS: [u32; 2] = [1_400, 2_800];

/// The field's size in tiles, walls included.
const WIDTH: usize = 48;
const HEIGHT: usize = 24;

/// The columns where each seat's front rank forms up, its army filling rows
/// of `PER_ROW` behind it from the top row down.
const FRONT: [i32; 2] = [14, 30];
const PER_ROW: i32 = 5;
const TOP: i32 = 2;

/// Ticks between hunt orders, so each side turns to where the other went.
const ORDER_TICKS: u64 = 40;

/// Ticks a fight may last: five minutes.
const TICK_LIMIT: u64 = 6_000;

/// An army: how many of each kind.
pub type Army = Vec<(UnitKind, u32)>;

/// A block of structures: how many of each kind, built at their first rung.
pub type Guns = Vec<(BuildingKind, u32)>;

/// One side of a fight: an army, or a block of structures.
#[derive(Clone, Copy)]
struct Side<'a> {
    army: &'a [(UnitKind, u32)],
    guns: &'a [(BuildingKind, u32)],
}

impl<'a> Side<'a> {
    fn army(army: &'a [(UnitKind, u32)]) -> Self {
        Side { army, guns: &[] }
    }

    fn guns(guns: &'a [(BuildingKind, u32)]) -> Self {
        Side { army: &[], guns }
    }
}

/// What one army destroyed and lost, in scrap of value, a unit's value being
/// its price discounted by its missing health.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Trade {
    /// Value of the other army destroyed.
    pub killed: u64,
    /// Value of this army lost.
    pub lost: u64,
}

impl Trade {
    /// The same fight seen from the other army.
    #[must_use]
    pub fn reversed(self) -> Self {
        Trade {
            killed: self.lost,
            lost: self.killed,
        }
    }

    fn cell(self) -> String {
        match (self.killed, self.lost) {
            (0, 0) => "-".to_owned(),
            (_, 0) => "inf".to_owned(),
            #[expect(
                clippy::cast_precision_loss,
                reason = "a report figure; scrap totals stay far below 2^52"
            )]
            (killed, lost) => format!("{:.1}", killed as f64 / lost as f64),
        }
    }
}

/// Every kind against every other at each budget: `trades[budget][a][b]` is
/// kind `a` against kind `b`.
#[derive(Debug, Clone, Serialize)]
pub struct Matrix {
    /// The kinds, in row and column order.
    pub kinds: Vec<UnitKind>,
    /// Scrap a side spent, one table each.
    pub budgets: Vec<u32>,
    /// `trades[budget][a][b]`: kind `a` against kind `b`.
    pub trades: Vec<Vec<Vec<Trade>>>,
}

/// Every kind against every structure at each budget: `trades[budget][a][g]`
/// is kind `a` attacking structure `g`, from the kind's side.
#[derive(Debug, Clone, Serialize)]
pub struct GunMatrix {
    /// The kinds, in row order.
    pub kinds: Vec<UnitKind>,
    /// The structures, in column order.
    pub guns: Vec<BuildingKind>,
    /// Scrap a side spent, one table each.
    pub budgets: Vec<u32>,
    /// `trades[budget][a][g]`: kind `a` against structure `g`.
    pub trades: Vec<Vec<Vec<Trade>>>,
}

/// A mixed fight and how the first army fared.
#[derive(Debug, Clone, Serialize)]
pub struct Mixed {
    /// What the fight tests.
    pub name: String,
    /// The army the trade is read from.
    pub first: Army,
    /// The army it faced.
    pub second: Army,
    /// How the first army fared.
    pub trade: Trade,
}

/// A whole lab run.
#[derive(Debug, Clone, Serialize)]
pub struct Lab {
    /// Every pair of kinds at even prices.
    pub matrix: Matrix,
    /// Every kind against every structure at even prices.
    pub guns: GunMatrix,
    /// The mixed-army fights.
    pub mixed: Vec<Mixed>,
}

/// `first` against `second`, played from both seats and summed, from the
/// first army's side.
///
/// # Errors
/// An army that does not fit the field.
pub fn fight(first: &[(UnitKind, u32)], second: &[(UnitKind, u32)]) -> Result<Trade> {
    both_seats(Side::army(first), Side::army(second))
}

/// `army` attacking a block of `guns`, played from both seats and summed,
/// from the army's side.
///
/// # Errors
/// An army or a block that does not fit the field.
pub fn siege(army: &[(UnitKind, u32)], guns: &[(BuildingKind, u32)]) -> Result<Trade> {
    both_seats(Side::army(army), Side::guns(guns))
}

fn both_seats(first: Side<'_>, second: Side<'_>) -> Result<Trade> {
    let west = play(first, second)?;
    let east = play(second, first)?.reversed();
    Ok(Trade {
        killed: west.killed + east.killed,
        lost: west.lost + east.lost,
    })
}

/// As many of `kind` as `budget` buys, rounded, and at least one.
#[must_use]
pub fn even(kind: UnitKind, budget: u32) -> Army {
    let cost = kind.stats().cost.max(1);
    vec![(kind, ((budget + cost / 2) / cost).max(1))]
}

/// As many of the structure `kind` at its first rung as `budget` buys,
/// rounded, and at least one.
#[must_use]
pub fn even_guns(kind: BuildingKind, budget: u32) -> Guns {
    let cost = kind.base_stats().construction.cost.max(1);
    vec![(kind, ((budget + cost / 2) / cost).max(1))]
}

/// Every one of `kinds` against every one of `guns` at every one of
/// `budgets`, `jobs` fights at once.
///
/// # Errors
/// An army or a block that does not fit the field.
pub fn guns(
    kinds: &[UnitKind],
    guns: &[BuildingKind],
    budgets: &[u32],
    jobs: NonZeroUsize,
) -> Result<GunMatrix> {
    let fights: Vec<(usize, usize, usize)> = (0..budgets.len())
        .flat_map(|budget| {
            (0..kinds.len()).flat_map(move |a| (0..guns.len()).map(move |g| (budget, a, g)))
        })
        .collect();
    let results = fan_out_bounded(&fights, jobs, |&(budget, a, g)| {
        siege(
            &even(kinds[a], budgets[budget]),
            &even_guns(guns[g], budgets[budget]),
        )
    })?;
    let mut trades = vec![vec![vec![Trade::default(); guns.len()]; kinds.len()]; budgets.len()];
    for (&(budget, a, g), trade) in fights.iter().zip(results) {
        trades[budget][a][g] = trade;
    }
    Ok(GunMatrix {
        kinds: kinds.to_vec(),
        guns: guns.to_vec(),
        budgets: budgets.to_vec(),
        trades,
    })
}

/// Every pair of `kinds` at every one of `budgets`, `jobs` fights at once.
///
/// # Errors
/// An army that does not fit the field.
pub fn matrix(kinds: &[UnitKind], budgets: &[u32], jobs: NonZeroUsize) -> Result<Matrix> {
    let n = kinds.len();
    let pairs: Vec<(usize, usize, usize)> = (0..budgets.len())
        .flat_map(|budget| (0..n).flat_map(move |a| ((a + 1)..n).map(move |b| (budget, a, b))))
        .collect();
    let results = fan_out_bounded(&pairs, jobs, |&(budget, a, b)| {
        fight(
            &even(kinds[a], budgets[budget]),
            &even(kinds[b], budgets[budget]),
        )
    })?;
    let mut trades = vec![vec![vec![Trade::default(); n]; n]; budgets.len()];
    for (&(budget, a, b), trade) in pairs.iter().zip(results) {
        trades[budget][a][b] = trade;
        trades[budget][b][a] = trade.reversed();
    }
    Ok(Matrix {
        kinds: kinds.to_vec(),
        budgets: budgets.to_vec(),
        trades,
    })
}

/// The mixed fights: artillery as support behind brawlers, snipers behind a
/// screen, and a Mantle over a push against shells and against rails, each
/// at about even price.
#[must_use]
pub fn presets() -> Vec<(&'static str, Army, Army)> {
    use UnitKind::{
        Avalanche, Bombard, Breaker, Kestrel, Lancer, Mantle, Piledriver, Scuttler, Warden,
    };
    vec![
        (
            "Wardens and Bombards with a spotter v Wardens",
            vec![(Warden, 3), (Bombard, 3), (Kestrel, 1)],
            vec![(Warden, 5)],
        ),
        (
            "Wardens and Bombards with a spotter v Lancers",
            vec![(Warden, 3), (Bombard, 3), (Kestrel, 1)],
            vec![(Lancer, 13)],
        ),
        (
            "Wardens and Bombards with a spotter v Scuttlers",
            vec![(Warden, 3), (Bombard, 3), (Kestrel, 1)],
            vec![(Scuttler, 36)],
        ),
        (
            "Breaker and Avalanche with a spotter v Breakers",
            vec![(Breaker, 1), (Avalanche, 1), (Kestrel, 1)],
            vec![(Breaker, 2)],
        ),
        (
            "Piledrivers behind Wardens v Wardens",
            vec![(Piledriver, 2), (Warden, 4)],
            vec![(Warden, 7)],
        ),
        (
            "Piledrivers behind a Breaker v Breakers",
            vec![(Piledriver, 2), (Breaker, 1)],
            vec![(Breaker, 2)],
        ),
        (
            "Breakers under a Mantle v spotted Bombards",
            vec![(Breaker, 2), (Mantle, 1)],
            vec![(Bombard, 10), (Kestrel, 1)],
        ),
        (
            "Breakers under a Mantle v Piledrivers",
            vec![(Breaker, 2), (Mantle, 1)],
            vec![(Piledriver, 5)],
        ),
        (
            "Wardens under a Mantle v spotted Bombards",
            vec![(Warden, 4), (Mantle, 1)],
            vec![(Bombard, 8), (Kestrel, 1)],
        ),
        (
            "Wardens v spotted Bombards",
            vec![(Warden, 6)],
            vec![(Bombard, 8), (Kestrel, 1)],
        ),
        (
            "Wardens under a Mantle v Lancers",
            vec![(Warden, 4), (Mantle, 1)],
            vec![(Lancer, 15)],
        ),
    ]
}

/// `presets` fought `jobs` at once.
///
/// # Errors
/// An army that does not fit the field.
pub fn mixed(presets: &[(&str, Army, Army)], jobs: NonZeroUsize) -> Result<Vec<Mixed>> {
    let trades = fan_out_bounded(presets, jobs, |(_, first, second)| fight(first, second))?;
    Ok(presets
        .iter()
        .zip(trades)
        .map(|((name, first, second), trade)| Mixed {
            name: (*name).to_owned(),
            first: first.clone(),
            second: second.clone(),
            trade,
        })
        .collect())
}

/// The lab as tables: one matrix per budget, then each kind against the
/// structures per budget, then the mixed fights.
#[must_use]
pub fn report(lab: &Lab) -> String {
    let matrix = &lab.matrix;
    let mut out = String::new();
    for (budget, trades) in matrix.budgets.iter().zip(&matrix.trades) {
        let _ = writeln!(
            out,
            "{budget} scrap a side: value the row kind destroyed per value it lost against the column kind"
        );
        let _ = write!(out, "{:11}", "");
        for kind in &matrix.kinds {
            let _ = write!(out, "{:>6}", short(kind));
        }
        out.push('\n');
        for (a, row) in trades.iter().enumerate() {
            let _ = write!(out, "{:11}", format!("{:?}", matrix.kinds[a]));
            for (b, trade) in row.iter().enumerate() {
                let cell = if a == b {
                    "·".to_owned()
                } else {
                    trade.cell()
                };
                let _ = write!(out, "{cell:>6}");
            }
            out.push('\n');
        }
        out.push('\n');
    }
    let guns = &lab.guns;
    for (budget, trades) in guns.budgets.iter().zip(&guns.trades) {
        if guns.guns.is_empty() {
            break;
        }
        let _ = writeln!(
            out,
            "{budget} scrap a side: value the row kind destroyed per value it lost attacking the column structure"
        );
        let _ = write!(out, "{:11}", "");
        for gun in &guns.guns {
            let _ = write!(out, "{:>6}", short(gun));
        }
        out.push('\n');
        for (a, row) in trades.iter().enumerate() {
            let _ = write!(out, "{:11}", format!("{:?}", guns.kinds[a]));
            for trade in row {
                let _ = write!(out, "{:>6}", trade.cell());
            }
            out.push('\n');
        }
        out.push('\n');
    }
    if !lab.mixed.is_empty() {
        out.push_str("mixed: value the first army destroyed per value it lost\n");
        for fight in &lab.mixed {
            let _ = writeln!(out, "  {:52} {:>6}", fight.name, fight.trade.cell());
        }
    }
    out.push_str("- neither side damaged the other; inf: the row side lost nothing\n");
    out
}

/// Where `fight-lab --write` puts the bot's counter chart, from the
/// repository root.
pub const CHART: &str = "opponent/src/counters/chart.rs";

/// The bot's counter chart as this build measures it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measured {
    /// `trades[a][b]`: per mille of its value lost, the value `MEASURED[a]`
    /// destroys of `MEASURED[b]`.
    pub trades: Vec<Vec<u16>>,
    /// `against[a][g]`: per mille of its value lost, the value `MEASURED[a]`
    /// destroys of `GUNS[g]`.
    pub against: Vec<Vec<u16>>,
}

/// The bot's counter chart as this build measures it: each measured kind
/// against each other and against each measured structure, per mille of its
/// value lost, the value it destroys, the geometric mean over `BUDGETS`, held
/// within an eighth and eight times. Each pair of kinds is measured once and
/// its inverse read from the other side.
///
/// # Errors
/// An army or a block that does not fit the field.
pub fn chart(jobs: NonZeroUsize) -> Result<Measured> {
    let kinds = aftermine_opponent::MEASURED;
    let matrix = matrix(&kinds, &BUDGETS, jobs)?;
    let n = kinds.len();
    let mean = |trades: Vec<Trade>| -> u64 {
        geometric_mean(&trades.into_iter().map(ratio).collect::<Vec<_>>())
    };
    let mut trades = vec![vec![1_000_u16; n]; n];
    for a in 0..n {
        for b in (a + 1)..n {
            let mean = mean(matrix.trades.iter().map(|table| table[a][b]).collect());
            trades[a][b] = u16::try_from(mean).unwrap_or(u16::MAX);
            trades[b][a] = u16::try_from((1_000_000 + mean / 2) / mean).unwrap_or(u16::MAX);
        }
    }
    let guns = guns(&kinds, &aftermine_opponent::GUNS, &BUDGETS, jobs)?;
    let against = (0..n)
        .map(|a| {
            (0..guns.guns.len())
                .map(|g| {
                    let mean = mean(guns.trades.iter().map(|table| table[a][g]).collect());
                    u16::try_from(mean).unwrap_or(u16::MAX)
                })
                .collect()
        })
        .collect();
    Ok(Measured { trades, against })
}

/// The chart file for `measured`, stamped with this build's stats
/// fingerprint.
#[must_use]
pub fn chart_source(measured: &Measured) -> String {
    let stats = format!("{:016x}", aftermine_opponent::fingerprint());
    let stats: Vec<&str> = (0..4)
        .map(|group| &stats[group * 4..group * 4 + 4])
        .collect();
    let mut out = String::from(
        "//! Generated by `aftermine-driver fight-lab --write`; do not edit.\n\n\
         /// The fingerprint of the unit and structure stats the chart was\n\
         /// measured with.\n",
    );
    let _ = writeln!(
        out,
        "pub(super) const STATS: u64 = 0x{};\n",
        stats.join("_")
    );
    out.push_str(
        "/// `TRADES[a][b]`: per mille of its value lost, the value `MEASURED[a]`\n\
         /// destroys of `MEASURED[b]` at even prices.\n",
    );
    table(&mut out, "TRADES", &measured.trades);
    out.push_str(
        "\n/// `AGAINST[a][g]`: per mille of its value lost, the value `MEASURED[a]`\n\
         /// destroys of `GUNS[g]` at even prices.\n",
    );
    table(&mut out, "AGAINST", &measured.against);
    out
}

/// One generated table, `rows` by its first row's width.
fn table(out: &mut String, name: &str, rows: &[Vec<u16>]) {
    let (height, width) = (rows.len(), rows.first().map_or(0, Vec::len));
    let _ = writeln!(
        out,
        "pub(super) const {name}: [[u16; {width}]; {height}] = ["
    );
    for row in rows {
        let cells: Vec<String> = row.iter().map(u16::to_string).collect();
        let _ = writeln!(out, "    [{}],", cells.join(", "));
    }
    out.push_str("];\n");
}

/// Whether the checked-in chart is the one `measured` holds, for this
/// build's unit and structure stats.
///
/// # Errors
/// What differs.
pub fn check(measured: &Measured) -> Result<()> {
    let chart = aftermine_opponent::chart();
    if chart.stats != aftermine_opponent::fingerprint() {
        bail!(
            "the counter chart was measured with other unit or structure stats: \
             run `fight-lab --write`"
        );
    }
    let tables = [
        (
            "",
            chart
                .trades
                .iter()
                .map(|row| row.to_vec())
                .collect::<Vec<_>>(),
            &measured.trades,
        ),
        (
            " against structures",
            chart.against.iter().map(|row| row.to_vec()).collect(),
            &measured.against,
        ),
    ];
    for (which, checked, measured) in tables {
        if checked.len() != measured.len() {
            bail!("the counter chart{which} has another shape: run `fight-lab --write`");
        }
        for (a, (row, fresh)) in checked.iter().zip(measured).enumerate() {
            if row != fresh {
                bail!(
                    "the counter chart's {:?} row{which} is {row:?}, but this build measures \
                     {fresh:?}: run `fight-lab --write`",
                    aftermine_opponent::MEASURED[a]
                );
            }
        }
    }
    Ok(())
}

/// Per mille, what a fight's first army destroyed per value it lost, held
/// within an eighth and eight times; even when neither touched the other.
fn ratio(trade: Trade) -> u64 {
    match (trade.killed, trade.lost) {
        (0, 0) => 1_000,
        (_, 0) => 8_000,
        (killed, lost) => (killed * 1_000 / lost).clamp(125, 8_000),
    }
}

/// The geometric mean of `values`, rounded down.
fn geometric_mean(values: &[u64]) -> u64 {
    let product: u128 = values.iter().map(|value| u128::from(*value)).product();
    let n = u32::try_from(values.len()).unwrap_or(u32::MAX);
    // The largest root whose power stays within the product.
    let (mut low, mut high) = (0_u64, 8_001_u64);
    while low + 1 < high {
        let middle = low.midpoint(high);
        if u128::from(middle).pow(n) <= product {
            low = middle;
        } else {
            high = middle;
        }
    }
    low
}

/// A kind's first five letters, for a column head.
fn short(kind: &impl std::fmt::Debug) -> String {
    format!("{kind:?}").chars().take(5).collect()
}

/// One fight, `west` in seat 0 and `east` in seat 1, from the west's side.
fn play(west: Side<'_>, east: Side<'_>) -> Result<Trade> {
    let mut state = field(west, east)?
        .build()
        .context("staging a fight-lab field")?;
    let start = [worth(&state, 0), worth(&state, 1)];
    for tick in 0..TICK_LIMIT {
        if over(&state) {
            break;
        }
        let orders = if tick % ORDER_TICKS == 0 {
            orders(&state)
        } else {
            Vec::new()
        };
        state.tick(&orders);
    }
    Ok(Trade {
        killed: start[1] - worth(&state, 1),
        lost: start[0] - worth(&state, 0),
    })
}

/// The open field with `west` formed up on seat 0's side and `east` on
/// seat 1's. A block of structures forms up like an army, a free tile
/// between footprints.
fn field(west: Side<'_>, east: Side<'_>) -> Result<Scenario> {
    let wall = "#".repeat(WIDTH);
    let mut map = vec![wall.clone()];
    map.extend((2..HEIGHT).map(|_| format!("#{}#", ".".repeat(WIDTH - 2))));
    map.push(wall);
    let bottom = i32::try_from(HEIGHT).unwrap_or(i32::MAX) - 1;
    let mut units = Vec::new();
    let mut buildings = Vec::new();
    for (player, side) in [(0_u8, west), (1, east)] {
        if !side.army.is_empty() && !side.guns.is_empty() {
            bail!("a fight-lab side is an army or a block of structures, not both");
        }
        let step = side
            .guns
            .iter()
            .map(|(kind, _)| kind.size().0.max(kind.size().1))
            .max()
            .unwrap_or(1)
            + 1;
        let mut index = 0_i32;
        for &(kind, count) in side.guns {
            let (width, height) = kind.size();
            for _ in 0..count {
                let (column, row) = (index % PER_ROW, index / PER_ROW);
                let y = TOP + row * step;
                if y + height > bottom {
                    bail!(
                        "a block of {} structures does not fit the field",
                        side.guns.iter().map(|(_, count)| count).sum::<u32>()
                    );
                }
                let x = if player == 0 {
                    FRONT[0] - column * step - (width - 1)
                } else {
                    FRONT[1] + column * step
                };
                buildings.push(BuildingSpec { player, kind, x, y });
                index += 1;
            }
        }
        let army = side.army;
        let mut index = 0_i32;
        for &(kind, count) in army {
            for _ in 0..count {
                let (column, row) = (index % PER_ROW, index / PER_ROW);
                let y = TOP + row;
                if y >= bottom {
                    bail!(
                        "an army of {} units does not fit the field",
                        army_size(army)
                    );
                }
                let x = if player == 0 {
                    FRONT[0] - column
                } else {
                    FRONT[1] + column
                };
                units.push(UnitSpec { player, kind, x, y });
                index += 1;
            }
        }
    }
    let seat = |name: &str| PlayerSpec {
        name: name.to_owned(),
        team: None,
        scrap: 0,
        bot: false,
        bot_config: None,
    };
    Ok(Scenario {
        mode: ScenarioMode::Sandbox,
        name: "fight lab".to_owned(),
        map,
        levels: Vec::new(),
        players: vec![seat("west"), seat("east")],
        units,
        buildings,
        meta: None,
    })
}

fn army_size(army: &[(UnitKind, u32)]) -> u32 {
    army.iter().map(|(_, count)| count).sum()
}

/// What `player`'s units and structures are worth: each one's price by its
/// health left, a structure's price being all that was paid for its rungs.
fn worth(state: &State, player: u8) -> u64 {
    let units: u64 = state
        .units()
        .iter()
        .filter(|unit| unit.player == PlayerId(player))
        .map(|unit| {
            let stats = unit.kind.stats();
            u64::from(stats.cost) * u64::from(unit.hp) / u64::from(stats.max_hp.max(1))
        })
        .sum();
    let structures: u64 = state
        .buildings()
        .iter()
        .filter(|building| building.player == PlayerId(player))
        .map(|building| {
            let paid = u64::from(building.kind.invested_cost(building.tier));
            let max_hp = building.kind.tier_stats(building.tier).max_hp;
            paid * u64::from(building.hp) / u64::from(max_hp.max(1))
        })
        .sum();
    units + structures
}

/// Whether the fight is over: neither side can still hurt the other.
fn over(state: &State) -> bool {
    !hurts(state, 0, 1) && !hurts(state, 1, 0)
}

/// Whether one of `attacker`'s units or standing structures has a weapon
/// for one of `target`'s units or structures, which stand on the ground.
fn hurts(state: &State, attacker: u8, target: u8) -> bool {
    let units = state.units();
    let buildings = state.buildings();
    let mut domains: Vec<Domain> = units
        .iter()
        .filter(|unit| unit.player == PlayerId(target))
        .map(|unit| unit.kind.stats().domain)
        .collect();
    if buildings
        .iter()
        .any(|building| building.player == PlayerId(target))
    {
        domains.push(Domain::Ground);
    }
    let unit_weapons = units
        .iter()
        .filter(|unit| unit.player == PlayerId(attacker))
        .flat_map(|unit| unit.kind.stats().weapons);
    let gun_weapons = buildings
        .iter()
        .filter(|building| {
            building.player == PlayerId(attacker) && building.built() && building.hp > 0
        })
        .flat_map(|building| building.kind.tier_stats(building.tier).weapons);
    unit_weapons
        .chain(gun_weapons)
        .any(|weapon| domains.iter().any(|domain| weapon.targets.covers(*domain)))
}

/// Each side's units not already attacking hunt toward the other's middle
/// unit, or its middle structure once it has no units. A fresh order would
/// replace an attack under way, dropping a braced gun's aim or an
/// aircraft's run, so those keep at it.
fn orders(state: &State) -> Vec<PlayerCommand> {
    let units = state.units();
    [0_u8, 1]
        .into_iter()
        .filter_map(|player| {
            let own: Vec<_> = units
                .iter()
                .filter(|unit| unit.player == PlayerId(player))
                .filter(|unit| !matches!(unit.order, aftermine_sim::Order::Attack { .. }))
                .map(|unit| unit.id)
                .collect();
            let foes: Vec<_> = units
                .iter()
                .filter(|unit| unit.player != PlayerId(player))
                .map(aftermine_sim::Unit::tile)
                .collect();
            let blocks: Vec<_> = state
                .buildings()
                .iter()
                .filter(|building| building.player != PlayerId(player))
                .map(|building| building.anchor)
                .collect();
            let targets = if foes.is_empty() { blocks } else { foes };
            let goal = *targets.get(targets.len() / 2)?;
            (!own.is_empty()).then_some(PlayerCommand {
                player: PlayerId(player),
                command: Command::Hunt {
                    units: own,
                    goal,
                    queue: false,
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
