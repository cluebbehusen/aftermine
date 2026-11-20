//! Game-state tests: an agent's recorded game, restored at the moment
//! before it turned, played on by this build's bot against a scripted
//! continuation of the player.
//!
//! A replay holds every seat's commands, so the state at the fork rebuilds
//! exactly whatever bot recorded it. By default this build's bot watches
//! the recorded game up to the fork, deciding as it goes while the recorded
//! commands drive the game, so it reaches the fork with its memory of the
//! enemy; its own orders were never carried out, so its running plans can
//! be stale. A cold start gives it no memory at all. Then a [`Challenger`]
//! of the case's family takes over the player's seat and units. After the case's
//! window the bot is checked for what it kept: its Foundry, or its
//! Harvesters.

use crate::challengers::{Challenger, Family};
use crate::ledger::{ImpactLedger, worth_at};
use aftermine_kit::controller::{record_events, seat_controllers};
use aftermine_sim::{BuildingKind, PlayerId, State, UnitKind};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

/// What a case checks the bot kept at the end of its window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    /// The bot has not lost and still has a finished Foundry.
    Foundry,
    /// The bot has not lost and keeps at least half the Harvesters it had
    /// at the fork.
    Harvesters,
}

/// One game-state test.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// The case's name.
    pub name: String,
    /// The recorded game; a leading `~/` means the home directory.
    pub replay: String,
    /// The tick the game is restored at.
    pub at: u64,
    /// Ticks played after the fork.
    #[serde(rename = "for")]
    pub window: u64,
    /// The family that continues the player's seat.
    pub family: String,
    /// What the bot must keep.
    pub check: Check,
}

/// A manifest of cases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// What the manifest is for.
    #[serde(default)]
    pub note: String,
    /// The cases.
    pub cases: Vec<Case>,
}

/// How one case played out for one continuation seed.
#[derive(Debug, Clone, Serialize)]
pub struct ForkRow {
    /// The case's name.
    pub case: String,
    /// The continuation's seed.
    pub seed: u64,
    /// What the case checks.
    pub check: Check,
    /// Whether the bot kept it.
    pub pass: bool,
    /// The tick the game ended or the window closed.
    pub ticks: u64,
    /// Whether the bot lost within the window.
    pub bot_lost: bool,
    /// The bot's Harvesters at the fork and at the end.
    pub harvesters: (usize, usize),
    /// The bot's finished Foundries at the end.
    pub foundries: usize,
    /// The bot's share of the two seats' net worth at the fork and at the end.
    pub worth_share: (f64, f64),
}

/// `path` with a leading `~/` resolved against `$HOME`.
fn expand(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(path),
    }
}

/// The bot seat and the player seat of a recorded two-seat game.
fn seats(state: &State, scenario: &aftermine_sim::Scenario) -> Result<(PlayerId, PlayerId)> {
    ensure!(
        state.players().len() == 2,
        "a game-state test needs a two-seat game"
    );
    match (scenario.players[0].bot, scenario.players[1].bot) {
        (false, true) => Ok((PlayerId(1), PlayerId(0))),
        (true, false) => Ok((PlayerId(0), PlayerId(1))),
        _ => bail!("a game-state test needs one bot seat and one player seat"),
    }
}

fn harvesters(state: &State, seat: PlayerId) -> usize {
    state
        .units()
        .iter()
        .filter(|unit| unit.player == seat && unit.kind == UnitKind::Harvester)
        .count()
}

fn foundries(state: &State, seat: PlayerId) -> usize {
    state
        .buildings()
        .iter()
        .filter(|building| {
            building.player == seat && building.kind == BuildingKind::Foundry && building.built()
        })
        .count()
}

/// Each seat's share of net worth in `state`, from a ledger opened on it.
fn share_now(state: &State, bot: PlayerId) -> f64 {
    let ledgers = ImpactLedger::new(state).finish(state);
    let tick = state.current_tick();
    let worth = |seat: PlayerId| {
        ledgers
            .get(usize::from(seat.0))
            .and_then(|ledger| worth_at(ledger, tick))
            .unwrap_or(0)
    };
    share(worth(bot), worth(PlayerId(1 - bot.0)))
}

fn share(part: u64, other: u64) -> f64 {
    if part + other == 0 {
        0.5
    } else {
        part as f64 / (part + other) as f64
    }
}

