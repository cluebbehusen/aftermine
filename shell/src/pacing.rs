//! Frame pacing: how often the shell draws, as a whole number of the
//! display's refreshes.
//!
//! Presenting on every `n`th refresh keeps frames even and tear-free, so the
//! frame rates on offer are the refresh divided by whole numbers. The rate
//! decides only how often the picture is drawn: the simulation ticks from
//! wall time, so no frame rate changes an outcome.

use crate::config::FrameRate;

/// A frequency, in millihertz so a 59.94 Hz display keeps its fraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Rate(u32);

/// The slowest rate offered below a display's own: 30 frames a second, with
/// half a frame of tolerance so a 59.94 Hz display still offers 29.97.
const FLOOR: Rate = Rate(29_500);

/// The most refreshes one frame spans.
const MOST_REFRESHES: u32 = 4;

/// Half a frame a second, in millihertz: a capped setting takes any rate
/// within this of its number, so a choice stored by its label's rounded
/// number picks itself again.
const HALF_FRAME: u32 = 500;

impl Rate {
    /// Whole frames a second, rounded half up: 29.97 reads as 30.
    pub(crate) fn fps(self) -> u32 {
        (self.0 + HALF_FRAME) / 1000
    }

    /// How the rate reads to a player.
    pub(crate) fn label(self) -> String {
        fps_label(self.fps())
    }
}

fn fps_label(fps: u32) -> String {
    format!("{fps} FPS")
}

/// A display's refresh rate as the platform reports it, within what a real
/// display runs at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Refresh(Rate);

impl Refresh {
    /// The refresh a platform's `hz` reading means, or none for a reading
    /// no display gives: one that is not finite or lies outside 20 to
    /// 1000 Hz.
    pub(crate) fn from_hz(hz: f64) -> Option<Self> {
        (20.0..=1000.0)
            .contains(&hz)
            .then(|| Self(Rate(millihertz(hz))))
    }

    /// The rate presenting on every `n`th refresh gives.
    pub(crate) fn every(self, n: u32) -> Rate {
        Rate(self.0.0 / n.max(1))
    }

    /// How many refreshes each choice spans, fastest first: the refresh
    /// itself, then every slower rate down to [`FLOOR`]. A display slower
    /// than the floor offers only its own rate.
    fn divisors(self) -> impl Iterator<Item = u32> {
        (1..=MOST_REFRESHES).take_while(move |&n| n == 1 || self.every(n) >= FLOOR)
    }

    /// The divisor of the slowest choice.
    fn slowest(self) -> u32 {
        self.divisors().last().unwrap_or(1)
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "callers pass a finite reading of at most 1000 Hz"
)]
fn millihertz(hz: f64) -> u32 {
    (hz * 1000.0).round() as u32
}

/// The frame rates a display at `refresh` offers, fastest first: choice `i`
/// presents on every `i + 1`th refresh.
pub(crate) fn choices(refresh: Refresh) -> Vec<Rate> {
    refresh.divisors().map(|n| refresh.every(n)).collect()
}

/// How many refreshes each frame spans under `setting` at `refresh`. A cap
/// takes the fastest choice within half a frame of it, or else the slowest
/// choice, so a cap stored on one display still holds on another.
pub(crate) fn divisor(setting: FrameRate, refresh: Refresh) -> u32 {
    match setting {
        FrameRate::Full => 1,
        FrameRate::Capped(fps) => {
            let ceiling = fps.saturating_mul(1000).saturating_add(HALF_FRAME);
            refresh
                .divisors()
                .find(|&n| refresh.every(n).0 <= ceiling)
                .unwrap_or_else(|| refresh.slowest())
        }
    }
}

/// The setting that picks the choice `n` refreshes apart. The top choice is
/// [`FrameRate::Full`], so it follows the display to whatever rate it runs.
pub(crate) fn setting_at(refresh: Refresh, n: u32) -> FrameRate {
    if n <= 1 {
        FrameRate::Full
    } else {
        FrameRate::Capped(refresh.every(n).fps())
    }
}

/// How `setting` reads on a display at `refresh`: the rate it gives there.
/// With no refresh known, a cap reads as its own number and the top choice
/// as Full.
pub(crate) fn label(setting: FrameRate, refresh: Option<Refresh>) -> String {
    match (refresh, setting) {
        (Some(refresh), _) => refresh.every(divisor(setting, refresh)).label(),
        (None, FrameRate::Capped(fps)) => fps_label(fps),
        (None, FrameRate::Full) => "Full".to_owned(),
    }
}

#[cfg(test)]
mod tests;
