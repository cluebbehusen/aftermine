//! Worlds stored beside a recording so a viewer can restore one instead of
//! re-simulating from the origin.
//!
//! Keyframes are a cache over the command log, never a second source of
//! truth: each carries its world's hash, a restore checks it, and a viewer
//! that simulates past one drops the record's keyframes if the hashes
//! disagree. A live match keeps one every [`KEYFRAME_EVERY`] absolute ticks;
//! a background thread encodes them, so the frame that reaches one pays only
//! for a clone of the world.

use crate::compression;
use anyhow::{Context, Result, ensure};
use oxide_sim::State;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, mpsc};

/// Absolute ticks between the keyframes a live match stores.
pub const KEYFRAME_EVERY: u64 = 1024;

/// Most keyframes a record carries: one per [`KEYFRAME_EVERY`] ticks across
/// [`crate::MAX_REPLAY_TICKS`].
pub const MAX_KEYFRAMES: usize = 2048;

/// Keyframes per chain: the first is compressed alone and each later one
/// against its predecessor, so a restore decompresses at most this many.
const CHAIN: usize = 8;

/// Largest decoded world accepted.
pub(crate) const MAX_DECODED: u64 = 64 << 20;

const LEVEL: i32 = 3;

/// How a keyframe's bytes were compressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Encoding {
    /// On its own.
    Full,
    /// Against the decoded bytes of the keyframe before it.
    Delta,
}

/// One compressed world, before commands stamped `tick` execute.
#[derive(Debug, Clone)]
pub(crate) struct Keyframe {
    pub(crate) tick: u64,
    pub(crate) hash: u64,
    pub(crate) encoding: Encoding,
    /// Length of the world's CBOR encoding.
    pub(crate) decoded: u64,
    pub(crate) bytes: Arc<[u8]>,
}

/// A record's keyframes, ascending by tick.
#[derive(Debug, Clone, Default)]
pub struct Keyframes(Vec<Keyframe>);

impl Keyframes {
    pub(crate) fn new(frames: Vec<Keyframe>) -> Self {
        Self(frames)
    }

    pub(crate) fn frames(&self) -> &[Keyframe] {
        &self.0
    }

    /// How many the record carries.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the record carries none.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The absolute tick of each keyframe, ascending.
    pub fn ticks(&self) -> impl Iterator<Item = u64> + '_ {
        self.0.iter().map(|frame| frame.tick)
    }

    /// Checks the index a record carries for keyframes of its own span: on
    /// the [`KEYFRAME_EVERY`] grid after `start` and at most `end`, chains
    /// no longer than a restore may decompress, and bounded sizes. Contents
    /// are checked when a keyframe is decoded.
    pub(crate) fn validate(&self, start: u64, end: u64) -> Result<()> {
        ensure!(self.0.len() <= MAX_KEYFRAMES, "too many keyframes");
        let mut last = start;
        let mut run = 0;
        for frame in &self.0 {
            ensure!(
                frame.tick > last && frame.tick <= end && frame.tick.is_multiple_of(KEYFRAME_EVERY),
                "keyframe at tick {} is off the record's grid",
                frame.tick
            );
            ensure!(
                frame.decoded <= MAX_DECODED,
                "keyframe exceeds its size bound"
            );
            run = match frame.encoding {
                Encoding::Full => 1,
                Encoding::Delta => {
                    ensure!((1..CHAIN).contains(&run), "keyframe chain too long");
                    run + 1
                }
            };
            last = frame.tick;
        }
        Ok(())
    }

    /// Index of the last keyframe at or before `tick`.
    pub(crate) fn at_or_before(&self, tick: u64) -> Option<usize> {
        self.0
            .partition_point(|frame| frame.tick <= tick)
            .checked_sub(1)
    }

    /// The hash recorded for the world at `tick`, if a keyframe holds it.
    pub(crate) fn hash_at(&self, tick: u64) -> Option<u64> {
        self.at_or_before(tick)
            .map(|index| &self.0[index])
            .filter(|frame| frame.tick == tick)
            .map(|frame| frame.hash)
    }

    /// Decodes keyframe `index`, checked against its index entry and against
    /// `origin`, a world of the same match.
    pub(crate) fn decode(&self, index: usize, origin: &State) -> Result<State> {
        decode(&self.0[..=index], origin)
    }

    /// Keeps the first `len` keyframes; chains stay whole from their start.
    pub(crate) fn truncate(&mut self, len: usize) {
        self.0.truncate(len);
    }
}

/// Decodes the last of `frames`, which ends a chain, against `origin`.
pub(crate) fn decode(frames: &[Keyframe], origin: &State) -> Result<State> {
    let first = frames
        .iter()
        .rposition(|frame| frame.encoding == Encoding::Full)
        .context("keyframe chain has no full start")?;
    ensure!(frames.len() - first <= CHAIN, "keyframe chain too long");
    let mut raw = Vec::new();
    for frame in &frames[first..] {
        let dictionary = (frame.encoding == Encoding::Delta).then_some(raw.as_slice());
        raw = compression::decompress(&frame.bytes, frame.decoded, MAX_DECODED, dictionary)?;
    }
    let frame = frames.last().context("no keyframe to decode")?;
    let mut reader = raw.as_slice();
    let state: State = ciborium::from_reader(&mut reader).context("keyframe world")?;
    ensure!(reader.is_empty(), "trailing keyframe bytes");
    ensure!(
        state.current_tick() == frame.tick && state.hash() == frame.hash,
        "keyframe world does not match its index"
    );
    crate::checkpoint::same_setup(origin, &state)?;
    Ok(state)
}

