//! Two runs of the same ladder legs, compared game by game: how many games
//! played identically, every game whose winner changed, the paired change in
//! the higher rung's share of net worth, and each rung's headline seat
//! measures on both sides.
//!
//! A game is one leg of one map, comparison, stance and run. Comparing the
//! same game in both runs removes the spread between games, so a change
//! shows with far fewer games than comparing two pooled shares.

use crate::bot_eval::EvaluationLeg;
use crate::bot_ladder::{ScoredLadderRow, Tally, Verdict, Won, build_report, winner};
use crate::ledger::{SeatLedger, WORTH_TICKS, mean_interval, team_shares};
use crate::seat_summary::SeatSummary;
use aftermine_sim::scenario::{BotDifficulty, BotStance};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

/// A ladder row with the digests that tell two runs of its game apart.
#[derive(Debug, Clone, Deserialize)]
pub struct DiffRow {
    /// The fields the ladder report reads.
    #[serde(flatten)]
    pub scored: ScoredLadderRow,
    /// Canonical final state hash.
    pub final_hash: String,
    /// Digest of the tick-stamped command stream.
    pub command_hash: String,
    /// Digest of the scenario and leg: the same in both runs of one game.
    pub evaluation_fingerprint: String,
}

/// Reads one run's rows.
pub fn load(path: &Path) -> Result<Vec<DiffRow>> {
    crate::bot_ladder::load_rows(&[path.to_path_buf()])
}

/// Both runs compared.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LadderDiff {
    /// Games in each run.
    pub games: u32,
    /// Games with the same commands and final state in both runs.
    pub identical: u32,
    /// Games that played differently.
    pub changed: u32,
    /// Comparisons, in the order the base rows first name them.
    pub comparisons: Vec<ComparisonDiff>,
    /// Each rung's seats, by manifest and then from the lowest rung up.
    pub rungs: Vec<RungDiff>,
}

/// One comparison in both runs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComparisonDiff {
    /// Manifest whose rows these are.
    pub manifest: String,
    /// The rung expected to win.
    pub higher: BotDifficulty,
    /// The rung it plays.
    pub lower: BotDifficulty,
    /// Games played.
    pub games: u32,
    /// Of those, games that played identically.
    pub identical: u32,
    /// The base run's legs and pairs.
    pub base: Tally,
    /// The base run against the gate.
    pub base_verdict: Verdict,
    /// The branch run's legs and pairs.
    pub branch: Tally,
    /// The branch run against the gate.
    pub branch_verdict: Verdict,
    /// Games whose winner changed, a decided game that became undecided
    /// included, in base row order.
    pub flips: Vec<Flip>,
    /// The paired change in the higher rung's share of net worth.
    pub worth: Vec<WorthChange>,
}

/// A game whose winner changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Flip {
    /// Scenario file stem.
    pub map: String,
    /// Stance of both seats.
    pub stance: BotStance,
    /// Seed cell.
    pub run: u64,
    /// Seat-pair leg.
    pub leg: EvaluationLeg,
    /// The rung that won on base, absent when no rung did.
    pub base_winner: Option<BotDifficulty>,
    /// Ticks the base game ran.
    pub base_ticks: u64,
    /// The rung that won on the branch, absent when no rung did.
    pub branch_winner: Option<BotDifficulty>,
    /// Ticks the branch game ran.
    pub branch_ticks: u64,
}

/// The mean change, branch less base, in the higher rung's share of net
/// worth at one tick over games with a share in both runs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorthChange {
    /// Tick compared.
    pub tick: u64,
    /// Games with a share at that tick in both runs.
    pub games: u32,
    /// Mean change in share, -1 to 1.
    pub mean: f64,
    /// 95% interval of the mean over games, absent with fewer than two.
    pub interval: Option<[f64; 2]>,
}

/// One rung's seats in both runs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RungDiff {
    /// Manifest whose rows these are.
    pub manifest: String,
    /// The rung.
    pub rung: BotDifficulty,
    /// Seat-games pooled on base.
    pub base_seat_games: u32,
    /// Seat-games pooled on the branch.
    pub branch_seat_games: u32,
    /// Failure incidents, reactivity and fights, side by side.
    pub measures: Vec<MeasureDiff>,
}

/// How a measure reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasureKind {
    /// Incidents per seat-game.
    PerSeatGame,
    /// A share, 0 to 1.
    Share,
    /// A ratio of two quantities.
    Ratio,
}

/// One measure's value in one run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Measured {
    /// The value, absent with nothing to measure.
    pub value: Option<f64>,
    /// 95% interval, for the measures that have one.
    pub interval: Option<[f64; 2]>,
    /// What the value is taken over: seat-games, situations that arose, or
    /// the fights' denominator.
    pub over: u64,
}

