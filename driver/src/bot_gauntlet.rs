//! Scripted challengers against the bot: each game seats one [`Challenger`]
//! and one configured bot on a two-seat map, plays to the result or a tick
//! limit, and records who won and both seats' net worth. The challenger takes
//! seat 0 on even seeds and seat 1 on odd ones, so neither seat favors a
//! family.
//!
//! Results are omniscient QA evidence and never reach either player.

use crate::challengers::{Challenger, Family};
use crate::ledger::{ImpactLedger, worth_at};
use aftermine_kit::GameReplay;
use aftermine_kit::controller::{record_events, seat_controllers};
use aftermine_sim::scenario::{BotConfig, BotDifficulty, BotStance};
use aftermine_sim::{Event, PlayerId, SIM_VERSION, Scenario};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::fmt::Write as _;
use std::num::NonZeroUsize;
use std::path::PathBuf;

/// The bot's personality seed is the game's seed plus this.
pub const PERSONALITY_OFFSET: u64 = 9000;

/// What a gauntlet plays: every family on every map, rung, stance and seed.
#[derive(Debug, Clone)]
pub struct GauntletPlan {
    /// Families to play.
    pub families: Vec<Family>,
    /// Two-seat maps, each with its name.
    pub maps: Vec<(String, Scenario)>,
    /// The bot's rungs.
    pub difficulties: Vec<BotDifficulty>,
    /// The bot's stances.
    pub stances: Vec<BotStance>,
    /// Game seeds, from the first to one before the last.
    pub seeds: std::ops::Range<u64>,
    /// The tick a game stops at undecided.
    pub tick_limit: u64,
    /// Where each game's replay goes, if anywhere.
    pub replay_dir: Option<PathBuf>,
}

/// Who won a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Winner {
    /// The scripted player.
    Challenger,
    /// The bot.
    Bot,
    /// Nobody by the tick limit, or a draw.
    None,
}

/// One game.
#[derive(Debug, Clone, Copy)]
struct Game {
    family: Family,
    map: usize,
    difficulty: BotDifficulty,
    stance: BotStance,
    seed: u64,
}

/// How one game ended.
#[derive(Debug, Clone, Serialize)]
pub struct GameRow {
    /// The challenger's family.
    pub family: Family,
    /// The map's name.
    pub map: String,
    /// The bot's rung.
    pub difficulty: String,
    /// The bot's stance.
    pub stance: String,
    /// The game's seed: the challenger's, and the bot's personality seed
    /// less [`PERSONALITY_OFFSET`].
    pub seed: u64,
    /// The challenger's seat.
    pub challenger_seat: u8,
    /// The tick the game ended or was stopped at.
    pub ticks: u64,
    /// Who won.
    pub winner: Winner,
    /// The challenger's net worth at the end.
    pub challenger_worth: u64,
    /// The bot's net worth at the end.
    pub bot_worth: u64,
    /// Commands the challenger issued.
    pub challenger_commands: u64,
    /// Of those, commands the game rejected.
    pub challenger_rejected: u64,
}

/// Plays one game to its result or `tick_limit`.
fn play(
    name: &str,
    scenario: &Scenario,
    game: Game,
    tick_limit: u64,
    replay_dir: Option<&PathBuf>,
) -> Result<GameRow> {
    ensure!(
        scenario.players.len() == 2,
        "{name} seats {} players; a gauntlet game needs two",
        scenario.players.len()
    );
    let challenger_seat = u8::from(game.seed % 2 == 1);
    let bot_seat = 1 - challenger_seat;
    let mut scenario = scenario.clone();
    for (index, seat) in scenario.players.iter_mut().enumerate() {
        let bot = index == usize::from(bot_seat);
        seat.bot = bot;
        seat.bot_config = bot.then(|| {
            BotConfig::new(
                game.difficulty,
                game.stance,
                game.seed.wrapping_add(PERSONALITY_OFFSET),
            )
        });
    }
    let player = PlayerId(challenger_seat);
    let mut challenger = Challenger::new(game.family, player, game.seed, &scenario)?;
    let mut bots =
        seat_controllers(&scenario).with_context(|| format!("seating the bot on {name}"))?;
    let mut state = scenario.build().context("building the gauntlet scenario")?;
    let mut replay = replay_dir.map(|_| {
        let mut replay = GameReplay::new(
            SIM_VERSION,
            crate::build_identity().label(),
            scenario.clone(),
        );
        replay.meta.kind = Some("bot-gauntlet".into());
        replay.meta.description = Some(format!(
            "bot-gauntlet {} seed {} seat {challenger_seat} against {} {}",
            game.family, game.seed, game.difficulty, game.stance
        ));
        replay
    });
    let mut ledger = ImpactLedger::new(&state);
    let (mut issued, mut rejected) = (0_u64, 0_u64);
    while state.current_tick() < tick_limit && state.result().is_none() {
        let mut commands = challenger.act(&state);
        issued += commands.len() as u64;
        let bot_commands = aftermine_kit::bot_execution::commands(&state, &mut bots);
        if challenger_seat == 0 {
            commands.extend(bot_commands);
        } else {
            commands.splice(0..0, bot_commands);
        }
        let report = aftermine_kit::runner::record_and_tick(&mut state, &commands, replay.as_mut());
        record_events(&mut bots, &report);
        rejected += report
            .events
            .iter()
            .filter(
                |event| matches!(event, Event::CommandRejected { player: p, .. } if *p == player),
            )
            .count() as u64;
        ledger.observe(&state, &report);
    }
    let ticks = state.current_tick();
    let winners = state.winners();
    let ledgers = ledger.finish(&state);
    let worth = |seat: u8| {
        ledgers
            .get(usize::from(seat))
            .and_then(|l| worth_at(l, ticks))
            .unwrap_or(0)
    };
    let winner = if winners.contains(&player) {
        Winner::Challenger
    } else if winners.contains(&PlayerId(bot_seat)) {
        Winner::Bot
    } else {
        Winner::None
    };
    if let (Some(dir), Some(mut replay)) = (replay_dir, replay) {
        replay.meta.ticks = Some(ticks);
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let file = dir.join(format!(
            "{}-{name}-{}-{}-{}.json",
            game.family, game.difficulty, game.stance, game.seed
        ));
        replay
            .save(&file)
            .with_context(|| format!("saving {}", file.display()))?;
    }
    Ok(GameRow {
        family: game.family,
        map: name.to_string(),
        difficulty: game.difficulty.to_string(),
        stance: game.stance.to_string(),
        seed: game.seed,
        challenger_seat,
        ticks,
        winner,
        challenger_worth: worth(challenger_seat),
        bot_worth: worth(bot_seat),
        challenger_commands: issued,
        challenger_rejected: rejected,
    })
}

