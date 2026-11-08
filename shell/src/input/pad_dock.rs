//! The queue strip: a controller's way onto the orders dock.
//!
//! The left-stick click lights the dock's chip nearest the command band.
//! The D-pad walks the dock's chips and the Stop button above them, and
//! steps across to the drone column beside them; A presses the lit one as
//! a click would, and B or another click leaves. While the strip is open it
//! owns the pad's buttons and left stick and the camera holds still; the
//! left trigger still queues, Menu still opens the pause menu, View still
//! pauses, and Y leaves it for the command ring. It closes when the dock
//! empties, the selection changes or another hand takes over.

use super::{InputState, deny};
use crate::action::BindingMap;
use crate::game::Game;
use crate::layout::{CardHit, CardRow, LayoutModel};
use crate::nav::{Axis, Nav};
use crate::panel::CardAction;
use aftermine_protocol::PadButton;
use aftermine_sim::{BuildingId, UnitId};
use macroquad::prelude::{Rect, Vec2};

/// The open strip: the lit entry and the selection it opened over.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DockFocus {
    /// The lit entry as the HUD last published it.
    lit: CardHit,
    /// The selection the strip opened over; any other closes it.
    selection: (Vec<UnitId>, Vec<BuildingId>),
}

impl DockFocus {
    /// The lit entry as the HUD last published it: its row, index, rect
    /// and what a press on it does.
    pub(crate) fn lit(&self) -> CardHit {
        self.lit
    }
}

/// One pressable entry of the dock as the HUD published it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Entry {
    pub(crate) hit: CardHit,
    /// Whether it stands in the drone column rather than the dock.
    drones: bool,
}

/// The entries of one published row, each in `drones`' column.
fn row(
    row: CardRow,
    slots: &[(Rect, CardAction)],
    drones: bool,
) -> impl Iterator<Item = Entry> + '_ {
    slots
        .iter()
        .enumerate()
        .map(move |(index, &(rect, action))| Entry {
            hit: CardHit {
                row,
                index,
                rect,
                action,
            },
            drones,
        })
}

/// The dock's pressable entries: its chips and Stop button, then the
/// drone column's chips, each column top to bottom. Chips that only
/// display are left out.
pub(crate) fn entries(layout: &LayoutModel) -> Vec<Entry> {
    let mut all: Vec<Entry> = row(
        CardRow::Queue,
        &layout.queue_slots[..layout.queue_count],
        false,
    )
    .chain(row(
        CardRow::Stop,
        std::slice::from_ref(&layout.queue_stop),
        false,
    ))
    .chain(row(
        CardRow::Drones,
        &layout.drone_slots[..layout.drone_count],
        true,
    ))
    .filter(|entry| entry.hit.rect.w > 0.0 && entry.hit.action != CardAction::None)
    .collect();
    all.sort_by(|a, b| {
        a.drones
            .cmp(&b.drones)
            .then(a.hit.rect.y.total_cmp(&b.hit.rect.y))
    });
    all
}

/// The entry nearest `to` among `entries`, only in the drone column or
/// only in the dock when `drones` says which.
fn nearest(entries: &[Entry], to: Vec2, drones: Option<bool>) -> Option<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| drones.is_none_or(|column| entry.drones == column))
        .min_by(|(_, a), (_, b)| {
            let (a, b) = (a.hit.rect.center(), b.hit.rect.center());
            a.distance(to).total_cmp(&b.distance(to))
        })
        .map(|(at, _)| at)
}

/// Where `focus` stands in `entries`: on the entry it lit if that still
/// stands, else on the one nearest where it stood.
fn lit_in(entries: &[Entry], focus: &DockFocus) -> Option<usize> {
    let lit = focus.lit;
    entries
        .iter()
        .position(|entry| (entry.hit.row, entry.hit.index) == (lit.row, lit.index))
        .or_else(|| nearest(entries, lit.rect.center(), None))
}

/// A controller button pressed during a match. Returns whether the strip
/// took it; a button the strip takes down, it also takes up.
pub(super) fn press(
    game: &mut Game,
    input: &mut InputState,
    bindings: &BindingMap,
    pad: u64,
    button: PadButton,
) -> bool {
    let taken = if input.dock.is_some() {
        match button {
            // Queueing, the pause menu and pausing reach past the strip.
            PadButton::LeftTrigger | PadButton::Start | PadButton::Select => false,
            // Y leaves the strip for the command ring.
            PadButton::North => {
                close(input);
                false
            }
            PadButton::South => {
                press_lit(game, input, bindings);
                true
            }
            PadButton::East | PadButton::LeftStick => {
                close(input);
                true
            }
            PadButton::DpadUp => {
                walk(game, input, Nav::Up);
                true
            }
            PadButton::DpadDown => {
                walk(game, input, Nav::Down);
                true
            }
            PadButton::DpadLeft => {
                cross(game, input, false);
                true
            }
            PadButton::DpadRight => {
                cross(game, input, true);
                true
            }
            _ => true,
        }
    } else if button == PadButton::LeftStick && input.ring.is_none() {
        open(game, input);
        true
    } else {
        false
    };
    if taken {
        input.dock_held.push((pad, button));
    }
    taken
}

