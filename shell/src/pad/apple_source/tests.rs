use super::*;

#[test]
fn listed_controllers_keep_their_pad_ids_and_leavers_disconnect() {
    let mut listing = Listing::<u32>::default();
    let mut readings = Vec::new();
    assert_eq!(listing.update(&[10, 20], &mut readings), [0, 1]);
    assert!(readings.is_empty(), "nothing has left yet");

    assert_eq!(listing.update(&[20, 30], &mut readings), [1, 2]);
    assert_eq!(readings, [Reading::Disconnected { pad: 0 }]);

    readings.clear();
    assert_eq!(
        listing.update(&[30, 10], &mut readings),
        [2, 3],
        "a controller that comes back gets a new id, as the fork's do"
    );
    assert_eq!(readings, [Reading::Disconnected { pad: 1 }]);

    readings.clear();
    assert!(listing.update(&[], &mut readings).is_empty());
    assert_eq!(
        readings,
        [
            Reading::Disconnected { pad: 2 },
            Reading::Disconnected { pad: 3 },
        ]
    );
}

#[test]
fn every_family_keeps_its_name() {
    assert_eq!(family(Family::Xbox), PadFamily::Xbox);
    assert_eq!(family(Family::PlayStation), PadFamily::PlayStation);
    assert_eq!(family(Family::Switch), PadFamily::Switch);
    assert_eq!(family(Family::Other), PadFamily::Other);
}

#[test]
fn apples_center_buttons_land_on_their_positions_and_home_stays_with_the_system() {
    assert_eq!(pad_button(Button::Options), Some(PadButton::Select));
    assert_eq!(pad_button(Button::Menu), Some(PadButton::Start));
    assert_eq!(pad_button(Button::South), Some(PadButton::South));
    assert_eq!(
        pad_button(Button::LeftShoulder),
        Some(PadButton::LeftShoulder)
    );
    assert_eq!(pad_button(Button::Home), None);
    let mut mapped: Vec<PadButton> = Button::ALL.into_iter().filter_map(pad_button).collect();
    let count = mapped.len();
    mapped.sort_by_key(|button| format!("{button:?}"));
    mapped.dedup();
    assert_eq!(mapped.len(), count, "two buttons share a name");
    assert_eq!(count, Button::ALL.len() - 1, "only Home is left out");
}