/// The families `names` lists, in order and without repeats: family names,
/// or `tuning` and `held-out` for those groups.
pub fn families(names: &[String]) -> Result<Vec<Family>> {
    let mut chosen: Vec<Family> = Vec::new();
    for name in names {
        let named = match name.as_str() {
            "tuning" => Family::TUNING.to_vec(),
            "held-out" => Family::HELD_OUT.to_vec(),
            "yardstick" => Family::YARDSTICK.to_vec(),
            name => vec![name.parse()?],
        };
        for family in named {
            if !chosen.contains(&family) {
                chosen.push(family);
            }
        }
    }
    Ok(chosen)
}

/// Every game of `plan`, family by family.
fn games(plan: &GauntletPlan) -> Vec<Game> {
    let mut games = Vec::new();
    for &family in &plan.families {
        for map in 0..plan.maps.len() {
            for &difficulty in &plan.difficulties {
                for &stance in &plan.stances {
                    for seed in plan.seeds.clone() {
                        games.push(Game {
                            family,
                            map,
                            difficulty,
                            stance,
                            seed,
                        });
                    }
                }
            }
        }
    }
    games
}

/// Plays every game of `plan`, at most `jobs` at once, and returns their
/// rows in plan order.
pub fn run(plan: &GauntletPlan, jobs: NonZeroUsize) -> Result<Vec<GameRow>> {
    ensure!(!plan.families.is_empty(), "a gauntlet needs a family");
    ensure!(!plan.maps.is_empty(), "a gauntlet needs a map");
    ensure!(
        !plan.difficulties.is_empty() && !plan.stances.is_empty(),
        "a gauntlet needs a rung and a stance"
    );
    ensure!(!plan.seeds.is_empty(), "a gauntlet needs a seed");
    let games = games(plan);
    crate::pool::fan_out_bounded(&games, jobs, |game| {
        let (name, scenario) = &plan.maps[game.map];
        play(
            name,
            scenario,
            *game,
            plan.tick_limit,
            plan.replay_dir.as_ref(),
        )
    })
}

/// One family's record at one rung.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Record {
    /// The family.
    pub family: Family,
    /// The bot's rung.
    pub difficulty: String,
    /// Games played.
    pub games: u32,
    /// Games the challenger won.
    pub challenger_wins: u32,
    /// Games the bot won.
    pub bot_wins: u32,
    /// Games with no winner.
    pub undecided: u32,
    /// The challenger's share of decided games.
    pub share: Option<f64>,
    /// The 95% Wilson interval of that share.
    pub wilson: Option<[f64; 2]>,
}

/// Pools `rows` by family and rung, in first-seen order.
pub fn records(rows: &[GameRow]) -> Vec<Record> {
    let mut records: Vec<Record> = Vec::new();
    for row in rows {
        let known = records
            .iter()
            .position(|r| r.family == row.family && r.difficulty == row.difficulty);
        let index = known.unwrap_or_else(|| {
            records.push(Record {
                family: row.family,
                difficulty: row.difficulty.clone(),
                games: 0,
                challenger_wins: 0,
                bot_wins: 0,
                undecided: 0,
                share: None,
                wilson: None,
            });
            records.len() - 1
        });
        let record = &mut records[index];
        record.games += 1;
        match row.winner {
            Winner::Challenger => record.challenger_wins += 1,
            Winner::Bot => record.bot_wins += 1,
            Winner::None => record.undecided += 1,
        }
    }
    for record in &mut records {
        let decided = record.challenger_wins + record.bot_wins;
        if decided > 0 {
            record.share = Some(f64::from(record.challenger_wins) / f64::from(decided));
            record.wilson = Some(crate::bot_ladder::wilson(record.challenger_wins, decided));
        }
    }
    records
}

/// The records as a table.
pub fn report(records: &[Record]) -> String {
    let mut out = String::from(
        "family              rung       games  challenger  bot  undecided  share  95% interval\n",
    );
    for r in records {
        let share = match (r.share, r.wilson) {
            (Some(share), Some([low, high])) => format!("{share:>5.2}  [{low:.2}, {high:.2}]"),
            _ => "    -  -".to_string(),
        };
        let _ = writeln!(
            out,
            "{:<19} {:<10} {:>5}  {:>10}  {:>3}  {:>9}  {share}",
            r.family.name(),
            r.difficulty,
            r.games,
            r.challenger_wins,
            r.bot_wins,
            r.undecided,
        );
    }
    out
}

#[cfg(test)]
mod tests;
