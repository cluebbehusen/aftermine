use super::*;
use macroquad::prelude::vec2;

fn feed(form: &mut Form, events: &[RawEvent]) -> (Edit, Vec<SoundKind>) {
    let mut sounds = Vec::new();
    let edit = form.update(events, &mut sounds);
    (edit, sounds.into_iter().map(|(kind, _)| kind).collect())
}

fn text(value: &str) -> Vec<RawEvent> {
    value.chars().map(|ch| RawEvent::Text { ch }).collect()
}

fn key(key: Key) -> [RawEvent; 1] {
    [RawEvent::KeyDown { key }]
}

fn tap(at: Vec2) -> [RawEvent; 2] {
    [
        RawEvent::TouchDown {
            id: 1,
            x: at.x,
            y: at.y,
        },
        RawEvent::TouchUp {
            id: 1,
            x: at.x,
            y: at.y,
        },
    ]
}

fn single(value: &str, max: usize) -> Form {
    Form::new(
        "SAVE GAME",
        "SAVE",
        vec![Field::new(value, max, |ch| ch.is_ascii())],
        0,
    )
}

fn digits_only(value: &str) -> Result<(), &'static str> {
    if value.is_empty() || !value.chars().all(|ch| ch.is_ascii_digit()) {
        Err("digits please")
    } else {
        Ok(())
    }
}

/// A name and a digits-only code, the way the Join form pairs fields.
fn pair(name: &str, code: &str, focus: usize) -> Form {
    Form::new(
        "JOIN MATCH",
        "JOIN",
        vec![
            Field::new(name, 8, |ch| ch.is_ascii_alphabetic()).labeled("Name", "you"),
            Field::new(code, 6, |ch| ch.is_ascii_digit() || ch == 'x')
                .labeled("Code", "123")
                .validated(digits_only)
                .keyboard(Keyboard::Address)
                .weight(2.0),
        ],
        focus,
    )
}

#[test]
fn a_field_filters_caps_and_commits_a_trimmed_value() {
    let mut form = single("ab\u{e9}c", 5);
    assert_eq!(
        form.menu(|_| ()).view().items,
        vec!["abc_"],
        "the prefill is filtered"
    );
    feed(&mut form, &text("def"));
    assert_eq!(
        form.menu(|_| ()).view().items,
        vec!["abcde_"],
        "the cap refuses more"
    );
    feed(&mut form, &key(Key::Backspace));
    feed(&mut form, &text(" "));
    assert_eq!(feed(&mut form, &key(Key::Enter)).0, Edit::Commit);
    assert_eq!(form.value(0), "abcd");
}

#[test]
fn a_blank_field_refuses_to_commit_and_escape_cancels() {
    crate::render::set_viewport(1280.0, 800.0);
    let mut form = single("  ", 10);
    let (edit, sounds) = feed(&mut form, &key(Key::Enter));
    assert_eq!(edit, Edit::Stay);
    assert_eq!(sounds, vec![SoundKind::Denied]);
    assert_eq!(form.error, Some("type something first"));
    feed(&mut form, &text("a"));
    assert_eq!(form.error, None, "an edit clears the refusal");
    assert_eq!(feed(&mut form, &key(Key::Escape)).0, Edit::Cancel);
    let mut form = single("x", 10);
    let layout = form.layout(vec2(1280.0, 800.0), crate::render::ui_scale());
    assert_eq!(
        feed(&mut form, &tap(layout.cancel.center())).0,
        Edit::Cancel
    );
}

