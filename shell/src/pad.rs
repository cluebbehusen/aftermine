//! Game controllers: the hardware adapter between a platform's pad API and
//! the input funnel.
//!
//! A platform source reports [`Reading`]s, and [`Pads`] turns them into the
//! same pad [`RawEvent`]s the debug protocol injects. Rules only hardware
//! needs live here, so injected input arrives already clean: stick dead
//! zones, sending a stick only when it moves, dropping states the funnel
//! already has, and releasing a pad's buttons and centering its sticks
//! before it disconnects. A source may report each change as it happens or
//! restate every control each frame; both produce the same events.
//!
//! gilrs is the source on Windows and Linux, Steam Deck included, and
//! Apple's GameController, through the miniquad fork, on Mac and iPad. Each
//! platform API gets one source module here, and a platform without one
//! reads no pads.

use aftermine_protocol::{Key, PadButton, PadFamily, RawEvent, Stick};
use macroquad::prelude::Vec2;
use std::collections::BTreeMap;

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod gilrs_source;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use gilrs_source::Source;

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple_source;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use apple_source::Source;

/// Stick travel, as a share of full deflection, that still reads as
/// centered: worn sticks rest this far off center.
const INNER_DEAD_ZONE: f32 = 0.2;
/// Stick travel that already reads as full deflection, since many sticks
/// stop short of the rim.
const OUTER_DEAD_ZONE: f32 = 0.95;
/// How far a stick moves from the position last sent before it is sent
/// again. Steps this small are invisible in camera speed and keep a noisy
/// stick from sending every frame.
const STICK_STEP: f32 = 1.0 / 32.0;
/// A sent position this long counts as full deflection.
const FULL_TRAVEL: f32 = 0.999;

/// The button that confirms in menus. Position decides, so it is the same
/// button on every family whatever its label.
pub(crate) const CONFIRM: PadButton = PadButton::South;
/// The button that goes back in menus.
pub(crate) const BACK: PadButton = PadButton::East;

/// The buttons that step tabs in menus, back and forward.
pub(crate) const TABS: (PadButton, PadButton) = (PadButton::LeftShoulder, PadButton::RightShoulder);
/// The buttons that page menus, up and down.
pub(crate) const PAGES: (PadButton, PadButton) = (PadButton::LeftTrigger, PadButton::RightTrigger);
/// The button that deletes the saved game or replay under the menu cursor.
pub(crate) const DELETE: PadButton = PadButton::West;

/// The canonical menu keys controller buttons press: the D-pad moves the
/// cursor, the bumpers press the tab keys, the triggers page and the left
/// face button presses the delete key. Key bindings never move this
/// layout.
const MENU_KEYS: [(PadButton, Key); 11] = [
    (PadButton::DpadUp, Key::Up),
    (PadButton::DpadDown, Key::Down),
    (PadButton::DpadLeft, Key::Left),
    (PadButton::DpadRight, Key::Right),
    (CONFIRM, Key::Enter),
    (BACK, Key::Escape),
    (DELETE, Key::X),
    (TABS.0, Key::Q),
    (TABS.1, Key::E),
    (PAGES.0, Key::PageUp),
    (PAGES.1, Key::PageDown),
];

/// The canonical menu key a controller button presses, if any.
fn menu_key(button: PadButton) -> Option<Key> {
    MENU_KEYS
        .iter()
        .find(|(pad, _)| *pad == button)
        .map(|(_, key)| *key)
}

/// Turns controller presses into the canonical menu keys of their fixed
/// layout, which every menu screen reads, and drops buttons with no menu
/// role. Other events pass through. Run it after the key bindings, so the
/// keys it makes are never bound again.
pub(crate) fn menu_keys(events: &[RawEvent]) -> Vec<RawEvent> {
    events
        .iter()
        .filter_map(|event| match *event {
            RawEvent::PadDown { button, .. } => {
                menu_key(button).map(|key| RawEvent::KeyDown { key })
            }
            RawEvent::PadUp { button, .. } => menu_key(button).map(|key| RawEvent::KeyUp { key }),
            other => Some(other),
        })
        .collect()
}

