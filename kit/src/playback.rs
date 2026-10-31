//! Replay playback: a read-only walk through a recorded match.
//!
//! No recorder and no bots: this engine replays the log through raw
//! [`State::tick`] exactly as recorded. A seek restores the nearest stored
//! world at or before its target (a keyframe the recording carries, one the
//! background fill kept, or the origin) and re-simulates the rest. A seeked
//! position must hash the same as a straight run from the recording origin.

use crate::keyframes::{ChainEncoder, Keyframe, Keyframes};
use crate::{GameReplay, Recording};
use aftermine_sim::{SIM_VERSION, State};
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Finest spacing of the worlds the background fill keeps.
const FILL_EVERY: u64 = 128;

/// Most worlds the background fill keeps; a longer record spaces them out.
const MAX_FILLED: u64 = 512;

/// A loaded replay with a current position.
pub struct Playback {
    replay: Arc<GameReplay>,
    /// The world at the recording origin.
    origin: State,
    /// The world at the current position.
    pub state: State,
    /// Motion emitted by the most recently simulated tick.
    pub last_motion: Vec<aftermine_sim::GroundMotion>,
    /// Index into `replay.commands` of the first command not yet fed.
    next_cmd: usize,
    total: u64,
    /// Keyframes the recording carries, until one fails to decode or the
    /// fill finds one that disagrees with the log.
    stored: Keyframes,
    fill: Option<Fill>,
}

/// Where a restorable world comes from.
#[derive(Clone, Copy)]
enum Source {
    Stored(usize),
    Filled(usize),
}

/// Ticks between the worlds the fill keeps for a record spanning `span`
/// ticks: never denser than [`FILL_EVERY`], never more than [`MAX_FILLED`],
/// power-of-two so keyframe ticks land on the fill's grid too.
fn fill_spacing(span: u64) -> u64 {
    FILL_EVERY
        .max(span.div_ceil(MAX_FILLED))
        .next_power_of_two()
}

impl Playback {
    /// Validates and opens a replay at its origin. Cross-version records are
    /// refused because replays reproduce only on the sim that wrote them.
    pub fn load(replay: GameReplay) -> Result<Self> {
        Self::open(Recording {
            replay,
            keyframes: Keyframes::default(),
        })
    }

    /// [`Playback::load`] with the keyframes stored beside the replay.
    pub fn open(recording: Recording) -> Result<Self> {
        // Seeking is synchronous: a structurally valid file claiming an
        // absurd length would hang the viewer at the first End press.
        const MAX_INTERACTIVE_TICKS: u64 = 2_000_000;
        let Recording { replay, keyframes } = recording;
        replay
            .validate(Some(SIM_VERSION))
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let origin = crate::recording::initial_state(&replay)?;
        let total = crate::replay_duration(&replay);
        anyhow::ensure!(
            total <= MAX_INTERACTIVE_TICKS,
            "replay spans {total} ticks, beyond the {MAX_INTERACTIVE_TICKS}-tick interactive limit"
        );
        keyframes.validate(replay.start_tick(), total)?;
        Ok(Self {
            replay: Arc::new(replay),
            state: origin.clone(),
            origin,
            last_motion: Vec::new(),
            next_cmd: 0,
            total,
            stored: keyframes,
            fill: None,
        })
    }

    /// Starts re-simulating the whole record on a background thread,
    /// keeping compressed worlds a seek can restore and checking the
    /// stored keyframes against the log. Seeks are exact with or without
    /// it; the fill only shortens them.
    pub fn start_fill(&mut self) {
        if self.fill.is_some() {
            return;
        }
        let shared = Arc::new(FillShared::default());
        let work = FillWork {
            replay: Arc::clone(&self.replay),
            state: self.origin.clone(),
            total: self.total,
            spacing: fill_spacing(self.total - self.start()),
            stored: self.stored.clone(),
            shared: Arc::clone(&shared),
        };
        let worker = std::thread::Builder::new()
            .name("aftermine-replay-fill".into())
            .spawn(move || work.run())
            .ok();
        self.fill = Some(Fill { shared, worker });
    }

    /// First absolute tick available in the recording.
    pub fn start(&self) -> u64 {
        self.replay.start_tick()
    }

    /// The absolute end tick.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// The current position.
    pub fn position(&self) -> u64 {
        self.state.current_tick()
    }

    /// Whether the record is exhausted.
    pub fn at_end(&self) -> bool {
        self.position() >= self.total
    }

    /// Advances up to `ticks`, stopping at the end of the record, and
    /// returns every event the replayed world emitted on the way.
    pub fn advance(&mut self, ticks: u64) -> Vec<aftermine_sim::Event> {
        let mut events = Vec::new();
        for _ in 0..ticks {
            if self.at_end() {
                break;
            }
            events.extend(self.step());
        }
        events
    }

    /// Jumps to `target` (clamped to the record), bit-identical to having
    /// played straight there.
    pub fn seek(&mut self, target: u64) {
        self.seek_step(target, u64::MAX);
    }

