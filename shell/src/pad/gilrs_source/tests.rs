use super::*;

fn device(vendor: u16, product: u16) -> Device {
    Device {
        vendor: Some(vendor),
        product: Some(product),
    }
}

/// Steam's file for a docked Deck: its own controls in slot 0, an Xbox pad
/// in slot 1 and a PlayStation pad in slot 2.
const DOCKED: &str = "[slot 0]\nname=Steam Deck\nVID=0x28de\nPID=0x1205\ntype=steamdeck\n\
                      [slot 1]\nname=Xbox Wireless Controller\nVID=0x045e\nPID=0x0b13\n\
                      [slot 2]\nname=DualSense\nVID=1356\nPID=3302\n";

#[test]
fn vendor_ids_name_the_pad_family() {
    let none = SteamPads::default();
    let named = |vendor, product| family(device(vendor, product), "", &none);
    assert_eq!(named(0x045e, 0x0b13), PadFamily::Xbox);
    assert_eq!(named(0x054c, 0x0ce6), PadFamily::PlayStation);
    assert_eq!(named(0x057e, 0x2009), PadFamily::Switch);
    assert_eq!(named(0x2dc8, 0x3106), PadFamily::Other);
    assert_eq!(family(Device::default(), "", &none), PadFamily::Other);
    assert_eq!(
        named(0x28de, 0x1205),
        PadFamily::SteamDeck,
        "the Deck's own controls, read without Steam Input"
    );
    assert_eq!(named(0x28de, 0x1102), PadFamily::Xbox, "a Steam Controller");
}

#[test]
fn a_steam_input_pad_is_named_for_the_controller_behind_it() {
    let docked = SteamPads::parse(DOCKED);
    let virtual_pad = |name| family(device(0x28de, 0x11ff), name, &docked);
    assert_eq!(
        virtual_pad("Microsoft X-Box 360 pad 0"),
        PadFamily::SteamDeck
    );
    assert_eq!(virtual_pad("Microsoft X-Box 360 pad 1"), PadFamily::Xbox);
    assert_eq!(
        virtual_pad("Microsoft X-Box 360 pad 2"),
        PadFamily::PlayStation
    );
    assert_eq!(
        virtual_pad("Microsoft X-Box 360 pad 7"),
        PadFamily::Xbox,
        "a slot Steam says nothing about keeps Xbox's layout"
    );
    assert_eq!(
        family(
            device(0x28de, 0x11ff),
            "Microsoft X-Box 360 pad 0",
            &SteamPads::default()
        ),
        PadFamily::Xbox,
        "without Steam's file a virtual pad keeps Xbox's layout"
    );
    assert_eq!(
        family(device(0x045e, 0x0b13), "Xbox pad 0", &docked),
        PadFamily::Xbox,
        "only Steam's own virtual pads consult its file"
    );
}

#[test]
fn steams_file_keeps_each_slots_first_entry_and_skips_what_it_cannot_read() {
    let pads = SteamPads::parse(
        "VID=0x054c\n[slot 3]\nVID=0x057e\nPID=0x2009\n[slot 3]\nVID=0x045e\n\
         [slot x]\nVID=0x054c\n[slot 4]\nVID=nonsense\n",
    );
    assert_eq!(pads.behind("pad 3"), Some(device(0x057e, 0x2009)));
    assert_eq!(
        pads.behind("pad 4"),
        Some(Device::default()),
        "an unreadable id is unknown"
    );
    assert_eq!(pads.behind("pad 5"), None);
    assert_eq!(pads.behind("Steam Deck"), None, "a name without a slot");
}

#[test]
fn gilrs_buttons_keep_their_positions() {
    use gilrs::Button;
    assert_eq!(pad_button(Button::South), Some(PadButton::South));
    assert_eq!(pad_button(Button::East), Some(PadButton::East));
    assert_eq!(
        pad_button(Button::LeftTrigger),
        Some(PadButton::LeftShoulder)
    );
    assert_eq!(
        pad_button(Button::RightTrigger2),
        Some(PadButton::RightTrigger)
    );
    assert_eq!(pad_button(Button::DPadLeft), Some(PadButton::DpadLeft));
    assert_eq!(pad_button(Button::Mode), None, "the guide is the system's");
    assert_eq!(pad_button(Button::Unknown), None);
}

#[test]
fn stick_axes_turn_up_into_negative_y() {
    assert_eq!(
        stick_axis(1, gilrs::Axis::LeftStickY, 0.5),
        Some(Reading::Axis {
            pad: 1,
            stick: Stick::Left,
            axis: Axis::Y,
            value: -0.5,
        })
    );
    assert_eq!(
        stick_axis(1, gilrs::Axis::RightStickX, 0.5),
        Some(Reading::Axis {
            pad: 1,
            stick: Stick::Right,
            axis: Axis::X,
            value: 0.5,
        })
    );
    assert_eq!(stick_axis(1, gilrs::Axis::LeftZ, 1.0), None);
    assert_eq!(stick_axis(1, gilrs::Axis::DPadX, 1.0), None);
}
