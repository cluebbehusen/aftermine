//! The tab strip a menu screen hangs under its title: one segment per
//! tab, the active one filled. Like the BACK button, it sees a frame's
//! pointer events first, and the list beneath gets only the ones it
//! leaves alone.

use crate::assets::Sprites;
use crate::hint_line::HintLine;
use crate::numeric;
use crate::press::{Fed, Press};
use crate::render::prim::stroke_rect;
use crate::theme::{
    BORDER_STRONG, CARD_IDLE, MIN_TOUCH_TARGET, SURFACE_MENU, Stroke, TEXT_ACCENT, TEXT_BODY,
    TEXT_PRIMARY, TEXT_SECONDARY, TEXT_TITLE, Type,
};
use aftermine_protocol::{Key, PadFamily, RawEvent};
use macroquad::prelude::{Rect, Vec2, WHITE, draw_rectangle, draw_text, measure_text};

/// How the Siri Remote, which has no tab buttons, reaches a strip.
pub(crate) const REMOTE_HINT: &str = "Swipe up for tabs";
/// What the remote does once it is on a strip.
pub(crate) const ON_STRIP_HINT: &str = "{dpad} to switch tabs - {back} to leave";

/// The keys that step tabs in menus now, while both have one, or the
/// bumpers' icons while a controller is in use. The Siri Remote has neither;
/// it moves onto the strip instead.
pub(crate) fn keys() -> Option<(HintLine, HintLine)> {
    if let Some(family) = crate::platform::hands().pad {
        if family == PadFamily::Remote {
            return None;
        }
        let icon = |button| HintLine::default().pad(crate::pad::button_icon(family, button));
        return Some((icon(crate::pad::TABS.0), icon(crate::pad::TABS.1)));
    }
    crate::menu::tab_key(crate::action::Action::PrevTab)
        .zip(crate::menu::tab_key(crate::action::Action::NextTab))
        .map(|(prev, next)| (HintLine::from(prev), HintLine::from(next)))
}

/// The tab keys to draw beside a strip: none for a player without keys
/// or a controller, who has no way to press them.
pub(crate) fn shown_keys() -> Option<(HintLine, HintLine)> {
    let hands = crate::platform::hands();
    keys().filter(|_| hands.keys || hands.pad.is_some())
}

/// The logical height a strip takes from the list beneath it, for
/// [`crate::menu::Heading::Page`].
pub(crate) const BAND: f32 = 36.0;

/// The segments of a `count`-tab strip, centred under a page title.
pub(crate) fn rects(count: usize) -> Vec<Rect> {
    let s = crate::render::ui_scale();
    let view = crate::render::viewport();
    let top = crate::menu::page_title_baseline(s) + 20.0 * s;
    // Never shorter than a fingertip, however small the interface scale.
    let height = (MIN_TOUCH_TARGET * s).max(MIN_TOUCH_TARGET);
    let area = Rect::new(16.0 * s, top, view.x - 32.0 * s, height);
    rects_in(count, area, 150.0 * s)
}

/// The segments of a `count`-tab strip centred in `area`, no wider than
/// `widest` each.
pub(crate) fn rects_in(count: usize, area: Rect, widest: f32) -> Vec<Rect> {
    let width = widest.min(area.w / count.max(1) as f32);
    let left = area.x + (area.w - width * count as f32) * 0.5;
    (0..count)
        .map(|tab| Rect::new(left + tab as f32 * width, area.y, width, area.h))
        .collect()
}

/// A strip's pointer state, the tab a press armed, and whether the Siri
/// Remote's cursor is on it.
#[derive(Default)]
pub(crate) struct TabStrip {
    press: Press<usize>,
    focused: bool,
}

impl TabStrip {
    /// Drops a half-made press, as when the screen stops routing to the
    /// strip.
    pub(crate) fn cancel(&mut self) {
        self.press.cancel();
    }

    /// Whether the Siri Remote's cursor is on the strip, not on what lies
    /// below it.
    pub(crate) fn focused(&self) -> bool {
        self.focused
    }

    /// [`Self::route_remote`] for a strip above `menu`: Up from its first row
    /// reaches the strip, and the row cursor hides while the strip has it.
    pub(crate) fn route_remote_above<R: Clone>(
        &mut self,
        events: &[RawEvent],
        menu: &mut crate::menu::Menu<R>,
    ) -> (Option<isize>, Vec<RawEvent>) {
        let routed = self.route_remote(events, menu.at_top());
        menu.cursor_hidden = self.focused;
        routed
    }

    /// Takes the arrow and confirm keys of a frame while the Siri Remote is
    /// in use, whose only way to a tab is to move onto the strip. `at_top`
    /// says Up may enter it. Left and right step the tab, and down or a click
    /// goes back. Returns the tab step and the events left for the screen.
    pub(crate) fn route_remote(
        &mut self,
        events: &[RawEvent],
        at_top: bool,
    ) -> (Option<isize>, Vec<RawEvent>) {
        let remote = crate::platform::hands().pad == Some(PadFamily::Remote);
        self.route_keys(events, at_top, remote)
    }

