//! Paint select: holding A grows a brush around the reticle, from what the
//! reticle picks across to a fixed full size, and everything the brush
//! covers, where it stands and wherever the reticle carries it, joins the
//! sweep. The sweep collects what the brush reaches along the ground the
//! reticle crosses, so a fast stick skips nothing, and on release selects
//! by the box's priority ([`select::kept`]), so a paint keeps every rule a
//! box keeps. Nothing selects before A comes up, and B abandons the sweep.

use super::select::{self, Kept};
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
/// Trail points one sweep keeps. Past them the trail runs straight on to
/// the reticle, while the sweep itself keeps collecting.
const TRAIL_CAP: usize = 512;
/// Logical pixels the reticle travels between trail points.
const TRAIL_STEP: f32 = 4.0;
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
    /// The ground swept, one stroke per stretch between camera jumps.
    trail: Vec<Vec<Vec2>>,
    /// The wall clock as the sweep began, which its brush grows from.
    began: f64,
}

impl Sweep {
    fn at(ground: Vec2, now: f64) -> Self {
        Self {
            units: BTreeSet::new(),
            buildings: BTreeSet::new(),
            trail: vec![vec![ground]],
            began: now,
        }
    }

    /// The brush's radius in logical pixels at `now`: the reticle's
    /// `reach` as the sweep begins, growing to its full size at UI scale
    /// `ui`.
    fn radius(&self, now: f64, reach: f32, ui: f32) -> f32 {
        let grown = crate::numeric::to_f32(((now - self.began) / GROW_SECS).clamp(0.0, 1.0));
        reach + (BRUSH * ui - reach).max(0.0) * grown
    }

    /// Everything the brush of `radius` reaches along `from` to `to`.
    fn catch(&mut self, game: &Game, from: Vec2, to: Vec2, radius: f32) {
        let (units, buildings) = select::swept_by_pad(game, from, to, radius);
        self.units.extend(units);
        self.buildings.extend(buildings);
    }

    /// Lays a trail point at `to` once the reticle has come far enough.
    fn extend_trail(&mut self, to: Vec2, zoom: f32, ui: f32) {
        let points: usize = self.trail.iter().map(Vec::len).sum();
        let Some(stroke) = self.trail.last_mut() else {
            return;
        };
        let far = stroke
            .last()
            .is_none_or(|last| (to - *last).length() * zoom >= TRAIL_STEP * ui);
        if points < TRAIL_CAP && far {
            stroke.push(to);
        }
    }
}

/// How a release of A ends its press.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Release {
    /// The reticle never left the spot: a tap at this ground.
    Tap(Vec2),
    /// A sweep selected what it caught.
    Swept,
    /// No press of this controller was held.
    Nothing,
}

/// A goes down with the reticle over `ground`.
pub(super) fn press(input: &mut InputState, pad: u64, ground: Vec2) {
    input.pad_press = Some(Press {
        pad,
        ground,
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
    let over_chrome = super::click_on_hud(game, input.reticle.point());
    let Some(press) = input.pad_press.as_mut() else {
        return;
    };
    let jumped = (before - press.last).length() * camera.zoom > SAME_PLACE * ui;
    press.last = after;
    let from = if let Some(sweep) = press.sweep.as_mut() {
        if jumped {
            sweep.trail.push(vec![before]);
        }
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
    let sweep = press.sweep.get_or_insert_with(|| Sweep::at(from, now));
    if !over_chrome {
        sweep.catch(game, from, after, sweep.radius(now, reach, ui));
    }
    sweep.extend_trail(after, camera.zoom, ui);
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
        let mut sweep = Sweep::at(press.ground, input.now);
        let reach = super::reticle::reach(input).unit_px(input.ui);
        sweep.catch(game, press.ground, press.ground, reach);
        press.sweep = Some(sweep);
    }
    let Some(sweep) = press.sweep else {
        return Release::Tap(press.ground);
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

/// A sweep as the frame draws it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Preview<'a> {
    /// The brush's center on screen: the reticle.
    pub(crate) brush: Vec2,
    /// The brush's radius in logical pixels: what it catches now.
    pub(crate) radius: f32,
    /// The ground swept, stroke by stroke.
    pub(crate) trail: &'a [Vec<Vec2>],
    /// The ground under the reticle, where the last stroke runs on to.
    pub(crate) head: Vec2,
    /// What a release would select of what the sweep has caught.
    pub(crate) kept: Option<Kept>,
}

/// The sweep in progress, while the controller points.
pub(crate) fn preview<'a>(
    game: &crate::game::Scene<'_>,
    input: &'a InputState,
) -> Option<Preview<'a>> {
    if input.last_pointer != Pointer::Pad {
        return None;
    }
    let sweep = input.pad_press.as_ref()?.sweep.as_ref()?;
    let units: Vec<UnitId> = sweep.units.iter().copied().collect();
    let buildings: Vec<BuildingId> = sweep.buildings.iter().copied().collect();
    Some(Preview {
        brush: input.reticle.point(),
        radius: sweep.radius(
            input.now,
            super::reticle::reach(input).unit_px(input.ui),
            input.ui,
        ),
        trail: &sweep.trail,
        head: input.reticle.ground(&game.presentation.camera),
        kept: select::kept(game, &units, &buildings),
    })
}

#[cfg(test)]
mod tests;
