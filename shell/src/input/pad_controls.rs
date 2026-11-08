//! A controller's presses in a match, all at the reticle: A selects, or
//! paints while held ([`super::paint`]), X gives the contextual order, the
//! bumpers select the army, a held LT queues, RT narrows the selection by
//! kind, and B backs out. While a placement or targeted order is armed, A
//! and X confirm it at the reticle as a click there would, and a held A
//! drags a placement row. Each takes the path its mouse counterpart takes.
//! The D-pad and the stick clicks belong to [`super::reticle`], except
//! D-pad right, which opens the group ring of [`super::pad_ring`] as Y
//! opens its command ring.

use super::paint::{self, Release};
use super::select::{self, Picked};
use super::{DOUBLE_CLICK_SECS, DOUBLE_CLICK_SLOP, InputState, Presser};
use crate::action::{Action, BindingMap};
use crate::game::Game;
use aftermine_protocol::PadButton;
use aftermine_sim::{UnitId, UnitKind};
use macroquad::prelude::Vec2;

/// The group RT narrowed last and the kind it left selected. It stands
/// only while the selection is still exactly that kind of the group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Narrowed {
    group: Vec<UnitId>,
    kind: UnitKind,
}

/// A controller button pressed during a match.
pub(super) fn press(
    game: &mut Game,
    input: &mut InputState,
    bindings: &BindingMap,
    pad: u64,
    button: PadButton,
) {
    match button {
        PadButton::South => press_select(game, input, bindings, pad),
        PadButton::West => order(game, input, bindings, pad),
        PadButton::East => back(game, input, bindings),
        PadButton::LeftShoulder => {
            super::dispatch_action(game, input, bindings, Action::SelectArmy);
        }
        PadButton::RightShoulder => {
            super::dispatch_action(game, input, bindings, Action::SelectArmyInView);
        }
        PadButton::LeftTrigger => {
            input.queue_pads.insert(pad);
        }
        PadButton::RightTrigger => narrow(game, input),
        _ => {}
    }
}

/// A controller button released during a match.
pub(super) fn release(game: &mut Game, input: &mut InputState, pad: u64, button: PadButton) {
    match button {
        PadButton::South => {
            if super::finish_stroke(game, input, Presser::Pad(pad)) {
                return;
            }
            if let Release::Tap(ground, picked) = paint::release(game, input, pad) {
                tap(game, input, ground, picked);
            }
        }
        PadButton::LeftTrigger => {
            input.queue_pads.remove(&pad);
        }
        _ => {}
    }
}

/// A goes down. Chrome under the reticle is the HUD's, so it takes no
/// press. An armed placement opens a stroke at the reticle, which a held A
/// drags into a row, and an armed targeted order takes the reticle as its
/// target. A second A while one is held, a press or a row, does nothing.
fn press_select(game: &mut Game, input: &mut InputState, bindings: &BindingMap, pad: u64) {
    if input.pad_press.is_some() || input.stroke_by().is_some() {
        return;
    }
    let point = input.reticle.point();
    if select::under_hud_for_pad(game, point) {
        super::deny(game);
        return;
    }
    if input.armed_mode().is_some() {
        super::armed_click(game, input, bindings, point, Presser::Pad(pad));
        return;
    }
    let ground = input.reticle.ground(&game.presentation.camera);
    let reach = super::reticle::reach(input);
    let picked = select::pick(game, input.reticle.point(), input.ui, reach);
    paint::press(input, pad, ground, picked);
}