/// A controller button's icon, named by what it shows: families that print
/// the same label share one. The order matches the shell's atlas key table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PadIcon {
    A,
    B,
    X,
    Y,
    Cross,
    Circle,
    Square,
    Triangle,
    Lb,
    Rb,
    L1,
    R1,
    L,
    R,
    Lt,
    Rt,
    L2,
    R2,
    Zl,
    Zr,
    Ls,
    Rs,
    L3,
    R3,
    View,
    Menu,
    Create,
    Options,
    Minus,
    Plus,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    /// The D-pad's up and down arms: moving through a list.
    DpadUpDown,
}

impl PadIcon {
    /// Every icon, in declaration order.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 35] = [
        Self::A,
        Self::B,
        Self::X,
        Self::Y,
        Self::Cross,
        Self::Circle,
        Self::Square,
        Self::Triangle,
        Self::Lb,
        Self::Rb,
        Self::L1,
        Self::R1,
        Self::L,
        Self::R,
        Self::Lt,
        Self::Rt,
        Self::L2,
        Self::R2,
        Self::Zl,
        Self::Zr,
        Self::Ls,
        Self::Rs,
        Self::L3,
        Self::R3,
        Self::View,
        Self::Menu,
        Self::Create,
        Self::Options,
        Self::Minus,
        Self::Plus,
        Self::DpadUp,
        Self::DpadDown,
        Self::DpadLeft,
        Self::DpadRight,
        Self::DpadUpDown,
    ];

    /// What the icon shows, as copy. It stays ASCII, so PlayStation's
    /// shapes are spelled out.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::X => "X",
            Self::Y => "Y",
            Self::Cross => "Cross",
            Self::Circle => "Circle",
            Self::Square => "Square",
            Self::Triangle => "Triangle",
            Self::Lb => "LB",
            Self::Rb => "RB",
            Self::L1 => "L1",
            Self::R1 => "R1",
            Self::L => "L",
            Self::R => "R",
            Self::Lt => "LT",
            Self::Rt => "RT",
            Self::L2 => "L2",
            Self::R2 => "R2",
            Self::Zl => "ZL",
            Self::Zr => "ZR",
            Self::Ls => "LS",
            Self::Rs => "RS",
            Self::L3 => "L3",
            Self::R3 => "R3",
            Self::View => "View",
            Self::Menu => "Menu",
            Self::Create => "Create",
            Self::Options => "Options",
            Self::Minus => "Minus",
            Self::Plus => "Plus",
            Self::DpadUp => "D-pad up",
            Self::DpadDown => "D-pad down",
            Self::DpadLeft => "D-pad left",
            Self::DpadRight => "D-pad right",
            Self::DpadUpDown => "D-pad",
        }
    }
}

