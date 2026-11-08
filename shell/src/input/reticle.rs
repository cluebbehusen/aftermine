//! The match's controller pointer: a reticle at the center of the screen.
//!
//! The reticle marks a spot on the ground, and the camera centers on that
//! spot as far as the map's edge allows; where the edge holds the camera
//! back, the reticle sits off center over the spot. The left stick moves
//! the spot, faster the further it is pushed, and the right stick zooms
//! about it. Because the spot is ground rather than a screen offset, a zoom,
//! a resized window or a jump never moves it. The reticle snaps onto a unit
//! or structure near it, except while a structure is armed for placement,
//! and D-pad up, down and left and a right-stick click jump the camera.
//! Menus never see this: they read pad buttons as menu keys.

pub(crate) use super::select::Picked;
use super::select::{Reach, pick};
use super::{InputState, Pointer, dispatch_action};
use crate::action::{Action, BindingMap};
use crate::camera::Camera;
use crate::config::ReticleSnap;
use crate::game::Game;
use aftermine_protocol::PadButton;
use macroquad::prelude::Vec2;

/// How sharply pan speed rises with stick travel: speed follows travel to
/// this power, so a light push aims finely and a full push matches the pan
/// keys while both speeds are at their defaults.
const PAN_CURVE: f32 = 2.0;
/// Zoom steps per second at full right-stick travel, in wheel notches.
const ZOOM_NOTCHES_PER_SEC: f32 = 6.0;
/// Logical pixels an off-center reticle keeps from the window's edge.
const EDGE_MARGIN: f32 = 32.0;
/// An offset under this many logical pixels is rounding: the reticle is
/// centered.
const CENTERED: f32 = 0.01;

/// How far around the reticle it snaps, and the share of stick speed left
/// while it rests on something, so a passing unit is easy to stop on. A
/// stick aims far less finely than a mouse; with snap off, the reticle
/// picks as a mouse does and never slows.
fn snap_feel(snap: ReticleSnap) -> (Reach, f32) {
    match snap {
        ReticleSnap::Off => (Reach::MOUSE, 1.0),
        ReticleSnap::Light => (Reach::around(16.0), 0.75),
        ReticleSnap::Standard => (Reach::around(24.0), 0.5),
        ReticleSnap::Strong => (Reach::around(32.0), 0.35),
    }
}

/// What the reticle picks and snaps across under the player's snap
/// setting: a press at the reticle, an order's aim and a paint brush's
/// start all reach as far.
pub(super) fn reach(input: &InputState) -> Reach {
    snap_feel(input.pad_prefs.snap).0
}

/// Where the reticle sits and what it rests on.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Reticle {
    /// The ground the reticle marks while the map's edge holds the camera
    /// off it; none while the reticle sits at the camera's center.
    aim: Option<Vec2>,
    /// Where the camera stood when the reticle last settled. A camera that
    /// moved since, under another pointer, takes the reticle back to center.
    settled: Option<Vec2>,
    /// The reticle's screen point as it last settled.
    point: Vec2,
    /// The unit or structure the reticle has snapped onto.
    snap: Option<Picked>,
}

impl Reticle {
    /// The reticle's screen point.
    pub(crate) fn point(&self) -> Vec2 {
        self.point
    }

    /// The ground the reticle marks.
    pub(crate) fn ground(&self, camera: &Camera) -> Vec2 {
        self.aim.unwrap_or(camera.center)
    }

    /// The unit or structure the reticle has snapped onto.
    pub(crate) fn snap(&self) -> Option<Picked> {
        self.snap
    }

    /// Brings the camera to the marked ground as far as the map's edge
    /// allows, keeps the reticle inside the window, and places it over the
    /// ground.
    fn settle(&mut self, camera: &mut Camera, ui: f32) {
        let mut offset = Vec2::ZERO;
        if let Some(aim) = self.aim {
            camera.pan(aim - camera.center);
            let bound = (camera.viewport() * 0.5 - Vec2::splat(EDGE_MARGIN * ui)).max(Vec2::ZERO);
            offset = ((aim - camera.center) * camera.zoom).clamp(-bound, bound);
            self.aim = (offset.length() >= CENTERED).then(|| camera.center + offset / camera.zoom);
            if self.aim.is_none() {
                offset = Vec2::ZERO;
            }
        }
        self.point = camera.viewport() * 0.5 + offset;
        self.settled = Some(camera.center);
    }
}

/// Pans by the held sticks and keys, zooms by the right stick, then settles
/// what the reticle rests on. Runs every frame of a match: sticks send only
/// when they move. `keys` is the world travel the held pan keys ask for.
pub(super) fn update(game: &mut Game, input: &mut InputState, dt: f32, keys: Vec2) {
    let prefs = input.pad_prefs;
    let (reach, friction) = snap_feel(prefs.snap);
    // A paint sweep wants an even stroke, so nothing it passes slows it.
    let painting = super::paint::painting(input);
    // A structure goes on the ground under the reticle, never onto what it
    // rests on, so while one is armed nothing snaps or slows the reticle.
    let snaps = input.placing_kind().is_none();
    let ui = input.ui;
    let (left, right) = input.camera_sticks();
    // A stick held since a menu was up moves the camera too, so it makes
    // the pad the pointer even though the match never saw it move.
    if left != Vec2::ZERO || right != Vec2::ZERO {
        input.pad_points();
    }
    let pad = input.last_pointer == Pointer::Pad;
    let camera = &mut game.presentation.camera;
    let reticle = &mut input.reticle;
    if pad {
        // While the pad points, every pan moves the marked ground, and the
        // camera follows it as far as the map's edge allows.
        let mut travel = keys;
        if left != Vec2::ZERO {
            let friction = if snaps && reticle.snap.is_some() && !painting {
                friction
            } else {
                1.0
            };
            let speed = super::PAN_PX_PER_SEC
                * prefs.stick_speed
                * left.length().powf(PAN_CURVE)
                * friction;
            travel += left.normalize() * speed * dt / camera.zoom;
        }
        if travel != Vec2::ZERO {
            reticle.aim = Some(reticle.ground(camera) + travel);
        }
    } else {
        camera.pan(keys);
        if reticle.settled != Some(camera.center) {
            reticle.aim = None;
        }
    }
    reticle.settle(camera, ui);
    if pad && right.y != 0.0 {
        // Up zooms in, as the wheel rolled away does, unless the player
        // flipped the stick; the wheel's own setting leaves it alone.
        let notches = -right.y * ZOOM_NOTCHES_PER_SEC * dt;
        camera.zoom_at(
            reticle.point,
            if prefs.zoom_inverted {
                -notches
            } else {
                notches
            },
        );
    }
    input.reticle.snap = (pad && snaps)
        .then(|| {
            let point = input.reticle.point;
            (!super::click_on_hud(game, point))
                .then(|| pick(game, point, ui, reach))
                .flatten()
        })
        .flatten();
}

/// A controller button pressed during a match. D-pad up, down and left
/// jump the camera as the matching keys do, and a right-stick click
/// centers on the selection; D-pad right belongs to [`super::pad_ring`].
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
/// match goes through here.
pub(super) fn look_at(game: &mut Game, input: &mut InputState, world: Vec2) {
    let camera = &mut game.presentation.camera;
    camera.center = world;
    camera.pan(Vec2::ZERO); // re-clamp
    input.reticle.aim = Some(world);
    input.reticle.settle(camera, input.ui);
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
