//! Windows and Linux pads, Steam Deck included, read through gilrs.

use super::{Axis, Reading};
use aftermine_protocol::{PadButton, PadFamily, Stick};
use gilrs::{EventType, Filter, GamepadId, Gilrs, GilrsBuilder};

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
        let present = gilrs
            .gamepads()
            .map(|(id, gamepad)| Reading::Connected {
                pad: pad_id(id),
                family: family(gamepad.vendor_id()),
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
                EventType::Connected => Some(Reading::Connected {
                    pad,
                    family: family(self.gilrs.gamepad(event.id).vendor_id()),
                }),
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

/// The family a USB vendor id names. Valve's id covers the Steam Deck's
/// controls and every pad Steam Input presents, all in Xbox's layout.
pub(super) fn family(vendor: Option<u16>) -> PadFamily {
    match vendor {
        Some(0x045e | 0x28de) => PadFamily::Xbox,
        Some(0x054c) => PadFamily::PlayStation,
        Some(0x057e) => PadFamily::Switch,
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
