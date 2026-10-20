//! Match pacing from net worth: which side is ahead as a match goes on,
//! whether the eventual winner was ever well behind, how often the lead
//! changes hands, and how much of a match plays on once it is settled.
//! Evaluation rows and re-executed recordings feed the same measurement
//! through each seat's [`SeatLedger`].
//!
//! A side is a team, worth its seats' net worth summed. Shares are judged
//! against an equal split, so the same lines hold for two sides and for a
//! free-for-all. Everything is omniscient QA evidence that never reaches a
//! controller.

use crate::ledger::{SeatLedger, WORTH_PERIOD, worth_at};
use oxide_sim::{GameResult, TICKS_PER_SECOND};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// Ticks per minute.
const MINUTE: u64 = TICKS_PER_SECOND as u64 * 60;

/// Per mille of an equal split a side must hold to count as ahead: 55% of
/// two sides' worth.
pub const AHEAD_PER_MILLE: u64 = 1_100;

/// Per mille of an equal split a winner must have fallen to after the
/// opening for its win to count as a comeback: 40% of two sides' worth.
pub const COMEBACK_PER_MILLE: u64 = 800;

/// Ticks of the opening. Every side starts even, so leads and lows count
/// only from samples after it.
pub const OPENING_TICKS: u64 = 3 * MINUTE;

/// Minutes at which the side ahead is recorded.
pub const LEAD_MINUTES: [u64; 3] = [5, 10, 15];

/// One match's pacing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pacing {
    /// Teams in the match.
    pub sides: u8,
    /// The team that won, absent for a draw or an undecided match.
    pub winner: Option<u8>,
    /// Tick the measurement ended: the decision, or the recording's end.
    pub end: u64,
    /// The side ahead at each of [`LEAD_MINUTES`] the recording covers.
    pub leads: Vec<LeadAt>,
    /// Times the side ahead changed after the opening. A side that falls
    /// back to even keeps the lead until another side is ahead.
    pub lead_changes: u32,
    /// The winner's lowest share of all net worth after the opening, per
    /// mille.
    pub winner_low: Option<u32>,
    /// First sampled tick from which the winner was ahead at every sample
    /// to the end; absent when it was not ahead at the end.
    pub settled_at: Option<u64>,
}

/// The side ahead at one minute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LeadAt {
    /// The minute.
    pub minute: u64,
    /// The team ahead, absent when no side was.
    pub team: Option<u8>,
}

impl Pacing {
    /// Whether the winner fell to [`COMEBACK_PER_MILLE`] of an equal split
    /// after the opening.
    pub fn comeback(&self) -> bool {
        self.winner_low
            .is_some_and(|low| u64::from(low) * u64::from(self.sides) <= COMEBACK_PER_MILLE)
    }

    /// Per mille of the match played after the winner settled ahead.
    pub fn played_out(&self) -> Option<u32> {
        let settled = self.settled_at?;
        let share = (self.end - settled) * 1_000 / self.end.max(1);
        Some(u32::try_from(share).expect("a per mille share fits in u32"))
    }
}

/// Each team's summed net worth at one sampled tick.
struct Sample {
    tick: u64,
    worth: BTreeMap<u8, u64>,
    total: u64,
}

impl Sample {
    /// The team holding [`AHEAD_PER_MILLE`] of an equal split, if exactly
    /// one team holds the most.
    fn ahead(&self, sides: u64) -> Option<u8> {
        let most = self.worth.values().copied().max()?;
        let mut leaders = self.worth.iter().filter(|(_, worth)| **worth == most);
        let (&team, _) = leaders.next()?;
        let clear =
            leaders.next().is_none() && most * sides * 1_000 >= AHEAD_PER_MILLE * self.total;
        (clear && self.total > 0).then_some(team)
    }

    /// `team`'s share of all worth, per mille.
    fn share(&self, team: u8) -> u64 {
        self.worth.get(&team).copied().unwrap_or(0) * 1_000 / self.total.max(1)
    }
}

/// The pacing of a match whose seats, on `teams`, kept `ledgers` and which
/// ended in `result`; absent with fewer than two teams or no worth sampled.
pub fn pacing(ledgers: &[&SeatLedger], teams: &[u8], result: Option<GameResult>) -> Option<Pacing> {
    let first = ledgers.first()?;
    let distinct: BTreeSet<u8> = teams.iter().copied().collect();
    if ledgers.len() != teams.len() || distinct.len() < 2 {
        return None;
    }
    let sides = u8::try_from(distinct.len()).expect("teams fit in u8");
    let end = first.end;
    let periods = end.saturating_sub(first.start) / WORTH_PERIOD;
    let mut ticks: Vec<u64> = (1..=periods)
        .map(|period| first.start + period * WORTH_PERIOD)
        .filter(|tick| *tick < end)
        .collect();
    ticks.push(end);
    let samples: Vec<Sample> = ticks
        .into_iter()
        .filter_map(|tick| {
            let mut worth = BTreeMap::new();
            for (ledger, team) in ledgers.iter().zip(teams) {
                *worth.entry(*team).or_default() += worth_at(ledger, tick)?;
            }
            let total = worth.values().sum();
            Some(Sample { tick, worth, total })
        })
        .collect();
    if samples.is_empty() {
        return None;
    }
    let width = u64::from(sides);
    let leads = LEAD_MINUTES
        .iter()
        .filter_map(|&minute| {
            let tick = minute * MINUTE;
            let sample = samples.iter().rev().find(|sample| sample.tick <= tick)?;
            (tick <= end).then(|| LeadAt {
                minute,
                team: sample.ahead(width),
            })
        })
        .collect();
    let after: Vec<&Sample> = samples
        .iter()
        .filter(|sample| sample.tick > OPENING_TICKS)
        .collect();
    let mut lead_changes = 0;
    let mut holder = None;
    for sample in &after {
        if let Some(team) = sample.ahead(width) {
            if holder.is_some_and(|holder| holder != team) {
                lead_changes += 1;
            }
            holder = Some(team);
        }
    }
    let winner = match result {
        Some(GameResult::Victory { team }) if distinct.contains(&team) => Some(team),
        _ => None,
    };
    let winner_low = winner.and_then(|team| {
        after
            .iter()
            .map(|sample| sample.share(team))
            .min()
            .map(|low| u32::try_from(low).expect("a per mille share fits in u32"))
    });
    let settled_at = winner.and_then(|team| {
        after
            .iter()
            .rev()
            .take_while(|sample| sample.ahead(width) == Some(team))
            .last()
            .map(|sample| sample.tick)
    });
    Some(Pacing {
        sides,
        winner,
        end,
        leads,
        lead_changes,
        winner_low,
        settled_at,
    })
}

