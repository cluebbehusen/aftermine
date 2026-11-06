use super::*;

fn feed(adapter: &mut Adapter, readings: &[Reading]) -> Vec<RawEvent> {
    let mut events = Vec::new();
    for reading in readings {
        adapter.feed(*reading, &mut events);
    }
    events
}

fn connected(pad: u64) -> Adapter {
    let mut adapter = Adapter::default();
    feed(
        &mut adapter,
        &[Reading::Connected {
            pad,
            family: PadFamily::Xbox,
        }],
    );
    adapter
}

fn button(pad: u64, button: PadButton, down: bool) -> Reading {
    Reading::Button { pad, button, down }
}

fn axis(pad: u64, stick: Stick, axis: Axis, value: f32) -> Reading {
    Reading::Axis {
        pad,
        stick,
        axis,
        value,
    }
}

fn stick_events(events: &[RawEvent]) -> Vec<(f32, f32)> {
    events
        .iter()
        .filter_map(|event| match *event {
            RawEvent::PadStick { x, y, .. } => Some((x, y)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_pad_is_announced_once_and_unannounced_pads_are_ignored() {
    let mut adapter = Adapter::default();
    let connect = Reading::Connected {
        pad: 4,
        family: PadFamily::Switch,
    };
    assert_eq!(
        feed(&mut adapter, &[connect, connect]),
        [RawEvent::PadConnected {
            pad: 4,
            family: PadFamily::Switch,
        }]
    );
    assert!(
        feed(
            &mut adapter,
            &[
                button(9, PadButton::South, true),
                axis(9, Stick::Left, Axis::X, 1.0),
                Reading::Disconnected { pad: 9 },
            ]
        )
        .is_empty(),
        "a pad never announced reaches nothing"
    );
}

#[test]
fn a_button_sends_each_edge_once() {
    let mut adapter = connected(0);
    let events = feed(
        &mut adapter,
        &[
            button(0, PadButton::South, true),
            button(0, PadButton::South, true),
            button(0, PadButton::East, false),
            button(0, PadButton::South, false),
            button(0, PadButton::South, false),
        ],
    );
    assert_eq!(
        events,
        [
            RawEvent::PadDown {
                pad: 0,
                button: PadButton::South,
            },
            RawEvent::PadUp {
                pad: 0,
                button: PadButton::South,
            },
        ]
    );
}

#[test]
fn a_disconnect_releases_held_buttons_and_centers_sticks_first() {
    let mut adapter = connected(2);
    feed(
        &mut adapter,
        &[
            button(2, PadButton::LeftTrigger, true),
            button(2, PadButton::DpadUp, true),
            axis(2, Stick::Right, Axis::Y, 1.0),
        ],
    );
    let events = feed(&mut adapter, &[Reading::Disconnected { pad: 2 }]);
    assert_eq!(
        events,
        [
            RawEvent::PadUp {
                pad: 2,
                button: PadButton::LeftTrigger,
            },
            RawEvent::PadUp {
                pad: 2,
                button: PadButton::DpadUp,
            },
            RawEvent::PadStick {
                pad: 2,
                stick: Stick::Right,
                x: 0.0,
                y: 0.0,
            },
            RawEvent::PadDisconnected { pad: 2 },
        ]
    );
    assert!(
        feed(
            &mut adapter,
            &[
                Reading::Disconnected { pad: 2 },
                button(2, PadButton::South, true)
            ]
        )
        .is_empty(),
        "a gone pad sends nothing more"
    );
}

#[test]
fn a_resting_stick_inside_the_dead_zone_sends_nothing() {
    let mut adapter = connected(0);
    let drift = [
        axis(0, Stick::Left, Axis::X, 0.12),
        axis(0, Stick::Left, Axis::Y, -0.15),
        axis(0, Stick::Left, Axis::X, -0.1),
    ];
    assert!(feed(&mut adapter, &drift).is_empty());
}

#[test]
fn a_stick_sends_when_it_moves_a_step_and_lands_exactly_on_center_and_rim() {
    let mut adapter = connected(0);
    let leave = feed(&mut adapter, &[axis(0, Stick::Left, Axis::X, 0.3)]);
    let [(x, y)] = stick_events(&leave)[..] else {
        panic!("leaving the dead zone sends: {leave:?}");
    };
    assert!(
        x > 0.0 && x < 0.2 && y == 0.0,
        "travel past the zone is stretched: {x}"
    );

    let jitter = feed(&mut adapter, &[axis(0, Stick::Left, Axis::X, 0.305)]);
    assert!(
        jitter.is_empty(),
        "noise under a step is dropped: {jitter:?}"
    );

    let push = feed(&mut adapter, &[axis(0, Stick::Left, Axis::X, 0.6)]);
    assert_eq!(stick_events(&push).len(), 1, "a step's worth sends");

    let rim = feed(&mut adapter, &[axis(0, Stick::Left, Axis::X, 0.96)]);
    let [(x, y)] = stick_events(&rim)[..] else {
        panic!("reaching the rim sends: {rim:?}");
    };
    assert!(
        (x - 1.0).abs() < 1e-6 && y == 0.0,
        "past the outer zone is full: {x}"
    );

    let release = feed(&mut adapter, &[axis(0, Stick::Left, Axis::X, 0.1)]);
    assert_eq!(
        stick_events(&release),
        [(0.0, 0.0)],
        "rest is exactly centered"
    );
}

#[test]
fn the_dead_zone_is_round_and_diagonals_stay_inside_full_travel() {
    // A diagonal push reaches past 1 on a square-gated stick; it still
    // reads as full deflection, not faster.
    let full = dead_zoned(Vec2::new(1.0, 1.0));
    assert!((full.length() - 1.0).abs() < 1e-6, "{full:?}");
    assert!((full.x - full.y).abs() < 1e-6, "direction kept: {full:?}");
    // Each axis alone is inside the zone, but together they are out.
    assert_eq!(dead_zoned(Vec2::new(0.15, 0.0)), Vec2::ZERO);
    assert_ne!(dead_zoned(Vec2::new(0.15, 0.15)), Vec2::ZERO);
}

#[test]
fn a_garbled_axis_reads_as_centered() {
    let mut adapter = connected(0);
    feed(&mut adapter, &[axis(0, Stick::Left, Axis::X, 1.0)]);
    let events = feed(&mut adapter, &[axis(0, Stick::Left, Axis::X, f32::NAN)]);
    assert_eq!(stick_events(&events), [(0.0, 0.0)]);
}

#[test]
fn restating_every_control_each_frame_sends_what_reporting_changes_does() {
    // A polled platform API restates its whole pad each frame; an
    // event-driven one reports only changes. The funnel must not tell
    // them apart.
    let frames: [(bool, f32); 4] = [(false, 0.0), (true, 0.5), (true, 0.52), (false, 0.0)];
    let mut polled = connected(0);
    let mut changes = connected(0);
    let mut from_polled = Vec::new();
    let mut from_changes = Vec::new();
    let mut previous = (false, 0.0);
    for (down, x) in frames {
        from_polled.extend(feed(
            &mut polled,
            &[
                button(0, PadButton::South, down),
                axis(0, Stick::Left, Axis::X, x),
                axis(0, Stick::Left, Axis::Y, 0.0),
                axis(0, Stick::Right, Axis::X, 0.0),
                axis(0, Stick::Right, Axis::Y, 0.0),
            ],
        ));
        let mut reported = Vec::new();
        if down != previous.0 {
            reported.push(button(0, PadButton::South, down));
        }
        if x != previous.1 {
            reported.push(axis(0, Stick::Left, Axis::X, x));
        }
        from_changes.extend(feed(&mut changes, &reported));
        previous = (down, x);
    }
    assert_eq!(from_polled, from_changes);
    assert_eq!(stick_events(&from_polled).len(), 2, "{from_polled:?}");
}

#[test]
fn buttons_are_named_by_family_at_each_position() {
    use PadFamily as F;
    let families = [F::Xbox, F::PlayStation, F::Switch, F::Other];
    let buttons = [
        PadButton::South,
        PadButton::East,
        PadButton::West,
        PadButton::North,
        PadButton::DpadUp,
        PadButton::DpadDown,
        PadButton::DpadLeft,
        PadButton::DpadRight,
        PadButton::LeftBumper,
        PadButton::RightBumper,
        PadButton::LeftTrigger,
        PadButton::RightTrigger,
        PadButton::LeftStick,
        PadButton::RightStick,
        PadButton::Select,
        PadButton::Start,
    ];
    for family in families {
        let names: Vec<_> = buttons.iter().map(|b| button_name(family, *b)).collect();
        for name in &names {
            assert!(!name.is_empty() && name.is_ascii(), "{family:?} {name:?}");
        }
        let mut distinct = names.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), names.len(), "{family:?} names collide");
    }
    // The bottom face button confirms on every pad; a Switch prints B there.
    assert_eq!(button_name(F::Xbox, CONFIRM), "A");
    assert_eq!(button_name(F::Switch, CONFIRM), "B");
    assert_eq!(button_name(F::Switch, BACK), "A");
    assert_eq!(button_name(F::Other, CONFIRM), "A");
}

#[test]
fn the_dpad_moves_menus_and_two_face_buttons_confirm_and_go_back() {
    assert_eq!(menu_action(PadButton::DpadUp), Some(Action::MenuUp));
    assert_eq!(menu_action(PadButton::DpadRight), Some(Action::MenuRight));
    assert_eq!(menu_action(CONFIRM), Some(Action::Confirm));
    assert_eq!(menu_action(BACK), Some(Action::Back));
    assert_eq!(menu_action(PadButton::West), None);
    assert_eq!(menu_action(PadButton::Start), None);
}
