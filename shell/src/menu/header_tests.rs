use super::*;
use aftermine_protocol::{Key, RawEvent};
use macroquad::prelude::vec2;

/// Rows `[H] a b [H] c d`, each row standing for its own index.
fn sectioned() -> Menu<usize> {
    let row = |label: &str, index| Line::Row(Label::from(label), index);
    Menu::new(
        "T",
        vec![
            Line::Header("- one -".to_string()),
            row("a", 1),
            row("b", 2),
            Line::Header("- two -".to_string()),
            row("c", 4),
            row("d", 5),
        ],
    )
}

fn press(menu: &mut Menu<usize>, key: Key) -> Option<usize> {
    let mut mouse = vec2(0.0, 0.0);
    menu.handle(&[RawEvent::KeyDown { key }], &mut mouse)
        .map(|picked| picked.value)
}

#[test]
fn the_cursor_never_rests_on_a_header() {
    let mut menu = sectioned();
    assert_eq!(menu.selected, 1, "construction snaps off the header");
    press(&mut menu, Key::Down);
    assert_eq!(menu.selected, 2);
    press(&mut menu, Key::Down);
    assert_eq!(menu.selected, 4, "down hops the section label");
    press(&mut menu, Key::Up);
    assert_eq!(menu.selected, 2, "up hops it too");
    press(&mut menu, Key::Home);
    assert_eq!(menu.selected, 1, "Home lands on the first real row");
    press(&mut menu, Key::End);
    assert_eq!(menu.selected, 5, "End on the last real row");
}

#[test]
fn a_header_never_activates() {
    let mut menu = sectioned();
    menu.select(0);
    assert_eq!(menu.selected, 1, "select snaps forward off the header");
    // Enter activates the snapped row, never the header.
    assert_eq!(press(&mut menu, Key::Enter), Some(1));
}

#[test]
fn an_all_header_menu_refuses_keyboard_activation() {
    let mut menu: Menu<usize> = Menu::new(
        "HEADERS",
        vec![
            Line::Header("one".to_string()),
            Line::Header("two".to_string()),
        ],
    );

    assert!(menu.is_header(menu.selected));
    assert_eq!(press(&mut menu, Key::Enter), None);
}

#[test]
fn wheel_scroll_cannot_pin_the_cursor_onto_a_header() {
    // A short window forces the riding clamp; the ride must snap off
    // headers or Enter activates a section label.
    let mut menu = sectioned();
    crate::render::set_viewport(1280.0, 400.0);
    menu.select(5);
    let mut mouse = vec2(0.0, 0.0);
    for _ in 0..6 {
        menu.handle(&[RawEvent::Wheel { delta: 1.0 }], &mut mouse);
        assert!(
            !menu.is_header(menu.selected),
            "the riding cursor rests on header row {}",
            menu.selected
        );
    }
}

#[test]
fn mouse_activation_requires_a_release_on_the_armed_row() {
    crate::render::set_viewport(1280.0, 800.0);
    let mut menu = sectioned();
    let first = menu.item_rect(1).expect("first item").center();
    let second = menu.item_rect(2).expect("second item").center();
    let mut mouse = vec2(0.0, 0.0);

    assert_eq!(
        menu.handle(
            &[RawEvent::MouseMove {
                x: second.x,
                y: second.y,
            }],
            &mut mouse,
        ),
        None
    );
    assert_eq!(menu.view().hover, Some(2));
    assert_eq!(menu.selected, 1, "hover is not keyboard selection");

    assert_eq!(
        menu.handle(
            &[
                RawEvent::MouseDown {
                    button: aftermine_protocol::MouseButton::Left,
                    x: first.x,
                    y: first.y,
                },
                RawEvent::MouseUp {
                    button: aftermine_protocol::MouseButton::Left,
                    x: second.x,
                    y: second.y,
                },
            ],
            &mut mouse,
        ),
        None,
        "dragging to another row cancels the press"
    );
    assert_eq!(
        menu.handle(
            &[
                RawEvent::MouseDown {
                    button: aftermine_protocol::MouseButton::Left,
                    x: second.x,
                    y: second.y,
                },
                RawEvent::MouseUp {
                    button: aftermine_protocol::MouseButton::Left,
                    x: second.x,
                    y: second.y,
                },
            ],
            &mut mouse,
        ),
        Some(Activation {
            value: 2,
            column: None
        })
    );
    assert_eq!(menu.selected, 2);
}

#[test]
fn paging_keeps_the_keyboard_target_visible_and_off_section_headers() {
    crate::render::set_viewport(1280.0, 400.0);
    let mut menu = sectioned();
    let mut mouse = vec2(0.0, 0.0);
    for key in [Key::PageDown, Key::PageDown, Key::PageUp, Key::PageUp] {
        menu.handle(&[RawEvent::KeyDown { key }], &mut mouse);
        let [first, end] = menu.visible_range();
        assert!((first..end).contains(&menu.selected));
        assert!(!menu.is_header(menu.selected));
    }
}