/// One headline measure in both runs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeasureDiff {
    /// What it measures.
    pub name: String,
    /// How it reads.
    pub kind: MeasureKind,
    /// On base.
    pub base: Measured,
    /// On the branch.
    pub branch: Measured,
    /// Branch less base, when both have a value.
    pub change: Option<f64>,
}

/// A game's place in its ladder: manifest, map, higher and lower rung,
/// stance, run and leg.
type GameKey = (String, String, String, String, String, u64, &'static str);

fn key(row: &ScoredLadderRow) -> GameKey {
    let label = &row.ladder;
    (
        label.manifest.clone(),
        label.map.clone(),
        label.higher.to_string(),
        label.lower.to_string(),
        label.stance.to_string(),
        label.run,
        row.leg.name(),
    )
}

/// The game in words, for refusals.
fn game(row: &ScoredLadderRow) -> String {
    let label = &row.ladder;
    format!(
        "{} {} {} against {}, {} run {}, {} leg",
        label.manifest,
        label.map,
        label.higher,
        label.lower,
        label.stance,
        label.run,
        row.leg.name()
    )
}

/// Each game's row index, refusing a game that repeats.
fn index(rows: &[DiffRow], side: &str) -> Result<BTreeMap<GameKey, usize>> {
    let mut games = BTreeMap::new();
    for (at, row) in rows.iter().enumerate() {
        ensure!(
            games.insert(key(&row.scored), at).is_none(),
            "{} repeats in the {side} rows",
            game(&row.scored)
        );
    }
    Ok(games)
}

/// The rung that won, from [`winner`].
fn rung(row: &ScoredLadderRow, won: Won) -> Option<BotDifficulty> {
    won.map(|higher| {
        if higher {
            row.ladder.higher
        } else {
            row.ladder.lower
        }
    })
}

/// The higher rung's per mille share of net worth at each of
/// [`WORTH_TICKS`].
fn higher_shares(row: &ScoredLadderRow) -> [Option<u32>; 4] {
    let ledgers: Vec<&SeatLedger> = row
        .evidence
        .iter()
        .map(|evidence| &evidence.ledger)
        .collect();
    let higher_seat = u8::from(row.leg == EvaluationLeg::Swapped);
    team_shares(&ledgers, &[0, 1], higher_seat)
}

/// One comparison's games while they are read.
#[derive(Default)]
struct ComparisonGames {
    games: u32,
    identical: u32,
    flips: Vec<Flip>,
    worth: [Vec<f64>; 4],
}

/// Pairs every game of `base` with the same game in `branch`, refusing a
/// game missing from either side, repeated in one, or played from a
/// different plan, then compares them.
pub fn diff(base: &[DiffRow], branch: &[DiffRow]) -> Result<LadderDiff> {
    let base_games = index(base, "base")?;
    let branch_games = index(branch, "branch")?;
    if let Some(row) = branch
        .iter()
        .find(|row| !base_games.contains_key(&key(&row.scored)))
    {
        bail!("{} is missing from the base rows", game(&row.scored));
    }
    let mut comparisons: BTreeMap<(String, String, String), ComparisonGames> = BTreeMap::new();
    let mut identical = 0;
    for row in base {
        let Some(&at) = branch_games.get(&key(&row.scored)) else {
            bail!("{} is missing from the branch rows", game(&row.scored));
        };
        let other = &branch[at];
        ensure!(
            row.evaluation_fingerprint == other.evaluation_fingerprint,
            "{} played a different match in each run: its evaluation fingerprints differ",
            game(&row.scored)
        );
        let same = row.command_hash == other.command_hash && row.final_hash == other.final_hash;
        identical += u32::from(same);
        let label = &row.scored.ladder;
        let games = comparisons
            .entry((
                label.manifest.clone(),
                label.higher.to_string(),
                label.lower.to_string(),
            ))
            .or_default();
        games.games += 1;
        games.identical += u32::from(same);
        let (before, after) = (winner(&row.scored)?, winner(&other.scored)?);
        if before != after {
            games.flips.push(Flip {
                map: label.map.clone(),
                stance: label.stance,
                run: label.run,
                leg: row.scored.leg,
                base_winner: rung(&row.scored, before),
                base_ticks: row.scored.duration_ticks,
                branch_winner: rung(&other.scored, after),
                branch_ticks: other.scored.duration_ticks,
            });
        }
        let shares = higher_shares(&row.scored)
            .into_iter()
            .zip(higher_shares(&other.scored));
        for (samples, pair) in games.worth.iter_mut().zip(shares) {
            if let (Some(before), Some(after)) = pair {
                samples.push((f64::from(after) - f64::from(before)) / 1_000.0);
            }
        }
    }
    let scored = |rows: &[DiffRow]| -> Vec<ScoredLadderRow> {
        rows.iter().map(|row| row.scored.clone()).collect()
    };
    let base_report = build_report(&scored(base)).context("scoring the base rows")?;
    let branch_report = build_report(&scored(branch)).context("scoring the branch rows")?;
    let mut diffs = Vec::new();
    for before in base_report.comparisons {
        let after = branch_report
            .comparisons
            .iter()
            .find(|each| {
                each.manifest == before.manifest
                    && each.higher == before.higher
                    && each.lower == before.lower
            })
            .context("every base comparison has branch games")?;
        let games = comparisons
            .remove(&(
                before.manifest.clone(),
                before.higher.to_string(),
                before.lower.to_string(),
            ))
            .unwrap_or_default();
        diffs.push(ComparisonDiff {
            manifest: before.manifest,
            higher: before.higher,
            lower: before.lower,
            games: games.games,
            identical: games.identical,
            base: before.overall,
            base_verdict: before.verdict,
            branch: after.overall.clone(),
            branch_verdict: after.verdict,
            flips: games.flips,
            worth: WORTH_TICKS
                .iter()
                .zip(&games.worth)
                .filter_map(|(tick, samples)| {
                    let (mean, interval) = mean_interval(samples)?;
                    Some(WorthChange {
                        tick: *tick,
                        games: u32::try_from(samples.len()).expect("game counts fit in u32"),
                        mean,
                        interval,
                    })
                })
                .collect(),
        });
    }
    let mut rungs = Vec::new();
    for before in &base_report.rungs {
        let after = branch_report
            .rungs
            .iter()
            .find(|each| each.manifest == before.manifest && each.rung == before.rung)
            .context("every base rung has branch seats")?;
        rungs.push(RungDiff {
            manifest: before.manifest.clone(),
            rung: before.rung,
            base_seat_games: before.summary.seat_legs,
            branch_seat_games: after.summary.seat_legs,
            measures: measures(&before.summary)
                .into_iter()
                .zip(measures(&after.summary))
                .map(|((name, kind, base), (_, _, branch))| MeasureDiff {
                    name,
                    kind,
                    base,
                    branch,
                    change: base
                        .value
                        .zip(branch.value)
                        .map(|(before, after)| after - before),
                })
                .collect(),
        });
    }
    let games = u32::try_from(base.len()).context("game counts fit in u32")?;
    Ok(LadderDiff {
        games,
        identical,
        changed: games - identical,
        comparisons: diffs,
        rungs,
    })
}

/// A rung's headline measures, always the same list in the same order:
/// failure incidents per seat-game, each situation's answered share, then
/// the fight measures.
fn measures(summary: &SeatSummary) -> Vec<(String, MeasureKind, Measured)> {
    let seat_games = u64::from(summary.seat_legs);
    let per_seat_game = |incidents: u64| Measured {
        value: (seat_games > 0).then(|| incidents as f64 / seat_games as f64),
        interval: None,
        over: seat_games,
    };
    let mut out: Vec<(String, MeasureKind, Measured)> = [
        ("repeated orders", summary.repeated_orders),
        ("abandoned sites", summary.abandoned_sites),
        ("starved production", summary.starved_production),
        ("stuck missions", summary.stuck_missions),
        ("idle army", summary.idle_army),
    ]
    .into_iter()
    .map(|(name, incidents)| {
        (
            format!("{name} per seat-game"),
            MeasureKind::PerSeatGame,
            per_seat_game(incidents),
        )
    })
    .collect();
    for (name, reactions) in summary.reactivity.items() {
        out.push((
            format!("{name} answered"),
            MeasureKind::Share,
            Measured {
                value: (reactions.arose > 0)
                    .then(|| reactions.answered as f64 / reactions.arose as f64),
                interval: None,
                over: reactions.arose,
            },
        ));
    }
    let fights = &summary.fights;
    let total = fights.total();
    let ratio = |ratio: Option<(f64, Option<[f64; 2]>)>, over: u64| Measured {
        value: ratio.map(|(value, _)| value),
        interval: ratio.and_then(|(_, interval)| interval),
        over,
    };
    out.extend([
        (
            "lost alone".to_owned(),
            MeasureKind::Share,
            ratio(
                fights.ratio(|leg| u64::from(leg.alone), |leg| u64::from(leg.losses)),
                u64::from(total.losses),
            ),
        ),
        (
            "met short".to_owned(),
            MeasureKind::Share,
            ratio(
                fights.ratio(|leg| u64::from(leg.met_short), |leg| u64::from(leg.met)),
                u64::from(total.met),
            ),
        ),
        (
            "came late".to_owned(),
            MeasureKind::Share,
            ratio(
                fights.ratio(|leg| u64::from(leg.late), |leg| u64::from(leg.arrivals)),
                u64::from(total.arrivals),
            ),
        ),
        (
            "killed per lost".to_owned(),
            MeasureKind::Ratio,
            ratio(fights.ratio(|leg| leg.killed, |leg| leg.lost), total.lost),
        ),
    ]);
    out
}

impl LadderDiff {
    /// The diff as text.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "bot-ladder-diff: {} games, {} identical, {} changed\n",
            self.games, self.identical, self.changed
        );
        for comparison in &self.comparisons {
            render_comparison(&mut out, comparison);
        }
        for rung in &self.rungs {
            render_rung(&mut out, rung);
        }
        out
    }
}

