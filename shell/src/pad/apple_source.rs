//! Mac, iPad and Apple TV pads, read through Apple's GameController framework
//! in the miniquad fork.
//!
//! GameController reports each frame's pads whole, so this source restates
//! every connected pad each read and lets the adapter send only what
//! changed. A pad missing from a read has disconnected. On Apple TV the Siri
//! Remote, a micro gamepad, reads as a pad of the remote family: its center
//! clicks, the ring around it is a D-pad, and swipes tap the D-pad too. A
//! remote with no ring clicks with its whole surface. Elsewhere micro gamepads
//! are left out.

use super::{Axis, Reading};
use crate::platform::{PLATFORM, Platform};
use aftermine_protocol::{PadButton, PadFamily, Stick};
use macroquad::miniquad::gamepad::{
    self, Button, ControllerId, Family, Gamepad, MicroButton, MicroGamepad,
};
use macroquad::prelude::{Vec2, vec2};
use std::collections::BTreeMap;

mod swipe;
use swipe::Swipe;

/// The pads the last read listed.
pub(super) struct Source {
    listing: Listing<ControllerId>,
    /// Each listed remote's touch in progress.
    swipes: BTreeMap<ControllerId, Swipe>,
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
            swipes: BTreeMap::new(),
        })
    }

    /// Restates every connected gamepad. Call it on the main thread, as
    /// the frame loop does.
    pub(super) fn read(&mut self, readings: &mut Vec<Reading>) {
        let controllers = gamepad::controllers();
        let remotes: &[MicroGamepad] = if PLATFORM == Platform::Tv {
            &controllers.micro_gamepads
        } else {
            &[]
        };
        let ids: Vec<ControllerId> = controllers
            .gamepads
            .iter()
            .map(|gamepad| gamepad.id)
            .chain(remotes.iter().map(|remote| remote.id))
            .collect();
        let pads = self.listing.update(&ids, readings);
        let (gamepad_pads, remote_pads) = pads.split_at(controllers.gamepads.len());
        for (pad, gamepad) in gamepad_pads.iter().zip(&controllers.gamepads) {
            restate(*pad, gamepad, readings);
        }
        self.swipes
            .retain(|id, _| remotes.iter().any(|remote| remote.id == *id));
        for (pad, remote) in remote_pads.iter().zip(remotes) {
            restate_remote(
                *pad,
                &remote_state(remote),
                self.swipes.entry(remote.id).or_default(),
                readings,
            );
        }
    }
}

/// What a remote's buttons and touch mean. A remote that can tell its center
/// from its ring clicks with the center alone.
fn remote_state(remote: &MicroGamepad) -> RemoteState {
    let click = if remote.has(MicroButton::Center) {
        MicroButton::Center
    } else {
        MicroButton::Select
    };
    RemoteState {
        touch: vec2(remote.touch.x, -remote.touch.y),
        surface: remote.is_pressed(MicroButton::Select),
        held: [
            (click, super::CONFIRM),
            (MicroButton::PlayPause, PadButton::PlayPause),
            (MicroButton::Menu, super::BACK),
            (MicroButton::Up, PadButton::DpadUp),
            (MicroButton::Down, PadButton::DpadDown),
            (MicroButton::Left, PadButton::DpadLeft),
            (MicroButton::Right, PadButton::DpadRight),
        ]
        .into_iter()
        .filter(|(button, _)| remote.is_pressed(*button))
        .map(|(_, pad_button)| pad_button)
        .collect(),
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

/// What a Siri Remote is doing at one read.
#[derive(Debug, Clone, Default)]
struct RemoteState {
    /// Where the touch is, x to the right and y down; zero with no touch.
    touch: Vec2,
    /// A press anywhere on the surface, which no swipe travels through.
    surface: bool,
    /// The buttons down, as the pad buttons they stand for.
    held: Vec<PadButton>,
}

/// Every pad button a remote can hold.
const REMOTE_BUTTONS: [PadButton; 7] = [
    super::CONFIRM,
    PadButton::PlayPause,
    super::BACK,
    PadButton::DpadUp,
    PadButton::DpadDown,
    PadButton::DpadLeft,
    PadButton::DpadRight,
];

/// One remote's whole state as readings: its connection, its buttons, and
/// any swipe travel since the last read, raw and as a D-pad tap.
fn restate_remote(pad: u64, state: &RemoteState, swipe: &mut Swipe, readings: &mut Vec<Reading>) {
    readings.push(Reading::Connected {
        pad,
        family: PadFamily::Remote,
    });
    for button in REMOTE_BUTTONS {
        readings.push(Reading::Button {
            pad,
            button,
            down: state.held.contains(&button),
        });
    }
    let (tap, travel) = swipe.feed(state.touch, state.surface);
    if travel != Vec2::ZERO {
        readings.push(Reading::Swipe {
            pad,
            x: travel.x,
            y: travel.y,
        });
    }
    if let Some(button) = tap {
        for down in [true, false] {
            readings.push(Reading::Button { pad, button, down });
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
