//! One bot seat's decisions in a recorded match, recovered after the fact.
//!
//! A replay keeps every command, not why a bot sent it. Bots decide only
//! from their seat's fog view, their own failed orders, and memory built
//! from the same, all of which replaying reproduces, so re-running the
//! seat's bot alongside the record recovers each decision's trace. The
//! re-run must send exactly the commands the record holds for that seat:
//! a bot built from different code would trace a different bot, so the
//! first divergence is refused.

use aftermine_kit::controller::{SeatTrace, record_events, seat_controllers};
use aftermine_kit::{GameReplay, ReplayPlayback, bounded_replay_duration};
use aftermine_opponent::Launch;
use aftermine_sim::{PlayerCommand, PlayerId, SIM_VERSION};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::collections::BTreeMap;
use std::ops::RangeInclusive;

/// One decision of the traced seat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraceRow {
    /// The tick the seat decided on, before that tick's commands ran.
    pub tick: u64,
    /// What the seat knew, wanted, bought and ran.
    pub trace: SeatTrace,
    /// The attacks this decision launched: what each needed against the
    /// force it expected, and what it sent.
    pub launches: Vec<Launch>,
}

/// Re-runs `replay` from its start and hands `sink` every decision `seat`'s
/// bot made at a tick in `window`. Refuses a replay whose recorded commands
/// for `seat` differ from what its bot sends now, at the first tick they
/// differ; ticks after the window are not run.
pub fn trace(
    replay: &GameReplay,
    seat: PlayerId,
    window: RangeInclusive<u64>,
    mut sink: impl FnMut(&TraceRow) -> Result<()>,
) -> Result<()> {
    replay
        .validate(Some(SIM_VERSION))
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    ensure!(
        replay.origin.is_none(),
        "tracing re-runs the bots from the match's start, and this replay starts from a saved world"
    );
    let total = bounded_replay_duration(replay)?;
    let mut state = replay
        .setup
        .build()
        .context("building the replay's scenario")?;
    let mut bots = seat_controllers(&replay.setup).context("building the replay's bot seats")?;
    ensure!(
        bots.iter().any(|bot| bot.player() == seat),
        "seat {} has no bot in this replay",
        seat.0
    );
    let mut recorded: BTreeMap<u64, Vec<&PlayerCommand>> = BTreeMap::new();
    for timed in &replay.commands {
        if timed.command.player == seat {
            recorded.entry(timed.tick).or_default().push(&timed.command);
        }
    }
    let mut playback = ReplayPlayback::new(replay);
    for _ in 0..total {
        let tick = state.current_tick();
        if tick > *window.end() {
            break;
        }
        for bot in &mut bots {
            if bot.player() != seat {
                bot.act(&state);
                continue;
            }
            let (sent, decision) = bot.act_traced(&state);
            let held = recorded.get(&tick).map_or(&[][..], Vec::as_slice);
            // A client may also have commanded the seat; the bot's
            // commands are recorded after the client's.
            let ours = held
                .get(held.len().saturating_sub(sent.len())..)
                .unwrap_or(&[]);
            if ours.len() != sent.len()
                || ours.iter().zip(&sent).any(|(held, sent)| **held != *sent)
            {
                bail!(
                    "at tick {tick} seat {}'s bot sends {} command(s) where the replay holds {}; \
                     this build's bot is not the one that played (the replay was recorded by {})",
                    seat.0,
                    sent.len(),
                    held.len(),
                    replay.meta.build
                );
            }
            if let Some(trace) = decision.filter(|_| window.contains(&tick)) {
                sink(&TraceRow {
                    tick,
                    trace,
                    launches: bot.opponent().launches().to_vec(),
                })?;
            }
        }
        let report = playback.step(&mut state);
        record_events(&mut bots, &report);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
