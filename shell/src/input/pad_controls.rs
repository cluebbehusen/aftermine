//! A controller's presses in a match, all at the reticle: A selects, or
//! paints while held ([`super::paint`]), X gives the contextual order, the
//! bumpers select the army, a held LT queues, RT narrows the selection by
//! kind, and B backs out. Each takes the path its mouse counterpart takes.
//! The D-pad and the stick clicks belong to [`super::reticle`].

use super::paint::{self, Release};
use super::select::{self, Picked};
use super::{DOUBLE_CLICK_SECS, DOUBLE_CLICK_SLOP, InputState};
use crate::action::{Action, BindingMap};
use crate::game::Game;
use crate::numeric;
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

/// A goes down. An armed placement or targeted order waits for its own
/// target, and chrome under the reticle is the HUD's, so neither takes a
/// press; a second controller's A while one is held does nothing.
fn press_select(game: &mut Game, input: &mut InputState, pad: u64) {
    if input.pad_press.is_some() {
        return;
    }
    if input.armed_mode().is_some() || select::under_hud_for_pad(game, input.reticle.point()) {
        super::deny(game);
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
    if double {
        let at = picked
            .and_then(|picked| on_screen(game, picked))
            .unwrap_or(point);
        select::select_all_of_kind_on_screen(game, at, ui, reach);
    } else {
        select::select_picked(game, picked, point, additive);
    }
    input.last_pad_tap = Some((now, ground));
}

/// Where a picked unit or building stands on screen now, while it stands.
fn on_screen(game: &Game, picked: Picked) -> Option<Vec2> {
    let camera = &game.presentation.camera;
    let world = match picked {
        Picked::Unit(id, _) => game
            .state
            .unit(id)
            .map(|unit| vec2(unit.pos.x.to_num::<f32>(), unit.pos.y.to_num::<f32>())),
        Picked::Building(id, _) => game.state.building(id).map(|building| {
            let center = building.center();
            vec2(center.x.to_num::<f32>(), center.y.to_num::<f32>())
        }),
    };
    world.map(|world| camera.to_screen(world))
}

/// X: the contextual order a right-click at the reticle gives, aimed by
/// [`aim`].
fn order(game: &mut Game, input: &mut InputState, bindings: &BindingMap) {
    let point = input.reticle.point();
    if select::under_hud_for_pad(game, point) {
        super::deny(game);
        return;
    }
    if super::stand_down_for_order(input) {
        return;
    }
    let aim = aim(game, input, point);
    super::world_order(game, input, bindings, aim);
}

/// Where an order at the reticle's `point` lands: the ground under it, as
/// a right-click's, unless the reticle reaches something the order acts on.
/// Patrol waypoints and rally points always stay on the ground; selected
/// defenses take an enemy in reach; a unit selection takes what
/// [`units_target`] finds.
fn aim(game: &Game, input: &InputState, point: Vec2) -> Vec2 {
    let (ui, reach) = (input.ui, super::reticle::reach(input));
    let target = if input.patrol_route.is_some() {
        None
    } else if game.presentation.selection.units.is_empty() {
        // Selected defenses take the nearest enemy one of them can fire at.
        let selected = crate::building_actions::SelectedBuildings::inspect(&game.view());
        let fires_at = |domain| !selected.defenses(Some(domain)).is_empty();
        select::hostile_in_reach(game, point, ui, reach, &fires_at)
    } else {
        units_target(game, point, ui, reach)
    };
    let camera = &game.presentation.camera;
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

/// What a unit selection's order at `point` acts on within the reticle's
/// reach, checked in a right-click's order: an own structure under it or
/// bracketed, for a selection with workers; an own transport bracketed, for
/// one with units that board; the nearest enemy in reach, unless every
/// selected unit scouts, as scouts go to the ground they are sent to; a
/// wounded own machine bracketed, for workers. Anything else leaves the
/// order on the ground.
fn units_target(game: &Game, point: Vec2, ui: f32, reach: select::Reach) -> Option<Picked> {
    let human = game.presentation.human;
    let selected = &game.presentation.selection.units;
    let units: Vec<&aftermine_sim::Unit> = selected
        .iter()
        .filter_map(|id| game.state.unit(*id))
        .collect();
    let workers = units.iter().any(|u| u.kind.stats().harvest.is_some());
    let riders = units.iter().any(|u| u.kind.stats().transport_size > 0);
    let scouts = units.iter().all(|u| u.kind.stats().scouts);
    let bracketed = select::pick(game, point, ui, reach);
    let own_unit = |test: &dyn Fn(&aftermine_sim::Unit) -> bool| match bracketed {
        Some(Picked::Unit(id, owner)) if owner == human && !selected.contains(&id) => game
            .state
            .unit(id)
            .filter(|unit| test(unit))
            .map(|_| Picked::Unit(id, owner)),
        _ => None,
    };
    let structure = || {
        let tile = numeric::tile_at(game.presentation.camera.to_world(point));
        game.state
            .buildings_at(tile)
            .find(|b| b.player == human)
            .map(|b| Picked::Building(b.id, human))
            .or(bracketed.filter(|p| matches!(p, Picked::Building(_, owner) if *owner == human)))
    };
    let wounded = |unit: &aftermine_sim::Unit| {
        unit.hp > 0
            && unit.hp < unit.kind.stats().max_hp
            && unit.kind.stats().repairable()
            && unit.domain() == aftermine_sim::stats::Domain::Ground
    };
    workers
        .then(structure)
        .flatten()
        .or_else(|| {
            riders
                .then(|| own_unit(&|unit| unit.kind.stats().transport_capacity > 0))
                .flatten()
        })
        .or_else(|| {
            (!scouts)
                .then(|| select::hostile_in_reach(game, point, ui, reach, &|_| true))
                .flatten()
        })
        .or_else(|| workers.then(|| own_unit(&wounded)).flatten())
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