#[test]
fn typing_lands_in_the_focused_field_and_focus_moves_by_key_and_tap() {
    crate::render::set_viewport(1280.0, 800.0);
    let mut form = pair("", "", 0);
    feed(&mut form, &text("Al1"));
    assert_eq!(
        form.menu(|i| i).view().items,
        vec!["Name: Al_", "Code: "],
        "each field takes only its own characters"
    );
    assert_eq!(form.keyboard(), Keyboard::Text);
    feed(&mut form, &key(Key::Tab));
    feed(&mut form, &text("42"));
    assert_eq!(form.menu(|i| i).view().items, vec!["Name: Al", "Code: 42_"]);
    assert_eq!(form.menu(|i| i).selected, 1);
    assert_eq!(form.keyboard(), Keyboard::Address);
    feed(&mut form, &key(Key::Down));
    assert_eq!(form.focus, 0, "focus wraps forward");
    feed(&mut form, &key(Key::Up));
    assert_eq!(form.focus, 1, "and back");

    let layout = form.layout(vec2(1280.0, 800.0), crate::render::ui_scale());
    feed(&mut form, &tap(layout.fields[0].center()));
    assert_eq!(form.focus, 0);
    assert!(form.take_keyboard_request(), "a tap raises the keyboard");
    assert!(!form.take_keyboard_request(), "once");
}

#[test]
fn enter_advances_then_commits_and_a_bad_field_takes_focus_with_its_reason() {
    crate::render::set_viewport(1280.0, 800.0);
    let mut form = pair("", "12x", 0);
    feed(&mut form, &text("Al"));
    assert_eq!(feed(&mut form, &key(Key::Enter)).0, Edit::Stay);
    assert_eq!(form.focus, 1, "Enter moves on from the name");
    let (edit, sounds) = feed(&mut form, &key(Key::Enter));
    assert_eq!(edit, Edit::Stay);
    assert_eq!(sounds, vec![SoundKind::Denied]);
    assert_eq!(form.error, Some("digits please"));

    let mut form = pair("", "12", 1);
    let layout = form.layout(vec2(1280.0, 800.0), crate::render::ui_scale());
    feed(&mut form, &tap(layout.confirm.center()));
    assert_eq!(form.focus, 0, "the blank name takes focus");
    assert_eq!(form.error, Some("type something first"));
    feed(&mut form, &text("Al"));
    assert_eq!(
        feed(&mut form, &tap(layout.confirm.center())).0,
        Edit::Commit
    );
    assert_eq!((form.value(0), form.value(1)), ("Al", "12"));
}

#[test]
fn the_face_stays_above_an_ipad_keyboard() {
    for (weights, labeled) in [(&[1.0][..], false), (&[1.0, 2.0][..], true)] {
        for view in [
            vec2(1133.0, 744.0),
            vec2(1180.0, 820.0),
            vec2(1194.0, 834.0),
            vec2(1366.0, 1024.0),
        ] {
            for s in [1.0, 1.25, 1.5] {
                let layout = layout(view, s, weights, labeled);
                for rect in layout
                    .fields
                    .iter()
                    .chain([&layout.cancel, &layout.confirm])
                {
                    assert!(
                        rect.y + rect.h <= view.y * 0.45,
                        "{rect:?} reaches the keyboard at {view} ui {s}"
                    );
                }
            }
        }
        let small = vec2(640.0, 400.0);
        let layout = layout(small, 1.0, weights, labeled);
        let rects: Vec<Rect> = layout
            .fields
            .iter()
            .copied()
            .chain([layout.cancel, layout.confirm])
            .collect();
        for (index, rect) in rects.iter().enumerate() {
            assert!(rect.x >= 0.0 && rect.x + rect.w <= small.x);
            assert!(rect.y >= layout.hint_y && rect.y + rect.h <= small.y);
            for other in &rects[index + 1..] {
                assert!(!rect.overlaps(other), "{rect:?} overlaps {other:?}");
            }
        }
        if labeled {
            assert!(layout.label_y < layout.fields[0].y);
        }
    }
}

#[test]
fn a_heavier_field_gets_the_wider_share_of_the_row() {
    let layout = layout(vec2(1280.0, 800.0), 1.0, &[1.0, 2.0], true);
    let (name, code) = (layout.fields[0], layout.fields[1]);
    assert!((code.w - 2.0 * name.w).abs() < 0.01);
    assert!(name.x + name.w < code.x);
    assert_eq!(name.y, code.y);
}
