//! The composition search: armies mixing up to three unit kinds fight at
//! equal scrap, looking for a mix nobody can beat.
//!
//! A round-robin over every mix would take about a million duels, so the
//! search grows a pool instead (the double oracle method). It solves the
//! blend of the pool's mixes that no pool member beats, tests every legal mix
//! against that blend, and adds the strongest challenger, until no challenger
//! gains anything. A blend of one mix is a dominant composition; a blend of
//! several means the strong mixes counter each other.
//!
//! An offline evaluation tool: it must never feed `oxide-opponent`, whose
//! specification forbids plan search inside the bot.

use crate::duel_lab::{self, Force, Formation, Layout};
use anyhow::Result;
use oxide_sim::UnitKind;
use serde::Serialize;
use std::fmt::Write as _;
use std::num::NonZeroUsize;

/// A challenger gaining less than this against the blend leaves it standing.
const SETTLED: f64 = 0.02;
/// Rounds before the search stops growing its pool.
const ROUND_LIMIT: usize = 40;
/// Fictitious-play iterations when solving the pool's blend.
const SOLVER_ITERATIONS: usize = 20_000;
/// Share below which a mix is left out of the reported blend.
const REPORTED_WEIGHT: f64 = 0.01;

/// One to three unit kinds splitting a budget evenly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mix(pub Vec<UnitKind>);

impl Mix {
    /// Every mix of one to `max_kinds` of `kinds` that a budget can field:
    /// each kind's share must buy at least one body. Smaller mixes first,
    /// each size in roster order.
    pub fn all(kinds: &[UnitKind], max_kinds: usize, budget: u32) -> Vec<Self> {
        let mut mixes = Vec::new();
        for size in 1..=max_kinds.min(kinds.len()) {
            let share = budget / u32::try_from(size).expect("mix sizes are small");
            let mut picks: Vec<usize> = (0..size).collect();
            loop {
                let mix: Vec<UnitKind> = picks.iter().map(|&i| kinds[i]).collect();
                if mix.iter().all(|kind| kind.stats().cost <= share) {
                    mixes.push(Self(mix));
                }
                // Advance to the next combination in lexicographic order.
                let Some(slot) = (0..size).rev().find(|&s| picks[s] < kinds.len() - size + s)
                else {
                    break;
                };
                picks[slot] += 1;
                for next in slot + 1..size {
                    picks[next] = picks[next - 1] + 1;
                }
            }
        }
        mixes
    }

    /// The army this mix fields at a budget: each kind gets an even share,
    /// rounded to the nearest whole body and at least one.
    pub fn force(&self, budget: u32) -> Force {
        let share = budget / u32::try_from(self.0.len()).expect("mix sizes are small");
        Force::Army(
            self.0
                .iter()
                .map(|&kind| {
                    let cost = kind.stats().cost.max(1);
                    (kind, ((share + cost / 2) / cost).max(1))
                })
                .collect(),
        )
    }

    /// The kinds joined with `+`.
    pub fn name(&self) -> String {
        self.0
            .iter()
            .map(|kind| kind.name())
            .collect::<Vec<_>>()
            .join(" + ")
    }
}

/// `a`'s mean score against `b` over every layout, formation and seat: each
/// leg's share of value kept by `a` minus `b`'s. Forces that cannot hurt
/// each other draw without a fight.
pub fn payoff(a: &Force, b: &Force, tick_limit: u32) -> Result<f64> {
    if !a.threatens(b) && !b.threatens(a) {
        return Ok(0.0);
    }
    let (spent_a, spent_b) = (f64::from(a.spent()), f64::from(b.spent()));
    let mut total = 0.0;
    let mut legs = 0.0;
    for layout in Layout::ALL {
        for formation in Formation::ALL {
            for a_west in [true, false] {
                let (west, east) = if a_west { (a, b) } else { (b, a) };
                let fought = duel_lab::fight(west, east, layout, formation, tick_limit)?;
                let (mine, theirs) = if a_west {
                    (fought.west, fought.east)
                } else {
                    (fought.east, fought.west)
                };
                total += mine.retained / spent_a - theirs.retained / spent_b;
                legs += 1.0;
            }
        }
    }
    Ok(total / legs)
}

/// Legs one payoff fights.
pub const LEGS_PER_PAYOFF: usize = Layout::ALL.len() * Formation::ALL.len() * 2;

