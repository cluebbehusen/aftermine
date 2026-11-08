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

fn remote_readings(state: &RemoteState, swipe: &mut Swipe) -> Vec<Reading> {
    let mut readings = Vec::new();
    restate_remote(5, state, swipe, &mut readings);
    readings
}

#[test]
fn a_remote_reads_as_the_remote_family_with_click_and_back_by_position() {
    let readings = remote_readings(
        &RemoteState {
            surface: true,
            held: vec![PadButton::South],
            ..RemoteState::default()
        },
        &mut Swipe::default(),
    );
    assert_eq!(
        readings[..4],
        [
            Reading::Connected {
                pad: 5,
                family: PadFamily::Remote,
            },
            Reading::Button {
                pad: 5,
                button: PadButton::South,
                down: true,
            },
            Reading::Button {
                pad: 5,
                button: PadButton::PlayPause,
                down: false,
            },
            Reading::Button {
                pad: 5,
                button: PadButton::East,
                down: false,
            },
        ]
    );
}

#[test]
fn a_remotes_ring_holds_a_dpad_direction_and_clicks_nothing() {
    let mut swipe = Swipe::default();
    let press = RemoteState {
        touch: vec2(-0.8, 0.0),
        surface: true,
        held: vec![PadButton::DpadLeft],
    };
    remote_readings(&press, &mut swipe);
    let readings = remote_readings(&press, &mut swipe);
    let down = |button| {
        readings
            .iter()
            .filter(|reading| {
                matches!(reading, Reading::Button { button: at, down: true, .. } if *at == button)
            })
            .count()
    };
    assert_eq!(down(PadButton::DpadLeft), 1, "it stays held");
    assert_eq!(down(PadButton::South), 0, "the ring is not a click");
    assert!(
        readings
            .iter()
            .all(|reading| !matches!(reading, Reading::Swipe { .. })),
        "a finger shifting as it presses is no swipe"
    );
}

#[test]
fn a_remotes_swipe_presses_and_releases_a_dpad_direction_in_one_read() {
    let mut swipe = Swipe::default();
    let at = |x, y| RemoteState {
        touch: vec2(x, y),
        ..RemoteState::default()
    };
    remote_readings(&at(0.1, 0.1), &mut swipe);
    let readings = remote_readings(&at(0.1, 0.6), &mut swipe);
    assert_eq!(
        readings[8..],
        [
            Reading::Swipe {
                pad: 5,
                x: 0.0,
                y: 0.5,
            },
            Reading::Button {
                pad: 5,
                button: PadButton::DpadDown,
                down: true,
            },
            Reading::Button {
                pad: 5,
                button: PadButton::DpadDown,
                down: false,
            },
        ]
    );
}
