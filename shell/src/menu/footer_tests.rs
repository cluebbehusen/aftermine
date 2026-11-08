use super::*;

#[test]
fn the_footer_names_only_what_the_hands_in_use_have() {
    use crate::platform::{ALL_HANDS, assert_copy_fits};
    assert!(
        menu_footer(ALL_HANDS[0], true)
            .copy()
            .ends_with("confirm - or click")
    );
    for hands in ALL_HANDS {
        for scrolls in [false, true] {
            assert_copy_fits(hands, &menu_footer(hands, scrolls).copy());
        }
    }
}

#[test]
fn touch_rows_stay_a_full_fingertip_tall_and_desktop_rows_pack() {
    for s in [1.0, 1.25] {
        let long_list_short_window = (300.0 * s, 20);
        let touch = row_pitch(long_list_short_window.0, long_list_short_window.1, s, true);
        assert!(
            touch - ROW_GAP * s >= crate::theme::MIN_TOUCH_TARGET * s,
            "a touch row's target is at least a fingertip"
        );
        let desktop = row_pitch(long_list_short_window.0, long_list_short_window.1, s, false);
        assert_eq!(desktop, MIN_ROW * s, "desktop packs before it scrolls");
        assert_eq!(row_pitch(600.0 * s, 5, s, false), ITEM_HEIGHT * s);
        assert_eq!(row_pitch(600.0 * s, 5, s, true), touch, "one touch pitch");
    }
}

#[test]
fn a_keyless_footer_offers_a_scroll_only_when_the_list_scrolls() {
    let [_, mouse, _, touch] = crate::platform::ALL_HANDS;
    assert_eq!(menu_footer(touch, false).copy(), "tap to choose");
    assert!(menu_footer(touch, true).copy().ends_with("drag to scroll"));
    assert_eq!(menu_footer(mouse, false).copy(), "click to choose");
    assert!(menu_footer(mouse, true).copy().ends_with("scroll for more"));
}

#[test]
fn a_controller_in_use_shows_its_own_buttons_and_no_keys() {
    use crate::hint_line::HintLine;
    use crate::pad::PadIcon;
    use aftermine_protocol::PadFamily;
    let pad = |family| crate::platform::Hands {
        pad: Some(family),
        ..crate::platform::ALL_HANDS[0]
    };
    let footer = |confirm| {
        HintLine::default()
            .pad(PadIcon::DpadUpDown)
            .text(" select - ")
            .pad(confirm)
            .text(" confirm")
    };
    for (family, confirm) in [
        (PadFamily::Xbox, PadIcon::A),
        (PadFamily::PlayStation, PadIcon::Cross),
        (PadFamily::Switch, PadIcon::B),
        (PadFamily::SteamDeck, PadIcon::A),
        (PadFamily::Other, PadIcon::A),
    ] {
        assert_eq!(
            menu_footer(pad(family), true),
            footer(confirm),
            "{family:?}"
        );
        for hands in crate::platform::ALL_HANDS {
            let line = menu_footer(
                crate::platform::Hands {
                    pad: Some(family),
                    ..hands
                },
                true,
            );
            crate::platform::assert_touch_copy(&line.copy());
        }
    }
    assert_eq!(
        menu_footer(pad(PadFamily::PlayStation), false).copy(),
        "D-pad select - Cross confirm"
    );
}

#[test]
fn a_coaching_line_reads_as_its_filled_template_for_every_hand() {
    let templates = [
        "{confirm} saves | {back} cancels",
        "D-pad or {confirm} change a value | {prev_tab}/{next_tab} switch tabs",
        "D-pad select - {page_up}/{page_down} page - {confirm} plays - {back} back",
        "{confirm} watches | {delete} deletes | {prev_tab}/{next_tab} switch tabs",
        "{up}/{down} or click select",
        "no tokens at all",
    ];
    let hands = crate::platform::ALL_HANDS
        .into_iter()
        .chain(crate::platform::pad_hands());
    for hands in hands {
        crate::platform::set_hands(hands);
        for template in templates {
            assert_eq!(
                hint_line(template).copy(),
                binding_hint(template),
                "{hands:?} {template:?}"
            );
        }
    }
    crate::platform::set_hands(crate::platform::Hands::BUILD);
}

#[test]
fn a_controller_in_use_sees_its_own_icons_and_keys_only_where_it_has_no_button() {
    use crate::hint_line::HintLine;
    use crate::pad::PadIcon;
    use aftermine_protocol::PadFamily;
    crate::platform::set_hands(crate::platform::Hands {
        pad: Some(PadFamily::PlayStation),
        ..crate::platform::ALL_HANDS[0]
    });
    let line = hint_line(
        "{confirm} watches | {delete} deletes | {up} scrolls | {prev_tab}/{next_tab} tabs",
    );
    crate::platform::set_hands(crate::platform::Hands::BUILD);
    let up = MENU_BINDINGS.with(|bindings| bindings.borrow().label(crate::action::Action::MenuUp));
    assert_eq!(
        line,
        HintLine::default()
            .pad(PadIcon::Cross)
            .text(" watches | ")
            .pad(PadIcon::Square)
            .text(format!(" deletes | {up} scrolls | "))
            .pad(PadIcon::L1)
            .text("/")
            .pad(PadIcon::R1)
            .text(" tabs")
    );
}

fn remote_hands() -> crate::platform::Hands {
    crate::platform::Hands {
        pad: Some(aftermine_protocol::PadFamily::Remote),
        ..crate::platform::ALL_HANDS[3]
    }
}

#[test]
fn a_remote_footer_names_its_buttons_in_words() {
    crate::platform::set_hands(remote_hands());
    assert_eq!(
        menu_footer(remote_hands(), true).copy(),
        "Swipe to select - Click to confirm"
    );
    crate::platform::set_hands(crate::platform::Hands::BUILD);
}

#[test]
fn a_remote_template_fills_its_buttons_with_words() {
    crate::platform::set_hands(remote_hands());
    let template = "{confirm} to take it - {dpad} to move - {back} to leave";
    let expected = "Click to take it - Swipe to move - Back to leave";
    assert_eq!(binding_hint(template), expected);
    assert_eq!(hint_line(template).copy(), expected);
    assert_eq!(
        hint_line(crate::tabs::ON_STRIP_HINT).copy(),
        "Swipe to switch tabs - Back to leave"
    );
    crate::platform::set_hands(crate::platform::Hands::BUILD);
}

#[test]
fn other_controllers_still_read_d_pad_and_back_back() {
    use aftermine_protocol::PadFamily;
    let hands = crate::platform::Hands {
        pad: Some(PadFamily::Xbox),
        ..crate::platform::ALL_HANDS[3]
    };
    crate::platform::set_hands(hands);
    assert_eq!(
        binding_hint("{dpad} moves - {back} back"),
        "D-pad moves - B back"
    );
    crate::platform::set_hands(crate::platform::Hands::BUILD);
}