/// An A that never left its spot selects what the reticle bracketed as it
/// went down, as a click does, and a second tap there soon after sweeps
/// that kind in view, as a double-click does. What it bracketed is kept
/// from the press, so a unit that moves on before A comes up is still the
/// one selected; if it has since died or gone into fog, the press spot
/// picks again.
fn tap(game: &mut Game, input: &mut InputState, ground: Vec2, pressed: Option<Picked>) {
    let camera = &game.presentation.camera;
    let point = camera.to_screen(ground);
    let zoom = camera.zoom;
    let ui = input.ui;
    let reach = super::reticle::reach(input);
    let additive = input.queue_held();
    let now = input.now;
    let picked = match pressed {
        Some(picked) if picked.seen(&game.view()) => Some(picked),
        Some(_) => select::pick(game, point, ui, reach),
        None => None,
    };
    // Only a tap that picked something sweeps its kind, as a fingertip's
    // does: over salvage the pick reaches no further than a unit's body.
    let double = !additive
        && picked.is_some()
        && input.last_pad_tap.take().is_some_and(|(at, spot)| {
            now - at < DOUBLE_CLICK_SECS
                && (spot - ground).length() * zoom <= DOUBLE_CLICK_SLOP * ui
        });
    match picked {
        Some(picked) if double => select::select_all_like(game, picked),
        _ => select::select_picked(game, picked, point, additive),
    }
    input.last_pad_tap = Some((now, ground));
}

/// X: the contextual order a right-click at the reticle gives, reaching as
/// far around it as the reticle snaps. While a mode is armed, X confirms it
/// at the reticle as A does, and in placement it is a tap of A, placing one
/// structure; while a held A lays a row, the row has the placement.
fn order(game: &mut Game, input: &mut InputState, bindings: &BindingMap, pad: u64) {
    let point = input.reticle.point();
    if select::under_hud_for_pad(game, point) || input.stroke_by().is_some() {
        super::deny(game);
        return;
    }
    if input.armed_mode().is_some() {
        super::armed_click(game, input, bindings, point, Presser::Pad(pad));
        super::finish_stroke(game, input, Presser::Pad(pad));
        return;
    }
    if super::stand_down_for_order(input) {
        return;
    }
    let reach = super::reticle::reach(input).in_world(input.ui, game.presentation.camera.zoom);
    super::world_order(game, input, bindings, point, reach);
}

/// B: a held A goes first, leaving the selection as it was; then what Back
/// walks out of, the selection last.
fn back(game: &mut Game, input: &mut InputState, bindings: &BindingMap) {
    if paint::abandon(input) {
        return;
    }
    super::dispatch_action(game, input, bindings, Action::Back);
}

/// RT: narrows the selection to its first unit kind in the roster's order,
/// then each press to the next kind of the same group, and after the last
/// back to the whole group. A selection of one kind has nothing to narrow.
fn narrow(game: &mut Game, input: &mut InputState) {
    let alive = |ids: &[UnitId]| -> Vec<UnitId> {
        ids.iter()
            .copied()
            .filter(|id| game.state.unit(*id).is_some())
            .collect()
    };
    let of_kind = |ids: &[UnitId], kind: UnitKind| -> Vec<UnitId> {
        ids.iter()
            .copied()
            .filter(|id| game.state.unit(*id).is_some_and(|unit| unit.kind == kind))
            .collect()
    };
    let selection = alive(&game.presentation.selection.units);
    // The group RT left, while the selection is still the kind it left.
    let (group, last) = match input.narrowed.take() {
        Some(narrowed) => {
            let group = alive(&narrowed.group);
            if of_kind(&group, narrowed.kind) == selection {
                (group, Some(narrowed.kind))
            } else {
                (selection, None)
            }
        }
        None => (selection, None),
    };
    let kinds: Vec<UnitKind> = crate::panel::kind_counts(
        group
            .iter()
            .filter_map(|id| game.state.unit(*id).map(|unit| unit.kind)),
    )
    .into_iter()
    .map(|(kind, _)| kind)
    .collect();
    if kinds.len() < 2 {
        super::deny(game);
        return;
    }
    let next = match last {
        None => kinds.first().copied(),
        Some(kind) => kinds
            .iter()
            .position(|k| *k == kind)
            .and_then(|at| kinds.get(at + 1).copied()),
    };
    game.presentation.selection.units = match next {
        Some(kind) => {
            let narrowed = of_kind(&group, kind);
            input.narrowed = Some(Narrowed { group, kind });
            narrowed
        }
        None => group,
    };
}

#[cfg(test)]
mod aim_tests;
#[cfg(test)]
mod tests;
