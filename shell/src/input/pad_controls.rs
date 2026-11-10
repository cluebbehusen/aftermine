//! A controller's presses in a match, at the reticle: A selects, or
//! paints while held ([`super::paint`]), X gives the contextual order, the
//! bumpers select the army, a held LT queues, RT narrows the selection by
//! kind or widens one kind to every one of it, and B backs out. Menu opens
//! the pause menu and View pauses, as the top bar's buttons do. While a
//! placement or targeted order is armed, A and X confirm it at the reticle
//! as a click there would, and a held A drags a placement row; while Launch
//! is armed, the bumpers fire fewer or more warheads instead. Each takes
//! the path its mouse counterpart takes.
//! The D-pad and the stick clicks belong to [`super::reticle`], except
//! D-pad right, which opens the group ring of [`super::pad_ring`] as Y
//! opens its command ring.

use super::paint::{self, Release};
use super::select::{self, Picked};
use super::{DOUBLE_CLICK_SECS, DOUBLE_CLICK_SLOP, InputState, Presser};
use crate::action::{Action, BindingMap};
use crate::game::Game;
use aftermine_protocol::PadButton;
use aftermine_sim::{BuildingId, BuildingKind, UnitId, UnitKind};
use macroquad::prelude::Vec2;

/// The group RT narrowed last and the kind it left selected. It stands
/// only while the selection is still exactly that kind of the group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Narrowed {
    Units {
        group: Vec<UnitId>,
        kind: UnitKind,
    },
    Buildings {
        group: Vec<BuildingId>,
        kind: BuildingKind,
    },
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
        PadButton::East => back(game, input, bindings, pad),
        // Launch takes the bumpers over, as the strike ribbon's steppers.
        PadButton::LeftShoulder | PadButton::RightShoulder if input.striking.is_some() => {
            super::step_salvo(&game.view(), input, button == PadButton::RightShoulder);
        }
        PadButton::LeftShoulder => {
            super::dispatch_action(game, input, bindings, Action::SelectArmy);
        }
        PadButton::RightShoulder => {
            super::dispatch_action(game, input, bindings, Action::SelectArmyInView);
        }
        PadButton::LeftTrigger => {
            input.queue_pads.insert(pad);
        }
        PadButton::RightTrigger => by_kind(game, input, bindings),
        // The top bar's buttons: Menu opens the pause menu, as the menu
        // button does, and View pauses and resumes, as the pause button
        // does.
        PadButton::Start => input.menu_requested = true,
        PadButton::Select => super::dispatch_action(game, input, bindings, Action::TogglePause),
        _ => {}
    }
}

/// A controller button released during a match.
pub(super) fn release(game: &mut Game, input: &mut InputState, pad: u64, button: PadButton) {
    match button {
        PadButton::South => {
            let placing = input.placing.is_some();
            if super::finish_stroke(game, input, Presser::Pad(pad)) {
                placed(input, placing);
                return;
            }
            if let Release::Tap(ground, picked) = paint::release(game, input, pad) {
                tap(game, input, ground, picked);
            }
        }
        PadButton::LeftTrigger => {
            input.queue_pads.remove(&pad);
            if !input.queue_held() {
                let placing = input.placing.is_some();
                input.queue_released();
                placed(input, placing);
            }
        }
        _ => {}
    }
}

/// A placement a controller ended returns to the selection. Pad mode
/// draws no palette, so one left open would blank the panel and turn Y
/// into the build ring. An open build ring keeps it, as LT let go over
/// one leaves it: that ring closes construction as it closes.
fn placed(input: &mut InputState, placing: bool) {
    if placing && input.placing.is_none() && !super::pad_ring::building(input.ring.as_ref()) {
        input.close_construction();
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
        let placing = input.placing.is_some();
        super::armed_click(game, input, bindings, point, Presser::Pad(pad));
        super::finish_stroke(game, input, Presser::Pad(pad));
        placed(input, placing);
        return;
    }
    if super::stand_down_for_order(input) {
        return;
    }
    let reach = super::reticle::reach(input).in_world(input.ui, game.presentation.camera.zoom);
    super::world_order(game, input, bindings, point, reach);
}

/// B: a held A goes first, leaving the selection as it was; then what Back
/// walks out of, the selection last. Out of a placement it goes back to the
/// build ring the structure was picked from, where Back goes back to the
/// palette pad mode doesn't draw.
fn back(game: &mut Game, input: &mut InputState, bindings: &BindingMap, pad: u64) {
    if paint::abandon(input) {
        return;
    }
    let placing = input.placing.is_some();
    super::dispatch_action(game, input, bindings, Action::Back);
    if placing && input.placing.is_none() {
        super::pad_ring::reopen_build(game, input, pad);
    }
}

/// One RT press over a selection: the next kind of the group RT narrowed
/// last, the selection's first kind, or the whole group after its last.
enum Cycle<Id, Kind> {
    /// The selection holds one kind, or none, and no cycle stands.
    OneKind,
    /// Select `ids`, the group's members of `kind`, and remember both.
    Narrow {
        group: Vec<Id>,
        kind: Kind,
        ids: Vec<Id>,
    },
    /// Back to the whole group after its last kind.
    Whole(Vec<Id>),
}

