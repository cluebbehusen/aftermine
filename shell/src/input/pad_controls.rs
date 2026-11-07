//! A controller's presses in a match, all at the reticle: A selects, or
//! paints while held ([`super::paint`]), X gives the contextual order, the
//! bumpers select the army, a held LT queues, RT narrows the selection by
//! kind, and B backs out. Each takes the path its mouse counterpart takes.
//! The D-pad and the stick clicks belong to [`super::reticle`].

use super::paint::{self, Release};
use super::select::{self, Picked};
use super::{DOUBLE_CLICK_SECS, DOUBLE_CLICK_SLOP, InputState, click_on_hud};
use crate::action::{Action, BindingMap};
use crate::game::Game;
use aftermine_protocol::PadButton;
use aftermine_sim::{UnitId, UnitKind};
use macroquad::prelude::{Vec2, vec2};

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
        PadButton::South => press_select(game, input, pad),
        PadButton::West => order(game, input, bindings),
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
            if let Release::Tap(ground) = paint::release(game, input, pad) {
                tap(game, input, ground);
            }
        }
        PadButton::LeftTrigger => {
            input.queue_pads.remove(&pad);
        }
        _ => {}
    }
}

/// A goes down. An armed placement or targeted order waits for its own
/// target, and chrome under the reticle is the HUD's, so neither takes a
/// press; a second controller's A while one is held does nothing.
fn press_select(game: &mut Game, input: &mut InputState, pad: u64) {
    if input.pad_press.is_some() {
        return;
    }
    if input.armed_mode().is_some() || click_on_hud(game, input.reticle.point()) {
        super::deny(game);
        return;
    }
    let ground = input.reticle.ground(&game.presentation.camera);
    paint::press(input, pad, ground);
}

/// An A that never left its spot selects what the reticle bracketed as it
/// went down, as a click does, and a second tap there soon after sweeps
/// that kind in view, as a double-click does.
fn tap(game: &mut Game, input: &mut InputState, ground: Vec2) {
    let camera = &game.presentation.camera;
    let point = camera.to_screen(ground);
    let zoom = camera.zoom;
    let ui = input.ui;
    let reach = super::reticle::reach(input);
    let additive = input.queue_held();
    let now = input.now;
    let double = !additive
        && input.last_pad_tap.take().is_some_and(|(at, spot)| {
            now - at < DOUBLE_CLICK_SECS
                && (spot - ground).length() * zoom <= DOUBLE_CLICK_SLOP * ui
        });
    if double {
        select::select_all_of_kind_on_screen(game, point, ui, reach);
    } else {
        select::click_select(game, point, additive, ui, reach);
    }
    input.last_pad_tap = Some((now, ground));
}

/// X: the contextual order a right-click at the reticle gives, aimed by
/// [`aim`].
fn order(game: &mut Game, input: &mut InputState, bindings: &BindingMap) {
    let point = input.reticle.point();
    if click_on_hud(game, point) {
        super::deny(game);
        return;
    }
    if super::stand_down_for_order(input) {
        return;
    }
    let aim = aim(game, point, input.ui, super::reticle::reach(input));
    super::world_order(game, input, bindings, aim);
}

/// Where an order at the reticle's `point` lands. What a right-click names
/// before any enemy comes first: an own structure in reach for a selection
/// with workers, an own transport the reticle brackets for one with units
/// that board. Then the nearest enemy in reach, then what the reticle
/// brackets, then the ground under it. A building is aimed at its
/// footprint's nearest point, so the order names it as a click on it does.
fn aim(game: &Game, point: Vec2, ui: f32, reach: select::Reach) -> Vec2 {
    let camera = &game.presentation.camera;
    let human = game.presentation.human;
    let selected = &game.presentation.selection.units;
    let selection_has = |test: &dyn Fn(&aftermine_sim::Unit) -> bool| {
        selected
            .iter()
            .any(|id| game.state.unit(*id).is_some_and(test))
    };
    let workers = selection_has(&|unit| unit.kind.stats().harvest.is_some());
    let riders = selection_has(&|unit| unit.kind.stats().transport_size > 0);
    let bracketed = select::pick(game, point, ui, reach);
    let transport = bracketed.filter(|picked| match *picked {
        Picked::Unit(id, owner) => {
            owner == human
                && !selected.contains(&id)
                && game
                    .state
                    .unit(id)
                    .is_some_and(|unit| unit.kind.stats().transport_capacity > 0)
        }
        Picked::Building(..) => false,
    });
    let target = workers
        .then(|| select::own_structure_in_reach(game, point, ui, reach))
        .flatten()
        .map(|id| Picked::Building(id, human))
        .or_else(|| transport.filter(|_| riders))
        .or_else(|| select::hostile_in_reach(game, point, ui, reach))
        .or(bracketed);
    match target {
        Some(Picked::Unit(id, _)) => game.state.unit(id).map_or(point, |unit| {
            camera.to_screen(vec2(unit.pos.x.to_num::<f32>(), unit.pos.y.to_num::<f32>()))
        }),
        Some(Picked::Building(id, _)) => game.state.building(id).map_or(point, |building| {
            // Just inside the footprint: its far edge is the next tile's.
            let (min, max) = select::footprint(building);
            let inside = Vec2::splat(0.01);
            camera.to_screen(camera.to_world(point).clamp(min + inside, max - inside))
        }),
        None => point,
    }
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
mod tests;
