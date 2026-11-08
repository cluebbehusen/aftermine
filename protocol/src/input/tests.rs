use super::*;
use crate::tests::assert_every_tag_sampled;

fn roundtrip(event: RawEvent) -> RawEvent {
    let json = serde_json::to_string(&event).unwrap();
    serde_json::from_str(&json).unwrap()
}

/// Contiguous index per [`RawEvent`] variant, in declaration order.
fn event_tag(event: &RawEvent) -> usize {
    match event {
        RawEvent::MouseMove { .. } => 0,
        RawEvent::MouseDown { .. } => 1,
        RawEvent::MouseUp { .. } => 2,
        RawEvent::Wheel { .. } => 3,
        RawEvent::KeyDown { .. } => 4,
        RawEvent::KeyUp { .. } => 5,
        RawEvent::TouchDown { .. } => 6,
        RawEvent::TouchMove { .. } => 7,
        RawEvent::TouchUp { .. } => 8,
        RawEvent::TouchCancel { .. } => 9,
        RawEvent::Text { .. } => 10,
        RawEvent::PadConnected { .. } => 11,
        RawEvent::PadDisconnected { .. } => 12,
        RawEvent::PadDown { .. } => 13,
        RawEvent::PadUp { .. } => 14,
        RawEvent::PadStick { .. } => 15,
        RawEvent::PadSwipe { .. } => 16,
    }
}

const EVENT_VARIANTS: usize = 17;

#[test]
fn every_raw_event_variant_including_touch_survives_a_roundtrip() {
    // Pins the wire contract for every variant, including the touch events.
    let events = [
        RawEvent::MouseMove { x: 1.5, y: 2.5 },
        RawEvent::MouseDown {
            button: MouseButton::Left,
            x: 3.0,
            y: 4.0,
        },
        RawEvent::MouseUp {
            button: MouseButton::Right,
            x: 5.0,
            y: 6.0,
        },
        RawEvent::Wheel { delta: -2.0 },
        RawEvent::KeyDown { key: Key::Escape },
        RawEvent::KeyUp { key: Key::Shift },
        RawEvent::TouchDown {
            id: 7,
            x: 8.0,
            y: 9.0,
        },
        RawEvent::TouchMove {
            id: 7,
            x: 10.0,
            y: 11.0,
        },
        RawEvent::TouchUp {
            id: 7,
            x: 12.0,
            y: 13.0,
        },
        RawEvent::TouchCancel { id: 7 },
        RawEvent::Text { ch: 'k' },
        RawEvent::PadConnected {
            pad: 3,
            family: PadFamily::PlayStation,
        },
        RawEvent::PadDisconnected { pad: 3 },
        RawEvent::PadDown {
            pad: 3,
            button: PadButton::South,
        },
        RawEvent::PadUp {
            pad: 3,
            button: PadButton::DpadLeft,
        },
        RawEvent::PadStick {
            pad: 3,
            stick: Stick::Right,
            x: 0.25,
            y: -1.0,
        },
        RawEvent::PadSwipe {
            pad: 2,
            x: 0.25,
            y: -0.5,
        },
    ];
    assert_every_tag_sampled(events.iter().map(event_tag), EVENT_VARIANTS, "raw event");
    for event in events {
        assert_eq!(
            roundtrip(event),
            event,
            "raw event did not survive: {event:?}"
        );
    }
}

#[test]
fn the_middle_mouse_button_survives_a_roundtrip() {
    let event = RawEvent::MouseDown {
        button: MouseButton::Middle,
        x: 0.0,
        y: 0.0,
    };
    assert_eq!(roundtrip(event), event);
}

#[test]
fn pad_events_speak_the_documented_wire_names() {
    // Scripts and the driver write these by hand; renaming one breaks them.
    let cases = [
        (
            r#"{"type":"pad_connected","pad":0,"family":"playstation"}"#,
            RawEvent::PadConnected {
                pad: 0,
                family: PadFamily::PlayStation,
            },
        ),
        (
            r#"{"type":"pad_connected","pad":3,"family":"steamdeck"}"#,
            RawEvent::PadConnected {
                pad: 3,
                family: PadFamily::SteamDeck,
            },
        ),
        (
            r#"{"type":"pad_connected","pad":4,"family":"remote"}"#,
            RawEvent::PadConnected {
                pad: 4,
                family: PadFamily::Remote,
            },
        ),
        (
            r#"{"type":"pad_down","pad":4,"button":"play_pause"}"#,
            RawEvent::PadDown {
                pad: 4,
                button: PadButton::PlayPause,
            },
        ),
        (
            r#"{"type":"pad_swipe","pad":4,"x":0.25,"y":0.5}"#,
            RawEvent::PadSwipe {
                pad: 4,
                x: 0.25,
                y: 0.5,
            },
        ),
        (
            r#"{"type":"pad_down","pad":1,"button":"dpad_up"}"#,
            RawEvent::PadDown {
                pad: 1,
                button: PadButton::DpadUp,
            },
        ),
        (
            r#"{"type":"pad_down","pad":1,"button":"left_shoulder"}"#,
            RawEvent::PadDown {
                pad: 1,
                button: PadButton::LeftShoulder,
            },
        ),
        (
            r#"{"type":"pad_up","pad":1,"button":"right_trigger"}"#,
            RawEvent::PadUp {
                pad: 1,
                button: PadButton::RightTrigger,
            },
        ),
        (
            r#"{"type":"pad_stick","pad":2,"stick":"left","x":0.5,"y":-0.5}"#,
            RawEvent::PadStick {
                pad: 2,
                stick: Stick::Left,
                x: 0.5,
                y: -0.5,
            },
        ),
        (
            r#"{"type":"pad_disconnected","pad":2}"#,
            RawEvent::PadDisconnected { pad: 2 },
        ),
    ];
    for (json, event) in cases {
        assert_eq!(serde_json::from_str::<RawEvent>(json).unwrap(), event);
        assert_eq!(serde_json::to_string(&event).unwrap(), json);
    }
    assert!(
        serde_json::from_str::<RawEvent>(r#"{"type":"pad_down","pad":1,"button":"a"}"#).is_err(),
        "buttons are named by position, never by label"
    );
    assert!(
        serde_json::from_str::<RawEvent>(
            r#"{"type":"pad_down","pad":1,"button":"south","player":0}"#
        )
        .is_err(),
        "an unknown field is refused"
    );
}
