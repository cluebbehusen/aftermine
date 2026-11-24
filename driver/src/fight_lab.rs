//! The fight lab: armies meet on open ground, and the report gives what
//! each destroyed of the other's value per value it lost. The matrix pits
//! one kind against another at even prices; the presets mix kinds the way
//! armies fight in games: artillery behind a front line with a spotter,
//! snipers behind a screen, a Mantle over a push. Each fight plays twice,
//! the armies trading seats, and sums both, so the seats' turn order favors
//! neither. Both sides hunt the other's middle unit, with no terrain,
//! building or micro: the lab reads how kinds trade, not how a match goes.

use crate::pool::fan_out_bounded;
use aftermine_sim::scenario::{PlayerSpec, Scenario, ScenarioMode, UnitSpec};
use aftermine_sim::stats::UnitKind;
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
    /// The mixed-army fights.
    pub mixed: Vec<Mixed>,
}

/// `first` against `second`, played from both seats and summed, from the
/// first army's side.
///
/// # Errors
/// An army that does not fit the field.
pub fn fight(first: &[(UnitKind, u32)], second: &[(UnitKind, u32)]) -> Result<Trade> {
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

/// The lab as tables: one matrix per budget, then the mixed fights.
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
            let _ = write!(out, "{:>6}", short(*kind));
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
    if !lab.mixed.is_empty() {
        out.push_str("mixed: value the first army destroyed per value it lost\n");
        for fight in &lab.mixed {
            let _ = writeln!(out, "  {:52} {:>6}", fight.name, fight.trade.cell());
        }
    }
    out.push_str("- neither side damaged the other; inf: the row side lost nothing\n");
    out
}

/// A kind's first five letters, for a column head.
fn short(kind: UnitKind) -> String {
    format!("{kind:?}").chars().take(5).collect()
}

/// One fight, `west` in seat 0 and `east` in seat 1, from the west's side.
fn play(west: &[(UnitKind, u32)], east: &[(UnitKind, u32)]) -> Result<Trade> {
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
/// seat 1's.
fn field(west: &[(UnitKind, u32)], east: &[(UnitKind, u32)]) -> Result<Scenario> {
    let wall = "#".repeat(WIDTH);
    let mut map = vec![wall.clone()];
    map.extend((2..HEIGHT).map(|_| format!("#{}#", ".".repeat(WIDTH - 2))));
    map.push(wall);
    let mut units = Vec::new();
    for (player, army) in [(0_u8, west), (1, east)] {
        let mut index = 0_i32;
        for &(kind, count) in army {
            for _ in 0..count {
                let (column, row) = (index % PER_ROW, index / PER_ROW);
                let y = TOP + row;
                if y >= i32::try_from(HEIGHT).unwrap_or(i32::MAX) - 1 {
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
        buildings: Vec::new(),
        meta: None,
    })
}

fn army_size(army: &[(UnitKind, u32)]) -> u32 {
    army.iter().map(|(_, count)| count).sum()
}

/// What `player`'s units are worth: each one's price by its health left.
fn worth(state: &State, player: u8) -> u64 {
    state
        .units()
        .iter()
        .filter(|unit| unit.player == PlayerId(player))
        .map(|unit| {
            let stats = unit.kind.stats();
            u64::from(stats.cost) * u64::from(unit.hp) / u64::from(stats.max_hp.max(1))
        })
        .sum()
}

/// Whether the fight is over: neither side can still hurt the other.
fn over(state: &State) -> bool {
    !hurts(state, 0, 1) && !hurts(state, 1, 0)
}

/// Whether one of `attacker`'s units has a weapon for one of `target`'s.
fn hurts(state: &State, attacker: u8, target: u8) -> bool {
    let units = state.units();
    units
        .iter()
        .filter(|unit| unit.player == PlayerId(attacker))
        .any(|unit| {
            units
                .iter()
                .filter(|other| other.player == PlayerId(target))
                .any(|other| {
                    let domain = other.kind.stats().domain;
                    unit.kind
                        .stats()
                        .weapons
                        .iter()
                        .any(|weapon| weapon.targets.covers(domain))
                })
        })
}

/// Each side hunts toward the other's middle unit.
fn orders(state: &State) -> Vec<PlayerCommand> {
    let units = state.units();
    [0_u8, 1]
        .into_iter()
        .filter_map(|player| {
            let own: Vec<_> = units
                .iter()
                .filter(|unit| unit.player == PlayerId(player))
                .map(|unit| unit.id)
                .collect();
            let foes: Vec<_> = units
                .iter()
                .filter(|unit| unit.player != PlayerId(player))
                .collect();
            let goal = foes.get(foes.len() / 2)?.tile();
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
