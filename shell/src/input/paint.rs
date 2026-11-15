//! Paint select: holding A grows a brush around the reticle, from what the
//! reticle picks across to a fixed full size, and everything the brush
//! covers, where it stands and wherever the reticle carries it, joins the
//! sweep. The sweep collects what the brush reaches along the ground the
//! reticle crosses, so a fast stick skips nothing, and on release selects
//! by the box's priority ([`select::kept`]), so a paint keeps every rule a
//! box keeps. Nothing selects before A comes up, and B abandons the sweep.

use super::select::{self, Kept, Picked};
use super::{InputState, Pointer};
use crate::game::Game;
use aftermine_sim::{BuildingId, UnitId};
use macroquad::prelude::Vec2;
use std::collections::BTreeSet;

/// Seconds A may be held still and still be a tap; held longer, it paints.
const TAP_SECS: f64 = 0.2;
/// Seconds the brush takes to grow from the reticle's reach to its full
/// size.
const GROW_SECS: f64 = 0.6;
/// The brush's full radius in logical pixels. It is the same whatever the
/// snap setting, which changes only where the brush starts.
const BRUSH: f32 = 96.0;
/// Logical pixels between two grounds that still read as one place: any
/// farther between frames is a camera jump, not a sweep.
const SAME_PLACE: f32 = 1.0;

/// A held A: where and when it went down and, once it has been held past a
/// tap or the reticle has left that spot, the sweep it paints.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Press {
    /// The controller holding A.
    pub(super) pad: u64,
    /// The ground under the reticle as A went down, or where a camera jump
    /// carried a press that had not yet begun to paint.
    ground: Vec2,
    /// What the reticle bracketed as A went down, which a tap selects.
    picked: Option<Picked>,
    /// The wall clock as A went down.
    at: f64,
    /// The ground under the reticle as the last frame left it.
    last: Vec2,
    sweep: Option<Sweep>,
}

/// What a sweep has caught so far and the ground it has crossed.
#[derive(Debug, Clone, PartialEq)]
struct Sweep {
    units: BTreeSet<UnitId>,
    buildings: BTreeSet<BuildingId>,
    /// The wall clock as the sweep began, which its brush grows from.
    began: f64,
}

impl Sweep {
    fn at(now: f64) -> Self {
        Self {
            units: BTreeSet::new(),
            buildings: BTreeSet::new(),
            began: now,
        }
    }

    /// The brush's radius in logical pixels at `now`: the reticle's
    /// `reach` as the sweep begins, growing to its full size at UI scale
    /// `ui`.
    fn radius(&self, now: f64, reach: f32, ui: f32) -> f32 {
        let grown = crate::numeric::to_f32(((now - self.began) / GROW_SECS).clamp(0.0, 1.0));
        let grown = (grown * grown * grown * (grown * (grown * 6.0 - 15.0) + 10.0)).clamp(0.0, 1.0);
        reach + (BRUSH * ui - reach).max(0.0) * grown
    }

    /// Everything the brush of `radius` reaches along `from` to `to`.
    fn catch(&mut self, game: &Game, from: Vec2, to: Vec2, radius: f32) {
        let (units, buildings) = select::swept_by_pad(game, from, to, radius);
        self.units.extend(units);
        self.buildings.extend(buildings);
    }
}

/// How a release of A ends its press.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Release {
    /// The reticle never left the spot: a tap at this ground, on what the
    /// reticle bracketed as A went down.
    Tap(Vec2, Option<Picked>),
    /// A sweep selected what it caught.
    Swept,
    /// No press of this controller was held.
    Nothing,
}

/// A goes down with the reticle over `ground`, bracketing `picked`.
pub(super) fn press(input: &mut InputState, pad: u64, ground: Vec2, picked: Option<Picked>) {
    input.pad_press = Some(Press {
        pad,
        ground,
        picked,
        at: input.now,
        last: ground,
        sweep: None,
    });
}

