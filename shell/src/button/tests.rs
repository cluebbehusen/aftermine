use super::*;
use crate::tests::support::{click, tap, touch_down, touch_up};
use macroquad::prelude::vec2;

/// A click (or, with `touch`, a tap) on the BACK button.
pub(crate) fn press_back(touch: bool) -> [RawEvent; 2] {
    let p = corner_slot(0, crate::render::ui_scale()).center();
    if touch { tap(9, p) } else { click(p) }
}

#[test]
fn the_back_button_claims_its_presses_and_passes_the_rest() {
    let mut back = BackButton::default();
    for touch in [false, true] {
        let (pressed, rest) = back.route(&press_back(touch));
        assert!(pressed);
        assert!(rest.is_empty(), "the screen never sees the press");
    }
    // Pressed on BACK, released elsewhere: nothing, and the release
    // stays the button's.
    let p = corner_slot(0, crate::render::ui_scale()).center();
    let (pressed, rest) = back.route(&[touch_down(1, p), touch_up(1, vec2(600.0, 400.0))]);
    assert!(!pressed);
    assert!(rest.is_empty());
    let elsewhere = [touch_down(2, vec2(600.0, 400.0))];
    assert_eq!(back.route(&elsewhere), (false, elsewhere.to_vec()));
}

#[test]
fn the_back_button_takes_no_press_in_pad_mode() {
    let mut back = BackButton::default();
    let p = corner_slot(0, crate::render::ui_scale()).center();
    let down = touch_down(1, p);
    let up = touch_up(1, p);
    assert_eq!(back.route(&[down]), (false, Vec::new()), "armed");
    for hands in crate::platform::tests::pad_hands() {
        crate::platform::set_hands(hands);
        for touch in [false, true] {
            let press = press_back(touch);
            assert_eq!(back.route(&press), (false, press.to_vec()), "{hands:?}");
        }
    }
    crate::platform::set_hands(crate::platform::Hands::BUILD);
    assert_eq!(
        back.route(&[up]),
        (false, vec![up]),
        "pad mode disarmed the press made before it"
    );
}

#[test]
fn corner_slots_are_fingertip_sized_and_never_overlap() {
    for s in [0.75, 1.0, 1.5] {
        let first = corner_slot(0, s);
        let second = corner_slot(1, s);
        assert!(first.h >= crate::theme::MIN_TOUCH_TARGET * s);
        assert!(first.x > 0.0 && first.y > 0.0);
        assert!(first.x + first.w < second.x, "slots keep a gap at {s}");
    }
}
