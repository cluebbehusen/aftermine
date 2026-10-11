//! A rejoining client's world on the wire: CBOR in one zstd frame, behind
//! its decoded length so a forged snapshot cannot inflate without bound.

use anyhow::{Context, Result, ensure};
use oxide_kit::world_snapshot::WorldSnapshot;
use std::io::{Read, Write};

/// The most a snapshot may decode to.
const MAX_DECODED: u64 = 64 << 20;

/// The snapshot's bytes.
pub(crate) fn encode(snapshot: &WorldSnapshot) -> Result<Vec<u8>> {
    let mut payload = Vec::new();
    ciborium::into_writer(snapshot, &mut payload)?;
    let decoded = payload.len() as u64;
    ensure!(decoded <= MAX_DECODED, "snapshot exceeds its decoded limit");
    let mut encoder = zstd::stream::Encoder::new(Vec::new(), 3)?;
    encoder.include_checksum(true)?;
    encoder.write_all(&payload)?;
    let mut bytes = decoded.to_le_bytes().to_vec();
    bytes.extend(encoder.finish()?);
    Ok(bytes)
}

/// Reads and checks a snapshot that arrived over the wire.
pub(crate) fn decode(bytes: &[u8]) -> Result<WorldSnapshot> {
    let (length, compressed) = bytes
        .split_first_chunk::<8>()
        .context("snapshot is too short")?;
    let decoded = u64::from_le_bytes(*length);
    ensure!(decoded <= MAX_DECODED, "snapshot exceeds its decoded limit");
    let mut payload = Vec::new();
    zstd::stream::read::Decoder::new(compressed)?
        .take(decoded + 1)
        .read_to_end(&mut payload)?;
    ensure!(payload.len() as u64 == decoded, "snapshot length mismatch");
    let snapshot: WorldSnapshot = ciborium::from_reader(payload.as_slice())?;
    snapshot.validate()?;
    Ok(snapshot)
}

#[cfg(test)]
mod tests;
