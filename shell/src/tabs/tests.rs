use super::*;
use aftermine_protocol::MouseButton;

fn click(at: Vec2) -> [RawEvent; 2] {
    let button = MouseButton::Left;
    [
        RawEvent::MouseDown {
            button,
            x: at.x,
            y: at.y,
        },
        RawEvent::MouseUp {
            button,
            x: at.x,
            y: at.y,
        },
    ]
}

#[test]
fn segments_sit_side_by_side_inside_the_window_at_every_scale() {
    for scale in [0.75, 1.0, 1.25, 1.5] {
        crate::render::set_viewport(1280.0, 800.0);
        crate::render::set_user_scale(scale);
        let rects = rects(4);
        assert!(
            rects
                .windows(2)
                .all(|pair| pair[0].right() <= pair[1].x + 0.01)
        );
        assert!(rects[0].x > 0.0 && rects[3].right() < 1280.0);
        assert!(rects.iter().all(|rect| rect.h >= MIN_TOUCH_TARGET));
    }
    crate::render::set_user_scale(1.0);
}

#[test]
fn a_press_on_a_tab_is_claimed_and_anything_else_passes_through() {
    crate::render::set_viewport(1280.0, 800.0);
    crate::render::set_user_scale(1.0);
    let mut strip = TabStrip::default();
    let rects = rects(4);
    let (picked, rest) = strip.route(&click(rects[2].center()), 4);
    assert_eq!(picked, Some(2));
    assert!(rest.is_empty(), "the list beneath never sees the press");

    let tap = [
        RawEvent::TouchDown {
            id: 3,
            x: rects[1].center().x,
            y: rects[1].center().y,
        },
        RawEvent::TouchUp {
            id: 3,
            x: rects[1].center().x,
            y: rects[1].center().y,
        },
    ];
    assert_eq!(strip.route(&tap, 4).0, Some(1));

    let below = click(Vec2::new(640.0, 700.0));
    let (picked, rest) = strip.route(&below, 4);
    assert_eq!(picked, None);
    assert_eq!(rest, below.to_vec());
}

#[test]
fn a_press_released_on_another_tab_picks_nothing() {
    crate::render::set_viewport(1280.0, 800.0);
    crate::render::set_user_scale(1.0);
    let mut strip = TabStrip::default();
    let rects = rects(4);
    let button = MouseButton::Left;
    let (from, to) = (rects[0].center(), rects[3].center());
    let events = [
        RawEvent::MouseDown {
            button,
            x: from.x,
            y: from.y,
        },
        RawEvent::MouseUp {
            button,
            x: to.x,
            y: to.y,
        },
    ];
    assert_eq!(strip.route(&events, 4).0, None);
}
