//! Windows and Linux pads, Steam Deck included, read through gilrs.

use super::{Axis, Reading};
use aftermine_protocol::{PadButton, PadFamily, Stick};
use gilrs::{EventType, Filter, GamepadId, Gilrs, GilrsBuilder};
use std::collections::BTreeMap;

/// gilrs, and the pads it found already connected when it started.
pub(super) struct Source {
    gilrs: Gilrs,
    /// gilrs sends no connection event for a pad present at startup, so
    /// the first read announces these.
    present: Vec<Reading>,
}

impl Source {
    /// Starts gilrs, or reports why it cannot start.
    pub(super) fn open() -> Option<Self> {
        // The adapter owns dead zones and change filtering, so gilrs's
        // default filters, which add a second per-axis dead zone, stay off.
        // Its D-pad filter runs by hand because some Linux pads report the
        // D-pad as a hat axis, and it reads the pad state gilrs keeps, so
        // that state is updated only after filtering.
        let built = GilrsBuilder::new()
            .with_default_filters(false)
            .set_update_state(false)
            .with_force_feedback(false)
            .build();
        let gilrs = match built {
            Ok(gilrs) => gilrs,
            Err(error) => {
                eprintln!("Controllers unavailable: {error}");
                return None;
            }
        };
        let steam = SteamPads::read();
        let present = gilrs
            .gamepads()
            .map(|(id, gamepad)| Reading::Connected {
                pad: pad_id(id),
                family: family(identity(&gamepad), gamepad.os_name(), &steam),
            })
            .collect();
        Some(Self { gilrs, present })
    }

    /// Appends everything gilrs reported since the last read.
    pub(super) fn read(&mut self, readings: &mut Vec<Reading>) {
        readings.append(&mut self.present);
        while let Some(event) = self
            .gilrs
            .next_event()
            .filter_ev(&gilrs::ev::filter::axis_dpad_to_button, &mut self.gilrs)
        {
            if event.is_dropped() {
                continue;
            }
            self.gilrs.update(&event);
            let pad = pad_id(event.id);
            let reading = match event.event {
                EventType::Connected => {
                    let gamepad = self.gilrs.gamepad(event.id);
                    Some(Reading::Connected {
                        pad,
                        family: family(identity(&gamepad), gamepad.os_name(), &SteamPads::read()),
                    })
                }
                EventType::Disconnected => Some(Reading::Disconnected { pad }),
                EventType::ButtonPressed(button, _) => {
                    pad_button(button).map(|button| Reading::Button {
                        pad,
                        button,
                        down: true,
                    })
                }
                EventType::ButtonReleased(button, _) => {
                    pad_button(button).map(|button| Reading::Button {
                        pad,
                        button,
                        down: false,
                    })
                }
                EventType::AxisChanged(axis, value, _) => stick_axis(pad, axis, value),
                _ => None,
            };
            readings.extend(reading);
        }
    }
}

fn pad_id(id: GamepadId) -> u64 {
    usize::from(id) as u64
}

/// A controller's USB identity, as far as the platform reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Device {
    pub(super) vendor: Option<u16>,
    pub(super) product: Option<u16>,
}

const VALVE: u16 = 0x28de;
/// The Steam Deck's built-in controls, read without Steam Input.
const STEAM_DECK: u16 = 0x1205;
/// The pad Steam Input presents in place of each controller it wraps.
const STEAM_VIRTUAL_PAD: u16 = 0x11ff;

fn identity(gamepad: &gilrs::Gamepad<'_>) -> Device {
    Device {
        vendor: gamepad.vendor_id(),
        product: gamepad.product_id(),
    }
}

/// The controllers behind Steam Input's virtual pads, by slot, from the
/// file Steam names in `SteamVirtualGamepadInfo` for the games it starts.
#[derive(Debug, Default)]
pub(super) struct SteamPads {
    real: BTreeMap<u32, Device>,
}

impl SteamPads {
    /// Steam's current file. Steam rewrites it as controllers come and go,
    /// so each connection reads it again; outside Steam there is none.
    fn read() -> Self {
        std::env::var_os("SteamVirtualGamepadInfo")
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map_or_else(Self::default, |text| Self::parse(&text))
    }

