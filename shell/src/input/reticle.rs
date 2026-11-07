//! The match's controller pointer: a reticle at the center of the screen.
//!
//! The left stick moves the camera under the reticle, faster the further it
//! is pushed, and the right stick zooms about it. Where the camera meets the
//! map's edge, further travel moves the reticle off center instead, and
//! travel back spends that drift before the camera moves again. The reticle
//! snaps onto a unit or structure near it, and the D-pad and a right-stick
//! click jump the camera. Menus never see this: they read pad buttons as
//! menu keys.

pub(crate) use super::select::Picked;
use super::select::pick;
use super::{InputState, Pointer, dispatch_action};
use crate::action::{Action, BindingMap};
use crate::camera::Camera;
use crate::game::Game;
use aftermine_protocol::PadButton;
use macroquad::prelude::Vec2;

/// How sharply pan speed rises with stick travel: speed follows travel to
/// this power, so a light push aims finely and a full push matches the pan
/// keys.
const PAN_CURVE: f32 = 2.0;
/// The share of pan speed left while the reticle rests on something, so a
/// passing unit is easy to stop on.
const SNAP_FRICTION: f32 = 0.5;
/// Zoom steps per second at full right-stick travel, in wheel notches.
const ZOOM_NOTCHES_PER_SEC: f32 = 6.0;
/// Logical pixels the drifting reticle keeps from the window's edge.
const DRIFT_MARGIN: f32 = 32.0;
/// Leftover travel under this many pixels is rounding, not a clamp.
const DRIFT_EPSILON: f32 = 0.01;

/// Where the reticle sits and what it rests on.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Reticle {
    /// The viewport's center, as of the last frame.
    center: Vec2,
    /// How far the reticle sits from center, in logical pixels, after
    /// the camera met the map's edge.
    drift: Vec2,
    /// The unit or structure the reticle has snapped onto.
    snap: Option<Picked>,
}

impl Reticle {
    /// The reticle's screen point.
    pub(crate) fn point(&self) -> Vec2 {
        self.center + self.drift
    }

    /// The unit or structure the reticle has snapped onto.
    pub(crate) fn snap(&self) -> Option<Picked> {
        self.snap
    }
}

/// Pans and zooms by the held sticks, then settles what the reticle rests
/// on. Runs every frame of a match: sticks send only when they move.
pub(super) fn update(game: &mut Game, input: &mut InputState, dt: f32) {
    let prefs = input.camera_prefs;
    let ui = input.ui;
    let (left, right) = input.sticks();
    let camera = &mut game.presentation.camera;
    let reticle = &mut input.reticle;
    reticle.center = camera.viewport() * 0.5;
    let bound = drift_bound(camera, ui);
    // Where the map's edge held back this frame's zoom, the reticle moves
    // instead, so it stays on the ground it was zooming about.
    reticle.drift += camera.take_glide_held() * camera.zoom;
    if left != Vec2::ZERO {
        let friction = if reticle.snap.is_some() {
            SNAP_FRICTION
        } else {
            1.0
        };
        let speed =
            super::PAN_PX_PER_SEC * prefs.pan_speed * left.length().powf(PAN_CURVE) * friction;
        let travel = left.normalize() * speed * dt;
        reticle.drift = drive(camera, reticle.drift, travel, bound);
    }
    // A smaller window pulls a drifted reticle back inside it.
    reticle.drift = reticle.drift.clamp(-bound, bound);
    if right.y != 0.0 {
        // Up zooms in, as the wheel rolled away does.
        let notches = -right.y * ZOOM_NOTCHES_PER_SEC * dt;
        crate::camera::controls::wheel_zoom(camera, reticle.point(), notches, prefs);
    }
    input.reticle.snap = (input.last_pointer == Pointer::Pad)
        .then(|| {
            let point = input.reticle.point();
            (!super::click_on_hud(game, point))
                .then(|| pick(game, point, ui, Pointer::Pad))
                .flatten()
        })
        .flatten();
}

/// How far the reticle may drift from center each way.
fn drift_bound(camera: &Camera, ui: f32) -> Vec2 {
    (camera.viewport() * 0.5 - Vec2::splat(DRIFT_MARGIN * ui)).max(Vec2::ZERO)
}

/// Moves the view by `travel` screen pixels and returns the new drift.
/// Travel back toward center spends the drift first; travel the camera's
/// clamp refuses moves the reticle instead.
fn drive(camera: &mut Camera, drift: Vec2, travel: Vec2, bound: Vec2) -> Vec2 {
    let mut drift = drift;
    let mut travel = travel;
    for axis in 0..2 {
        if drift[axis] * travel[axis] < 0.0 {
            let spent = travel[axis].abs().min(drift[axis].abs()) * travel[axis].signum();
            drift[axis] += spent;
            travel[axis] -= spent;
        }
    }
    let before = camera.center;
    camera.pan(travel / camera.zoom);
    let moved = (camera.center - before) * camera.zoom;
    let mut refused = travel - moved;
    for axis in 0..2 {
        if refused[axis].abs() < DRIFT_EPSILON {
            refused[axis] = 0.0;
        }
    }
    (drift + refused).clamp(-bound, bound)
}

/// A controller button pressed during a match. The D-pad jumps the camera
/// as the matching keys do, and a right-stick click centers on the
/// selection.
pub(super) fn press(
    game: &mut Game,
    input: &mut InputState,
    bindings: &BindingMap,
    button: PadButton,
) {
    match button {
        PadButton::DpadUp => dispatch_action(game, input, bindings, Action::HomeCamera),
        PadButton::DpadDown => dispatch_action(game, input, bindings, Action::CycleIdleWorker),
        PadButton::DpadLeft => dispatch_action(game, input, bindings, Action::JumpToLastAlert),
        PadButton::RightStick => {
            if let Some(world) = selection_center(game) {
                look_at(game, input, world);
            }
        }
        _ => {}
    }
}

/// Jumps the camera to `world` and rests the reticle on it, off center
/// where the map's edge holds the camera back. Every camera jump in a
/// match goes through here, so a drift left at an edge never outlives the
/// view it belonged to.
pub(super) fn look_at(game: &mut Game, input: &mut InputState, world: Vec2) {
    let camera = &mut game.presentation.camera;
    camera.center = world;
    camera.pan(Vec2::ZERO); // re-clamp
    let bound = drift_bound(camera, input.ui);
    input.reticle.center = camera.viewport() * 0.5;
    input.reticle.drift = ((world - camera.center) * camera.zoom).clamp(-bound, bound);
}

fn selection_center(game: &Game) -> Option<Vec2> {
    let selection = &game.presentation.selection;
    let members = if selection.units.is_empty() {
        super::Group::Buildings(selection.buildings.clone())
    } else {
        super::Group::Units(selection.units.clone())
    };
    members.center(&game.view())
}

#[cfg(test)]
mod tests;
