use super::*;

#[test]
fn vendor_ids_name_the_pad_family() {
    for on_deck in [false, true] {
        assert_eq!(family(Some(0x045e), on_deck), PadFamily::Xbox);
        assert_eq!(family(Some(0x054c), on_deck), PadFamily::PlayStation);
        assert_eq!(family(Some(0x057e), on_deck), PadFamily::Switch);
        assert_eq!(family(Some(0x2dc8), on_deck), PadFamily::Other);
        assert_eq!(family(None, on_deck), PadFamily::Other);
    }
    assert_eq!(
        family(Some(0x28de), false),
        PadFamily::Xbox,
        "a pad Steam Input presents on a desktop"
    );
    assert_eq!(
        family(Some(0x28de), true),
        PadFamily::SteamDeck,
        "the Deck's own controls"
    );
}

#[test]
fn only_a_deck_board_reads_as_a_steam_deck() {
    assert!(is_steam_deck("Valve\n", "Jupiter\n"), "the LCD Deck");
    assert!(is_steam_deck("Valve\n", "Galileo\n"), "the OLED Deck");
    assert!(!is_steam_deck("Valve\n", "Index\n"));
    assert!(!is_steam_deck("LENOVO\n", "Jupiter\n"));
    assert!(!is_steam_deck("", ""));
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
