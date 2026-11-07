use super::*;

fn left(x: f32, y: f32) -> RawEvent {
    RawEvent::PadStick {
        pad: 0,
        stick: Stick::Left,
        x,
        y,
    }
}

/// The arrows a frame pressed, in order.
fn pressed(events: &[RawEvent]) -> Vec<Key> {
    events
        .iter()
        .filter_map(|event| match *event {
            RawEvent::KeyDown { key } => Some(key),
            _ => None,
        })
        .collect()
}

#[test]
fn a_push_past_the_threshold_presses_the_dominant_arrow_once() {
    let mut stick = MenuStick::default();
    assert_eq!(pressed(&stick.arrows(&[left(0.0, 0.4)], 0.0)), []);
    let events = stick.arrows(&[left(0.2, 0.7)], 0.0);
    assert_eq!(
        events,
        [
            RawEvent::KeyDown { key: Key::Down },
            RawEvent::KeyUp { key: Key::Down }
        ],
        "the stick event itself is consumed"
    );
    assert_eq!(pressed(&stick.arrows(&[left(0.1, 0.9)], 0.0)), []);
    for (x, y, key) in [
        (0.0, -1.0, Key::Up),
        (-0.8, 0.3, Key::Left),
        (0.6, -0.2, Key::Right),
        (0.6, 0.6, Key::Down),
    ] {
        let mut stick = MenuStick::default();
        assert_eq!(pressed(&stick.arrows(&[left(x, y)], 0.0)), [key]);
    }
}

#[test]
fn a_held_arrow_lets_go_only_under_the_lower_threshold() {
    let mut stick = MenuStick::default();
    stick.arrows(&[left(0.0, 0.6)], 0.0);
    // Easing back past the press threshold still holds the arrow, so a
    // stick resting near it never presses twice.
    assert_eq!(pressed(&stick.arrows(&[left(0.0, 0.35)], 0.0)), []);
    assert_eq!(pressed(&stick.arrows(&[left(0.0, 0.6)], 0.0)), []);
    assert_eq!(pressed(&stick.arrows(&[left(0.0, 0.2)], 0.0)), []);
    assert_eq!(pressed(&stick.arrows(&[left(0.0, 0.6)], 0.0)), [Key::Down]);
}

#[test]
fn turning_the_stick_presses_the_new_arrow_at_once() {
    let mut stick = MenuStick::default();
    stick.arrows(&[left(0.0, 0.8)], 0.0);
    assert_eq!(pressed(&stick.arrows(&[left(0.8, 0.1)], 0.0)), [Key::Right]);
    assert_eq!(pressed(&stick.arrows(&[left(0.0, -0.8)], 0.0)), [Key::Up]);
}

#[test]
fn a_held_arrow_repeats_after_a_pause_then_steadily_one_press_a_frame() {
    let mut stick = MenuStick::default();
    stick.arrows(&[left(0.0, 1.0)], 0.0);
    assert_eq!(pressed(&stick.arrows(&[], 0.3)), []);
    assert_eq!(pressed(&stick.arrows(&[], 0.15)), [Key::Down]);
    assert_eq!(pressed(&stick.arrows(&[], 0.1)), []);
    assert_eq!(pressed(&stick.arrows(&[], 0.05)), [Key::Down]);
    assert_eq!(
        pressed(&stick.arrows(&[], 5.0)),
        [Key::Down],
        "a long frame still presses once"
    );
    stick.arrows(&[left(0.0, 0.0)], 0.0);
    assert_eq!(
        pressed(&stick.arrows(&[], 5.0)),
        [],
        "a centered stick is quiet"
    );
}

#[test]
fn reset_forgets_a_held_arrow_and_other_events_pass_through() {
    let mut stick = MenuStick::default();
    stick.arrows(&[left(-1.0, 0.0)], 0.0);
    stick.reset();
    assert_eq!(pressed(&stick.arrows(&[], 5.0)), []);
    let others = [
        RawEvent::PadStick {
            pad: 0,
            stick: Stick::Right,
            x: 0.0,
            y: 1.0,
        },
        RawEvent::KeyDown { key: Key::Enter },
    ];
    assert_eq!(stick.arrows(&others, 0.0), others);
}