/// A frame of a held A. Once A has been held past a tap, or the reticle has
/// travelled past its reach from where A went down, the press paints:
/// everything the growing brush reaches along the ground the reticle
/// crossed since `before` joins the sweep. A camera jump starts a new
/// stretch without sweeping the ground in between, and chrome over the
/// reticle hides the ground it would paint.
pub(super) fn sweep(game: &Game, input: &mut InputState, before: Vec2) {
    if input.last_pointer != Pointer::Pad {
        return;
    }
    let (ui, now) = (input.ui, input.now);
    let reach = super::reticle::reach(input).unit_px(ui);
    let camera = &game.presentation.camera;
    let after = input.reticle.ground(camera);
    let Some(press) = input.pad_press.as_mut() else {
        return;
    };
    let jumped = (before - press.last).length() * camera.zoom > SAME_PLACE * ui;
    press.last = after;
    let from = if press.sweep.is_some() {
        before
    } else {
        if jumped {
            press.ground = before;
        }
        let held = now - press.at >= TAP_SECS;
        let left = (after - press.ground).length() * camera.zoom > reach;
        if !held && !left {
            return;
        }
        press.ground
    };
    let sweep = press.sweep.get_or_insert_with(|| Sweep::at(now));
    sweep.catch(game, from, after, sweep.radius(now, reach, ui));
}

/// A comes up on `pad`: a sweep selects what it caught, adding to the
/// selection while queue is held, and a press that neither moved nor was
/// held past a tap is a tap there.
pub(super) fn release(game: &mut Game, input: &mut InputState, pad: u64) -> Release {
    let Some(mut press) = input.pad_press.take_if(|press| press.pad == pad) else {
        return Release::Nothing;
    };
    // Held past a tap, it paints even if no frame has drawn the brush yet.
    if press.sweep.is_none() && input.now - press.at >= TAP_SECS {
        let mut sweep = Sweep::at(input.now);
        let reach = super::reticle::reach(input).unit_px(input.ui);
        sweep.catch(game, press.ground, press.ground, reach);
        press.sweep = Some(sweep);
    }
    let Some(sweep) = press.sweep else {
        return Release::Tap(press.ground, press.picked);
    };
    let units: Vec<UnitId> = sweep.units.into_iter().collect();
    let buildings: Vec<BuildingId> = sweep.buildings.into_iter().collect();
    select::select_swept(game, &units, &buildings, input.queue_held());
    Release::Swept
}

/// Drops a held A, swept or not, leaving the selection as it was. Returns
/// whether there was one.
pub(super) fn abandon(input: &mut InputState) -> bool {
    input.pad_press.take().is_some()
}

/// Whether a paint sweep is under way, while the controller points.
pub(crate) fn painting(input: &InputState) -> bool {
    input.last_pointer == Pointer::Pad
        && input
            .pad_press
            .as_ref()
            .is_some_and(|press| press.sweep.is_some())
}

/// The controller cursor's visual radius before and during a paint sweep.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Cursor {
    Idle,
    Held(f32),
    Sweeping(f32),
}

pub(crate) fn cursor(input: &InputState) -> Cursor {
    if input.last_pointer != Pointer::Pad {
        return Cursor::Idle;
    }
    let Some(press) = input.pad_press.as_ref() else {
        return Cursor::Idle;
    };
    let reach = super::reticle::reach(input).unit_px(input.ui);
    if let Some(sweep) = &press.sweep {
        Cursor::Sweeping(sweep.radius(input.now, reach, input.ui))
    } else {
        let held = crate::numeric::to_f32(((input.now - press.at) / TAP_SECS).clamp(0.0, 1.0));
        let anticipation = held * held * (3.0 - 2.0 * held);
        let idle = 11.0 * input.ui;
        Cursor::Held(idle + (reach - idle) * anticipation)
    }
}

/// What a release of the sweep would select, using the same priority as a box.
pub(crate) fn caught(game: &crate::game::Scene<'_>, input: &InputState) -> Option<Kept> {
    if !painting(input) {
        return None;
    }
    let sweep = input.pad_press.as_ref()?.sweep.as_ref()?;
    let units: Vec<UnitId> = sweep.units.iter().copied().collect();
    let buildings: Vec<BuildingId> = sweep.buildings.iter().copied().collect();
    select::kept(game, &units, &buildings)
}

#[cfg(test)]
mod tests;
