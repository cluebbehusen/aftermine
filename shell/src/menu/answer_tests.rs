use super::{Answer, Label, Menu};
use crate::game::SoundKind;
use aftermine_protocol::{Key, RawEvent};
use macroquad::prelude::vec2;

fn question() -> Menu<bool> {
    Menu::rows(
        "DELETE THIS SAVE?",
        [
            (Label::from("Cancel"), false),
            (Label::from("Delete"), true),
        ],
    )
}

#[test]
fn a_dismissal_outranks_a_row_picked_in_the_same_frame_and_both_click() {
    let enter = [RawEvent::KeyDown { key: Key::Enter }];
    let (mut mouse, mut sounds) = (vec2(0.0, 0.0), Vec::new());
    let mut menu = question();
    menu.select(1);
    assert_eq!(
        menu.answer(&enter, &mut mouse, true, &mut sounds),
        Some(Answer::Dismissed)
    );
    assert_eq!(
        menu.answer(&enter, &mut mouse, false, &mut sounds),
        Some(Answer::Picked(true))
    );
    assert_eq!(menu.answer(&[], &mut mouse, false, &mut sounds), None);
    assert_eq!(sounds, [(SoundKind::Click, None), (SoundKind::Click, None)]);
}
