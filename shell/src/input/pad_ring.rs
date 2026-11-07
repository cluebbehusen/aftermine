//! The command ring on a controller.
//!
//! Y opens a ring for the selection, and it stays open with nothing held:
//! the left stick points at a slice, A picks it, X cancels one of what it
//! queues, B goes back one ring and closes the first, and Y closes every
//! ring, as does holding B. While a ring is open it owns the pad's buttons
//! and left stick; the left trigger still queues. A ring is read off the
//! panel the HUD last published, as every card press is, and a pick
//! presses the card itself. The build rings step the same construction
//! state the build keys do, so the panel shows the palette beneath them.

use super::{InputState, deny};
use crate::action::{Action, BindingMap};
use crate::game::{Game, Scene};
use crate::panel::{CardAction, Panel};
use crate::radial::{self, Branch, Pick, Ring};
use aftermine_protocol::PadButton;
use macroquad::prelude::Vec2;

/// How long B must stay down to close every ring.
const HOLD_CLOSE_SECS: f64 = 0.5;

/// An open ring: which ring, what the stick points at, and the B that may
/// close every ring.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PadRing {
    /// The branches from the first ring to the one showing.
    path: Vec<Branch>,
    /// The slice the stick last pointed at. Letting the stick go keeps
    /// it, so A still picks it.
    pointed: Option<Pick>,
    /// The wall clock when it opened, for its scale-in.
    opened_at: f64,
    /// The pad holding B and since when, while that hold may still close
    /// every ring.
    back_held: Option<(u64, f64)>,
}

impl PadRing {
    /// When the ring opened.
    pub(crate) fn opened_at(&self) -> f64 {
        self.opened_at
    }

    /// The index of the slice the stick points at in `ring`.
    pub(crate) fn pointed(&self, ring: &Ring) -> Option<usize> {
        ring.position(self.pointed?)
    }
}

/// The open ring as the selection offers it now, off the published panel.
pub(crate) fn current(game: &Scene<'_>, input: &InputState) -> Option<Ring> {
    let open = input.ring.as_ref()?;
    let panel = game.presentation.panel_model.borrow();
    shown(game, panel.as_ref()?, open)
}

/// The ring `open` shows from `panel`.
pub(crate) fn shown(game: &Scene<'_>, panel: &Panel, open: &PadRing) -> Option<Ring> {
    radial::ring(game, panel, &open.path)
}

/// A controller button pressed during a match. Returns whether the ring
/// took it; a button the ring takes down, it also takes up.
pub(super) fn press(
    game: &mut Game,
    input: &mut InputState,
    bindings: &BindingMap,
    pad: u64,
    button: PadButton,
) -> bool {
    let taken = if input.ring.is_some() {
        match button {
            // Queueing and the pause menu reach past the ring.
            PadButton::LeftTrigger | PadButton::Start => false,
            PadButton::South => {
                pick(game, input, bindings);
                true
            }
            PadButton::West => {
                cancel(game, input, bindings);
                true
            }
            PadButton::East => {
                back(input, pad);
                true
            }
            PadButton::North => {
                dismiss(input);
                true
            }
            _ => true,
        }
    } else if button == PadButton::North {
        open(game, input);
        true
    } else {
        false
    };
    if taken {
        input.ring_held.push((pad, button));
    }
    taken
}

/// A controller button released during a match. Returns whether the ring
/// took its press, and so takes the release too: the A that picked a
/// structure never places one.
pub(super) fn release(input: &mut InputState, pad: u64, button: PadButton) -> bool {
    if button == PadButton::East
        && let Some(ring) = input.ring.as_mut()
        && ring.back_held.is_some_and(|(held, _)| held == pad)
    {
        ring.back_held = None;
    }
    let Some(at) = input
        .ring_held
        .iter()
        .position(|held| *held == (pad, button))
    else {
        return false;
    };
    input.ring_held.remove(at);
    true
}

/// Each frame: the stick points, a long-held B closes every ring, a ring
/// whose branch has gone falls back to the ring before it, and one left
/// with nothing to offer closes. A stick still pushed when a ring closed
/// moves the camera only once it has been let go.
pub(super) fn update(game: &Game, input: &mut InputState) {
    let stick = input.sticks().0;
    if stick == Vec2::ZERO {
        input.stick_latched = false;
    }
    let Some(open) = input.ring.as_ref() else {
        return;
    };
    let held_long = open
        .back_held
        .is_some_and(|(_, since)| input.now - since >= HOLD_CLOSE_SECS);
    let Some(ring) = current(&game.view(), input).filter(|_| !held_long) else {
        dismiss(input);
        return;
    };
    let toward =
        radial::slice_toward(stick, ring.slices.len()).map(|index| ring.slices[index].pick);
    let Some(open) = input.ring.as_mut() else {
        return;
    };
    if open.path != ring.path {
        open.path.clone_from(&ring.path);
        open.pointed = None;
    }
    if toward.is_some() {
        open.pointed = toward;
    } else if open
        .pointed
        .is_some_and(|pick| ring.position(pick).is_none())
    {
        open.pointed = None;
    }
}

