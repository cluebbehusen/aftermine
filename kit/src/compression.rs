//! Single checksummed Zstandard frames with exact, bounded decoded lengths.
//!
//! Stored bytes are untrusted: decoding refuses a frame without a checksum,
//! trailing bytes, or output of any length but the one its index declares.

use anyhow::{Context, Result, ensure};

/// Compresses `raw` into one checksummed frame, against `dictionary`'s raw
/// content when given.
pub(crate) fn compress(raw: &[u8], dictionary: Option<&[u8]>, level: i32) -> Result<Vec<u8>> {
    let mut compressor = match dictionary {
        Some(dictionary) => zstd::bulk::Compressor::with_dictionary(level, dictionary)?,
        None => zstd::bulk::Compressor::new(level)?,
    };
    compressor.set_parameter(zstd::zstd_safe::CParameter::ChecksumFlag(true))?;
    Ok(compressor.compress(raw)?)
}

/// Decompresses one frame that must decode to exactly `decoded` bytes, at
/// most `max`, against the same `dictionary` it was compressed with.
pub(crate) fn decompress(
    bytes: &[u8],
    decoded: u64,
    max: u64,
    dictionary: Option<&[u8]>,
) -> Result<Vec<u8>> {
    ensure!(decoded <= max, "compressed frame decodes beyond its bound");
    ensure!(
        bytes.get(4).is_some_and(|descriptor| descriptor & 4 != 0),
        "compressed frame has no checksum"
    );
    let frame = zstd::zstd_safe::find_frame_compressed_size(bytes)
        .map_err(|code| anyhow::anyhow!("invalid compressed frame: {code}"))?;
    ensure!(frame == bytes.len(), "trailing compressed bytes");
    let capacity = usize::try_from(decoded).context("decoded length")?;
    let raw = match dictionary {
        Some(dictionary) => {
            zstd::bulk::Decompressor::with_dictionary(dictionary)?.decompress(bytes, capacity)?
        }
        None => zstd::bulk::decompress(bytes, capacity)?,
    };
    ensure!(raw.len() == capacity, "decoded length mismatch");
    Ok(raw)
}

#[cfg(test)]
mod tests;
