//! The labeled action button full-screen surfaces share, and the top-left
//! BACK button built from it.

use crate::numeric;
use crate::press::{Fed, Press};
use crate::render::prim::stroke_rect;
use crate::theme;
use aftermine_protocol::RawEvent;
use macroquad::prelude::{Rect, Vec2, draw_rectangle, draw_text, measure_text};

/// The `index`th button slot along the top-left corner: a fingertip
/// tall, wide enough for a short verb, and clear of the window edge.
pub(crate) fn corner_slot(index: usize, s: f32) -> Rect {
    let width = 104.0 * s;
    Rect::new(
        16.0 * s + index as f32 * (width + 8.0 * s),
        16.0 * s,
        width,
        crate::theme::MIN_TOUCH_TARGET * s,
    )
}

/// Draws one action button; `active` marks hover or keyboard focus.
pub(crate) fn draw(rect: Rect, label: &str, active: bool, s: f32) {
    draw_rectangle(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        if active {
            theme::SURFACE_CARD
        } else {
            theme::SURFACE_PANEL
        },
    );
    stroke_rect(
        Rect::new(rect.x, rect.y, rect.w, rect.h),
        if active {
            crate::platform::cursor_stroke()
        } else {
            theme::Stroke::Hairline
        }
        .at(s),
        if active {
            theme::TEXT_ACCENT
        } else {
            theme::TEXT_DISABLED
        },
    );
    let size = crate::theme::Type::Body.at(s);
    let dims = measure_text(label, None, numeric::font_size(size), 1.0);
    draw_text(
        label,
        rect.x + (rect.w - dims.width) * 0.5,
        rect.y + rect.h * 0.62,
        size,
        if active {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_BODY
        },
    );
}

/// The top-left BACK button full-screen menus leave through. It sees a
/// frame's pointer events first, and the screen beneath gets only the
/// ones it leaves alone, so a press on it never reaches a row. It lives
/// outside the rows, so list rebuilds do not disarm it. In pad mode a
/// controller's B or the Siri Remote's Back leaves instead, so the button
/// is neither drawn nor pressed; its corner stays empty, so nothing moves
/// when it returns.
#[derive(Default)]
pub(crate) struct BackButton {
    press: Press<()>,
}

impl BackButton {
    /// Drops a half-made press, as when the screen beneath changes.
    pub(crate) fn cancel(&mut self) {
        self.press.cancel();
    }

    /// Whether this frame pressed the button, and the events it left
    /// for the screen.
    pub(crate) fn route(&mut self, events: &[RawEvent]) -> (bool, Vec<RawEvent>) {
        if crate::platform::pad_mode() {
            self.press.cancel();
            return (false, events.to_vec());
        }
        let rect = corner_slot(0, crate::render::ui_scale());
        let mut rest = Vec::with_capacity(events.len());
        for event in events {
            match self
                .press
                .feed(event, |p, _| rect.contains(p).then_some(()))
            {
                Fed::Activated(()) => return (true, rest),
                Fed::Held => {}
                Fed::Ignored => rest.push(*event),
            }
        }
        (false, rest)
    }
}

/// Draws the top-left BACK button, lit under the mouse, outside pad mode.
pub(crate) fn draw_back(mouse: Vec2) {
    if crate::platform::pad_mode() {
        return;
    }
    let s = crate::render::ui_scale();
    let rect = corner_slot(0, s);
    draw(rect, "Back", crate::platform::hovers(rect, mouse), s);
}

#[cfg(test)]
pub(crate) mod tests;
