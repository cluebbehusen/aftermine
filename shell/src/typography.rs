//! Shared interface lettering for short labels and body copy.

use crate::numeric;
use macroquad::prelude::*;
use std::cell::RefCell;

pub(crate) fn entity_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut start = true;
    for c in name.chars() {
        if start {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        start = c == ' ';
    }
    out
}

/// The plural of an entity name: "Foundry" to "Foundries", "Repair Bay"
/// to "Repair Bays", and a name already ending in s, like "Airworks",
/// unchanged.
pub(crate) fn plural_name(name: &str) -> String {
    let mut letters = name.chars().rev();
    match (letters.next(), letters.next()) {
        (Some('s'), _) => name.to_string(),
        (Some('y'), Some(before)) if !"aeiou".contains(before) => {
            format!("{}ies", &name[..name.len() - 1])
        }
        _ => format!("{name}s"),
    }
}

/// `text` with its first letter capitalized. Refusal reasons are
/// lowercase fragments so they also read inside longer messages; a
/// surface that shows one alone starts it as a sentence.
pub(crate) fn sentence_case(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

thread_local! {
    static DISPLAY: RefCell<Option<Font>> = const { RefCell::new(None) };
}

pub(crate) fn install(font: Font) {
    DISPLAY.with(|current| *current.borrow_mut() = Some(font));
}

/// `text` cut to `max` characters, its last three spent on "...". Counts
/// chars, not bytes: names are player-typed, and slicing bytes could
/// split a multibyte character.
pub(crate) fn clip_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(max.saturating_sub(3)).collect();
    clipped.push_str("...");
    clipped
}

/// `text` clipped with "..." until `measure` says it fits `max_width`;
/// empty when not even four characters fit.
pub(crate) fn elide_to_width(text: &str, max_width: f32, measure: &impl Fn(&str) -> f32) -> String {
    if measure(text) <= max_width {
        return text.to_string();
    }
    for cap in (4..text.chars().count()).rev() {
        let candidate = clip_chars(text, cap);
        if measure(&candidate) <= max_width {
            return candidate;
        }
    }
    String::new()
}

/// The size at which body-face `text` fits `width`: `size` when it
/// already fits, else shrunk to fit, but never below `floor`, which comes from
/// the type scale so nothing shrinks past what the scale can read.
pub(crate) fn fit(text: &str, size: f32, width: f32, floor: crate::theme::Type) -> f32 {
    let measured = measure_text(text, None, numeric::font_size(size), 1.0).width;
    if measured <= width {
        size
    } else {
        (size * width / measured)
            .max(floor.at(crate::render::ui_scale()))
            .min(size)
    }
}

pub(crate) fn measure(text: &str, size: f32) -> TextDimensions {
    DISPLAY.with(|font| measure_text(text, font.borrow().as_ref(), numeric::font_size(size), 1.0))
}

pub(crate) fn draw(text: &str, x: f32, y: f32, size: f32, color: Color) {
    DISPLAY.with(|font| {
        draw_text_ex(
            text,
            x,
            y,
            TextParams {
                font: font.borrow().as_ref(),
                font_size: numeric::font_size(size),
                color,
                ..Default::default()
            },
        );
    });
}

#[cfg(test)]
mod casing_tests;

#[cfg(test)]
mod tests;
