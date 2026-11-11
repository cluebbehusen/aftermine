use super::*;
use crate::input::Pointer;
use aftermine_protocol::PadFamily;

const MOUSE_KEYS: Hands = Hands {
    pointer: Pointer::Mouse,
    keys: true,
    pad: None,
};
const MOUSE: Hands = Hands {
    keys: false,
    ..MOUSE_KEYS
};
const TOUCH_KEYS: Hands = Hands {
    pointer: Pointer::Touch,
    ..MOUSE_KEYS
};
const TOUCH: Hands = Hands {
    pointer: Pointer::Touch,
    ..MOUSE
};

fn pad(family: PadFamily) -> Hands {
    Hands {
        pad: Some(family),
        ..MOUSE_KEYS
    }
}

#[test]
fn a_key_span_shows_only_once_a_key_is_in_use_and_never_to_a_controller() {
    let template = Para::Same("[Press {pan} or drag|Drag] to look[, or zoom].");
    assert_eq!(
        template.text(MOUSE_KEYS),
        "Press {pan} or drag to look, or zoom."
    );
    assert_eq!(
        template.text(TOUCH_KEYS),
        "Press {pan} or drag to look, or zoom."
    );
    assert_eq!(template.text(MOUSE), "Drag to look.");
    assert_eq!(template.text(pad(PadFamily::Xbox)), "Drag to look.");
}

#[test]
fn verbs_follow_the_pointer_and_an_order_takes_the_controllers_order_button() {
    let template = Para::Same("{Click} it, then {right-click} the ground. {Right-click} it.");
    assert_eq!(
        template.text(MOUSE),
        "Click it, then right-click the ground. Right-click it."
    );
    assert_eq!(
        template.text(TOUCH),
        "Tap it, then long-press the ground. Long-press it."
    );
    assert_eq!(
        Para::Same("Then {right-click} the ground. {Right-click} it.").text(pad(PadFamily::Xbox)),
        "Then press X on the ground. Press X on it."
    );
    assert_eq!(
        Para::Same("Then {right-click} the ground.").text(pad(PadFamily::PlayStation)),
        "Then press Square on the ground."
    );
    // A controller has no click to name, so the token stays for the copy
    // tests to catch.
    assert_eq!(
        Para::Same("{Click} it.").text(pad(PadFamily::Xbox)),
        "{Click} it."
    );
}

#[test]
fn buttons_take_the_labels_of_the_family_in_use() {
    let template = Para::Same("{A} {B} {X} {Y} {LB} {LT} {Menu}");
    assert_eq!(template.text(pad(PadFamily::Xbox)), "A B X Y LB LT Menu");
    assert_eq!(
        template.text(pad(PadFamily::PlayStation)),
        "Cross Circle Square Triangle L1 L2 Options"
    );
    // Switch prints B at the bottom, where every other family's A sits.
    assert_eq!(template.text(pad(PadFamily::Switch)), "B A Y X L ZL Plus");
    assert_eq!(
        template.text(pad(PadFamily::SteamDeck)),
        "A B X Y L1 L2 Menu"
    );
}

#[test]
fn each_pair_of_hands_reads_its_own_template() {
    let pointer = Para::Pointer {
        pointer: "{Click} Build.",
        pad: "Press {Y}.",
    };
    assert_eq!(pointer.text(MOUSE), "Click Build.");
    assert_eq!(pointer.text(TOUCH), "Tap Build.");
    assert_eq!(pointer.text(pad(PadFamily::Xbox)), "Press Y.");
    let each = Para::Each {
        mouse: "[Shift-click to add.]",
        touch: "",
        pad: "Hold {LT} to add.",
    };
    assert_eq!(each.text(MOUSE_KEYS), "Shift-click to add.");
    assert_eq!(each.text(MOUSE), "", "a span alone empties its paragraph");
    assert_eq!(each.text(TOUCH_KEYS), "");
    assert_eq!(each.text(pad(PadFamily::Xbox)), "Hold LT to add.");
}
