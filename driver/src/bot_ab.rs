//! A/B games between this build's bot and the reference bot, a frozen copy
//! of an earlier build compiled in as `aftermine-opponent-ref`.
//!
//! Each pair plays one map, rung, stance and personality seed twice, the
//! bots exchanging seats, so neither seat nor seed favors one bot. A leg
//! scores 1 for the candidate's win, 0 for its loss and a half for a draw;
//! a leg still undecided at the tick limit scores the candidate's share of
//! the two seats' net worth. Pairs run a batch at a time, one personality
//! seed across every map, rung and stance, until a 99% interval on the
//! candidate's mean pair score settles the comparison, or the pair budget
//! runs out. The wide interval pays for looking after every batch.

use crate::ledger::{ImpactLedger, worth_at};
use aftermine_sim::scenario::{BotConfig, BotDifficulty, BotStance};
use aftermine_sim::{PlayerCommand, PlayerId, Scenario, State};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::num::NonZeroUsize;
use std::sync::Arc;

/// The two bots of an A/B test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Bot {
    /// This build's bot.
    Candidate,
    /// The frozen reference.
    Reference,
}

/// What an A/B test plays.
#[derive(Debug, Clone)]
pub struct AbPlan {
    /// Two-seat maps, each with its name.
    pub maps: Vec<(String, Scenario)>,
    /// Rungs both bots play at.
    pub difficulties: Vec<BotDifficulty>,
    /// Stances both bots play at.
    pub stances: Vec<BotStance>,
    /// The first batch's personality seed; batch `n` plays this plus `n`.
    pub seed_base: u64,
    /// The tick a leg stops at undecided.
    pub tick_limit: u64,
    /// The most pairs to play.
    pub max_pairs: usize,
    /// Half the width under which an interval around an even score counts
    /// as no difference: the smallest edge worth finding.
    pub margin: f64,
}

/// One game of a pair.
#[derive(Debug, Clone, Copy)]
struct Leg {
    map: usize,
    difficulty: BotDifficulty,
    stance: BotStance,
    seed: u64,
    candidate_seat: u8,
}

/// How one game ended.
#[derive(Debug, Clone, Serialize)]
pub struct LegResult {
    /// The map's name.
    pub map: String,
    /// Both bots' rung.
    pub difficulty: String,
    /// Both bots' stance.
    pub stance: String,
    /// Both bots' personality seed.
    pub seed: u64,
    /// The candidate's seat.
    pub candidate_seat: u8,
    /// The tick the game ended or was stopped at.
    pub ticks: u64,
    /// The winner, when there was one.
    pub winner: Option<Bot>,
    /// The candidate's net worth at the end.
    pub candidate_worth: u64,
    /// The reference's net worth at the end.
    pub reference_worth: u64,
    /// The candidate's score for the game, from 0 to 1.
    pub score: f64,
}

/// Where an A/B test stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The whole interval favors the candidate.
    Better,
    /// The whole interval favors the reference.
    Worse,
    /// The interval is narrower than the margin around an even score.
    NoDifference,
    /// Neither yet.
    Open,
}

/// The candidate's mean pair score with its 99% interval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Standing {
    /// Pairs played.
    pub pairs: usize,
    /// Mean pair score.
    pub mean: f64,
    /// The interval's low end.
    pub low: f64,
    /// The interval's high end.
    pub high: f64,
    /// What the interval says.
    pub verdict: Verdict,
}

/// The z value of a two-sided 99% interval.
const Z99: f64 = 2.576;

/// Where `scores`, one per pair, leave the comparison.
pub fn standing(scores: &[f64], margin: f64) -> Standing {
    let pairs = scores.len();
    if pairs < 2 {
        let mean = scores.first().copied().unwrap_or(0.5);
        return Standing {
            pairs,
            mean,
            low: 0.0,
            high: 1.0,
            verdict: Verdict::Open,
        };
    }
    let n = pairs as f64;
    let mean = scores.iter().sum::<f64>() / n;
    let variance = scores
        .iter()
        .map(|score| (score - mean).powi(2))
        .sum::<f64>()
        / (n - 1.0);
    let half = Z99 * (variance / n).sqrt();
    let (low, high) = (mean - half, mean + half);
    let verdict = if low > 0.5 {
        Verdict::Better
    } else if high < 0.5 {
        Verdict::Worse
    } else if low >= 0.5 - margin && high <= 0.5 + margin {
        Verdict::NoDifference
    } else {
        Verdict::Open
    };
    Standing {
        pairs,
        mean,
        low,
        high,
        verdict,
    }
}

/// One bot seated in a game.
enum Seat {
    Candidate(aftermine_opponent::Opponent, aftermine_opponent::OwnEvents),
    Reference(
        aftermine_opponent_ref::Opponent,
        aftermine_opponent_ref::OwnEvents,
    ),
}

impl Seat {
    fn act(&mut self, state: &State) -> Vec<PlayerCommand> {
        match self {
            Seat::Candidate(bot, events) => bot.act(state, events),
            Seat::Reference(bot, events) => bot.act(state, events),
        }
    }

    fn record(&mut self, player: PlayerId, events: &[aftermine_sim::Event]) {
        match self {
            Seat::Candidate(_, own) => own.record(player, events),
            Seat::Reference(_, own) => own.record(player, events),
        }
    }
}

