//! The camera hands every view of the battlefield shares. Held keys pan and
//! the wheel zooms in live play and both read-only viewers; the viewers add
//! middle-drag, minimap steering, and finger pan and pinch through
//! [`ViewerHands`], and controller sticks and the Siri Remote's camera modes
//! through [`RemoteCamera`]. Live play keeps its own pointer funnel, since its
//! presses also select and command.

use super::Camera;
use crate::action::{Action, ActionResolver};
use crate::config::{CameraPrefs, PadPrefs};
use crate::viewer_touch::ViewerTouch;
use aftermine_protocol::{MouseButton, PadButton, RawEvent};
use macroquad::prelude::{Vec2, vec2};

/// The direction the held pan keys point, before normalizing.
pub(crate) fn held_pan(resolver: &ActionResolver) -> Vec2 {
    let mut direction = Vec2::ZERO;
    if resolver.is_held(Action::PanUp) {
        direction.y -= 1.0;
    }
    if resolver.is_held(Action::PanDown) {
        direction.y += 1.0;
    }
    if resolver.is_held(Action::PanLeft) {
        direction.x -= 1.0;
    }
    if resolver.is_held(Action::PanRight) {
        direction.x += 1.0;
    }
    direction
}

/// Pans `camera` along `direction` at the player's pan speed for `dt`
/// seconds; a zero direction stays put.
pub(crate) fn pan_toward(camera: &mut Camera, direction: Vec2, prefs: CameraPrefs, dt: f32) {
    let travel = pan_travel(camera, direction, prefs, dt);
    camera.pan(travel);
}

/// The world travel a pan along `direction` at the player's pan speed asks
/// for over `dt` seconds; zero for a zero direction.
pub(crate) fn pan_travel(camera: &Camera, direction: Vec2, prefs: CameraPrefs, dt: f32) -> Vec2 {
    if direction == Vec2::ZERO {
        return Vec2::ZERO;
    }
    let world_per_second = crate::input::PAN_PX_PER_SEC * prefs.pan_speed / camera.zoom;
    direction.normalize() * world_per_second * dt
}

/// How sharply pan speed rises with stick travel: speed follows travel to
/// this power, so a light push aims finely and a full push matches the pan
/// keys while both speeds are at their defaults.
const PAN_CURVE: f32 = 2.0;
/// Zoom steps per second at full right-stick travel, in wheel notches.
const ZOOM_NOTCHES_PER_SEC: f32 = 6.0;

/// The world travel a left stick at `left` asks for over `dt` seconds, at
/// `speed` times the pan keys' pace at full travel.
pub(crate) fn stick_travel(camera: &Camera, left: Vec2, speed: f32, dt: f32) -> Vec2 {
    if left == Vec2::ZERO {
        return Vec2::ZERO;
    }
    let pace = crate::input::PAN_PX_PER_SEC * speed * left.length().powf(PAN_CURVE);
    left.normalize() * pace * dt / camera.zoom
}

/// Zooms toward the screen point `at` for a right stick pushed `y` down over
/// `dt` seconds. Up zooms in, as the wheel rolled away does, unless the
/// player flipped the stick; the wheel's own setting leaves it alone.
pub(crate) fn stick_zoom(camera: &mut Camera, at: Vec2, y: f32, inverted: bool, dt: f32) {
    if y != 0.0 {
        let notches = -y * ZOOM_NOTCHES_PER_SEC * dt;
        camera.zoom_at(at, if inverted { -notches } else { notches });
    }
}

/// What a read-only viewer's camera needs to know about the controllers.
#[derive(Debug, Clone, Default)]
pub(crate) struct ViewerPads {
    /// The Siri Remotes connected.
    pub(crate) remotes: Vec<u64>,
    /// The left stick's position.
    pub(crate) left: Vec2,
    /// The right stick's position.
    pub(crate) right: Vec2,
    pub(crate) prefs: PadPrefs,
}

/// What the Siri Remote's swipes and ring do in a read-only viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteMode {
    /// They belong to the viewer's own controls.
    Control,
    /// A swipe drags the map and a held ring pans.
    Pan,
    /// Up and down zoom.
    Zoom,
}

impl RemoteMode {
    /// What a swipe does in this mode, as copy.
    fn swipe_hint(self) -> &'static str {
        match self {
            Self::Control => "Swipe to seek or change speed",
            Self::Pan => "Swipe or hold a direction to pan",
            Self::Zoom => "Swipe up or down, or hold, to zoom",
        }
    }

    /// What the strip of modes calls this one.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Control => "Replay",
            Self::Pan => "Pan",
            Self::Zoom => "Zoom",
        }
    }
}

/// The directions of a Siri Remote's ring.
const RING: [PadButton; 4] = [
    PadButton::DpadUp,
    PadButton::DpadDown,
    PadButton::DpadLeft,
    PadButton::DpadRight,
];

