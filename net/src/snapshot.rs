//! A snapshot's bytes split into lines and put back together. The bytes
//! are opaque here; the caller encodes and validates the world they carry.

use std::time::Duration;

/// The raw bytes one part carries. As hex, a part line stays well under
/// [`crate::MAX_LINE_BYTES`].
pub const PART_BYTES: usize = 256 * 1024;

/// The longest snapshot a client accepts.
pub const MAX_SNAPSHOT_BYTES: u64 = 32 << 20;

/// The slowest wire a snapshot is given time to cross, in bytes of lines a
/// second. Hex doubles a snapshot, so this allows about 4 Mbit/s of it.
const SLOWEST_LINE_BYTES_PER_SECOND: u64 = 1 << 20;

/// Time to allow `bytes` of snapshot to cross the wire as hex parts.
pub fn transfer_time(bytes: u64) -> Duration {
    Duration::from_millis(bytes.saturating_mul(2_000) / SLOWEST_LINE_BYTES_PER_SECOND)
}

/// `bytes` as hex parts of [`PART_BYTES`] each, the last one shorter.
pub fn parts(bytes: &[u8]) -> Vec<String> {
    bytes.chunks(PART_BYTES).map(crate::token::hex).collect()
}

/// How many parts carry `bytes` bytes.
pub fn part_count(bytes: u64) -> u64 {
    bytes.div_ceil(PART_BYTES as u64)
}

/// A part that does not fit what its snapshot announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadPart;

/// Collects a snapshot's parts, refusing any that do not fit what was
/// announced.
#[derive(Debug)]
pub struct Assembler {
    expected: usize,
    parts: u32,
    received: u32,
    bytes: Vec<u8>,
}

impl Assembler {
    /// Expects `bytes` bytes in `parts` parts; `None` when that is too
    /// large or does not add up.
    pub fn new(bytes: u64, parts: u32) -> Option<Self> {
        if bytes == 0 || bytes > MAX_SNAPSHOT_BYTES || part_count(bytes) != u64::from(parts) {
            return None;
        }
        Some(Self {
            expected: usize::try_from(bytes).ok()?,
            parts,
            received: 0,
            bytes: Vec::new(),
        })
    }

    /// Adds the next part: the whole snapshot once the last arrives, or
    /// `Err` for a part that is malformed, misplaced, or too long.
    pub fn push(&mut self, hex: &str) -> Result<Option<Vec<u8>>, BadPart> {
        if self.received == self.parts || !hex.len().is_multiple_of(2) {
            return Err(BadPart);
        }
        let part = (0..hex.len())
            .step_by(2)
            .map(|at| {
                let digits = hex.get(at..at + 2).ok_or(BadPart)?;
                u8::from_str_radix(digits, 16).map_err(|_| BadPart)
            })
            .collect::<Result<Vec<u8>, BadPart>>()?;
        self.received += 1;
        let last = self.received == self.parts;
        let full = part.len() == PART_BYTES;
        if (!last && !full) || part.is_empty() || self.bytes.len() + part.len() > self.expected {
            return Err(BadPart);
        }
        self.bytes.extend(part);
        if !last {
            return Ok(None);
        }
        if self.bytes.len() != self.expected {
            return Err(BadPart);
        }
        Ok(Some(std::mem::take(&mut self.bytes)))
    }
}

#[cfg(test)]
mod tests;