/// Solve a symmetric zero-sum game by fictitious play: the blend of pool
/// members (rows of `payoffs`) that no member beats. Ties go to the lower
/// index, so the solve is deterministic.
fn solve(payoffs: &[Vec<f64>]) -> Vec<f64> {
    let k = payoffs.len();
    let mut plays = vec![0u32; k];
    let mut against = vec![0.0_f64; k];
    for _ in 0..SOLVER_ITERATIONS {
        let best = (0..k)
            .max_by(|&x, &y| against[x].total_cmp(&against[y]).then(y.cmp(&x)))
            .expect("the pool is never empty");
        plays[best] += 1;
        for (i, total) in against.iter_mut().enumerate() {
            *total += payoffs[i][best];
        }
    }
    plays
        .iter()
        .map(|&n| f64::from(n) / SOLVER_ITERATIONS as f64)
        .collect()
}

/// Where a search stopped.
#[derive(Debug, Clone, PartialEq)]
struct Settled {
    /// Candidate indices in the pool, in the order they joined.
    pool: Vec<usize>,
    /// Each pool member's share of the blend.
    weights: Vec<f64>,
    /// The strongest candidate against the blend, and its gain.
    challenger: (usize, f64),
    /// Rounds run.
    rounds: usize,
    /// `table[i][j]`: candidate `i`'s payoff against `j`, where evaluated.
    table: Vec<Vec<Option<f64>>>,
    /// Payoffs evaluated.
    evaluated: usize,
}

/// Grow a pool from `initial` over `n` candidates. `evaluate` answers a
/// batch of `(i, j)` pairs with `i`'s payoff against `j`; the table mirrors
/// each answer as `j`'s loss.
fn search(
    n: usize,
    initial: &[usize],
    mut evaluate: impl FnMut(&[(usize, usize)]) -> Result<Vec<f64>>,
) -> Result<Settled> {
    let mut table: Vec<Vec<Option<f64>>> = vec![vec![None; n]; n];
    for (i, row) in table.iter_mut().enumerate() {
        row[i] = Some(0.0);
    }
    let mut evaluated = 0;
    let mut fill = |table: &mut [Vec<Option<f64>>], pairs: Vec<(usize, usize)>| -> Result<usize> {
        let mut missing: Vec<(usize, usize)> = pairs
            .into_iter()
            .map(|(i, j)| (i.min(j), i.max(j)))
            .filter(|&(i, j)| table[i][j].is_none())
            .collect();
        missing.sort_unstable();
        missing.dedup();
        for (&(i, j), value) in missing.iter().zip(evaluate(&missing)?) {
            table[i][j] = Some(value);
            table[j][i] = Some(-value);
        }
        Ok(missing.len())
    };
    let mut pool = initial.to_vec();
    let mut rounds = 0;
    loop {
        rounds += 1;
        let pairs = pool
            .iter()
            .flat_map(|&i| pool.iter().map(move |&j| (i, j)))
            .collect();
        evaluated += fill(&mut table, pairs)?;
        let payoffs: Vec<Vec<f64>> = pool
            .iter()
            .map(|&i| {
                pool.iter()
                    .map(|&j| table[i][j].expect("pool payoffs are filled"))
                    .collect()
            })
            .collect();
        let weights = solve(&payoffs);
        let support: Vec<(usize, f64)> = pool
            .iter()
            .copied()
            .zip(weights.iter().copied())
            .filter(|&(_, w)| w > 0.0)
            .collect();
        let pairs = (0..n)
            .flat_map(|c| support.iter().map(move |&(s, _)| (c, s)))
            .collect();
        evaluated += fill(&mut table, pairs)?;
        let gain = |c: usize| -> f64 {
            support
                .iter()
                .map(|&(s, w)| w * table[c][s].expect("challenger payoffs are filled"))
                .sum()
        };
        let challenger = (0..n)
            .map(|c| (c, gain(c)))
            .max_by(|x, y| x.1.total_cmp(&y.1).then(y.0.cmp(&x.0)))
            .expect("there is at least one candidate");
        if challenger.1 <= SETTLED || pool.contains(&challenger.0) || rounds >= ROUND_LIMIT {
            return Ok(Settled {
                pool,
                weights,
                challenger,
                rounds,
                table,
                evaluated,
            });
        }
        pool.push(challenger.0);
    }
}

