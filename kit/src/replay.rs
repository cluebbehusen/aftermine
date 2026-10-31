//! Oxide's replay file boundary.
//!
//! A replay is plain JSON, as the driver writes for people and agents to
//! read, or an `.oxreplay` recording: eight-byte `OXIDEREP` magic, a
//! little-endian 32-bit header length, a JSON header, one checksummed
//! Zstandard frame holding the same JSON replay, then the keyframe frames
//! the header indexes, in order. Both load through the same strict wire.

use crate::GameReplay;
use crate::keyframes::{Encoding, Keyframe, Keyframes};
use aftermine_sim::{PlayerCommand, Scenario};
use chassis::replay::{
    MAX_REPLAY_BYTES, MAX_REPLAY_COMMANDS, Replay, ReplayError, ReplayMeta, TimedCommand,
};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

/// `.oxreplay` format revision.
const FORMAT: u32 = 1;
const MAGIC: &[u8; 8] = b"OXIDEREP";
const MAX_HEADER: u64 = 1 << 20;
/// Largest `.oxreplay` file accepted. Keyframes that would push a recording
/// past it are left out when it is written.
const MAX_RECORDING_BYTES: u64 = 256 << 20;
/// The record compresses once, on a save worker. This level takes about a
/// tenth of a second on a half-hour eight-seat match and saves a third over
/// the fastest one; higher levels cost several times more for little.
const RECORD_LEVEL: i32 = 15;

/// The replay envelope, strict about unknown fields; [`Scenario`] is strict
/// about its own.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayWire {
    meta: ReplayMeta,
    setup: Scenario,
    commands: Vec<TimedCommand<PlayerCommand>>,
    #[serde(default)]
    origin: Option<crate::recording::WorldOrigin>,
}

impl From<ReplayWire> for GameReplay {
    fn from(wire: ReplayWire) -> Self {
        let ReplayWire {
            meta,
            setup,
            commands,
            origin,
        } = wire;
        Replay {
            meta,
            setup,
            commands,
            origin,
        }
    }
}

/// Absolute end tick of a replay: its recorded duration, or one past its last
/// command when the metadata omits it.
pub fn replay_duration(replay: &GameReplay) -> u64 {
    replay.meta.ticks.unwrap_or_else(|| {
        replay
            .commands
            .last()
            .map_or(replay.start_tick(), |command| {
                command.tick.saturating_add(1)
            })
    })
}

/// [`replay_duration`], refused beyond [`crate::MAX_REPLAY_TICKS`]. The bound
/// is on the effective duration, so a replay without duration metadata is
/// bounded by its final command's tick.
pub fn bounded_replay_duration(replay: &GameReplay) -> anyhow::Result<u64> {
    let total = replay_duration(replay);
    anyhow::ensure!(
        total <= crate::MAX_REPLAY_TICKS,
        "replay spans {total} ticks, beyond the {}-tick bound",
        crate::MAX_REPLAY_TICKS
    );
    Ok(total)
}

/// Feeds a replay's recorded commands back into a state one tick at a time.
pub struct ReplayPlayback<'a> {
    cursor: chassis::replay::ReplayCursor<'a, PlayerCommand>,
}

impl<'a> ReplayPlayback<'a> {
    /// Starts at the replay's first recorded command.
    pub fn new(replay: &'a GameReplay) -> Self {
        Self {
            cursor: replay.cursor(),
        }
    }

    /// Runs the state's current tick with the commands recorded for it.
    pub fn step(&mut self, state: &mut aftermine_sim::State) -> aftermine_sim::TickReport {
        let commands: Vec<PlayerCommand> = self
            .cursor
            .take_tick(state.current_tick())
            .iter()
            .map(|timed| timed.command.clone())
            .collect();
        state.tick(&commands)
    }

    /// Whether every recorded command has been played. A full-length
    /// playback that leaves commands behind means the replay's duration
    /// metadata is wrong.
    pub fn is_finished(&self) -> bool {
        self.cursor.is_finished()
    }
}

/// Loads an Oxide replay from disk, JSON or `.oxreplay`, without its
/// keyframes.
pub fn load_replay(path: impl AsRef<Path>) -> Result<GameReplay, ReplayError> {
    load_recording(path).map(|recording| recording.replay)
}

