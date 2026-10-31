//! Durations as players read them. Ticks never reach a player: match
//! time, waits, and replay positions all read as a clock.

/// `total` seconds as `M:SS`, or `H:MM:SS` from an hour on.
pub(crate) fn seconds(total: u64) -> String {
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Match time at `tick`, whole seconds only, as the match clock shows it.
pub(crate) fn ticks(tick: u64) -> String {
    seconds(tick / u64::from(aftermine_sim::TICKS_PER_SECOND))
}

#[cfg(test)]
mod tests;
