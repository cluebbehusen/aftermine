//! Compact player checkpoints with independently readable metadata.

use crate::game::checkpoint::{GameCheckpoint, LanSave, RestoredGame, SaveCapture};
use crate::numeric::Fit;
use crate::saves::RecordKind;
use aftermine_sim::SIM_VERSION;
use anyhow::{Context, Result, ensure};
use chassis::replay::ReplayMeta;
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const VERSION: u32 = 1;
const MAGIC: &[u8; 8] = b"AFTERSAV";
const MAX_HEADER: usize = 64 * 1024;
const MAX_BYTES: usize = aftermine_kit::checkpoint::MAX_BYTES;
/// Saves compress on the save worker. Against the fastest level this takes
/// about a tenth of a second on a 20-minute eight-seat match and saves about
/// a quarter, most of what the match history adds.
const LEVEL: i32 = 15;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u32,
    meta: ReplayMeta,
    map: String,
    /// Each team's seat count, smallest first.
    teams: Vec<usize>,
    compressed_bytes: u64,
    decoded_bytes: u64,
}

pub struct RecordInfo {
    pub meta: ReplayMeta,
    pub map: String,
    /// Each team's seat count, smallest first.
    pub teams: Vec<usize>,
    pub problem: Option<Unavailable>,
}

/// Why this build cannot open a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// Written by another build; pre-launch builds never read each
    /// other's records.
    OtherVersion,
    /// Damaged or malformed.
    Unreadable,
}

impl Unavailable {
    /// The reason as a player reads it.
    pub fn reason(self) -> &'static str {
        match self {
            Unavailable::OtherVersion => "made by a different version of Aftermine",
            Unavailable::Unreadable => "This file is damaged",
        }
    }
}

pub(crate) fn write_capture(capture: SaveCapture, meta: ReplayMeta, path: &Path) -> Result<()> {
    ensure!(
        capture.is_lan() == RecordKind::of(&meta).lan(),
        "save kind does not match the match"
    );
    let checkpoint = capture.checkpoint()?;
    let mut payload = Vec::new();
    ciborium::into_writer(&checkpoint, &mut payload)?;
    ensure!(
        payload.len() <= MAX_BYTES,
        "save exceeds decoded byte limit"
    );
    // Metadata is obtained from the checkpoint itself, without restoring controllers.
    let (map, tick) = checkpoint.metadata();
    let teams = crate::menu::team_sizes(checkpoint.team_tags());
    ensure!(meta.ticks == Some(tick), "save metadata mismatch");
    let mut encoder = zstd::stream::Encoder::new(Vec::new(), LEVEL)?;
    encoder.include_checksum(true)?;
    encoder.write_all(&payload)?;
    let compressed = encoder.finish()?;
    let header = Header {
        version: VERSION,
        meta,
        map: map.to_owned(),
        teams,
        compressed_bytes: compressed.len() as u64,
        decoded_bytes: payload.len() as u64,
    };
    check_header(&header)?;
    let header = serde_json::to_vec(&header)?;
    ensure!(
        header.len() <= MAX_HEADER && 12 + header.len() + compressed.len() <= MAX_BYTES,
        "save exceeds byte limit"
    );
    chassis::fsx::write_atomic(path, |writer| {
        writer.write_all(MAGIC)?;
        writer.write_all(&header.len().fit::<u32>().to_le_bytes())?;
        writer.write_all(&header)?;
        writer.write_all(&compressed)?;
        Ok(())
    })
}

fn read_header(reader: &mut impl Read, file_bytes: u64) -> Result<Header> {
    ensure!(file_bytes <= MAX_BYTES as u64, "save exceeds byte limit");
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    ensure!(&magic == MAGIC, "unsupported player save format");
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    ensure!(length <= MAX_HEADER, "save header exceeds byte limit");
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    let header: Header = serde_json::from_slice(&bytes)?;
    ensure!(
        header.compressed_bytes <= MAX_BYTES as u64 && header.decoded_bytes <= MAX_BYTES as u64,
        "save payload exceeds byte limit"
    );
    ensure!(
        file_bytes == 12 + length as u64 + header.compressed_bytes,
        "save payload length mismatch"
    );
    Ok(header)
}

fn check_header(header: &Header) -> Result<()> {
    ensure!(
        header.version == VERSION,
        "unsupported save format {} (this build supports {VERSION})",
        header.version
    );
    ensure!(
        header.meta.sim_version == SIM_VERSION,
        "saved on sim v{}; this build runs v{SIM_VERSION}",
        header.meta.sim_version
    );
    ensure!(
        RecordKind::of(&header.meta) != RecordKind::Match,
        "invalid save kind"
    );
    let max = aftermine_sim::scenario::MAX_PLAYERS;
    ensure!(
        !header.teams.is_empty()
            && header.teams.len() <= max
            && header.teams.iter().all(|size| (1..=max).contains(size))
            && header.teams.iter().sum::<usize>() <= max
            && header.teams.is_sorted(),
        "invalid save teams"
    );
    Ok(())
}