/// Steps RT's cycle over the live `selection`. `narrowed` is the group RT
/// left last, with its dead members gone, and the kind it left; it counts
/// only while the selection is still exactly that kind of it. `kinds`
/// lists a group's kinds, and the kind RT left, in the roster tiles'
/// order: a kind whose last member died keeps its place, so the walk
/// moves on to the kind after it.
fn cycle<Id: Copy + PartialEq, Kind: Copy + PartialEq>(
    selection: Vec<Id>,
    narrowed: Option<(Vec<Id>, Kind)>,
    kind_of: impl Fn(Id) -> Option<Kind>,
    kinds: impl Fn(&[Id], Option<Kind>) -> Vec<Kind>,
) -> Cycle<Id, Kind> {
    let of_kind = |ids: &[Id], kind: Kind| -> Vec<Id> {
        ids.iter()
            .copied()
            .filter(|id| kind_of(*id) == Some(kind))
            .collect()
    };
    let (group, last) = match narrowed {
        Some((group, kind)) if of_kind(&group, kind) == selection => (group, Some(kind)),
        _ => (selection, None),
    };
    let kinds = kinds(&group, last);
    if kinds.len() < 2 {
        return Cycle::OneKind;
    }
    let next = match last {
        None => kinds.first().copied(),
        Some(kind) => kinds
            .iter()
            .position(|k| *k == kind)
            .and_then(|at| kinds.get(at + 1).copied()),
    };
    match next {
        Some(kind) => Cycle::Narrow {
            ids: of_kind(&group, kind),
            group,
            kind,
        },
        None => Cycle::Whole(group),
    }
}

/// RT: narrows a mixed selection to its first kind in the roster's order,
/// then each press to the next kind of the same group, and after the last
/// back to the whole group. A selection of one kind widens instead to
/// every one of that kind, as the panel's All button does.
fn by_kind(game: &mut Game, input: &mut InputState, bindings: &BindingMap) {
    let last = input.narrowed.take();
    let state = &game.state;
    let selection = &game.presentation.selection;
    // Units RT narrowed to that all died leave nothing selected; the walk
    // goes on over their group.
    let on_units = !selection.units.is_empty()
        || (selection.buildings.is_empty() && matches!(last, Some(Narrowed::Units { .. })));
    if on_units {
        let alive = |ids: &[UnitId]| -> Vec<UnitId> {
            ids.iter()
                .copied()
                .filter(|id| state.unit(*id).is_some())
                .collect()
        };
        let last = match last {
            Some(Narrowed::Units { group, kind }) => Some((alive(&group), kind)),
            _ => None,
        };
        let kind_of = |id| state.unit(id).map(|unit| unit.kind);
        match cycle(alive(&selection.units), last, kind_of, |ids, left| {
            crate::panel::kind_counts(ids.iter().filter_map(|id| kind_of(*id)).chain(left))
                .into_iter()
                .map(|(kind, _)| kind)
                .collect()
        }) {
            Cycle::OneKind => select_all(game, input, bindings),
            Cycle::Narrow { group, kind, ids } => {
                game.presentation.selection.units = ids;
                input.narrowed = Some(Narrowed::Units { group, kind });
            }
            Cycle::Whole(group) => game.presentation.selection.units = group,
        }
    } else {
        let alive = |ids: &[BuildingId]| -> Vec<BuildingId> {
            ids.iter()
                .copied()
                .filter(|id| state.building(*id).is_some())
                .collect()
        };
        let last = match last {
            Some(Narrowed::Buildings { group, kind }) => Some((alive(&group), kind)),
            _ => None,
        };
        let kind_of = |id| state.building(id).map(|b| b.kind);
        match cycle(alive(&selection.buildings), last, kind_of, |ids, left| {
            crate::panel::building_kinds(ids.iter().filter_map(|id| kind_of(*id)).chain(left))
        }) {
            Cycle::OneKind => select_all(game, input, bindings),
            Cycle::Narrow { group, kind, ids } => {
                game.presentation.selection.buildings = ids;
                input.narrowed = Some(Narrowed::Buildings { group, kind });
            }
            Cycle::Whole(group) => game.presentation.selection.buildings = group,
        }
    }
}

/// Presses the panel's All button for a selection of one kind, through the
/// card it published; with no such offer, as when every one is already
/// selected, the press is refused.
fn select_all(game: &mut Game, input: &mut InputState, bindings: &BindingMap) {
    let offer = game
        .presentation
        .panel_model
        .borrow()
        .as_ref()
        .and_then(|panel| panel.select_all.as_ref())
        .map(|all| all.card.action);
    match offer {
        Some(action) => super::activate_card(game, input, bindings, action),
        None => super::deny(game),
    }
}

#[cfg(test)]
mod aim_tests;
#[cfg(test)]
mod tests;
