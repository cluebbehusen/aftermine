use super::*;

#[test]
fn instruction_helpers_speak_touch_on_touch_only_builds() {
    assert_eq!(tap_or_click_capitalized(false), "Click");
    assert_touch_copy(tap_or_click_capitalized(true));
}

#[test]
fn a_tv_is_neither_a_desktop_nor_a_touch_device() {
    let facts = |platform: Platform| (platform.desktop(), platform.touch_only());
    assert_eq!(facts(Platform::Desktop), (true, false));
    assert_eq!(facts(Platform::Touch), (false, true));
    assert_eq!(facts(Platform::Tv), (false, false));
}

#[test]
fn only_a_desktop_starts_with_a_cursor_and_keys() {
    let desktop = Hands::at_launch(Platform::Desktop);
    assert_eq!(desktop.pointer, Pointer::Mouse);
    assert!(desktop.keys);
    for platform in [Platform::Touch, Platform::Tv] {
        let hands = Hands::at_launch(platform);
        assert_ne!(
            hands.pointer,
            Pointer::Mouse,
            "{platform:?} has nothing to hover"
        );
        assert!(
            !hands.keys,
            "{platform:?} names no keys before one is pressed"
        );
    }
}

#[test]
fn a_tv_names_its_remote_before_any_input_and_nothing_else_names_a_pad() {
    assert_eq!(Hands::at_launch(Platform::Desktop).pad, None);
    assert_eq!(Hands::at_launch(Platform::Touch).pad, None);
    assert_eq!(
        Hands::at_launch(Platform::Tv).pad,
        Some(PadFamily::Remote),
        "every Apple TV has a remote, and nothing on it taps"
    );
}

#[test]
fn a_controller_in_use_leaves_its_cursor_the_only_highlight() {
    use macroquad::prelude::{Rect, vec2};
    let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
    let inside = vec2(5.0, 5.0);
    for hands in ALL_HANDS {
        assert!(hands.hover_shown());
        set_hands(hands);
        assert!(hovers(rect, inside));
        assert!(!hovers(rect, vec2(20.0, 5.0)));
    }
    for hands in pad_hands() {
        assert!(!hands.hover_shown());
        set_hands(hands);
        assert!(!hovers(rect, inside), "a parked mouse lights nothing");
    }
    set_hands(Hands::BUILD);
}

#[test]
fn pad_mode_holds_while_a_controller_is_in_use_and_always_on_a_tv() {
    for hands in ALL_HANDS {
        assert!(!hands.pad_mode(Platform::Desktop));
        assert!(!hands.pad_mode(Platform::Touch));
        assert!(hands.pad_mode(Platform::Tv), "a TV has no pointer");
    }
    for hands in pad_hands() {
        for platform in [Platform::Desktop, Platform::Touch, Platform::Tv] {
            assert!(hands.pad_mode(platform), "{hands:?} on {platform:?}");
        }
    }
}
