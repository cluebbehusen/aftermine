//! How-to copy for the lesson and tip cards, worded for the hands in use.
//!
//! A template fills in three passes. A `[with keys|without]` span keeps its
//! first side once a key has been pressed and its second otherwise, and
//! `[with keys]` has no second side; a controller names no keys, so it
//! always takes the second. Then the pointer's verbs: `{click}` and
//! `{Click}` are a click or a tap, and `{right-click}` and `{Right-click}`
//! a right-click, a long-press, or a press of the controller's order button
//! on what follows. Last, a controller's buttons, written by their Xbox
//! labels as `{A}`, `{B}`, `{X}`, `{Y}`, `{LB}`, `{LT}` and `{Menu}`, take
//! the labels of the family in use. Key tokens such as `{build}` are left
//! for the player's bindings to fill.

use crate::platform::Hands;
use aftermine_protocol::PadButton;

/// One paragraph of how-to copy, as each pair of hands reads it. A
/// template that fills to nothing leaves the paragraph out.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Para {
    /// One template for every pair of hands.
    Same(&'static str),
    /// One template a mouse and a finger share, and a controller's own.
    Pointer {
        pointer: &'static str,
        pad: &'static str,
    },
    /// A mouse's, a finger's and a controller's own templates.
    Each {
        mouse: &'static str,
        touch: &'static str,
        pad: &'static str,
    },
}

impl Para {
    /// The paragraph as `hands` read it.
    pub(crate) fn text(self, hands: Hands) -> String {
        let template = match self {
            Para::Same(template) => template,
            Para::Pointer { pad, .. } | Para::Each { pad, .. } if hands.pad.is_some() => pad,
            Para::Pointer { pointer, .. } => pointer,
            Para::Each { touch, .. } if hands.touch() => touch,
            Para::Each { mouse, .. } => mouse,
        };
        fill(template, hands)
    }
}

/// The controller buttons copy names, by their Xbox labels.
const BUTTONS: [(&str, PadButton); 7] = [
    ("{A}", PadButton::South),
    ("{B}", PadButton::East),
    ("{X}", PadButton::West),
    ("{Y}", PadButton::North),
    ("{LB}", PadButton::LeftShoulder),
    ("{LT}", PadButton::LeftTrigger),
    ("{Menu}", PadButton::Start),
];

/// Fills `template` as `hands` read it; see the module's documentation.
fn fill(template: &str, hands: Hands) -> String {
    let mut text = spans(template, hands.keys && hands.pad.is_none());
    if let Some(family) = hands.pad {
        let order = crate::pad::button_name(family, PadButton::West);
        text = text
            .replace("{right-click}", &format!("press {order} on"))
            .replace("{Right-click}", &format!("Press {order} on"));
        for (token, button) in BUTTONS {
            text = text.replace(token, crate::pad::button_name(family, button));
        }
        return text;
    }
    let verbs = if hands.touch() {
        [
            ("{click}", "tap"),
            ("{Click}", "Tap"),
            ("{right-click}", "long-press"),
            ("{Right-click}", "Long-press"),
        ]
    } else {
        [
            ("{click}", "click"),
            ("{Click}", "Click"),
            ("{right-click}", "right-click"),
            ("{Right-click}", "Right-click"),
        ]
    };
    for (token, verb) in verbs {
        text = text.replace(token, verb);
    }
    text
}

/// Keeps each `[with keys|without]` span's side for `keys`.
fn spans(template: &str, keys: bool) -> String {
    let mut text = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('[') {
        let Some(len) = rest[open..].find(']') else {
            break;
        };
        text.push_str(&rest[..open]);
        let inner = &rest[open + 1..open + len];
        let (with, without) = inner.split_once('|').unwrap_or((inner, ""));
        text.push_str(if keys { with } else { without });
        rest = &rest[open + len + 1..];
    }
    text.push_str(rest);
    text
}

#[cfg(test)]
mod tests;