/// A controller button released during a match. Returns whether the strip
/// took its press, and so takes the release too: the A that cancelled a
/// chip never selects at the reticle.
pub(super) fn release(input: &mut InputState, pad: u64, button: PadButton) -> bool {
    let Some(at) = input
        .dock_held
        .iter()
        .position(|held| *held == (pad, button))
    else {
        return false;
    };
    input.dock_held.remove(at);
    true
}

/// Each frame: the strip follows the dock as the HUD published it,
/// lighting the entry nearest where the lit one stood once that one goes,
/// and closes once the dock has nothing to press or the selection changed.
pub(super) fn update(game: &Game, input: &mut InputState) {
    let Some(focus) = input.dock.as_ref() else {
        return;
    };
    let selection = &game.presentation.selection;
    if focus.selection.0 != selection.units || focus.selection.1 != selection.buildings {
        close(input);
        return;
    }
    let entries = entries(&game.presentation.layout.get());
    match lit_in(&entries, focus) {
        Some(at) => {
            if let Some(focus) = input.dock.as_mut() {
                focus.lit = entries[at].hit;
            }
        }
        None => close(input),
    }
}

/// Another pointer or a key took over: the strip belongs to the
/// controller, so it closes.
pub(super) fn other_hands(input: &mut InputState) {
    if input.dock.is_some() {
        close(input);
    }
}

/// A controller went away: its buttons are no longer held, and with no
/// controller left nothing could drive the strip.
pub(super) fn disconnected(input: &mut InputState, pad: u64) {
    input.dock_held.retain(|(held, _)| *held != pad);
    if input.pads.is_empty() {
        other_hands(input);
    }
}

/// The left-stick click: lights the dock's chip nearest the command band,
/// or the drone column's when the dock has none, unless a held A is still
/// selecting or laying a row, or there is nothing to press.
fn open(game: &mut Game, input: &mut InputState) {
    if input.pad_press.is_some() || input.stroke_by().is_some() {
        deny(game);
        return;
    }
    let entries = entries(&game.presentation.layout.get());
    let first = entries
        .iter()
        .rfind(|entry| !entry.drones)
        .or_else(|| entries.iter().rfind(|entry| entry.drones));
    let Some(first) = first else {
        deny(game);
        return;
    };
    let selection = &game.presentation.selection;
    input.dock = Some(DockFocus {
        lit: first.hit,
        selection: (selection.units.clone(), selection.buildings.clone()),
    });
}

/// D-pad up or down: the next entry up or down the lit one's column,
/// wrapping at its ends as every menu does.
fn walk(game: &Game, input: &mut InputState, nav: Nav) {
    let Some(focus) = input.dock.as_mut() else {
        return;
    };
    let entries = entries(&game.presentation.layout.get());
    let Some(at) = lit_in(&entries, focus) else {
        return;
    };
    let column: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry.drones == entries[at].drones)
        .collect();
    let Some(here) = column.iter().position(|entry| entry.hit == entries[at].hit) else {
        return;
    };
    if let Some(next) = crate::nav::step_line(column.len(), here, nav, Axis::Vertical, 1, |_| true)
    {
        focus.lit = column[next].hit;
    }
}

/// D-pad left or right: the entry of the other column nearest the lit
/// one, the drone column lying right of the dock.
fn cross(game: &mut Game, input: &mut InputState, right: bool) {
    let Some(focus) = input.dock.as_mut() else {
        return;
    };
    let entries = entries(&game.presentation.layout.get());
    match nearest(&entries, focus.lit.rect.center(), Some(right)) {
        Some(at) if entries[at].hit != focus.lit => focus.lit = entries[at].hit,
        _ => deny(game),
    }
}

/// A: presses the lit entry as a click on it would.
fn press_lit(game: &mut Game, input: &mut InputState, bindings: &BindingMap) {
    let entries = entries(&game.presentation.layout.get());
    let Some(hit) = input
        .dock
        .as_ref()
        .and_then(|focus| lit_in(&entries, focus))
        .map(|at| entries[at].hit)
    else {
        deny(game);
        return;
    };
    super::press_card(game, input, bindings, hit);
}

/// Closes the strip. A stick still pushed stays latched until let go, as
/// after the ring.
fn close(input: &mut InputState) {
    input.dock = None;
    if input.sticks().0 != Vec2::ZERO {
        input.stick_latched = true;
    }
}

#[cfg(test)]
mod tests;
