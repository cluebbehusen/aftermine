//! Mac and iPad pads, read through Apple's GameController framework in the
//! miniquad fork.
//!
//! GameController reports each frame's pads whole, so this source restates
//! every connected pad each read and lets the adapter send only what
//! changed. A pad missing from a read has disconnected. Micro gamepads such
//! as the Siri Remote are left out.

use super::{Axis, Reading};
use aftermine_protocol::{PadButton, PadFamily, Stick};
use macroquad::miniquad::gamepad::{self, Button, ControllerId, Family, Gamepad};

/// The pads the last read listed.
pub(super) struct Source {
    listing: Listing<ControllerId>,
}

impl Source {
    /// GameController needs no setup and is always there.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "matches the sources whose platform API can fail to start"
    )]
    pub(super) fn open() -> Option<Self> {
        Some(Self {
            listing: Listing::default(),
        })
    }

    /// Restates every connected gamepad. Call it on the main thread, as
    /// the frame loop does.
    pub(super) fn read(&mut self, readings: &mut Vec<Reading>) {
        let gamepads = gamepad::controllers().gamepads;
        let ids: Vec<ControllerId> = gamepads.iter().map(|gamepad| gamepad.id).collect();
        let pads = self.listing.update(&ids, readings);
        for (pad, gamepad) in pads.into_iter().zip(&gamepads) {
            restate(pad, gamepad, readings);
        }
    }
}

/// The pad id each listed controller goes by. The fork keeps its own ids
/// private, so the funnel's ids are handed out here, in connection order,
/// and never reused.
#[derive(Debug)]
struct Listing<K> {
    listed: Vec<(K, u64)>,
    next: u64,
}

impl<K> Default for Listing<K> {
    fn default() -> Self {
        Self {
            listed: Vec::new(),
            next: 0,
        }
    }
}

impl<K: Copy + PartialEq> Listing<K> {
    /// Reports every pad missing from `now` as disconnected, then returns
    /// the pad id of each controller in `now`, in order.
    fn update(&mut self, now: &[K], readings: &mut Vec<Reading>) -> Vec<u64> {
        self.listed.retain(|(key, pad)| {
            let stays = now.contains(key);
            if !stays {
                readings.push(Reading::Disconnected { pad: *pad });
            }
            stays
        });
        now.iter()
            .map(|key| {
                if let Some((_, pad)) = self.listed.iter().find(|(listed, _)| listed == key) {
                    *pad
                } else {
                    let pad = self.next;
                    self.next += 1;
                    self.listed.push((*key, pad));
                    pad
                }
            })
            .collect()
    }
}

/// One gamepad's whole state as readings: its connection, every button it
/// has, and both sticks.
fn restate(pad: u64, gamepad: &Gamepad, readings: &mut Vec<Reading>) {
    readings.push(Reading::Connected {
        pad,
        family: family(gamepad.family),
    });
    for button in Button::ALL {
        if let Some(pad_button) = pad_button(button)
            && gamepad.has(button)
        {
            readings.push(Reading::Button {
                pad,
                button: pad_button,
                down: gamepad.is_pressed(button),
            });
        }
    }
    for (stick, at) in [
        (Stick::Left, gamepad.left_stick),
        (Stick::Right, gamepad.right_stick),
    ] {
        for (axis, value) in [(Axis::X, at.x), (Axis::Y, -at.y)] {
            readings.push(Reading::Axis {
                pad,
                stick,
                axis,
                value,
            });
        }
    }
}

fn family(family: Family) -> PadFamily {
    match family {
        Family::Xbox => PadFamily::Xbox,
        Family::PlayStation => PadFamily::PlayStation,
        Family::Switch => PadFamily::Switch,
        Family::Other => PadFamily::Other,
    }
}

/// The protocol's name for a GameController button. Both name face buttons
/// by position. Apple's Options is the left center button, the protocol's
/// Select, whatever a PlayStation pad prints there; Menu is the right one.
/// The Home button belongs to the system.
fn pad_button(button: Button) -> Option<PadButton> {
    Some(match button {
        Button::South => PadButton::South,
        Button::East => PadButton::East,
        Button::West => PadButton::West,
        Button::North => PadButton::North,
        Button::LeftShoulder => PadButton::LeftShoulder,
        Button::RightShoulder => PadButton::RightShoulder,
        Button::LeftTrigger => PadButton::LeftTrigger,
        Button::RightTrigger => PadButton::RightTrigger,
        Button::LeftStick => PadButton::LeftStick,
        Button::RightStick => PadButton::RightStick,
        Button::DpadUp => PadButton::DpadUp,
        Button::DpadDown => PadButton::DpadDown,
        Button::DpadLeft => PadButton::DpadLeft,
        Button::DpadRight => PadButton::DpadRight,
        Button::Options => PadButton::Select,
        Button::Menu => PadButton::Start,
        Button::Home => return None,
    })
}

#[cfg(test)]
mod tests;