#[test]
fn fractional_trackpad_motion_accumulates_before_scrolling_a_row() {
    crate::render::set_viewport(1280.0, 400.0);
    let mut menu = sectioned();
    menu.heading = Heading::Wordmark;
    let mut mouse = vec2(0.0, 0.0);
    let start = menu.visible_range();
    for _ in 0..2 {
        menu.handle(&[RawEvent::Wheel { delta: -0.4 }], &mut mouse);
        assert_eq!(menu.visible_range(), start);
    }
    menu.handle(&[RawEvent::Wheel { delta: -0.4 }], &mut mouse);
    assert_ne!(
        menu.visible_range(),
        start,
        "three partial notches cross one row"
    );
}

fn controls() -> Menu<usize> {
    Menu::new(
        "KEYS",
        vec![
            Line::Header("Camera".to_string()),
            Line::Row(
                Label::Binding {
                    name: "Pan up".to_string(),
                    keys: ["W".to_string(), "I".to_string()],
                    marked: Some(1),
                },
                1,
            ),
        ],
    )
}

#[test]
fn a_binding_row_reads_as_one_line_with_its_marked_chord() {
    assert_eq!(controls().view().items, vec!["Camera", "Pan up: W | [I]"]);
}

#[test]
fn binding_rows_widen_the_list_and_report_the_clicked_chord() {
    crate::render::set_viewport(1280.0, 800.0);
    let mut menu = controls();
    let plain = Menu::list("PLAIN", vec!["one".to_string(), "two".to_string()]);
    let row = menu.item_rect(1).expect("the binding row shows");
    assert!(row.w > plain.item_rect(1).expect("a plain row").w);
    let columns = BindingColumns::of(row, crate::render::ui_scale());
    let click = |x: f32| {
        let button = aftermine_protocol::MouseButton::Left;
        let y = row.center().y;
        [
            RawEvent::MouseDown { button, x, y },
            RawEvent::MouseUp { button, x, y },
        ]
    };
    let mut mouse = vec2(0.0, 0.0);
    let picked = menu.handle(&click(columns.secondary.center().x), &mut mouse);
    assert_eq!(
        picked,
        Some(Activation {
            value: 1,
            column: Some(1)
        })
    );
    let picked = menu.handle(&click(columns.name.center().x), &mut mouse);
    assert_eq!(picked.and_then(|picked| picked.column), None);
    let picked = menu.handle(&[RawEvent::KeyDown { key: Key::Enter }], &mut mouse);
    assert_eq!(picked.and_then(|picked| picked.column), None);
}

fn record(title: &str, map: Option<&str>, tag: Option<&'static str>, format: &str) -> Label {
    Label::Record {
        title: title.to_string(),
        map: map.map(str::to_string),
        tag,
        format: format.to_string(),
        clock: "28:58".to_string(),
        when: "12 min ago".to_string(),
        available: true,
    }
}

#[test]
fn record_rows_widen_the_list_and_read_their_map_and_chip_in_order() {
    crate::render::set_viewport(1280.0, 800.0);
    let menu = Menu::rows(
        "SHELF",
        [
            (record("Long Causeway", None, Some("AUTO"), "1v1"), 0),
            (
                record("before the push", Some("Bridge Town"), Some("LAN"), "2v2"),
                1,
            ),
            (record("Long Neck", None, None, "1v1"), 2),
        ],
    );
    assert_eq!(
        menu.view().items,
        vec![
            "Long Causeway [AUTO] | 1v1 | 28:58 | 12 min ago",
            "before the push \u{b7} Bridge Town [LAN] | 2v2 | 28:58 | 12 min ago",
            "Long Neck | 1v1 | 28:58 | 12 min ago",
        ]
    );
    let plain = Menu::list("PLAIN", vec!["one".to_string()]);
    let row = menu.item_rect(0).expect("the record row shows");
    assert!(row.w > plain.item_rect(0).expect("a plain row").w);
}

#[test]
fn record_columns_take_their_widest_value_and_leave_the_title_the_rest() {
    let lines = vec![
        Line::Row(record("a", None, None, "1v1"), 0),
        Line::Row(record("b", None, None, "4-way FFA"), 1),
    ];
    let widest = widest_record_values(&lines, |text| text.chars().count() as f32 * 10.0);
    assert_eq!(widest, [90.0, 50.0, 100.0], "the widest of every record");

    let row = Rect::new(100.0, 0.0, 760.0, 44.0);
    let columns = RecordColumns::fit(row, 1.0, widest);
    let spans = [columns.title, columns.format, columns.clock, columns.when];
    assert!(
        spans.windows(2).all(|pair| pair[0].right() <= pair[1].x),
        "columns never overlap: {spans:?}"
    );
    assert!(row.x < columns.title.x && columns.when.right() < row.right());
    assert_eq!(
        [columns.format.w, columns.clock.w, columns.when.w],
        widest,
        "a column that fits holds its widest value whole"
    );
    assert!(columns.title.w > row.w * RecordColumns::TITLE_SHARE);

    let squeezed = RecordColumns::fit(row, 1.0, [400.0, 200.0, 300.0]);
    assert!(
        squeezed.title.w >= (row.w - 36.0) * RecordColumns::TITLE_SHARE - 0.01,
        "oversized columns shrink together before the title gives way: {squeezed:?}"
    );
    assert!(squeezed.title.x < squeezed.format.x && squeezed.when.right() < row.right());
}