/// Compresses one record's worlds in tick order, each against its
/// predecessor except where a chain starts.
#[derive(Default)]
pub(crate) struct ChainEncoder {
    previous: Vec<u8>,
    run: usize,
}

impl ChainEncoder {
    /// A chain that never deltas: every world compressed alone.
    pub(crate) fn encode_full(state: &State) -> Result<Keyframe> {
        Self::default().encode(state)
    }

    pub(crate) fn encode(&mut self, state: &State) -> Result<Keyframe> {
        let mut raw = Vec::new();
        ciborium::into_writer(state, &mut raw)?;
        // A loader refuses a record carrying a larger keyframe outright.
        ensure!(
            raw.len() as u64 <= MAX_DECODED,
            "world exceeds the keyframe size bound"
        );
        let delta = (1..CHAIN).contains(&self.run);
        let dictionary = delta.then_some(self.previous.as_slice());
        let bytes = compression::compress(&raw, dictionary, LEVEL)?;
        let frame = Keyframe {
            tick: state.current_tick(),
            hash: state.hash(),
            encoding: if delta {
                Encoding::Delta
            } else {
                Encoding::Full
            },
            decoded: raw.len() as u64,
            bytes: bytes.into(),
        };
        self.run = if delta { self.run + 1 } else { 1 };
        self.previous = raw;
        Ok(frame)
    }
}

/// Keeps a live match's keyframes as it reaches them. Each clone of the
/// world goes to a background thread that encodes it, started by the first
/// keyframe.
#[derive(Default)]
pub struct KeyframeRecorder {
    sender: Option<mpsc::Sender<State>>,
    shared: Arc<Shared>,
    queued: usize,
    last: Option<u64>,
}

#[derive(Default)]
struct Shared {
    encoded: Mutex<Encoded>,
    ready: Condvar,
}

#[derive(Default)]
struct Encoded {
    frames: Vec<Keyframe>,
    /// The encoder has exited and will add nothing more.
    stopped: bool,
}

fn lock(shared: &Shared) -> MutexGuard<'_, Encoded> {
    shared
        .encoded
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

impl KeyframeRecorder {
    /// Called after each tick: keeps the world when it sits on the grid.
    pub fn observe(&mut self, state: &State) {
        let tick = state.current_tick();
        if tick == 0
            || !tick.is_multiple_of(KEYFRAME_EVERY)
            || self.last.is_some_and(|last| tick <= last)
            || self.queued >= MAX_KEYFRAMES
        {
            return;
        }
        self.last = Some(tick);
        if self.sender.is_none() {
            self.sender = self.spawn();
        }
        if let Some(sender) = &self.sender
            && sender.send(state.clone()).is_ok()
        {
            self.queued += 1;
        }
    }

    fn spawn(&self) -> Option<mpsc::Sender<State>> {
        let (sender, worlds) = mpsc::channel();
        let shared = Arc::clone(&self.shared);
        std::thread::Builder::new()
            .name("oxide-keyframes".into())
            .spawn(move || encode_all(&worlds, &shared))
            .ok()
            .map(|_| sender)
    }

    /// Every keyframe kept so far, available once its encoding finishes.
    pub fn pending(&self) -> PendingKeyframes {
        PendingKeyframes {
            shared: Arc::clone(&self.shared),
            count: self.queued,
        }
    }

    /// The keyframes already encoded, without waiting for the rest.
    pub fn encoded(&self) -> Keyframes {
        Keyframes(lock(&self.shared).frames.clone())
    }
}

/// Encodes worlds until the recorder hangs up. A failed encoding ends the
/// chain there: later keyframes would delta against a missing predecessor.
fn encode_all(worlds: &mpsc::Receiver<State>, shared: &Shared) {
    struct Stop<'a>(&'a Shared);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            lock(self.0).stopped = true;
            self.0.ready.notify_all();
        }
    }
    let _stop = Stop(shared);
    let mut encoder = ChainEncoder::default();
    for world in worlds {
        let Ok(frame) = encoder.encode(&world) else {
            return;
        };
        lock(shared).frames.push(frame);
        shared.ready.notify_all();
    }
}

/// Keyframes a recorder kept, some possibly still encoding. Waiting moves
/// off the frame thread with a save.
pub struct PendingKeyframes {
    shared: Arc<Shared>,
    count: usize,
}

impl PendingKeyframes {
    /// Blocks until every keyframe kept before this was taken is encoded,
    /// or the encoder has stopped.
    pub fn wait(self) -> Keyframes {
        let mut encoded = lock(&self.shared);
        while encoded.frames.len() < self.count && !encoded.stopped {
            encoded = self
                .shared
                .ready
                .wait(encoded)
                .unwrap_or_else(PoisonError::into_inner);
        }
        Keyframes(encoded.frames.iter().take(self.count).cloned().collect())
    }
}

#[cfg(test)]
mod tests;