    /// Parses Steam's file: a `[slot N]` heading per virtual pad, then
    /// `key=value` lines whose `VID` and `PID` name the controller behind
    /// it. A repeated slot keeps its first entry.
    pub(super) fn parse(text: &str) -> Self {
        let mut real = BTreeMap::new();
        let mut slot = None;
        for line in text.lines().map(str::trim) {
            if let Some(heading) = line.strip_prefix("[slot ") {
                slot = heading
                    .strip_suffix(']')
                    .and_then(|n| n.trim().parse::<u32>().ok())
                    .filter(|n| !real.contains_key(n));
                if let Some(n) = slot {
                    real.insert(n, Device::default());
                }
                continue;
            }
            let (Some(n), Some((key, value))) = (slot, line.split_once('=')) else {
                continue;
            };
            let Some(device) = real.get_mut(&n) else {
                continue;
            };
            match key.trim().as_bytes() {
                b"VID" => device.vendor = id_number(value),
                b"PID" => device.product = id_number(value),
                _ => {}
            }
        }
        Self { real }
    }

    /// The controller behind the virtual pad the system names `name`.
    /// Steam names each one ending in `pad N`, N being its slot.
    pub(super) fn behind(&self, name: &str) -> Option<Device> {
        let rest = &name[name.find("pad ")? + 4..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        self.real.get(&digits.parse().ok()?).copied()
    }
}

/// A USB id written in hex with `0x` or in decimal.
fn id_number(value: &str) -> Option<u16> {
    let value = value.trim();
    match value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        Some(hex) => u16::from_str_radix(hex, 16).ok(),
        None => value.parse().ok(),
    }
}

/// The family a pad's identity names. Steam Input presents every
/// controller it wraps as the same Valve pad, so for those the family is
/// the controller Steam says is behind it, and one it says nothing about is
/// in Xbox's layout. The Deck's built-in controls name themselves when
/// read without Steam Input.
pub(super) fn family(device: Device, name: &str, steam: &SteamPads) -> PadFamily {
    let device = if device.vendor == Some(VALVE) && device.product == Some(STEAM_VIRTUAL_PAD) {
        steam.behind(name).unwrap_or(device)
    } else {
        device
    };
    match (device.vendor, device.product) {
        (Some(VALVE), Some(STEAM_DECK)) => PadFamily::SteamDeck,
        (Some(0x045e | VALVE), _) => PadFamily::Xbox,
        (Some(0x054c), _) => PadFamily::PlayStation,
        (Some(0x057e), _) => PadFamily::Switch,
        _ => PadFamily::Other,
    }
}

/// The protocol's name for a gilrs button. gilrs names face buttons by
/// position, as the protocol does. The guide button belongs to the system,
/// and the extra C and Z buttons of a few pads have no role.
pub(super) fn pad_button(button: gilrs::Button) -> Option<PadButton> {
    use gilrs::Button;
    Some(match button {
        Button::South => PadButton::South,
        Button::East => PadButton::East,
        Button::West => PadButton::West,
        Button::North => PadButton::North,
        Button::DPadUp => PadButton::DpadUp,
        Button::DPadDown => PadButton::DpadDown,
        Button::DPadLeft => PadButton::DpadLeft,
        Button::DPadRight => PadButton::DpadRight,
        Button::LeftTrigger => PadButton::LeftShoulder,
        Button::RightTrigger => PadButton::RightShoulder,
        Button::LeftTrigger2 => PadButton::LeftTrigger,
        Button::RightTrigger2 => PadButton::RightTrigger,
        Button::LeftThumb => PadButton::LeftStick,
        Button::RightThumb => PadButton::RightStick,
        Button::Select => PadButton::Select,
        Button::Start => PadButton::Start,
        Button::Mode | Button::C | Button::Z | Button::Unknown => return None,
    })
}

/// A stick axis change as a reading. gilrs reads up as positive, and the
/// protocol reads down.
pub(super) fn stick_axis(pad: u64, axis: gilrs::Axis, value: f32) -> Option<Reading> {
    use gilrs::Axis as A;
    let (stick, axis, value) = match axis {
        A::LeftStickX => (Stick::Left, Axis::X, value),
        A::LeftStickY => (Stick::Left, Axis::Y, -value),
        A::RightStickX => (Stick::Right, Axis::X, value),
        A::RightStickY => (Stick::Right, Axis::Y, -value),
        _ => return None,
    };
    Some(Reading::Axis {
        pad,
        stick,
        axis,
        value,
    })
}

#[cfg(test)]
mod tests;
