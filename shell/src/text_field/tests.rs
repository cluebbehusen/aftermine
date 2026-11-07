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
    let cancel = layout.cancel.expect("a form keeps Cancel by default");
    assert_eq!(feed(&mut form, &tap(cancel.center())).0, Edit::Cancel);
}

#[test]
fn a_form_without_cancel_spans_its_confirm_across_the_fields() {
    crate::render::set_viewport(1280.0, 800.0);
    let mut form = pair("Al", "12", 1).without_cancel();
    let layout = form.layout(vec2(1280.0, 800.0), crate::render::ui_scale());
    assert_eq!(layout.cancel, None);
    let (first, last) = (layout.fields[0], layout.fields[1]);
    assert_eq!(layout.confirm.x, first.x);
    assert!((layout.confirm.right() - last.right()).abs() < 0.01);
    assert!(layout.confirm.y > first.bottom(), "under the fields");

    let with_cancel = pair("Al", "12", 1).layout(vec2(1280.0, 800.0), crate::render::ui_scale());
    let old_cancel = with_cancel.cancel.expect("the paired form keeps Cancel");
    let left_edge = vec2(first.x + 4.0, old_cancel.center().y);
    assert_eq!(
        feed(&mut form, &tap(left_edge)).0,
        Edit::Commit,
        "where Cancel sat now confirms"
    );
    let mut form = pair("Al", "12", 1).without_cancel();
    assert_eq!(feed(&mut form, &key(Key::Escape)).0, Edit::Cancel);
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
    let shapes = [(&[1.0][..], false), (&[1.0, 2.0][..], true)];
    for ((weights, labeled), cancel) in shapes
        .into_iter()
        .flat_map(|shape| [true, false].map(|cancel| (shape, cancel)))
    {
        for view in [
            vec2(1133.0, 744.0),
            vec2(1180.0, 820.0),
            vec2(1194.0, 834.0),
            vec2(1366.0, 1024.0),
        ] {
            for s in [1.0, 1.25, 1.5] {
                let layout = layout(view, s, weights, labeled, cancel);
                for rect in layout
                    .fields
                    .iter()
                    .chain(&layout.cancel)
                    .chain([&layout.confirm])
                {
                    assert!(
                        rect.y + rect.h <= view.y * 0.45,
                        "{rect:?} reaches the keyboard at {view} ui {s}"
                    );
                }
            }
        }
        let small = vec2(640.0, 400.0);
        let layout = layout(small, 1.0, weights, labeled, cancel);
        let rects: Vec<Rect> = layout
            .fields
            .iter()
            .copied()
            .chain(layout.cancel)
            .chain([layout.confirm])
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
    let layout = layout(vec2(1280.0, 800.0), 1.0, &[1.0, 2.0], true, true);
    let (name, code) = (layout.fields[0], layout.fields[1]);
    assert!((code.w - 2.0 * name.w).abs() < 0.01);
    assert!(name.x + name.w < code.x);
    assert_eq!(name.y, code.y);
}

#[test]
fn the_controller_tab_and_page_keys_leave_a_field_alone() {
    // A pad's bumpers and triggers press Q, E, PageUp and PageDown as
    // keys, never as typed text, so a focused field keeps its value.
    let mut form = pair("ab", "12", 0);
    for button in [
        crate::pad::TABS.0,
        crate::pad::TABS.1,
        crate::pad::PAGES.0,
        crate::pad::PAGES.1,
    ] {
        let events = crate::pad::menu_keys(&[RawEvent::PadDown { pad: 0, button }]);
        assert_eq!(feed(&mut form, &events).0, Edit::Stay, "{button:?}");
    }
    assert_eq!((form.value(0), form.value(1)), ("ab", "12"));
}
