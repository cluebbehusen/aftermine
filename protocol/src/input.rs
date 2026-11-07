//! Raw input events: the shell's single input funnel.
//!
//! Every frame the shell turns whatever macroquad and the platform's
//! controller API report (and whatever arrived via
//! [`crate::Request::InjectEvent`]) into a list of these, then
//! maps them to camera operations, selection changes, and sim commands.
//! Injected and hardware events take the same path, so presentation-layer
//! tests need no OS-level input faking.
//!
//! Touch variants flow through the shell's touch handling (tap select, drag
//! pan, pinch zoom).
//!
//! Pad variants carry game controllers. Buttons are named by where they
//! sit, so one event means one physical button on every family of pad. The
//! shell's hardware adapter applies stick dead zones and sends a stick only
//! when it moves, so hardware and injected sticks carry the same clean
//! values.

use serde::{Deserialize, Serialize};

/// One input event, in logical window pixels where applicable.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RawEvent {
    /// Cursor moved.
    MouseMove {
        /// Window x in pixels.
        x: f32,
        /// Window y in pixels.
        y: f32,
    },
    /// Button pressed.
    MouseDown {
        /// Which button.
        button: MouseButton,
        /// Window x.
        x: f32,
        /// Window y.
        y: f32,
    },
    /// Button released.
    MouseUp {
        /// Which button.
        button: MouseButton,
        /// Window x.
        x: f32,
        /// Window y.
        y: f32,
    },
    /// Scroll wheel; positive is away from the user (zoom in).
    Wheel {
        /// Scroll amount in notches.
        delta: f32,
    },
    /// Key pressed.
    KeyDown {
        /// Which key.
        key: Key,
    },
    /// Key released.
    KeyUp {
        /// Which key.
        key: Key,
    },
    /// Touch began (mobile; unused on desktop).
    TouchDown {
        /// Stable touch id.
        id: u64,
        /// Window x.
        x: f32,
        /// Window y.
        y: f32,
    },
    /// Touch moved.
    TouchMove {
        /// Stable touch id.
        id: u64,
        /// Window x.
        x: f32,
        /// Window y.
        y: f32,
    },
    /// Touch ended.
    TouchUp {
        /// Stable touch id.
        id: u64,
        /// Window x.
        x: f32,
        /// Window y.
        y: f32,
    },
    /// The platform took a touch away (a system gesture, palm
    /// rejection). The finger ends without completing any gesture.
    TouchCancel {
        /// Stable touch id.
        id: u64,
    },
    /// A typed character, for text entry (save names). The shell emits
    /// these only for printable ASCII, since UI strings stay ASCII and the
    /// menu font is Latin-1. Screens consume them only while a text field
    /// has focus; letters stay semantic everywhere else.
    Text {
        /// The character as typed (layout- and shift-resolved).
        ch: char,
    },
    /// A game controller connected. One already connected when the shell
    /// starts arrives this way too, before any of its presses.
    PadConnected {
        /// Stable pad id while it stays connected.
        pad: u64,
        /// Whose layout it follows, which names its buttons.
        family: PadFamily,
    },
    /// A game controller disconnected. The hardware adapter releases its
    /// held buttons and centers its sticks first.
    PadDisconnected {
        /// The pad's id.
        pad: u64,
    },
    /// A controller button pressed.
    PadDown {
        /// The pad's id.
        pad: u64,
        /// Which button.
        button: PadButton,
    },
    /// A controller button released.
    PadUp {
        /// The pad's id.
        pad: u64,
        /// Which button.
        button: PadButton,
    },
    /// A stick's new position. Each axis runs from -1 to 1, x to the right
    /// and y down like window coordinates; (0, 0) is centered, which is
    /// also where the dead zone puts a resting stick.
    PadStick {
        /// The pad's id.
        pad: u64,
        /// Which stick.
        stick: Stick,
        /// Rightward deflection.
        x: f32,
        /// Downward deflection.
        y: f32,
    },
}

#[cfg(test)]
mod tests;

/// Mouse buttons the shell cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    /// Select / drag-select.
    Left,
    /// Context order (move / attack / harvest).
    Right,
    /// Camera drag-pan.
    Middle,
}