    /// One budgeted slice of a seek toward `target`: restores the nearest
    /// stored world like [`Playback::seek`], then simulates at most `budget`
    /// ticks. Returns true when the target is reached; callers loop across
    /// frames so a long seek shows progress instead of freezing rendering.
    pub fn seek_step(&mut self, target: u64, budget: u64) -> bool {
        let target = target.clamp(self.start(), self.total);
        let position = self.position();
        if target < position || self.best(target).is_some_and(|(tick, _)| tick > position) {
            self.restore(target);
        }
        let mut ran = 0;
        while self.position() < target && ran < budget {
            self.step();
            ran += 1;
        }
        self.position() >= target
    }

    /// The latest restorable world at or before `target`, preferring the
    /// fill's single-frame worlds to a stored chain at the same tick.
    fn best(&self, target: u64) -> Option<(u64, Source)> {
        let stored = self
            .stored_trusted()
            .then(|| self.stored.at_or_before(target))
            .flatten()
            .map(|index| (self.stored.frames()[index].tick, Source::Stored(index)));
        let filled = self.fill.as_ref().and_then(|fill| {
            let frames = fill.shared.frames();
            let index = frames
                .partition_point(|frame| frame.tick <= target)
                .checked_sub(1)?;
            Some((frames[index].tick, Source::Filled(index)))
        });
        match (stored, filled) {
            (Some(stored), Some(filled)) => Some(if stored.0 > filled.0 { stored } else { filled }),
            (stored, filled) => stored.or(filled),
        }
    }

    fn stored_trusted(&self) -> bool {
        !self
            .fill
            .as_ref()
            .is_some_and(|fill| fill.shared.stored_rejected.load(Ordering::Relaxed))
    }

    /// Installs the best world at or before `target`. A world that fails to
    /// decode is forgotten and the next best tried, down to the origin.
    fn restore(&mut self, target: u64) {
        loop {
            let restored = match self.best(target) {
                None => Ok(self.origin.clone()),
                Some((_, Source::Stored(index))) => self
                    .stored
                    .decode(index, &self.origin)
                    .map_err(|_| Source::Stored(index)),
                Some((_, Source::Filled(index))) => {
                    let frame = self
                        .fill
                        .as_ref()
                        .map(|fill| fill.shared.frames()[index].clone());
                    frame.ok_or(Source::Filled(index)).and_then(|frame| {
                        crate::keyframes::decode(std::slice::from_ref(&frame), &self.origin)
                            .map_err(|_| Source::Filled(index))
                    })
                }
            };
            match restored {
                Ok(state) => {
                    self.state = state;
                    self.last_motion.clear();
                    self.next_cmd = self
                        .replay
                        .commands
                        .partition_point(|c| c.tick < self.state.current_tick());
                    return;
                }
                Err(Source::Stored(_)) => self.stored = Keyframes::default(),
                Err(Source::Filled(index)) => {
                    if let Some(fill) = &self.fill {
                        fill.shared.frames().remove(index);
                    }
                }
            }
        }
    }

    fn step(&mut self) -> Vec<aftermine_sim::Event> {
        let tick = self.state.current_tick();
        let mut commands = Vec::new();
        while let Some(c) = self.replay.commands.get(self.next_cmd) {
            if c.tick != tick {
                break;
            }
            commands.push(c.command.clone());
            self.next_cmd += 1;
        }
        // Raw tick, never a recorder: playback must not re-record.
        let report = self.state.tick(&commands);
        self.last_motion = report.movement;
        report.events
    }

    /// Waits for the fill to finish the record.
    #[cfg(test)]
    fn finish_fill(&mut self) {
        if let Some(worker) = self.fill.as_mut().and_then(|fill| fill.worker.take()) {
            worker.join().expect("the fill finishes");
        }
    }
}

/// The background fill and what it has kept so far.
struct Fill {
    shared: Arc<FillShared>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Fill {
    fn drop(&mut self) {
        self.shared.cancelled.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.join().ok();
        }
    }
}

#[derive(Default)]
struct FillShared {
    /// Single-frame worlds, ascending by tick.
    frames: Mutex<Vec<Keyframe>>,
    /// A stored keyframe disagreed with the world the log produces.
    stored_rejected: AtomicBool,
    cancelled: AtomicBool,
}

impl FillShared {
    fn frames(&self) -> MutexGuard<'_, Vec<Keyframe>> {
        self.frames.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct FillWork {
    replay: Arc<GameReplay>,
    state: State,
    total: u64,
    spacing: u64,
    stored: Keyframes,
    shared: Arc<FillShared>,
}

impl FillWork {
    fn run(mut self) {
        let start = self.state.current_tick();
        let mut feed = crate::ReplayPlayback::new(&self.replay);
        loop {
            if self.shared.cancelled.load(Ordering::Relaxed) {
                return;
            }
            let tick = self.state.current_tick();
            if self
                .stored
                .hash_at(tick)
                .is_some_and(|hash| hash != self.state.hash())
            {
                self.shared.stored_rejected.store(true, Ordering::Relaxed);
            }
            if tick > start && tick.is_multiple_of(self.spacing) {
                let Ok(frame) = ChainEncoder::encode_full(&self.state) else {
                    return;
                };
                self.shared.frames().push(frame);
            }
            if tick >= self.total {
                return;
            }
            feed.step(&mut self.state);
        }
    }
}

#[cfg(test)]
mod tests;
