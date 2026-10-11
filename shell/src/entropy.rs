//! Fresh randomness for secrets that decide no outcome, such as rejoin
//! tokens. Nothing here may feed the simulation.

use oxide_net::Token;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 128 bits nobody can predict: std's hashers are keyed from the operating
/// system's randomness, and each draw also mixes in a counter and the time.
pub(crate) fn bits() -> u128 {
    static DRAWS: AtomicU64 = AtomicU64::new(0);
    let draw = DRAWS.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let half = |lane: u64| {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(draw);
        hasher.write_u128(nanos);
        hasher.write_u64(lane);
        hasher.finish()
    };
    (u128::from(half(0)) << 64) | u128::from(half(1))
}

/// A fresh rejoin token.
pub(crate) fn token() -> Token {
    Token::new(bits())
}

#[cfg(test)]
mod tests;