/// A controller button, named by position: the bottom face button is
/// [`PadButton::South`] whether the pad prints A, Cross, or B on it. A
/// platform API that names buttons by label maps them to positions in its
/// adapter. The guide button belongs to the system and never arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadButton {
    /// Bottom face button.
    South,
    /// Right face button.
    East,
    /// Left face button.
    West,
    /// Top face button.
    North,
    /// D-pad up.
    DpadUp,
    /// D-pad down.
    DpadDown,
    /// D-pad left.
    DpadLeft,
    /// D-pad right.
    DpadRight,
    /// Left shoulder button.
    LeftShoulder,
    /// Right shoulder button.
    RightShoulder,
    /// Left trigger, pressed past the platform's threshold.
    LeftTrigger,
    /// Right trigger, pressed past the platform's threshold.
    RightTrigger,
    /// Left stick pressed in.
    LeftStick,
    /// Right stick pressed in.
    RightStick,
    /// Left center button (View, Create, Minus).
    Select,
    /// Right center button (Menu, Options, Plus).
    Start,
}

/// A controller's analog stick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stick {
    /// Left stick.
    Left,
    /// Right stick.
    Right,
}

/// Whose button layout a pad follows. It decides what a button is called,
/// never what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PadFamily {
    /// Xbox pads, and pads Steam Input presents in Xbox's layout.
    Xbox,
    /// PlayStation pads.
    PlayStation,
    /// Nintendo Switch pads.
    Switch,
    /// The Steam Deck's built-in controls: Xbox's layout, with the Deck's
    /// labels on its shoulders, triggers and stick clicks.
    SteamDeck,
    /// A pad of unknown make; it is named like an Xbox pad.
    Other,
}

/// The keys the shell maps; extend alongside the input mapper. The shell's
/// binding map assigns their actions, so variant docs describe default
/// bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    /// Contextual navigation shortcut.
    Tab,
    /// Pan up.
    Up,
    /// Pan down.
    Down,
    /// Pan left.
    Left,
    /// Pan right.
    Right,
    /// Letter H.
    H,
    /// Letter S.
    S,
    /// Letter A.
    A,
    /// Pause / unpause.
    P,
    /// Arm a patrol route; pressed again, starts it.
    R,
    /// Open the build palette (harvester selected).
    B,
    /// Select and center the next idle own Harvester.
    N,
    /// Stop, scrap the selected construction site, or clear focus.
    X,
    /// Activate the highlighted menu item.
    Enter,
    /// Deselect.
    Escape,
    /// Center the camera on your Foundry.
    Space,
    /// Toggle debug overlay.
    F1,
    /// Modifier: additive selection (either physical shift key).
    Shift,
    /// Modifier: control-group assignment (either physical ctrl key).
    Ctrl,
    /// Control group 1 (recall; with Ctrl, assign).
    Num1,
    /// Control group 2.
    Num2,
    /// Control group 3.
    Num3,
    /// Control group 4.
    Num4,
    /// Control group 5.
    Num5,
    /// Sixth contextual digit (build palette / production slots; no
    /// control group behind it).
    Num6,
    /// Seventh contextual digit.
    Num7,
    /// Eighth contextual digit.
    Num8,
    /// Ninth contextual digit.
    Num9,
    /// Page up (menu scrolling).
    PageUp,
    /// Page down (menu scrolling).
    PageDown,
    /// Home (jump to list start).
    Home,
    /// End (jump to list end).
    End,
    /// Camera bookmark keys.
    F5,
    /// Camera bookmark keys.
    F6,
    /// Camera bookmark keys.
    F7,
    /// Camera bookmark keys.
    F8,
    /// Letter C; physical keys are interpreted by the shell binding map.
    C,
    /// See [`Key::C`].
    D,
    /// See [`Key::C`].
    E,
    /// Arm hunt in the classic profile.
    F,
    /// See [`Key::C`].
    G,
    /// See [`Key::C`].
    I,
    /// See [`Key::C`].
    J,
    /// Arm scout in the classic profile.
    K,
    /// See [`Key::C`].
    L,
    /// See [`Key::C`].
    M,
    /// See [`Key::C`].
    O,
    /// See [`Key::C`].
    Q,
    /// See [`Key::C`].
    T,
    /// See [`Key::C`].
    U,
    /// See [`Key::C`].
    V,
    /// See [`Key::C`].
    W,
    /// See [`Key::C`].
    Y,
    /// See [`Key::C`].
    Z,
    /// Delete the character before the caret (text entry only; not a
    /// bindable game verb).
    Backspace,
}
