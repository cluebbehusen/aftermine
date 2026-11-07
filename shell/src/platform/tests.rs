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
        assert_eq!(hands.pad, None);
    }
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
