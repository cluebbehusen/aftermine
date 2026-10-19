//! A match's command history, saved beside its session checkpoint so a
//! resumed match keeps recording onto its whole record.
//!
//! Loading checks the history's shape but never executes it. After a resume,
//! a background thread re-simulates it from the scenario: it rebuilds the
//! keyframes the resumed match is missing and checks that the history
//! reproduces the saved world. A history that does not is swapped for a record
//! that starts at the resume, as a save without one gets.

use crate::GameReplay;
use crate::build::BuildIdentity;
use crate::keyframes::{ChainEncoder, KEYFRAME_EVERY, KeyframeRecorder, Keyframes, MAX_KEYFRAMES};
use anyhow::{Result, ensure};
use chassis::replay::TimedCommand;
use oxide_sim::scenario::PlayerSpec;
use oxide_sim::{PlayerCommand, SIM_VERSION, Scenario, State};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// The commands a match recorded from its scenario's start.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionHistory {
    /// The seats as the record began. A seat a bot took over later is a bot
    /// in the session's scenario but not in its record's setup.
    players: Vec<PlayerSpec>,
    commands: Vec<TimedCommand<PlayerCommand>>,
}

impl SessionHistory {
    /// What `recorder` holds, when it records from its scenario's start; a
    /// record that starts at a world has no history to save.
    pub fn capture(recorder: &GameReplay) -> Option<Self> {
        recorder.origin.is_none().then(|| Self {
            players: recorder.setup.players.clone(),
            commands: recorder.commands.clone(),
        })
    }

    /// The record a match resumed at `saved` keeps writing onto, checked
    /// against the session's `scenario` without executing anything.
    pub fn recording(
        self,
        scenario: &Scenario,
        saved: &State,
        build: &BuildIdentity,
    ) -> Result<GameReplay> {
        ensure!(
            self.players.len() == scenario.players.len()
                && self
                    .players
                    .iter()
                    .zip(&scenario.players)
                    .all(|(began, seat)| {
                        PlayerSpec {
                            bot: seat.bot,
                            bot_config: seat.bot_config,
                            ..began.clone()
                        } == *seat
                    }),
            "history seats do not match the session"
        );
        let tick = saved.current_tick();
        ensure!(
            tick <= crate::MAX_REPLAY_TICKS,
            "history spans beyond the replay tick limit"
        );
        let mut setup = scenario.clone();
        setup.players = self.players;
        let mut replay = GameReplay::new(SIM_VERSION, build.label(), setup);
        replay.commands = self.commands;
        replay
            .validate(Some(SIM_VERSION))
            .map_err(|error| anyhow::anyhow!("history: {error}"))?;
        ensure!(
            replay.commands.last().is_none_or(|last| last.tick < tick),
            "history runs past the saved tick"
        );
        ensure!(
            replay
                .commands
                .iter()
                .all(|timed| usize::from(timed.command.player.0) < scenario.players.len()),
            "history command for a missing seat"
        );
        Ok(replay)
    }
}

/// A resumed match's history until a background thread has checked it.
pub struct PendingHistory {
    check: Check,
    /// The record from the resume, for a history that does not reproduce
    /// the saved world.
    fallback: GameReplay,
}

/// The keyframes of a history that reproduces the saved world, or none for
/// one that does not.
type Outcome = Option<Keyframes>;

#[derive(Default)]
struct Shared {
    outcome: Mutex<Option<Outcome>>,
    cancelled: AtomicBool,
}

impl Shared {
    fn outcome(&self) -> MutexGuard<'_, Option<Outcome>> {
        self.outcome.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Stops the rebuild once nothing waits for it.
struct Check(Arc<Shared>);

impl Drop for Check {
    fn drop(&mut self) {
        self.0.cancelled.store(true, Ordering::Relaxed);
    }
}

impl PendingHistory {
    /// Starts re-simulating `recorder`, the history of a match resumed at
    /// `saved`. Without a thread the history stays unchecked, as it does
    /// for a match that ends before its check.
    pub fn start(recorder: &GameReplay, saved: &State, fallback: GameReplay) -> Self {
        let shared = Arc::new(Shared::default());
        let rebuild = Rebuild {
            replay: recorder.clone(),
            tick: saved.current_tick(),
            hash: saved.hash(),
            shared: Arc::clone(&shared),
        };
        std::thread::Builder::new()
            .name("oxide-history".into())
            .spawn(move || rebuild.run())
            .ok();
        Self {
            check: Check(shared),
            fallback,
        }
    }

    /// Once the check has finished, hands a reproduced history's keyframes
    /// to `keyframes`, or swaps one that does not reproduce the saved world
    /// for the record from the resume, keeping the commands recorded since.
    /// Returns itself while the check runs.
    pub fn settle(
        self,
        recorder: &mut GameReplay,
        keyframes: &mut KeyframeRecorder,
    ) -> Option<Self> {
        let Some(outcome) = self.check.0.outcome().take() else {
            return Some(self);
        };
        if let Some(rebuilt) = outcome {
            keyframes.prepend(rebuilt);
        } else {
            let mut fallback = self.fallback;
            let resumed = fallback.start_tick();
            let since = recorder
                .commands
                .partition_point(|timed| timed.tick < resumed);
            fallback.commands = recorder.commands.split_off(since);
            *recorder = fallback;
        }
        None
    }

    /// Waits for the check to finish.
    #[cfg(test)]
    fn finish(&self) {
        while self.check.0.outcome().is_none() {
            std::thread::yield_now();
        }
    }
}

struct Rebuild {
    replay: GameReplay,
    tick: u64,
    hash: u64,
    shared: Arc<Shared>,
}

impl Rebuild {
    fn run(self) {
        if let Some(outcome) = self.rebuild() {
            *self.shared.outcome() = Some(outcome);
        }
    }

    /// Plays the history to the saved tick, keeping a keyframe at each grid
    /// tick on the way. None when cancelled.
    fn rebuild(&self) -> Option<Outcome> {
        let Ok(mut state) = self.replay.setup.build() else {
            return Some(None);
        };
        let mut feed = crate::ReplayPlayback::new(&self.replay);
        let mut encoder = Some(ChainEncoder::default());
        let mut frames = Vec::new();
        while state.current_tick() < self.tick {
            if self.shared.cancelled.load(Ordering::Relaxed) {
                return None;
            }
            feed.step(&mut state);
            if !state.current_tick().is_multiple_of(KEYFRAME_EVERY) || frames.len() >= MAX_KEYFRAMES
            {
                continue;
            }
            // A failed encoding ends the keyframes there, since later ones
            // would delta against a missing predecessor.
            match encoder.as_mut().map(|chain| chain.encode(&state)) {
                Some(Ok(frame)) => frames.push(frame),
                Some(Err(_)) => encoder = None,
                None => {}
            }
        }
        let reproduced = feed.is_finished() && state.hash() == self.hash;
        Some(reproduced.then(|| Keyframes::new(frames)))
    }
}

#[cfg(test)]
mod tests;
