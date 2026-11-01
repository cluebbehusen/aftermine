//! The tab strip a menu screen hangs under its title: one segment per
//! tab, the active one filled. Like the BACK button, it sees a frame's
//! pointer events first, and the list beneath gets only the ones it
//! leaves alone.

use crate::numeric;
use crate::press::{Fed, Press};
use crate::theme::{
    BORDER_STRONG, CARD_IDLE, MIN_TOUCH_TARGET, SURFACE_MENU, Stroke, TEXT_ACCENT, TEXT_BODY,
    TEXT_PRIMARY, TEXT_SECONDARY, Type,
};
use aftermine_protocol::RawEvent;
use macroquad::prelude::{
    Rect, Vec2, draw_rectangle, draw_rectangle_lines, draw_text, measure_text,
};

/// The keys that step tabs in menus now, while both have one.
pub(crate) fn keys() -> Option<(String, String)> {
    crate::menu::tab_key(crate::action::Action::PrevTab)
        .zip(crate::menu::tab_key(crate::action::Action::NextTab))
}

/// The logical height a strip takes from the list beneath it, for
/// [`crate::menu::Heading::Page`].
pub(crate) const BAND: f32 = 36.0;

/// The segments of a `count`-tab strip, centred under a page title.
pub(crate) fn rects(count: usize) -> Vec<Rect> {
    let s = crate::render::ui_scale();
    let view = crate::render::viewport();
    let width = (150.0 * s).min((view.x - 32.0 * s) / count.max(1) as f32);
    let left = (view.x - width * count as f32) * 0.5;
    let top = crate::menu::page_title_baseline(s) + 20.0 * s;
    // Never shorter than a fingertip, however small the interface scale.
    let height = (MIN_TOUCH_TARGET * s).max(MIN_TOUCH_TARGET);
    (0..count)
        .map(|tab| Rect::new(left + tab as f32 * width, top, width, height))
        .collect()
}

/// A strip's pointer state: the tab a press armed.
#[derive(Default)]
pub(crate) struct TabStrip {
    press: Press<usize>,
}

impl TabStrip {
    /// Drops a half-made press, as when the screen stops routing to the
    /// strip.
    pub(crate) fn cancel(&mut self) {
        self.press.cancel();
    }

    /// The tab this frame clicked or tapped among `count`, and the
    /// events left for the screen.
    pub(crate) fn route(
        &mut self,
        events: &[RawEvent],
        count: usize,
    ) -> (Option<usize>, Vec<RawEvent>) {
        let rects = rects(count);
        let mut rest = Vec::with_capacity(events.len());
        for event in events {
            match self
                .press
                .feed(event, |p, _| rects.iter().position(|rect| rect.contains(p)))
            {
                Fed::Activated(tab) => return (Some(tab), rest),
                Fed::Held => {}
                Fed::Ignored => rest.push(*event),
            }
        }
        (None, rest)
    }
}

/// Draws the strip over `labels` with `active` filled and the tab under
/// `mouse` lit. `keys`, when the player has a keyboard, names the keys
/// that step left and right, drawn beside the strip's ends.
pub(crate) fn draw(labels: &[&str], active: usize, mouse: Vec2, keys: Option<(&str, &str)>) {
    let s = crate::render::ui_scale();
    let rects = rects(labels.len());
    let size = Type::Body.at(s);
    for (tab, (rect, label)) in rects.iter().zip(labels).enumerate() {
        let current = tab == active;
        let hovered = rect.contains(mouse);
        draw_rectangle(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if current { CARD_IDLE } else { SURFACE_MENU },
        );
        draw_rectangle_lines(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            Stroke::Hairline.at(s),
            if current { TEXT_ACCENT } else { BORDER_STRONG },
        );
        let fitted =
            crate::typography::fit(label, size, rect.w - 12.0 * s, crate::theme::Type::Caption);
        let width = measure_text(label, None, numeric::font_size(fitted), 1.0).width;
        draw_text(
            label,
            rect.x + (rect.w - width) * 0.5,
            rect.y + rect.h * 0.5 + fitted * 0.35,
            fitted,
            match (current, hovered) {
                (true, _) => TEXT_PRIMARY,
                (false, true) => TEXT_BODY,
                (false, false) => TEXT_SECONDARY,
            },
        );
    }
    let (Some((prev, next)), Some(first), Some(last)) = (keys, rects.first(), rects.last()) else {
        return;
    };
    let size = Type::Small.at(s);
    let baseline = first.y + first.h * 0.5 + size * 0.35;
    let gap = 12.0 * s;
    let prev_width = measure_text(prev, None, numeric::font_size(size), 1.0).width;
    draw_text(
        prev,
        first.x - gap - prev_width,
        baseline,
        size,
        TEXT_SECONDARY,
    );
    draw_text(next, last.x + last.w + gap, baseline, size, TEXT_SECONDARY);
}

#[cfg(test)]
mod tests;