fn render_comparison(out: &mut String, comparison: &ComparisonDiff) {
    let _ = writeln!(
        out,
        "{}: {} against {}: {} games, {} identical, {} {} changed",
        comparison.manifest,
        comparison.higher,
        comparison.lower,
        comparison.games,
        comparison.identical,
        comparison.flips.len(),
        if comparison.flips.len() == 1 {
            "winner"
        } else {
            "winners"
        }
    );
    let share = |tally: &Tally, verdict: Verdict| match (tally.share, tally.wilson) {
        (Some(share), Some([low, high])) => format!(
            "{}/{} {:.0}% [{:.0}-{:.0}%] {}",
            tally.higher_wins,
            tally.decided,
            share * 100.0,
            low * 100.0,
            high * 100.0,
            verdict.name()
        ),
        _ => format!("no decided legs, {}", verdict.name()),
    };
    let _ = writeln!(
        out,
        "  {} share of decided legs: base {}; branch {}",
        comparison.higher,
        share(&comparison.base, comparison.base_verdict),
        share(&comparison.branch, comparison.branch_verdict)
    );
    let ending = |winner: Option<BotDifficulty>, ticks: u64| {
        let clock = crate::replay_summary::clock(ticks);
        winner.map_or_else(
            || format!("undecided at {clock}"),
            |rung| format!("{rung} won at {clock}"),
        )
    };
    for flip in &comparison.flips {
        let _ = writeln!(
            out,
            "  {} {} run {} {}: {} -> {}",
            flip.map,
            flip.stance,
            flip.run,
            flip.leg.name(),
            ending(flip.base_winner, flip.base_ticks),
            ending(flip.branch_winner, flip.branch_ticks)
        );
    }
    if !comparison.worth.is_empty() {
        let changes: Vec<String> = comparison
            .worth
            .iter()
            .map(|change| {
                let interval = change.interval.map_or_else(String::new, |[low, high]| {
                    format!(" [{:+.1} to {:+.1}]", low * 100.0, high * 100.0)
                });
                format!(
                    "{}k {}{interval} ({} games)",
                    change.tick / 1_000,
                    points(change.mean),
                    change.games
                )
            })
            .collect();
        let _ = writeln!(
            out,
            "  change in {} share of net worth, by game: {}",
            comparison.higher,
            changes.join(", ")
        );
    }
    out.push('\n');
}