/// Which of [`RING`] a Siri Remote is holding.
#[derive(Debug, Clone, Copy, Default)]
struct HeldRing([bool; RING.len()]);

impl HeldRing {
    fn set(&mut self, button: PadButton, held: bool) {
        if let Some(at) = RING.iter().position(|ring| *ring == button) {
            self.0[at] = held;
        }
    }

    /// Where the ring points, as a stick at full travel would: x to the
    /// right and y down.
    fn direction(self) -> Vec2 {
        let [up, down, left, right] = self.0;
        let axis = |minus: bool, plus: bool| f32::from(u8::from(plus)) - f32::from(u8::from(minus));
        vec2(axis(left, right), axis(up, down)).clamp_length_max(1.0)
    }
}

/// Logical pixels the map moves per unit of swipe travel. The touch surface
/// spans 2, so a swipe across it moves the map about half a TV screen.
const SWIPE_PAN_PX: f32 = 480.0;
/// Wheel notches one swipe step zooms: about 30%.
const SWIPE_ZOOM_NOTCHES: f32 = 2.0;

/// The camera a read-only viewer gives controllers. Sticks move it directly.
/// The Siri Remote has only one surface, so a click steps it through modes
/// that say what its swipes and ring mean.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RemoteCamera {
    modes: &'static [RemoteMode],
    at: usize,
    held: HeldRing,
    /// The remote that last pressed the ring. Its leaving lets go, since a
    /// pad that disconnects may never report its release.
    held_by: Option<u64>,
}

impl Default for RemoteCamera {
    /// A viewer with no controls of its own.
    fn default() -> Self {
        Self {
            modes: &[RemoteMode::Pan, RemoteMode::Zoom],
            at: 0,
            held: HeldRing::default(),
            held_by: None,
        }
    }
}

impl RemoteCamera {
    /// A viewer whose own controls come first.
    pub(crate) fn with_control() -> Self {
        Self {
            modes: &[RemoteMode::Control, RemoteMode::Pan, RemoteMode::Zoom],
            at: 0,
            held: HeldRing::default(),
            held_by: None,
        }
    }

    /// What the remote's swipes do now.
    pub(crate) fn mode(self) -> RemoteMode {
        self.modes[self.at]
    }

    /// The modes a click steps through, in order.
    pub(crate) fn modes(self) -> &'static [RemoteMode] {
        self.modes
    }

    /// Where the current mode stands in [`Self::modes`].
    pub(crate) fn mode_index(self) -> usize {
        self.at
    }

    /// What the remote's swipes do now, as copy.
    pub(crate) fn swipe_hint(self) -> &'static str {
        self.mode().swipe_hint()
    }

    /// What the remote's swipes do now and how to change that, as copy.
    pub(crate) fn hint(self) -> String {
        format!("{} | Click to change mode", self.swipe_hint())
    }

    /// Moves `camera` by a frame's sticks and takes the remote's camera
    /// input. Returns the events left for the viewer: everything from other
    /// controllers, and from the remote what its mode leaves.
    pub(crate) fn route(
        &mut self,
        events: &[RawEvent],
        camera: &mut Camera,
        pads: &ViewerPads,
        dt: f32,
    ) -> Vec<RawEvent> {
        camera.pan(stick_travel(camera, pads.left, pads.prefs.stick_speed, dt));
        let center = camera.viewport() * 0.5;
        stick_zoom(camera, center, pads.right.y, pads.prefs.zoom_inverted, dt);
        let remote = |pad| pads.remotes.contains(&pad);
        let mut rest = Vec::with_capacity(events.len());
        for event in events {
            match *event {
                RawEvent::PadDown { pad, button }
                    if remote(pad) && self.press(pad, button, camera, pads.prefs) => {}
                RawEvent::PadUp { pad, button } if remote(pad) => {
                    self.held.set(button, false);
                    rest.push(*event);
                }
                RawEvent::PadDisconnected { pad } => {
                    if self.held_by == Some(pad) {
                        self.held = HeldRing::default();
                    }
                    rest.push(*event);
                }
                RawEvent::PadSwipe { pad, x, y } if remote(pad) => {
                    if self.mode() == RemoteMode::Pan {
                        // The map follows the finger, as under a finger on iPad.
                        camera.pan(-vec2(x, y) * SWIPE_PAN_PX / camera.zoom);
                    }
                }
                _ => rest.push(*event),
            }
        }
        // A swipe's tap goes down and up in one batch, so only a press that
        // stays down moves anything here.
        let ring = self.held.direction();
        match self.mode() {
            RemoteMode::Control => {}
            RemoteMode::Pan => camera.pan(stick_travel(camera, ring, pads.prefs.stick_speed, dt)),
            RemoteMode::Zoom => {
                stick_zoom(camera, center, ring.y, pads.prefs.zoom_inverted, dt);
            }
        }
        rest
    }

    /// One remote press. Returns whether the camera took it.
    fn press(&mut self, pad: u64, button: PadButton, camera: &mut Camera, prefs: PadPrefs) -> bool {
        let center = camera.viewport() * 0.5;
        let flip = if prefs.zoom_inverted { -1.0 } else { 1.0 };
        match (self.mode(), button) {
            (_, crate::pad::CONFIRM) => {
                self.at = (self.at + 1) % self.modes.len();
                self.held = HeldRing::default();
            }
            (RemoteMode::Zoom, PadButton::DpadUp) => {
                self.hold(pad, button);
                camera.zoom_at(center, SWIPE_ZOOM_NOTCHES * flip);
            }
            (RemoteMode::Zoom, PadButton::DpadDown) => {
                self.hold(pad, button);
                camera.zoom_at(center, -SWIPE_ZOOM_NOTCHES * flip);
            }
            (
                RemoteMode::Pan | RemoteMode::Zoom,
                PadButton::DpadUp
                | PadButton::DpadDown
                | PadButton::DpadLeft
                | PadButton::DpadRight,
            ) => self.hold(pad, button),
            _ => return false,
        }
        true
    }

    fn hold(&mut self, pad: u64, button: PadButton) {
        self.held.set(button, true);
        self.held_by = Some(pad);
    }
}

