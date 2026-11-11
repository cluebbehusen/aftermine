//! Hint lines that show a controller's own buttons as icons among the
//! words, so a pad player reads the button they hold rather than its name.

use crate::assets::Sprites;
use crate::numeric;
use crate::pad::PadIcon;
use crate::theme::Type;
use aftermine_protocol::{PadButton, PadFamily};
use macroquad::prelude::*;

/// An icon's height as a share of the text size: a little taller than
/// capitals, so the label inside it reads at the same size as the copy.
const ICON_SCALE: f32 = 1.25;

/// One run of a hint line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Segment {
    Text(String),
    Pad(PadIcon),
}

/// A line of hint copy, some of it controller button icons.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HintLine {
    segments: Vec<Segment>,
}

impl From<String> for HintLine {
    fn from(text: String) -> Self {
        Self::default().text(text)
    }
}

impl HintLine {
    /// Appends copy.
    pub(crate) fn text(mut self, text: impl Into<String>) -> Self {
        self.segments.push(Segment::Text(text.into()));
        self
    }

    /// Appends a button icon.
    pub(crate) fn pad(mut self, icon: PadIcon) -> Self {
        self.segments.push(Segment::Pad(icon));
        self
    }

    /// Appends a button as `family` shows it: an icon, or the Siri Remote's
    /// word.
    pub(crate) fn button(self, family: PadFamily, button: PadButton) -> Self {
        match crate::pad::remote_name(family, button) {
            Some(word) => self.text(word),
            None => self.pad(crate::pad::button_icon(family, button)),
        }
    }

    /// The line with its opening copy sentence-cased. A line that opens on
    /// an icon reads like one that opens on the button's name, so it keeps
    /// its words as written.
    pub(crate) fn sentence_cased(mut self) -> Self {
        if let Some(Segment::Text(first)) = self.segments.first_mut() {
            *first = crate::typography::sentence_case(first);
        }
        self
    }

    /// The line's width at text `size`, given how wide `measure` finds a
    /// run of copy at that size.
    fn width_with(&self, size: f32, measure: impl Fn(&str, f32) -> f32) -> f32 {
        self.segments
            .iter()
            .map(|segment| match segment {
                Segment::Text(text) => measure(text, size),
                Segment::Pad(_) => icon_width(size),
            })
            .sum()
    }

    /// The size at which the line fits `width`: `size` when it already
    /// fits, else shrunk to fit, but never below `floor`, as
    /// [`crate::typography::fit`] does for plain copy.
    fn fit_with(
        &self,
        size: f32,
        width: f32,
        floor: f32,
        measure: impl Fn(&str, f32) -> f32,
    ) -> f32 {
        let measured = self.width_with(size, measure);
        if measured <= width {
            size
        } else {
            (size * width / measured).max(floor).min(size)
        }
    }

    /// The line's width at text `size`.
    pub(crate) fn width(&self, size: f32) -> f32 {
        self.width_with(size, measure)
    }

    /// The text size at which the line fits `width`: `largest` when it
    /// already fits, else shrunk to fit, but never below `smallest`.
    pub(crate) fn fit(&self, largest: Type, width: f32, smallest: Type) -> f32 {
        let ui = crate::render::ui_scale();
        self.fit_with(largest.at(ui), width, smallest.at(ui), measure)
    }

    /// Draws the line from `x` at text `size`, its baseline at `baseline`.
    /// Copy takes `text`; icons take `icons`, which tints the bone art and
    /// carries its opacity.
    pub(crate) fn draw_at(
        &self,
        sprites: &Sprites,
        x: f32,
        baseline: f32,
        size: f32,
        text: Color,
        icons: Color,
    ) {
        let mut x = x;
        for segment in &self.segments {
            match segment {
                Segment::Text(copy) => {
                    draw_text(copy, x, baseline, size, text);
                    x += measure(copy, size);
                }
                Segment::Pad(pad) => x += draw_icon(sprites, *pad, x, baseline, size, icons),
            }
        }
    }

    /// Draws the line centered on `center_x` with its baseline at
    /// `baseline`, at `largest` shrunk as far as `smallest` to fit
    /// `width`.
    #[expect(
        clippy::too_many_arguments,
        reason = "one call places, sizes, and colors the whole line"
    )]
    pub(crate) fn draw(
        &self,
        sprites: &Sprites,
        center_x: f32,
        baseline: f32,
        width: f32,
        (largest, smallest): (Type, Type),
        text: Color,
        icons: Color,
    ) {
        let size = self.fit(largest, width, smallest);
        let x = center_x - self.width(size) * 0.5;
        self.draw_at(sprites, x, baseline, size, text, icons);
    }
}

/// How wide a controller button's icon stands beside copy at text `size`.
pub(crate) fn icon_width(size: f32) -> f32 {
    size * ICON_SCALE
}

/// Draws `icon` beside copy at text `size`, from `x`, centered on the
/// capitals of a line whose baseline is `baseline`, tinted with `tint`.
/// Returns how wide it stands.
pub(crate) fn draw_icon(
    sprites: &Sprites,
    icon: PadIcon,
    x: f32,
    baseline: f32,
    size: f32,
    tint: Color,
) -> f32 {
    let width = icon_width(size);
    let cap = measure_text("H", None, numeric::font_size(size), 1.0).offset_y;
    let dpi = screen_dpi_scale();
    let snap = |v: f32| (v * dpi).round() / dpi;
    let extent = snap(width);
    let dest = Rect::new(
        snap(x),
        snap(baseline - cap * 0.5 - width * 0.5),
        extent,
        extent,
    );
    sprites.draw_canvas(dest, &[(sprites.pad_icon(icon), tint)]);
    width
}

/// How wide a run of copy is at text `size`.
fn measure(copy: &str, size: f32) -> f32 {
    measure_text(copy, None, numeric::font_size(size), 1.0).width
}

#[cfg(test)]
mod tests;