/// Another pointer or a key took over: the ring belongs to the
/// controller, so it closes.
pub(super) fn other_hands(input: &mut InputState) {
    if input.ring.is_some() {
        dismiss(input);
    }
}

/// A controller went away: its buttons are no longer held, and with no
/// controller left nothing could drive the ring.
pub(super) fn disconnected(input: &mut InputState, pad: u64) {
    input.ring_held.retain(|(held, _)| *held != pad);
    if input.pads.is_empty() {
        other_hands(input);
    }
}

/// Y on a closed ring: opens it, unless the selection offers nothing or a
/// held A is still selecting. Over an open palette it opens on the build
/// ring the palette shows.
fn open(game: &mut Game, input: &mut InputState) {
    if input.pad_press.is_some() {
        deny(game);
        return;
    }
    let path = if input.build_menu {
        std::iter::once(Branch::Build)
            .chain(input.build_category.map(Branch::Category))
            .collect()
    } else {
        Vec::new()
    };
    input.ring = Some(PadRing {
        path,
        pointed: None,
        opened_at: input.now,
        back_held: None,
    });
    if current(&game.view(), input).is_none() {
        input.ring = None;
        deny(game);
    }
}

/// The slice the stick points at in the open ring, if any.
fn pointed(game: &Game, input: &InputState) -> Option<radial::Slice> {
    let ring = current(&game.view(), input)?;
    let index = input.ring.as_ref()?.pointed(&ring)?;
    ring.slices.into_iter().nth(index)
}

/// A: presses the pointed slice's card, as a click on it would, or opens
/// its ring. A disabled card explains itself instead, and a pick worth
/// repeating leaves the ring open so its count climbs.
fn pick(game: &mut Game, input: &mut InputState, bindings: &BindingMap) {
    let Some(slice) = pointed(game, input) else {
        deny(game);
        return;
    };
    match slice.pick {
        Pick::Open(branch) => enter(game, input, bindings, branch),
        Pick::Card(_) if !slice.card.enabled => {
            if let Some(why) = slice.card.why {
                game.presentation.toast(why);
            }
        }
        Pick::Card(action) => {
            super::activate_card(game, input, bindings, action);
            if !repeats(action) {
                close(input);
            }
        }
    }
}

/// Opens a branch's ring. The build rings step the construction state as
/// B and a category key do, so the panel's palette judges what they offer.
fn enter(game: &mut Game, input: &mut InputState, bindings: &BindingMap, branch: Branch) {
    match branch {
        Branch::Build => super::dispatch_action(game, input, bindings, Action::ToggleBuildPalette),
        Branch::Category(category) => {
            super::dispatch_action(game, input, bindings, Action::BuildCategory(category));
        }
        Branch::Workers | Branch::Drones | Branch::More => {}
    }
    if let Some(open) = input.ring.as_mut() {
        open.path.push(branch);
        open.pointed = None;
    }
}

/// X: cancels one of what the pointed slice queues, as the dock's own
/// chip does.
fn cancel(game: &mut Game, input: &mut InputState, bindings: &BindingMap) {
    match pointed(game, input).and_then(|slice| slice.cancel) {
        Some(action) => super::activate_card(game, input, bindings, action),
        None => deny(game),
    }
}

/// Whether a pick is one a player repeats in a row, so the ring stays
/// open after it: training a unit, ordering or cancelling a round, and
/// switching autofill.
fn repeats(action: CardAction) -> bool {
    matches!(
        action,
        CardAction::Dispatch(
            Action::TrainSlot(_)
                | Action::OrderRound
                | Action::CancelRound
                | Action::ToggleAutofill
        ) | CardAction::FillRounds
    )
}

/// B: goes back one ring, stepping the construction state back with it,
/// and closes the first. While B stays down it may close every ring.
fn back(input: &mut InputState, pad: u64) {
    let Some(open) = input.ring.as_mut() else {
        return;
    };
    match open.path.pop() {
        None => {
            dismiss(input);
            return;
        }
        Some(Branch::Category(_)) => {
            input.stop_placing();
            input.build_category = None;
        }
        Some(Branch::Build) => input.close_construction(),
        Some(Branch::Workers | Branch::Drones | Branch::More) => {}
    }
    if let Some(open) = input.ring.as_mut() {
        open.pointed = None;
        open.back_held = Some((pad, input.now));
    }
}

/// Closes every ring without a pick: a palette the rings opened closes
/// with them, unless a placement is armed.
fn dismiss(input: &mut InputState) {
    let building = input
        .ring
        .as_ref()
        .is_some_and(|open| open.path.contains(&Branch::Build));
    if building && input.placing.is_none() {
        input.close_construction();
    }
    close(input);
}

/// Closes every ring. A stick still pushed stays latched until let go.
fn close(input: &mut InputState) {
    input.ring = None;
    if input.sticks().0 != Vec2::ZERO {
        input.stick_latched = true;
    }
}

#[cfg(test)]
mod tests;
