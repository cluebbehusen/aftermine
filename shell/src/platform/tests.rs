use super::*;

/// Fails when copy for a player without a keyboard names a key or an
/// unexpanded key token.
pub(crate) fn assert_keyless_copy(text: &str) {
    let lower = text.to_ascii_lowercase().replace("long-press", "");
    for word in ["press", "enter", "esc", "shift", "ctrl", "{", "["] {
        assert!(
            !lower.contains(word),
            "keyless copy {text:?} mentions {word:?}"
        );
    }
    assert!(text.is_ascii(), "copy {text:?} is not ASCII");
}

/// Fails when copy for a finger names a mouse action.
pub(crate) fn assert_mouseless_copy(text: &str) {
    let lower = text.to_ascii_lowercase();
    for word in ["click", "hover", "wheel", "middle drag"] {
        assert!(
            !lower.contains(word),
            "touch copy {text:?} mentions {word:?}"
        );
    }
    assert!(text.is_ascii(), "copy {text:?} is not ASCII");
}

/// Fails when touch-facing copy names a key, a mouse action, or an
/// unexpanded key token.
pub(crate) fn assert_touch_copy(text: &str) {
    assert_keyless_copy(text);
    assert_mouseless_copy(text);
}

/// Every pairing of pointer and keyboard, without a controller.
pub(crate) const ALL_HANDS: [Hands; 4] = [
    Hands {
        pointer: Pointer::Mouse,
        keys: true,
        pad: None,
    },
    Hands {
        pointer: Pointer::Mouse,
        keys: false,
        pad: None,
    },
    Hands {
        pointer: Pointer::Touch,
        keys: true,
        pad: None,
    },
    Hands {
        pointer: Pointer::Touch,
        keys: false,
        pad: None,
    },
];

/// Fails when copy names what `hands` lack: a key without a keyboard, or
/// a mouse action under a finger.
pub(crate) fn assert_copy_fits(hands: Hands, text: &str) {
    if !hands.keys {
        assert_keyless_copy(text);
    }
    if hands.touch() {
        assert_mouseless_copy(text);
    }
    assert!(text.is_ascii(), "copy {text:?} is not ASCII");
}

/// Every pointer and keyboard pairing with a controller of each family in
/// use.
pub(crate) fn pad_hands() -> impl Iterator<Item = Hands> {
    [
        PadFamily::Xbox,
        PadFamily::PlayStation,
        PadFamily::Switch,
        PadFamily::SteamDeck,
        PadFamily::Other,
        PadFamily::Remote,
    ]
    .into_iter()
    .flat_map(|family| {
        ALL_HANDS.map(|hands| Hands {
            pad: Some(family),
            ..hands
        })
    })
}

/// Every pointer and keyboard pairing as the HUD's cards word their copy in
/// pad mode: for each family the HUD can name, and for none.
pub(crate) fn card_pad_hands() -> impl Iterator<Item = Hands> {
    [
        Some(PadFamily::Xbox),
        Some(PadFamily::PlayStation),
        Some(PadFamily::Switch),
        Some(PadFamily::SteamDeck),
        Some(PadFamily::Other),
        None,
    ]
    .into_iter()
    .flat_map(|names| {
        ALL_HANDS.map(|hands| crate::input::HudHands::Pad { names }.copy_hands(hands))
    })
}

/// Fails when copy for a controller names a mouse action, a finger's
/// gesture or a key, or leaves a token or a key span unfilled.
pub(crate) fn assert_pad_wording(text: &str) {
    let lower = text.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphabetic() && c != '-')
        .collect();
    for word in [
        "click",
        "right-click",
        "double-click",
        "shift-click",
        "hover",
        "wheel",
        "tap",
        "double-tap",
        "finger",
        "fingers",
        "long-press",
        "pinch",
        "drag",
        "shift",
        "ctrl",
        "enter",
        "esc",
    ] {
        assert!(
            !words.contains(&word),
            "controller copy {text:?} says {word:?}"
        );
    }
    for mark in ['{', '}', '[', ']', '|'] {
        assert!(
            !text.contains(mark),
            "controller copy {text:?} leaves {mark:?} unfilled"
        );
    }
    assert!(text.is_ascii(), "copy {text:?} is not ASCII");
}

/// Fills `template` as a player with `hands` sees it and fails unless it
/// names the controller's confirm or back button and nothing a controller
/// player may lack: no key, no mouse action, no unfilled token.
pub(crate) fn assert_pad_copy(hands: Hands, template: &str) -> String {
    let family = hands.pad.expect("pad copy is for a controller in use");
    set_hands(hands);
    let text = crate::menu::binding_hint(template);
    set_hands(Hands::BUILD);
    // The remote's click is a button's name, not a mouse action.
    assert_touch_copy(&text.replace(
        crate::pad::button_name(PadFamily::Remote, crate::pad::CONFIRM),
        "",
    ));
    let names = [crate::pad::CONFIRM, crate::pad::BACK]
        .map(|button| crate::pad::button_name(family, button));
    assert!(
        names.iter().any(|name| text.contains(name)),
        "pad copy {text:?} names neither {names:?}"
    );
    text
}

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