/// A save, checked and ready: a local match to install, or a LAN match
/// for Match Setup to resume with its players.
pub(crate) enum Loaded {
    Local(Box<RestoredGame>),
    Lan(Box<LanSave>),
}

pub(crate) fn prepare(path: &Path) -> Result<Loaded> {
    let (header, payload) = read_payload(path)?;
    let mut reader = payload.as_slice();
    let checkpoint: GameCheckpoint = ciborium::from_reader(&mut reader)?;
    ensure!(reader.is_empty(), "trailing checkpoint payload");
    ensure!(
        checkpoint.is_lan() == RecordKind::of(&header.meta).lan(),
        "save kind does not match the match"
    );
    let slot = crate::autosave::SaveSlot::adopt(path, &crate::paths::records().autosave_folders());
    let (loaded, scenario, tick) = if checkpoint.is_lan() {
        let save = checkpoint.into_lan_save()?.with_slot(slot);
        let (scenario, tick) = (save.scenario().clone(), save.tick());
        (Loaded::Lan(Box::new(save)), scenario, tick)
    } else {
        let game = checkpoint.restore()?.with_slot(slot);
        let (scenario, tick) = (game.scenario().clone(), game.tick());
        (Loaded::Local(Box::new(game)), scenario, tick)
    };
    ensure!(
        header.meta.ticks == Some(tick)
            && header.map == scenario.name
            && header.teams
                == crate::menu::team_sizes(scenario.players.iter().map(|player| player.team)),
        "save metadata does not match session"
    );
    Ok(loaded)
}

/// The checked header and decompressed checkpoint bytes of a player save.
fn read_payload(path: &Path) -> Result<(Header, Vec<u8>)> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("loading save {}", path.display()))?;
    let len = file.metadata()?.len();
    let header = read_header(&mut file, len).context("loading save header")?;
    check_header(&header)?;
    let mut compressed = Vec::new();
    file.take(header.compressed_bytes + 1)
        .read_to_end(&mut compressed)?;
    ensure!(
        compressed.len() as u64 == header.compressed_bytes,
        "save payload length mismatch"
    );
    ensure!(
        compressed
            .get(4)
            .is_some_and(|descriptor| descriptor & 4 != 0),
        "save frame has no checksum"
    );
    let frame_len = zstd::zstd_safe::find_frame_compressed_size(&compressed)
        .map_err(|code| anyhow::anyhow!("invalid compressed frame: {code}"))?;
    ensure!(frame_len == compressed.len(), "trailing compressed payload");
    let mut payload = Vec::new();
    zstd::stream::read::Decoder::new(compressed.as_slice())?
        .take(header.decoded_bytes + 1)
        .read_to_end(&mut payload)?;
    ensure!(
        payload.len() as u64 == header.decoded_bytes,
        "decoded payload length mismatch"
    );
    Ok((header, payload))
}

pub fn inspect(path: &Path) -> Result<RecordInfo> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    inspect_reader(path, &mut file, len)
}

fn inspect_reader(path: &Path, file: &mut (impl Read + Seek), len: u64) -> Result<RecordInfo> {
    let mut magic = [0; 8];
    let compact = file.read_exact(&mut magic).is_ok() && &magic == MAGIC;
    if compact {
        file.seek(SeekFrom::Start(0))?;
        let header = read_header(file, len)?;
        let problem = if header.version != VERSION || header.meta.sim_version != SIM_VERSION {
            Some(Unavailable::OtherVersion)
        } else {
            check_header(&header).err().map(|_| Unavailable::Unreadable)
        };
        Ok(RecordInfo {
            meta: header.meta,
            map: header.map,
            teams: header.teams,
            problem,
        })
    } else if path.extension().and_then(|extension| extension.to_str()) == Some("amsave") {
        anyhow::bail!("invalid player save header");
    } else {
        // Replay inspection remains separate from player checkpoint restoration.
        let replay = aftermine_kit::load_replay(path)?;
        let problem = if replay.meta.sim_version == SIM_VERSION {
            replay
                .validate(Some(SIM_VERSION))
                .map_err(anyhow::Error::from)
                .and_then(|()| aftermine_kit::bounded_replay_duration(&replay).map(|_| ()))
                .err()
                .map(|_| Unavailable::Unreadable)
        } else {
            Some(Unavailable::OtherVersion)
        };
        let teams = crate::menu::team_sizes(replay.setup.players.iter().map(|p| p.team));
        Ok(RecordInfo {
            meta: replay.meta,
            map: replay.setup.name,
            teams,
            problem,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests;