/// A change in share as signed percentage points.
fn points(change: f64) -> String {
    format!("{:+.1} pts", change * 100.0)
}

fn render_rung(out: &mut String, rung: &RungDiff) {
    let _ = writeln!(
        out,
        "{}: {} seats, {} seat-games on base, {} on the branch",
        rung.manifest, rung.rung, rung.base_seat_games, rung.branch_seat_games
    );
    let _ = writeln!(
        out,
        "  {:<34} {:<26} {:<26} change",
        "measure", "base", "branch"
    );
    for measure in &rung.measures {
        if measure.base.value.is_none() && measure.branch.value.is_none() {
            continue;
        }
        let cell = |measured: &Measured| {
            let Some(value) = measured.value else {
                return "-".to_owned();
            };
            match measure.kind {
                MeasureKind::PerSeatGame => format!("{value:.2}"),
                MeasureKind::Share => {
                    let interval = measured.interval.map_or_else(String::new, |[low, high]| {
                        format!(
                            " [{:.0}-{:.0}%]",
                            low.max(0.0) * 100.0,
                            high.min(1.0) * 100.0
                        )
                    });
                    format!("{:.0}%{interval} of {}", value * 100.0, measured.over)
                }
                MeasureKind::Ratio => {
                    let interval = measured.interval.map_or_else(String::new, |[low, high]| {
                        format!(" [{:.2}-{high:.2}]", low.max(0.0))
                    });
                    format!("{value:.2}{interval}")
                }
            }
        };
        let change = measure.change.map_or_else(
            || "-".to_owned(),
            |change| match measure.kind {
                MeasureKind::Share => points(change),
                MeasureKind::PerSeatGame | MeasureKind::Ratio => format!("{change:+.2}"),
            },
        );
        let _ = writeln!(
            out,
            "  {:<34} {:<26} {:<26} {change}",
            measure.name,
            cell(&measure.base),
            cell(&measure.branch)
        );
    }
    out.push('\n');
}

#[cfg(test)]
mod tests;
