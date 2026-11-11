//! Platform facts the shell's presentation depends on, and the hands the
//! player is using right now.

use std::cell::Cell;

use crate::input::Pointer;
use aftermine_protocol::PadFamily;

/// The kind of device a build runs on. Platform facts (Quit, the system
/// keyboard) and fingertip sizing follow it; wording and hover follow
/// [`Hands`] instead. Code branches on [`PLATFORM`] rather than `#[cfg]`
/// so every side compiles, lints, and tests on every target; pure helpers
/// take the platform, or the fact they need, as a parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    /// Windows, macOS, and Linux: a mouse and keyboard, a command line,
    /// and a window the player closes.
    Desktop,
    /// iOS, where a finger is always at hand even if a keyboard or
    /// trackpad is attached.
    Touch,
    /// tvOS: no touch screen and no cursor, played from across the room.
    Tv,
}

/// The platform this build runs on.
pub(crate) const PLATFORM: Platform = if cfg!(target_os = "ios") {
    Platform::Touch
} else if cfg!(target_os = "tvos") {
    Platform::Tv
} else {
    Platform::Desktop
};

impl Platform {
    /// Whether a finger is always at hand, so controls take fingertip
    /// sizes and stay clear of the screen's rounded corners.
    pub(crate) const fn touch_only(self) -> bool {
        matches!(self, Self::Touch)
    }

    /// Whether this is a desktop app: it offers Quit, reads its command
    /// line, has a cursor from launch, and opens folders in a file
    /// manager. Elsewhere the system closes apps and the player types
    /// through the system keyboard.
    pub(crate) const fn desktop(self) -> bool {
        matches!(self, Self::Desktop)
    }
}

/// The pointer, keys, and controller the player is using, which decide how
/// copy speaks: tap or click, whether it names keys, and whether it names
/// a controller's buttons instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hands {
    /// The pointer used last.
    pub pointer: Pointer,
    /// Whether a hardware key has been pressed.
    pub keys: bool,
    /// The controller in use, by the family that names its buttons: one
    /// was pressed after the last key or pointer.
    pub pad: Option<PadFamily>,
}

impl Hands {
    /// What this build assumes before any input.
    pub const BUILD: Self = Self::at_launch(PLATFORM);

    /// What `platform` assumes before any input: a mouse and keyboard on
    /// the desktop, a finger alone on a touch device. A TV has neither a
    /// keyboard nor a cursor, so nothing hovers and copy names no keys, and
    /// every TV has its remote, so copy names that.
    pub const fn at_launch(platform: Platform) -> Self {
        match platform {
            Platform::Desktop => Self {
                pointer: Pointer::Mouse,
                keys: true,
                pad: None,
            },
            Platform::Touch => Self {
                pointer: Pointer::Touch,
                keys: false,
                pad: None,
            },
            Platform::Tv => Self {
                pointer: Pointer::Touch,
                keys: false,
                pad: Some(PadFamily::Remote),
            },
        }
    }

    /// Whether the pointer in use is a finger.
    pub fn touch(self) -> bool {
        self.pointer == Pointer::Touch
    }

    /// Whether a pointer's hover may light what lies under it. While a
    /// controller is in use its cursor is the only highlight, so a mouse
    /// left parked over another item never lights a second one.
    pub fn hover_shown(self) -> bool {
        self.pad.is_none()
    }

    /// Whether screens on `platform` stand in pad mode, leaving out the
    /// buttons only a pointer can press because a controller's buttons do
    /// what they do: while a controller is in use, and always on a TV,
    /// which has no pointer. A click, tap or key leaves it; a mouse that
    /// only moves does not.
    pub fn pad_mode(self, platform: Platform) -> bool {
        self.pad.is_some() || platform == Platform::Tv
    }
}

thread_local! {
    /// This frame's hands. The build's default, so tests and headless
    /// runs see the copy they always have.
    static HANDS: Cell<Hands> = const { Cell::new(Hands::BUILD) };
}

/// Publishes this frame's hands for the copy code.
pub(crate) fn set_hands(hands: Hands) {
    HANDS.with(|cell| cell.set(hands));
}

/// This frame's hands.
pub(crate) fn hands() -> Hands {
    HANDS.with(Cell::get)
}

/// Whether this frame's screens stand in pad mode; see [`Hands::pad_mode`].
pub(crate) fn pad_mode() -> bool {
    hands().pad_mode(PLATFORM)
}

/// Whether `rect` is lit as hovered by the pointer at `mouse` this frame.
pub(crate) fn hovers(rect: macroquad::prelude::Rect, mouse: macroquad::prelude::Vec2) -> bool {
    hands().hover_shown() && rect.contains(mouse)
}

/// The outline the menu cursor wears this frame: the focus weight, and
/// heavier while a controller is in use, whose player often reads the
/// screen from further away and has no pointer to find their place by.
pub(crate) fn cursor_stroke() -> crate::theme::Stroke {
    if hands().pad.is_some() {
        crate::theme::Stroke::Heavy
    } else {
        crate::theme::Stroke::Focus
    }
}

/// The pointer verb opening an instruction: a touch player taps what a
/// mouse player clicks.
pub(crate) fn tap_or_click_capitalized(touch_only: bool) -> &'static str {
    if touch_only { "Tap" } else { "Click" }
}

#[cfg(test)]
pub(crate) mod tests;