/// Plays one leg to its result or `tick_limit`.
fn play(name: &str, scenario: &Scenario, leg: Leg, tick_limit: u64) -> Result<LegResult> {
    let mut scenario = scenario.clone();
    ensure!(
        scenario.players.len() == 2,
        "{name} seats {} players; an A/B game needs two",
        scenario.players.len()
    );
    let config = BotConfig::new(leg.difficulty, leg.stance, leg.seed);
    for seat in &mut scenario.players {
        seat.bot = true;
        seat.bot_config = Some(config);
    }
    let candidate_map = Arc::new(
        aftermine_opponent::MapModel::from_scenario(&scenario)
            .with_context(|| format!("briefing the candidate on {name}"))?,
    );
    let reference_map = Arc::new(
        aftermine_opponent_ref::MapModel::from_scenario(&scenario)
            .with_context(|| format!("briefing the reference on {name}"))?,
    );
    let mut seats: Vec<(PlayerId, Seat)> = (0..2u8)
        .map(|index| {
            let player = PlayerId(index);
            let seat = if index == leg.candidate_seat {
                Seat::Candidate(
                    aftermine_opponent::Opponent::new(player, config, Arc::clone(&candidate_map)),
                    aftermine_opponent::OwnEvents::default(),
                )
            } else {
                Seat::Reference(
                    aftermine_opponent_ref::Opponent::new(
                        player,
                        config,
                        Arc::clone(&reference_map),
                    ),
                    aftermine_opponent_ref::OwnEvents::default(),
                )
            };
            (player, seat)
        })
        .collect();
    let mut state = scenario.build().context("building the A/B scenario")?;
    let mut ledger = ImpactLedger::new(&state);
    while state.current_tick() < tick_limit && state.result().is_none() {
        let mut commands = Vec::new();
        for (_, seat) in &mut seats {
            commands.extend(seat.act(&state));
        }
        let report = state.tick(&commands);
        for (player, seat) in &mut seats {
            seat.record(*player, &report.events);
        }
        ledger.observe(&state, &report);
    }
    let ticks = state.current_tick();
    let winners = state.winners();
    let ledgers = ledger.finish(&state);
    let candidate = usize::from(leg.candidate_seat);
    let worth = |seat: usize| {
        ledgers
            .get(seat)
            .and_then(|l| worth_at(l, ticks))
            .unwrap_or(0)
    };
    let (candidate_worth, reference_worth) = (worth(candidate), worth(1 - candidate));
    let winner = match winners.as_slice() {
        [] => None,
        seats if seats.contains(&PlayerId(leg.candidate_seat)) => Some(Bot::Candidate),
        _ => Some(Bot::Reference),
    };
    let score = match (winner, state.result()) {
        (Some(Bot::Candidate), _) => 1.0,
        (Some(Bot::Reference), _) => 0.0,
        (None, Some(_)) => 0.5,
        (None, None) => {
            let total = candidate_worth + reference_worth;
            if total == 0 {
                0.5
            } else {
                candidate_worth as f64 / total as f64
            }
        }
    };
    Ok(LegResult {
        map: name.to_string(),
        difficulty: leg.difficulty.to_string(),
        stance: leg.stance.to_string(),
        seed: leg.seed,
        candidate_seat: leg.candidate_seat,
        ticks,
        winner,
        candidate_worth,
        reference_worth,
        score,
    })
}

/// The legs of batch `batch`: every map, rung and stance at one seed, each
/// pair's two legs side by side.
fn batch_legs(plan: &AbPlan, batch: u64) -> Vec<Leg> {
    let seed = plan.seed_base + batch;
    let mut legs = Vec::new();
    for map in 0..plan.maps.len() {
        for &difficulty in &plan.difficulties {
            for &stance in &plan.stances {
                for candidate_seat in 0..2 {
                    legs.push(Leg {
                        map,
                        difficulty,
                        stance,
                        seed,
                        candidate_seat,
                    });
                }
            }
        }
    }
    legs
}

/// Plays batches until the comparison settles or the budget runs out,
/// handing each batch's legs to `on_batch` with the standing after it.
pub fn run(
    plan: &AbPlan,
    jobs: NonZeroUsize,
    mut on_batch: impl FnMut(&[LegResult], &Standing) -> Result<()>,
) -> Result<Standing> {
    ensure!(!plan.maps.is_empty(), "an A/B test needs a map");
    ensure!(
        !plan.difficulties.is_empty() && !plan.stances.is_empty(),
        "an A/B test needs a rung and a stance"
    );
    let mut scores = Vec::new();
    let mut batch = 0;
    loop {
        let legs = batch_legs(plan, batch);
        let results = crate::pool::fan_out_bounded(&legs, jobs, |leg| {
            let (name, scenario) = &plan.maps[leg.map];
            play(name, scenario, *leg, plan.tick_limit)
        })?;
        scores.extend(
            results
                .chunks(2)
                .map(|pair| pair.iter().map(|leg| leg.score).sum::<f64>() / pair.len() as f64),
        );
        let now = standing(&scores, plan.margin);
        on_batch(&results, &now)?;
        if now.verdict != Verdict::Open || scores.len() >= plan.max_pairs {
            return Ok(now);
        }
        batch += 1;
    }
}

#[cfg(test)]
mod tests;
