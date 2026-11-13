//! How much wall time one presented frame covers, and how much catch-up
//! work it may take on.

use crate::pacing::{self, Rate};
use std::time::Duration;

/// One presented frame's wall time. Presentation (camera glides, held
/// pans, effects, music) reads a clamped value so a suspension or a
/// clock jump cannot fling it; the live and replay clocks read the
/// unclamped value and cap their own catch-up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FrameTime {
    pub(crate) presentation: f32,
    pub(crate) raw: f32,
    /// The most ticks a session's clock runs this frame; debt past it is
    /// dropped, as after a hitch.
    pub(crate) catch_up: u32,
    /// The wall time a seek in flight may take this frame.
    pub(crate) seek_budget: Duration,
}

/// Presentation never advances more than this per frame.
const MAX_PRESENTATION_DT: f32 = 0.25;

impl FrameTime {
    /// A frame of `reported` seconds presented at `paced`, which sizes its
    /// catch-up work; `None` where the rate is unknown.
    ///
    /// Frame time comes from the system clock, which can step backward
    /// or produce garbage; either reads as no time passing, since a NaN
    /// in a clock accumulator would stall it for good.
    pub(crate) fn measure(reported: f32, paced: Option<Rate>) -> Self {
        let raw = if reported.is_finite() {
            reported.max(0.0)
        } else {
            0.0
        };
        Self {
            presentation: raw.min(MAX_PRESENTATION_DT),
            raw,
            catch_up: pacing::catch_up_ticks(paced),
            seek_budget: pacing::seek_budget(paced),
        }
    }
}

#[cfg(test)]
mod tests;