/// Pacing pooled over matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PacingSummary {
    /// Matches measured.
    pub matches: u32,
    /// Of those, the matches a team won.
    pub won: u32,
    /// At each of [`LEAD_MINUTES`]: won matches with a side ahead, and how
    /// many of those that side won.
    pub leads: Vec<LeadRow>,
    /// Won matches that were [`Pacing::comeback`]s.
    pub comebacks: u32,
    /// Lead changes summed over won matches.
    pub lead_changes: u32,
    /// Won matches whose lead changed at least once.
    pub changed: u32,
    /// Won matches whose winner settled ahead.
    pub settled: u32,
    /// Median per mille of those matches played after the winner settled.
    pub played_out: Option<u32>,
}

/// Won matches with a side ahead at one minute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LeadRow {
    /// The minute.
    pub minute: u64,
    /// Won matches with a side ahead then.
    pub ahead: u32,
    /// Of those, the matches that side won.
    pub held: u32,
}

/// Pools `games`.
pub fn summarize<'a>(games: impl IntoIterator<Item = &'a Pacing>) -> PacingSummary {
    let mut summary = PacingSummary {
        matches: 0,
        won: 0,
        leads: LEAD_MINUTES
            .iter()
            .map(|&minute| LeadRow {
                minute,
                ahead: 0,
                held: 0,
            })
            .collect(),
        comebacks: 0,
        lead_changes: 0,
        changed: 0,
        settled: 0,
        played_out: None,
    };
    let mut played_out = Vec::new();
    for game in games {
        summary.matches += 1;
        let Some(winner) = game.winner else {
            continue;
        };
        summary.won += 1;
        for lead in &game.leads {
            let (Some(team), Some(row)) = (
                lead.team,
                summary
                    .leads
                    .iter_mut()
                    .find(|row| row.minute == lead.minute),
            ) else {
                continue;
            };
            row.ahead += 1;
            row.held += u32::from(team == winner);
        }
        summary.comebacks += u32::from(game.comeback());
        summary.lead_changes += game.lead_changes;
        summary.changed += u32::from(game.lead_changes > 0);
        if let Some(share) = game.played_out() {
            summary.settled += 1;
            played_out.push(share);
        }
    }
    played_out.sort_unstable();
    summary.played_out = played_out.get(played_out.len() / 2).copied();
    summary
}

/// Writes `summary`'s lines, each starting with `indent`.
pub fn render(out: &mut String, summary: &PacingSummary, indent: &str) {
    let _ = writeln!(
        out,
        "{indent}{} of {} matches won; ahead means {}% of two sides' net worth, {} times an equal split",
        summary.won,
        summary.matches,
        AHEAD_PER_MILLE / 20,
        per_mille(AHEAD_PER_MILLE),
    );
    for row in &summary.leads {
        let _ = writeln!(
            out,
            "{indent}ahead at minute {:>2}: {} won matches, the side ahead won {} ({})",
            row.minute,
            row.ahead,
            row.held,
            percent(row.held, row.ahead),
        );
    }
    let _ = writeln!(
        out,
        "{indent}comebacks: {} of {} winners fell to {}% of two sides' worth ({} times an equal split) after minute {} ({})",
        summary.comebacks,
        summary.won,
        COMEBACK_PER_MILLE / 20,
        per_mille(COMEBACK_PER_MILLE),
        OPENING_TICKS / MINUTE,
        percent(summary.comebacks, summary.won),
    );
    let mean = if summary.won == 0 {
        "-".to_owned()
    } else {
        format!(
            "{:.2}",
            f64::from(summary.lead_changes) / f64::from(summary.won)
        )
    };
    let _ = writeln!(
        out,
        "{indent}lead changes: {mean} per won match; {} of {} had any ({})",
        summary.changed,
        summary.won,
        percent(summary.changed, summary.won),
    );
    let _ = writeln!(
        out,
        "{indent}settled: {} of {} winners stayed ahead to the end; median {} of the match played after that",
        summary.settled,
        summary.won,
        summary
            .played_out
            .map_or_else(|| "-".to_owned(), |share| format!("{}%", share / 10)),
    );
}

/// A per mille value as a decimal multiple, such as `1.1`.
fn per_mille(value: u64) -> String {
    format!("{}", value as f64 / 1_000.0)
}

fn percent(part: u32, whole: u32) -> String {
    if whole == 0 {
        "-".to_owned()
    } else {
        format!("{:.0}%", f64::from(part) * 100.0 / f64::from(whole))
    }
}

#[cfg(test)]
mod tests;
