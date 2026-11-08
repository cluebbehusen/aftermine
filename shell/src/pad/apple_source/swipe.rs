//! The Siri Remote's touch surface as D-pad taps and raw travel.
//!
//! Each [`STEP`] of travel taps one direction, so a short swipe moves a row
//! and a long one several. Only travel counts: touching and lifting send
//! nothing and a resting thumb sends nothing, so nothing repeats on a timer.
//! The raw travel is for whatever follows the finger itself.

use aftermine_protocol::PadButton;
use macroquad::prelude::Vec2;

/// The travel that taps a direction. The surface spans 2 across, so a swipe
/// from edge to edge taps five times.
const STEP: f32 = 0.4;

/// A touch in progress.
#[derive(Debug, Default)]
pub(super) struct Swipe {
    /// Where the travel toward the next tap is measured from.
    anchor: Option<Vec2>,
    /// Where the touch was at the last read.
    last: Option<Vec2>,
}

impl Swipe {
    /// Follows the touch to `touch`, x to the right and y down, where zero
    /// means nothing touches the surface. Returns the direction this read
    /// taps and the travel since the last read. It taps once a read at most,
    /// and later reads tap for any travel left over. A finger shifts as it
    /// clicks, so a click moves nothing.
    pub(super) fn feed(&mut self, touch: Vec2, clicking: bool) -> (Option<PadButton>, Vec2) {
        let travel = self
            .last
            .filter(|_| touch != Vec2::ZERO && !clicking)
            .map_or(Vec2::ZERO, |last| touch - last);
        self.last = (touch != Vec2::ZERO).then_some(touch);
        (self.tap(touch, clicking), travel)
    }

    fn tap(&mut self, touch: Vec2, clicking: bool) -> Option<PadButton> {
        if touch == Vec2::ZERO {
            self.anchor = None;
            return None;
        }
        let Some(anchor) = self.anchor.as_mut().filter(|_| !clicking) else {
            self.anchor = Some(touch);
            return None;
        };
        let travel = touch - *anchor;
        if travel.x.abs().max(travel.y.abs()) < STEP {
            return None;
        }
        // Drift along the other axis is dropped, so a swipe bent off line
        // never adds up to a tap sideways. A perfect diagonal reads vertical,
        // since most menus are lists.
        Some(if travel.y.abs() >= travel.x.abs() {
            anchor.x = touch.x;
            anchor.y += STEP.copysign(travel.y);
            if travel.y < 0.0 {
                PadButton::DpadUp
            } else {
                PadButton::DpadDown
            }
        } else {
            anchor.y = touch.y;
            anchor.x += STEP.copysign(travel.x);
            if travel.x < 0.0 {
                PadButton::DpadLeft
            } else {
                PadButton::DpadRight
            }
        })
    }
}

#[cfg(test)]
mod tests;