/// Plays `case` once with continuation seed `seed`, the bot watching the
/// game up to the fork unless `cold`.
pub fn play(case: &Case, seed: u64, cold: bool) -> Result<ForkRow> {
    let family: Family = case
        .family
        .parse()
        .map_err(|error| anyhow::anyhow!("case {}: {error}", case.name))?;
    let path = expand(&case.replay);
    let replay = aftermine_kit::load_replay(&path)
        .with_context(|| format!("case {}: loading {}", case.name, path.display()))?;
    let scenario = &replay.setup;
    let mut bots = seat_controllers(scenario).context("seating the bot")?;
    if !cold {
        // Playback reports each tick after it runs, so the opening decision
        // is taken here, on the recording's first state.
        let opening = aftermine_kit::recording::initial_state(&replay)
            .with_context(|| format!("case {}: the recording's first state", case.name))?;
        for bot in &mut bots {
            let _ = bot.act(&opening);
        }
    }
    let mut state = aftermine_kit::runner::run_replay_observed(
        &replay,
        None,
        Some(case.at),
        false,
        |state, report| {
            if cold {
                return;
            }
            record_events(&mut bots, report);
            // The fork's own tick decides in the loop below.
            if state.current_tick() < case.at {
                for bot in &mut bots {
                    let _ = bot.act(state);
                }
            }
        },
    )
    .with_context(|| format!("case {}: restoring tick {}", case.name, case.at))?;
    ensure!(
        state.current_tick() == case.at,
        "case {}: the replay ends at {} before the fork at {}",
        case.name,
        state.current_tick(),
        case.at
    );
    let (bot, player) = seats(&state, scenario)?;
    ensure!(
        state.result().is_none(),
        "case {}: the match was over by the fork",
        case.name
    );
    let mut challenger = Challenger::new(family, player, seed, scenario)?;
    let harvesters_at_fork = harvesters(&state, bot);
    let share_at_fork = share_now(&state, bot);
    let mut ledger = ImpactLedger::new(&state);
    let end = case.at + case.window;
    while state.current_tick() < end && state.result().is_none() {
        let mut commands = challenger.act(&state);
        let bot_commands = aftermine_kit::bot_execution::commands(&state, &mut bots);
        // A player's commands go before the bots', as the shell and the
        // session stage them.
        commands.extend(bot_commands);
        let report = state.tick(&commands);
        record_events(&mut bots, &report);
        ledger.observe(&state, &report);
    }
    let ticks = state.current_tick();
    let bot_lost = state.result().is_some() && !state.winners().contains(&bot);
    let ledgers = ledger.finish(&state);
    let worth = |seat: PlayerId| {
        ledgers
            .get(usize::from(seat.0))
            .and_then(|ledger| worth_at(ledger, ticks))
            .unwrap_or(0)
    };
    let (kept_harvesters, kept_foundries) = (harvesters(&state, bot), foundries(&state, bot));
    let pass = !bot_lost
        && match case.check {
            Check::Foundry => kept_foundries > 0,
            Check::Harvesters => 2 * kept_harvesters >= harvesters_at_fork,
        };
    Ok(ForkRow {
        case: case.name.clone(),
        seed,
        check: case.check,
        pass,
        ticks,
        bot_lost,
        harvesters: (harvesters_at_fork, kept_harvesters),
        foundries: kept_foundries,
        worth_share: (share_at_fork, share(worth(bot), worth(player))),
    })
}

/// Plays every case of `manifest` with every seed in `seeds`, in case then
/// seed order, the bot watching each game up to its fork unless `cold`.
pub fn run(
    manifest: &Manifest,
    seeds: std::ops::Range<u64>,
    cold: bool,
    jobs: NonZeroUsize,
) -> Result<Vec<ForkRow>> {
    let games: Vec<(&Case, u64)> = manifest
        .cases
        .iter()
        .flat_map(|case| seeds.clone().map(move |seed| (case, seed)))
        .collect();
    crate::pool::fan_out_bounded(&games, jobs, |(case, seed)| play(case, *seed, cold))
}

/// One line per case: passes of games, and the bot's mean worth share at
/// the fork and at the end.
pub fn report(rows: &[ForkRow]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let mut names: Vec<&str> = Vec::new();
    for row in rows {
        if !names.contains(&row.case.as_str()) {
            names.push(&row.case);
        }
    }
    let (mut passed, mut total) = (0, 0);
    for name in names {
        let case: Vec<&ForkRow> = rows.iter().filter(|row| row.case == name).collect();
        let passes = case.iter().filter(|row| row.pass).count();
        let mean = |pick: fn(&ForkRow) -> f64| {
            case.iter().map(|row| pick(row)).sum::<f64>() / case.len() as f64
        };
        let _ = writeln!(
            out,
            "{name:<16} {passes}/{} pass | bot worth share {:.2} -> {:.2}",
            case.len(),
            mean(|row| row.worth_share.0),
            mean(|row| row.worth_share.1)
        );
        passed += passes;
        total += case.len();
    }
    let _ = writeln!(out, "all cases: {passed}/{total} pass");
    out
}

#[cfg(test)]
mod tests;