/// The icon a family prints on a button. Switch pads swap the face labels
/// Xbox pads print at the same positions, and the Deck labels its
/// shoulders, triggers and stick clicks as PlayStation does.
pub(crate) fn button_icon(family: PadFamily, button: PadButton) -> PadIcon {
    use PadButton as B;
    use PadFamily as F;
    use PadIcon as I;
    match (family, button) {
        (_, B::DpadUp) => I::DpadUp,
        (_, B::DpadDown) => I::DpadDown,
        (_, B::DpadLeft) => I::DpadLeft,
        (_, B::DpadRight) => I::DpadRight,
        (F::PlayStation, B::South) => I::Cross,
        (F::PlayStation, B::East) => I::Circle,
        (F::PlayStation, B::West) => I::Square,
        (F::PlayStation, B::North) => I::Triangle,
        (F::PlayStation | F::SteamDeck, B::LeftShoulder) => I::L1,
        (F::PlayStation | F::SteamDeck, B::RightShoulder) => I::R1,
        (F::PlayStation | F::SteamDeck, B::LeftTrigger) => I::L2,
        (F::PlayStation | F::SteamDeck, B::RightTrigger) => I::R2,
        (F::PlayStation | F::SteamDeck, B::LeftStick) => I::L3,
        (F::PlayStation | F::SteamDeck, B::RightStick) => I::R3,
        (F::PlayStation, B::Select) => I::Create,
        (F::PlayStation, B::Start) => I::Options,
        (F::Switch, B::South) => I::B,
        (F::Switch, B::East) => I::A,
        (F::Switch, B::West) => I::Y,
        (F::Switch, B::North) => I::X,
        (F::Switch, B::LeftShoulder) => I::L,
        (F::Switch, B::RightShoulder) => I::R,
        (F::Switch, B::LeftTrigger) => I::Zl,
        (F::Switch, B::RightTrigger) => I::Zr,
        (F::Switch, B::Select) => I::Minus,
        (F::Switch, B::Start) => I::Plus,
        (_, B::South) => I::A,
        (_, B::East) => I::B,
        (_, B::West) => I::X,
        (_, B::North) => I::Y,
        (_, B::LeftShoulder) => I::Lb,
        (_, B::RightShoulder) => I::Rb,
        (_, B::LeftTrigger) => I::Lt,
        (_, B::RightTrigger) => I::Rt,
        (_, B::LeftStick) => I::Ls,
        (_, B::RightStick) => I::Rs,
        (_, B::Select) => I::View,
        (_, B::Start) => I::Menu,
    }
}

/// What a family calls a button: the name of the icon it prints there.
pub(crate) fn button_name(family: PadFamily, button: PadButton) -> &'static str {
    button_icon(family, button).name()
}

/// One thing a platform's pad API reported.
#[derive(Debug, Clone, Copy, PartialEq)]
// Only platforms with a pad source construct readings.
#[cfg_attr(
    not(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "macos",
        target_os = "ios"
    )),
    allow(dead_code)
)]
pub(crate) enum Reading {
    /// A pad is connected. Sources announce a pad before anything else
    /// about it; readings from a pad never announced are dropped.
    Connected { pad: u64, family: PadFamily },
    /// A pad is gone.
    Disconnected { pad: u64 },
    /// A button is down or up.
    Button {
        pad: u64,
        button: PadButton,
        down: bool,
    },
    /// One axis of a stick, from -1 to 1: x to the right, y down.
    Axis {
        pad: u64,
        stick: Stick,
        axis: Axis,
        value: f32,
    },
}

/// A stick's two axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "macos",
        target_os = "ios"
    )),
    allow(dead_code)
)]
pub(crate) enum Axis {
    X,
    Y,
}

/// The platform's pads, read once per frame into the funnel.
pub(crate) struct Pads {
    /// Absent where the platform has no pad API or it failed to start.
    source: Option<Source>,
    adapter: Adapter,
    readings: Vec<Reading>,
}

impl Pads {
    /// Starts reading this platform's pads. A platform API that fails to
    /// start reports why on stderr and reads no pads; the game plays on.
    pub(crate) fn open() -> Self {
        Self {
            source: Source::open(),
            adapter: Adapter::default(),
            readings: Vec::new(),
        }
    }

    /// Appends the pad events since the last poll.
    pub(crate) fn poll(&mut self, events: &mut Vec<RawEvent>) {
        if let Some(source) = &mut self.source {
            source.read(&mut self.readings);
        }
        for reading in self.readings.drain(..) {
            self.adapter.feed(reading, events);
        }
    }
}

/// A platform without a pad API the shell reads never has a source.
#[cfg(not(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "macos",
    target_os = "ios"
)))]
enum Source {}

#[cfg(not(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "macos",
    target_os = "ios"
)))]
impl Source {
    fn open() -> Option<Self> {
        None
    }

    fn read(&mut self, _readings: &mut Vec<Reading>) {
        match *self {}
    }
}