/// Zooms toward the screen point `at` by one wheel reading, honoring the
/// inverted-zoom preference.
pub(crate) fn wheel_zoom(camera: &mut Camera, at: Vec2, delta: f32, prefs: CameraPrefs) {
    camera.zoom_at(at, if prefs.zoom_inverted { -delta } else { delta });
}

/// A pointer's place against the minimap, read from the scene before the
/// camera is borrowed to move.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MinimapPoint {
    /// The world point under the pointer, when it is on the minimap.
    pub(crate) under: Option<Vec2>,
    /// Where a held press steers, clamped so sliding off the edge doesn't
    /// stall the pan.
    pub(crate) clamped: Option<Vec2>,
}

/// The read-only viewers' camera hands: middle-drag, the wheel, minimap
/// steering by mouse or finger, and finger pan and pinch.
#[derive(Default)]
pub(crate) struct ViewerHands {
    middle_anchor: Option<Vec2>,
    minimap_drag: bool,
    minimap_finger: Option<u64>,
    touch: ViewerTouch,
}

impl ViewerHands {
    /// Applies one pointer event to `camera`, tracking the cursor in `mouse`.
    pub(crate) fn pointer(
        &mut self,
        event: &RawEvent,
        minimap: MinimapPoint,
        camera: &mut Camera,
        mouse: &mut Vec2,
        prefs: CameraPrefs,
        ui: f32,
    ) {
        match *event {
            RawEvent::MouseMove { x, y } => {
                *mouse = vec2(x, y);
                if let Some(anchor) = self.middle_anchor {
                    camera.pan((anchor - *mouse) / camera.zoom);
                    self.middle_anchor = Some(*mouse);
                }
                if self.minimap_drag {
                    steer(camera, minimap.clamped);
                }
            }
            RawEvent::MouseDown {
                button: MouseButton::Left,
                x,
                y,
            } => {
                *mouse = vec2(x, y);
                if let Some(world) = minimap.under {
                    jump(camera, world);
                    self.minimap_drag = true;
                }
            }
            RawEvent::MouseUp {
                button: MouseButton::Left,
                ..
            } => self.minimap_drag = false,
            RawEvent::Wheel { delta } => wheel_zoom(camera, *mouse, delta, prefs),
            RawEvent::MouseDown {
                button: MouseButton::Middle,
                x,
                y,
            } => self.middle_anchor = Some(vec2(x, y)),
            RawEvent::MouseUp {
                button: MouseButton::Middle,
                ..
            } => self.middle_anchor = None,
            RawEvent::TouchDown { id, .. } => {
                if self.minimap_finger.is_none()
                    && let Some(world) = minimap.under
                {
                    self.minimap_finger = Some(id);
                    jump(camera, world);
                } else {
                    self.touch.apply(event, camera, ui);
                }
            }
            RawEvent::TouchMove { id, .. } => {
                if self.minimap_finger == Some(id) {
                    steer(camera, minimap.clamped);
                } else {
                    self.touch.apply(event, camera, ui);
                }
            }
            RawEvent::TouchUp { id, .. } | RawEvent::TouchCancel { id } => {
                if self.minimap_finger == Some(id) {
                    self.minimap_finger = None;
                } else {
                    self.touch.apply(event, camera, ui);
                }
            }
            _ => {}
        }
    }
}

fn jump(camera: &mut Camera, world: Vec2) {
    camera.center = world;
    camera.pan(Vec2::ZERO);
}

fn steer(camera: &mut Camera, clamped: Option<Vec2>) {
    if let Some(world) = clamped {
        jump(camera, world);
    }
}

#[cfg(test)]
mod tests;