/// A replay with the keyframes stored beside it.
pub struct Recording {
    /// The authoritative record.
    pub replay: GameReplay,
    /// Worlds a viewer may restore instead of re-simulating: a cache over
    /// `replay`, checked against it as it plays.
    pub keyframes: Keyframes,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u32,
    record: Frame,
    keyframes: Vec<IndexEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    compressed: u64,
    decoded: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexEntry {
    tick: u64,
    hash: u64,
    encoding: Encoding,
    compressed: u64,
    decoded: u64,
}

fn invalid(error: impl std::fmt::Display) -> ReplayError {
    ReplayError::Invalid(error.to_string())
}

/// Writes `replay` and as many of `keyframes` as fit the file's size bound
/// as an `.oxreplay` recording, atomically.
pub fn save_recording(
    path: impl AsRef<Path>,
    replay: &GameReplay,
    keyframes: &Keyframes,
) -> Result<(), ReplayError> {
    let json = serde_json::to_vec(replay)?;
    if json.len() > MAX_REPLAY_BYTES {
        return Err(invalid("replay exceeds its byte bound"));
    }
    let record = crate::compression::compress(&json, None, RECORD_LEVEL).map_err(invalid)?;
    let mut room = MAX_RECORDING_BYTES.saturating_sub(12 + MAX_HEADER + record.len() as u64);
    let kept = keyframes
        .frames()
        .iter()
        .take_while(|frame| {
            let size = frame.bytes.len() as u64;
            let fits = size <= room;
            room = room.saturating_sub(size);
            fits
        })
        .count();
    let mut keyframes = keyframes.clone();
    keyframes.truncate(kept);
    let header = Header {
        version: FORMAT,
        record: Frame {
            compressed: record.len() as u64,
            decoded: json.len() as u64,
        },
        keyframes: keyframes
            .frames()
            .iter()
            .map(|frame| IndexEntry {
                tick: frame.tick,
                hash: frame.hash,
                encoding: frame.encoding,
                compressed: frame.bytes.len() as u64,
                decoded: frame.decoded,
            })
            .collect(),
    };
    let header = serde_json::to_vec(&header)?;
    let header_len = u32::try_from(header.len())
        .ok()
        .filter(|len| u64::from(*len) <= MAX_HEADER)
        .ok_or_else(|| invalid("replay header exceeds its byte bound"))?;
    chassis::fsx::write_atomic(path, |writer| {
        writer.write_all(MAGIC)?;
        writer.write_all(&header_len.to_le_bytes())?;
        writer.write_all(&header)?;
        writer.write_all(&record)?;
        for frame in keyframes.frames() {
            writer.write_all(&frame.bytes)?;
        }
        Ok(())
    })
}

/// Loads a replay from disk with any keyframes it carries. JSON replays
/// carry none.
pub fn load_recording(path: impl AsRef<Path>) -> Result<Recording, ReplayError> {
    let path = path.as_ref();
    let mut file = std::fs::File::open(path)?;
    let mut magic = [0; 8];
    if file.read_exact(&mut magic).is_err() || &magic != MAGIC {
        let replay = GameReplay::load_with_decoder(path, |bytes| {
            Ok(serde_json::from_slice::<ReplayWire>(bytes)?.into())
        })?;
        return Ok(Recording {
            replay,
            keyframes: Keyframes::default(),
        });
    }
    let length = file.metadata()?.len();
    if length > MAX_RECORDING_BYTES {
        return Err(invalid(format!(
            "file is {length} bytes, beyond the {MAX_RECORDING_BYTES}-byte limit"
        )));
    }
    read_recording(&mut file, length - 8).map_err(invalid)
}

/// Reads the next `len` of the `remaining` bytes the file holds. Each frame
/// is read on its own, so loading never holds the whole file at once.
fn take(reader: &mut impl Read, remaining: &mut u64, len: u64) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(len <= *remaining, "truncated replay");
    *remaining -= len;
    let mut frame = vec![0; usize::try_from(len)?];
    reader.read_exact(&mut frame)?;
    Ok(frame)
}

/// Decodes the `remaining` bytes after the magic.
fn read_recording(reader: &mut impl Read, mut remaining: u64) -> anyhow::Result<Recording> {
    use anyhow::ensure;
    let length = take(reader, &mut remaining, 4)?;
    let length = u64::from(u32::from_le_bytes(length.try_into().expect("four bytes")));
    ensure!(length <= MAX_HEADER, "replay header exceeds its byte bound");
    let header: Header = serde_json::from_slice(&take(reader, &mut remaining, length)?)?;
    ensure!(
        header.version == FORMAT,
        "unsupported replay format {} (this build reads {FORMAT})",
        header.version
    );
    ensure!(
        header.keyframes.len() <= crate::keyframes::MAX_KEYFRAMES,
        "too many keyframes"
    );
    let json = crate::compression::decompress(
        &take(reader, &mut remaining, header.record.compressed)?,
        header.record.decoded,
        MAX_REPLAY_BYTES as u64,
        None,
    )?;
    let replay: GameReplay = serde_json::from_slice::<ReplayWire>(&json)?.into();
    ensure!(
        replay.commands.len() <= MAX_REPLAY_COMMANDS,
        "replay has too many commands"
    );
    let mut frames = Vec::with_capacity(header.keyframes.len());
    for entry in header.keyframes {
        frames.push(Keyframe {
            tick: entry.tick,
            hash: entry.hash,
            encoding: entry.encoding,
            decoded: entry.decoded,
            bytes: take(reader, &mut remaining, entry.compressed)?.into(),
        });
    }
    ensure!(remaining == 0, "trailing replay bytes");
    ensure!(reader.read(&mut [0])? == 0, "file changed while being read");
    let keyframes = Keyframes::new(frames);
    keyframes.validate(replay.start_tick(), replay_duration(&replay))?;
    Ok(Recording { replay, keyframes })
}

#[cfg(test)]
mod tests;