/// What the funnel has been told about each connected pad.
#[derive(Debug, Default)]
struct Adapter {
    pads: BTreeMap<u64, PadState>,
}

#[derive(Debug, Default)]
struct PadState {
    /// Held buttons, in press order.
    held: Vec<PadButton>,
    left: StickState,
    right: StickState,
}

impl PadState {
    fn stick(&mut self, stick: Stick) -> &mut StickState {
        match stick {
            Stick::Left => &mut self.left,
            Stick::Right => &mut self.right,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct StickState {
    /// The latest axes the source reported.
    raw: Vec2,
    /// The position last sent.
    sent: Vec2,
}

impl Adapter {
    fn feed(&mut self, reading: Reading, events: &mut Vec<RawEvent>) {
        match reading {
            Reading::Connected { pad, family } => {
                if let std::collections::btree_map::Entry::Vacant(slot) = self.pads.entry(pad) {
                    slot.insert(PadState::default());
                    events.push(RawEvent::PadConnected { pad, family });
                }
            }
            Reading::Disconnected { pad } => {
                let Some(state) = self.pads.remove(&pad) else {
                    return;
                };
                events.extend(
                    state
                        .held
                        .into_iter()
                        .map(|button| RawEvent::PadUp { pad, button }),
                );
                for (stick, at) in [(Stick::Left, state.left), (Stick::Right, state.right)] {
                    if at.sent != Vec2::ZERO {
                        events.push(RawEvent::PadStick {
                            pad,
                            stick,
                            x: 0.0,
                            y: 0.0,
                        });
                    }
                }
                events.push(RawEvent::PadDisconnected { pad });
            }
            Reading::Button { pad, button, down } => {
                let Some(state) = self.pads.get_mut(&pad) else {
                    return;
                };
                match (down, state.held.iter().position(|held| *held == button)) {
                    (true, None) => {
                        state.held.push(button);
                        events.push(RawEvent::PadDown { pad, button });
                    }
                    (false, Some(index)) => {
                        state.held.remove(index);
                        events.push(RawEvent::PadUp { pad, button });
                    }
                    _ => {}
                }
            }
            Reading::Axis {
                pad,
                stick,
                axis,
                value,
            } => {
                let Some(state) = self.pads.get_mut(&pad) else {
                    return;
                };
                let at = state.stick(stick);
                let value = if value.is_finite() {
                    value.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
                match axis {
                    Axis::X => at.raw.x = value,
                    Axis::Y => at.raw.y = value,
                }
                let now = dead_zoned(at.raw);
                if worth_sending(at.sent, now) {
                    at.sent = now;
                    events.push(RawEvent::PadStick {
                        pad,
                        stick,
                        x: now.x,
                        y: now.y,
                    });
                }
            }
        }
    }
}

/// A raw stick position with both dead zones applied: centered inside the
/// inner zone, full deflection past the outer, and the travel between
/// stretched over 0 to 1 so pushing just past the inner zone moves slowly
/// rather than jumping.
fn dead_zoned(raw: Vec2) -> Vec2 {
    let length = raw.length();
    if length <= INNER_DEAD_ZONE {
        return Vec2::ZERO;
    }
    let travel = ((length - INNER_DEAD_ZONE) / (OUTER_DEAD_ZONE - INNER_DEAD_ZONE)).min(1.0);
    raw / length * travel
}

/// Whether a stick has moved far enough from the position last sent to be
/// sent again. Leaving center, returning to it, and reaching full
/// deflection always send, so a resting stick reads exactly centered and a
/// stick pushed all the way reads full speed.
fn worth_sending(sent: Vec2, now: Vec2) -> bool {
    let full = |at: Vec2| at.length() >= FULL_TRAVEL;
    now != sent
        && (now == Vec2::ZERO
            || sent == Vec2::ZERO
            || (full(now) && !full(sent))
            || now.distance(sent) >= STICK_STEP)
}

#[cfg(test)]
mod tests;
