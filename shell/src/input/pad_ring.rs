//! The controller's rings.
//!
//! Y opens a ring of the selection's commands and D-pad right a ring of
//! the control groups. Either stays open with nothing held: the left stick
//! points at a slice, A picks it, X acts on it a second way, B goes back
//! one ring and closes the first, and the button that opened a ring closes
//! every ring, as does holding B; the other ring's button opens that ring
//! in its place. While a ring is open it owns the pad's buttons and left
//! stick; the left trigger still queues, Menu still opens the pause menu
//! and View still pauses.
//!
//! A command ring is read off the panel the HUD last published, as every
//! card press is, a pick presses the card itself, and X cancels one of
//! what a slice queues or else does what a right-click on its card does.
//! The build rings step the same construction state the build keys do, so
//! the panel shows the palette beneath them. The group ring recalls a
//! group with A and saves the selection to it with X, through the actions
//! the group keys dispatch.

use super::{InputState, deny};
use crate::action::{Action, BindingMap};
use crate::game::{Game, Scene};
use crate::panel::{CardAction, Panel};
use crate::radial::{self, Branch, Pick, Ring, RingKind};
use aftermine_protocol::PadButton;
use macroquad::prelude::Vec2;

/// How long B must stay down to close every ring.
const HOLD_CLOSE_SECS: f64 = 0.5;

/// An open ring: which ring, what the stick points at, and the B that may
/// close every ring.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PadRing {
    /// Which first ring it stands in.
    kind: RingKind,
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

/// The open ring as it stands now: the selection's commands off the
/// published panel, or the control groups.
pub(crate) fn current(game: &Scene<'_>, input: &InputState) -> Option<Ring> {
    let panel = game.presentation.panel_model.borrow();
    shown(game, panel.as_ref(), input)
}

/// The ring open in `input`, a command ring reading its cards from
/// `panel`.
pub(crate) fn shown(game: &Scene<'_>, panel: Option<&Panel>, input: &InputState) -> Option<Ring> {
    let open = input.ring.as_ref()?;
    match open.kind {
        RingKind::Commands => radial::ring(game, panel?, &open.path),
        RingKind::Groups => radial::groups(game, &input.groups),
    }
}

/// The first ring a button opens: Y the commands, D-pad right the groups.
fn opened_by(button: PadButton) -> Option<RingKind> {
    match button {
        PadButton::North => Some(RingKind::Commands),
        PadButton::DpadRight => Some(RingKind::Groups),
        _ => None,
    }
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
    let taken = if let Some(open) = input.ring.as_ref() {
        let kind = open.kind;
        match button {
            // Queueing, the pause menu and pausing reach past the ring.
            PadButton::LeftTrigger | PadButton::Start | PadButton::Select => false,
            PadButton::South => {
                pick(game, input, bindings);
                true
            }
            PadButton::West => {
                second(game, input, bindings);
                true
            }
            PadButton::East => {
                back(input, pad);
                true
            }
            _ => {
                match opened_by(button) {
                    Some(opens) if opens == kind => dismiss(input),
                    Some(opens) => swap(game, input, opens),
                    None => {}
                }
                true
            }
        }
    } else if let Some(kind) = opened_by(button) {
        open(game, input, kind);
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
    aim(&ring, input);
}

/// Points the open ring at the slice the stick pushes toward, and follows
/// the ring back when its branch has gone. It runs each frame and again as
/// A or X is handled: a frame records its stick moves before its buttons,
/// so a flick and a press in one frame pick where the flick points.
fn aim(ring: &Ring, input: &mut InputState) {
    let toward = radial::slice_toward(input.sticks().0, ring.slices.len())
        .map(|index| ring.slices[index].pick);
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

/// A ring's button with no ring open: opens its ring, unless it has
/// nothing to offer or a held A is still selecting or laying a row.
fn open(game: &mut Game, input: &mut InputState, kind: RingKind) {
    if input.pad_press.is_some() || input.stroke_by().is_some() || !show(game, input, kind) {
        deny(game);
    }
}

/// The other ring's button over an open ring: opens that ring in its
/// place, closing the open one as its own button would. Should the other
/// have nothing to offer, the open ring stays.
fn swap(game: &mut Game, input: &mut InputState, kind: RingKind) {
    let building = building(input.ring.as_ref());
    if !show(game, input, kind) {
        deny(game);
        return;
    }
    if building && input.placing.is_none() {
        input.close_construction();
    }
}

/// Shows `kind`'s first ring in place of any open one, as its button
/// opens it: over an open palette, the command ring opens on the build
/// ring the palette shows. Returns false, leaving any open ring as it
/// was, when `kind` has nothing to offer.
fn show(game: &Game, input: &mut InputState, kind: RingKind) -> bool {
    let path = if kind == RingKind::Commands && input.build_menu {
        std::iter::once(Branch::Build)
            .chain(input.build_category.map(Branch::Category))
            .collect()
    } else {
        Vec::new()
    };
    let was = input.ring.replace(PadRing {
        kind,
        path,
        pointed: None,
        opened_at: input.now,
        back_held: None,
    });
    if current(&game.view(), input).is_some() {
        return true;
    }
    input.ring = was;
    false
}

/// The slice the stick points at in the open ring as it stands now, if
/// any.
fn pointed(game: &Game, input: &mut InputState) -> Option<radial::Slice> {
    let ring = current(&game.view(), input)?;
    aim(&ring, input);
    let index = input.ring.as_ref()?.pointed(&ring)?;
    ring.slices.into_iter().nth(index)
}

/// A: presses the pointed slice's card, as a click on it would, opens its
/// ring, or recalls its group. A disabled card explains itself instead, an
/// empty group refuses, and a pick worth repeating leaves the ring open so
/// its count climbs.
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
        Pick::Group(_) if !slice.card.enabled => deny(game),
        Pick::Group(group) => {
            super::dispatch_action(game, input, bindings, Action::Slot(group));
            close(input);
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

/// X: saves the selection to the pointed group, as the group keys do,
/// and closes the ring; on a command, cancels one of what the slice
/// queues, as the dock's own chip does, or else does what a right-click
/// on its card does.
fn second(game: &mut Game, input: &mut InputState, bindings: &BindingMap) {
    let Some(slice) = pointed(game, input) else {
        deny(game);
        return;
    };
    match (slice.pick, slice.second()) {
        (Pick::Group(group), _) => {
            super::dispatch_action(game, input, bindings, Action::AssignGroup(group));
            close(input);
        }
        (_, Some(action)) => super::activate_card(game, input, bindings, action),
        (_, None) => deny(game),
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
    if building(input.ring.as_ref()) && input.placing.is_none() {
        input.close_construction();
    }
    close(input);
}

/// Whether `open` stands in the build rings, whose palette closes with
/// them.
fn building(open: Option<&PadRing>) -> bool {
    open.is_some_and(|open| open.path.contains(&Branch::Build))
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