/// One mix in the blend, with what beats it hardest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Blended {
    /// The mix.
    pub mix: Mix,
    /// Its share of the blend.
    pub weight: f64,
    /// The candidate that scores best against it.
    pub counter: Mix,
    /// That counter's score against it.
    pub counter_score: f64,
}

/// The search at one budget.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Outcome {
    /// Scrap each side spends.
    pub budget: u32,
    /// Legal mixes at this budget.
    pub mixes: usize,
    /// Rounds the pool grew for.
    pub rounds: usize,
    /// Payoffs evaluated.
    pub payoffs: usize,
    /// The blend no mix beats, heaviest first.
    pub blend: Vec<Blended>,
    /// The strongest mix against the blend when the search stopped.
    pub challenger: Mix,
    /// Its gain against the blend; at or below 0.02 the blend holds.
    pub challenger_gain: f64,
    /// Whether one mix alone holds: a dominant composition.
    pub dominant: bool,
}

/// Run the search at one budget across at most `jobs` workers.
pub fn run(
    kinds: &[UnitKind],
    max_kinds: usize,
    budget: u32,
    tick_limit: u32,
    jobs: NonZeroUsize,
) -> Result<Outcome> {
    let mixes = Mix::all(kinds, max_kinds, budget);
    anyhow::ensure!(!mixes.is_empty(), "no mix fits a budget of {budget}");
    let forces: Vec<Force> = mixes.iter().map(|mix| mix.force(budget)).collect();
    let singles: Vec<usize> = (0..mixes.len())
        .filter(|&i| mixes[i].0.len() == 1)
        .collect();
    let initial = if singles.is_empty() { vec![0] } else { singles };
    let settled = search(mixes.len(), &initial, |pairs| {
        crate::pool::fan_out_bounded(pairs, jobs, |&(i, j)| {
            payoff(&forces[i], &forces[j], tick_limit)
        })
    })?;
    let mut blend: Vec<Blended> = settled
        .pool
        .iter()
        .zip(&settled.weights)
        .filter(|&(_, &w)| w >= REPORTED_WEIGHT)
        .map(|(&m, &weight)| {
            let (counter, counter_score) = (0..mixes.len())
                .filter_map(|c| settled.table[c][m].map(|v| (c, v)))
                .max_by(|x, y| x.1.total_cmp(&y.1).then(y.0.cmp(&x.0)))
                .expect("every blend member met every candidate");
            Blended {
                mix: mixes[m].clone(),
                weight,
                counter: mixes[counter].clone(),
                counter_score,
            }
        })
        .collect();
    blend.sort_by(|x, y| y.weight.total_cmp(&x.weight));
    let dominant = blend.len() == 1 && settled.challenger.1 <= SETTLED;
    Ok(Outcome {
        budget,
        mixes: mixes.len(),
        rounds: settled.rounds,
        payoffs: settled.evaluated,
        blend,
        challenger: mixes[settled.challenger.0].clone(),
        challenger_gain: settled.challenger.1,
        dominant,
    })
}

/// The human-readable summary of every budget searched.
pub fn render(outcomes: &[Outcome]) -> String {
    let mut out = String::new();
    for o in outcomes {
        let _ = writeln!(
            out,
            "composition search at {} scrap: {} mixes, {} rounds, {} payoffs ({} legs each)",
            o.budget, o.mixes, o.rounds, o.payoffs, LEGS_PER_PAYOFF
        );
        let verdict = if o.dominant {
            "one mix holds alone: a dominant composition".to_string()
        } else if o.challenger_gain <= SETTLED {
            format!("{} mixes hold together; none dominates", o.blend.len())
        } else {
            format!(
                "unsettled after {} rounds: {} still gains {:+.2}",
                o.rounds,
                o.challenger.name(),
                o.challenger_gain
            )
        };
        let _ = writeln!(out, "  {verdict}");
        for b in &o.blend {
            let _ = writeln!(
                out,
                "  {:>4.0}%  {}   (best counter: {} {:+.2})",
                b.weight * 100.0,
                b.mix.name(),
                b.counter.name(),
                b.counter_score
            );
        }
        let _ = writeln!(
            out,
            "  strongest challenger left: {} {:+.2}\n",
            o.challenger.name(),
            o.challenger_gain
        );
    }
    out
}

#[cfg(test)]
mod tests;