    fn route_keys(
        &mut self,
        events: &[RawEvent],
        at_top: bool,
        remote: bool,
    ) -> (Option<isize>, Vec<RawEvent>) {
        if !remote {
            self.focused = false;
            return (None, events.to_vec());
        }
        let mut step = 0;
        let mut rest = Vec::with_capacity(events.len());
        for event in events {
            let RawEvent::KeyDown { key } = *event else {
                rest.push(*event);
                continue;
            };
            match (self.focused, key) {
                (false, Key::Up) if at_top => self.focused = true,
                (true, Key::Left) => step -= 1,
                (true, Key::Right) => step += 1,
                (true, Key::Down | Key::Enter) => self.focused = false,
                (true, Key::Up) => {}
                _ => rest.push(*event),
            }
        }
        ((step != 0).then_some(step), rest)
    }

    /// The tab this frame clicked or tapped among `count`, and the
    /// events left for the screen.
    pub(crate) fn route(
        &mut self,
        events: &[RawEvent],
        count: usize,
    ) -> (Option<usize>, Vec<RawEvent>) {
        self.route_in(events, &rects(count))
    }

    /// The tab this frame clicked or tapped among `rects`, and the events
    /// left for the screen.
    pub(crate) fn route_in(
        &mut self,
        events: &[RawEvent],
        rects: &[Rect],
    ) -> (Option<usize>, Vec<RawEvent>) {
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
/// that step left and right, drawn beside the strip's ends. `cursor` says
/// the menu cursor is on the strip, so the active tab wears it.
pub(crate) fn draw(
    sprites: &Sprites,
    labels: &[&str],
    active: usize,
    mouse: Vec2,
    keys: Option<&(HintLine, HintLine)>,
    cursor: bool,
) {
    draw_in(
        sprites,
        &rects(labels.len()),
        labels,
        active,
        mouse,
        keys,
        cursor,
    );
}

/// Draws a strip at `rects`, as [`draw`] does under a page title.
pub(crate) fn draw_in(
    sprites: &Sprites,
    rects: &[Rect],
    labels: &[&str],
    active: usize,
    mouse: Vec2,
    keys: Option<&(HintLine, HintLine)>,
    cursor: bool,
) {
    let s = crate::render::ui_scale();
    let size = Type::Body.at(s);
    for (tab, (rect, label)) in rects.iter().zip(labels).enumerate() {
        let current = tab == active;
        let hovered = crate::platform::hovers(*rect, mouse);
        draw_rectangle(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if current { CARD_IDLE } else { SURFACE_MENU },
        );
        stroke_rect(
            Rect::new(rect.x, rect.y, rect.w, rect.h),
            Stroke::Hairline.at(s),
            if current { TEXT_ACCENT } else { BORDER_STRONG },
        );
        if current && cursor {
            stroke_rect(
                Rect::new(rect.x, rect.y, rect.w, rect.h),
                crate::platform::cursor_stroke().at(s),
                TEXT_TITLE,
            );
        }
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
    // A bumper's icon carries its label inside it, so it needs the size
    // hint lines give icons to stay legible.
    let size = if crate::platform::hands().pad.is_some() {
        Type::Label
    } else {
        Type::Small
    }
    .at(s);
    let baseline = first.y + first.h * 0.5 + size * 0.35;
    let gap = 12.0 * s;
    let prev_x = first.x - gap - prev.width(size);
    prev.draw_at(sprites, prev_x, baseline, size, TEXT_SECONDARY, WHITE);
    next.draw_at(
        sprites,
        last.x + last.w + gap,
        baseline,
        size,
        TEXT_SECONDARY,
        WHITE,
    );
}

/// Each mode's width in the Siri Remote's strip of camera modes.
const REMOTE_MODE_WIDTH: f32 = 110.0;
/// The height of the Siri Remote's strip of camera modes.
pub(crate) const REMOTE_MODES_HEIGHT: f32 = 32.0;

/// Draws the Siri Remote's camera modes as a strip centred on `x` and standing
/// on `bottom`, with the one in use lit, so what its swipes do now is plain.
pub(crate) fn draw_remote_modes(
    sprites: &Sprites,
    remote: crate::camera::controls::RemoteCamera,
    x: f32,
    bottom: f32,
    mouse: Vec2,
) {
    let s = crate::render::ui_scale();
    let modes = remote.modes();
    let labels: Vec<&str> = modes.iter().map(|mode| mode.label()).collect();
    let widest = REMOTE_MODE_WIDTH * s;
    let width = widest * modes.len() as f32;
    let height = REMOTE_MODES_HEIGHT * s;
    let area = Rect::new(x - width * 0.5, bottom - height, width, height);
    draw_in(
        sprites,
        &rects_in(modes.len(), area, widest),
        &labels,
        remote.mode_index(),
        mouse,
        None,
        true,
    );
}

#[cfg(test)]
mod tests;
