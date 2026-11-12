use super::*;
use crate::tests::support::{keystroke, touch_down, touch_up};
use aftermine_protocol::Key;
use aftermine_protocol::MouseButton;
use macroquad::prelude::vec2;

/// Every glyph half as wide as the size: deterministic and headless.
fn fixed(_: Face, text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * size * 0.5
}

fn press(list: &mut LessonsScreen, events: &[RawEvent]) -> Out {
    let mut mouse = vec2(0.0, 0.0);
    let mut sounds = Vec::new();
    list.update(events, &mut mouse, &mut sounds, &fixed)
}

/// A click (or, with `touch`, a tap) at `p`.
fn at(p: Vec2, touch: bool) -> [RawEvent; 2] {
    let (x, y) = (p.x, p.y);
    if touch {
        [
            RawEvent::TouchDown { id: 3, x, y },
            RawEvent::TouchUp { id: 3, x, y },
        ]
    } else {
        let button = MouseButton::Left;
        [
            RawEvent::MouseDown { button, x, y },
            RawEvent::MouseUp { button, x, y },
        ]
    }
}

fn start() -> Vec2 {
    start_button(crate::render::ui_scale(), &fixed).center()
}

fn row(list: &LessonsScreen, index: usize) -> Vec2 {
    list.menu
        .item_rect(index)
        .expect("every row shows")
        .center()
}

#[test]
fn the_list_marks_finished_rows_and_opens_on_the_next_one() {
    let fresh = LessonsScreen::open(&[], false);
    assert_eq!(fresh.menu.title, "TUTORIAL");
    assert_eq!(
        fresh.menu.view().items,
        [
            "1. Command: ",
            "2. Scrap: ",
            "3. Build: ",
            "4. Fight: ",
            "5. Scout and repair: ",
            "6. First match: "
        ]
    );
    assert_eq!(fresh.menu.value(), Some(&Course::Lesson(Lesson::Command)));
    assert_eq!(
        fresh.briefing(),
        Some(Course::Lesson(Lesson::Command).briefing())
    );

    let next = LessonsScreen::open(&[Lesson::Command], false);
    assert_eq!(
        next.menu.view().items,
        [
            "1. Command: DONE",
            "2. Scrap: ",
            "3. Build: ",
            "4. Fight: ",
            "5. Scout and repair: ",
            "6. First match: "
        ]
    );
    assert_eq!(next.menu.value(), Some(&Course::Lesson(Lesson::Scrap)));
    assert_eq!(
        next.briefing(),
        Some(Course::Lesson(Lesson::Scrap).briefing())
    );

    let lessons_done = LessonsScreen::open(&Lesson::ALL, false);
    assert_eq!(lessons_done.menu.value(), Some(&Course::FirstMatch));
    assert_eq!(lessons_done.briefing(), Some(Course::FirstMatch.briefing()));

    let played = LessonsScreen::open(&[], true);
    assert_eq!(played.menu.view().items[5], "6. First match: DONE");
    assert_eq!(played.menu.value(), Some(&Course::Lesson(Lesson::Command)));

    let graduate = LessonsScreen::open(&Lesson::ALL, true);
    assert_eq!(
        graduate.menu.value(),
        Some(&Course::Lesson(Lesson::Command))
    );
}

#[test]
fn enter_starts_the_selected_row_and_back_or_escape_leaves() {
    let mut list = LessonsScreen::open(&[], false);
    assert_eq!(press(&mut list, &keystroke(Key::Down)), Out::Stay);
    assert_eq!(
        press(&mut list, &keystroke(Key::Enter)),
        Out::Start(Course::Lesson(Lesson::Scrap))
    );
    for _ in 0..4 {
        press(&mut list, &keystroke(Key::Down));
    }
    assert_eq!(
        press(&mut list, &keystroke(Key::Enter)),
        Out::Start(Course::FirstMatch)
    );
    assert_eq!(press(&mut list, &keystroke(Key::Escape)), Out::Leave);
    for touch in [false, true] {
        let mut list = LessonsScreen::open(&[], false);
        assert_eq!(
            press(&mut list, &crate::button::tests::press_back(touch)),
            Out::Leave
        );
    }
}

#[test]
fn pressing_a_row_selects_it_without_starting_even_when_already_selected() {
    for touch in [false, true] {
        let mut list = LessonsScreen::open(&[], false);
        let scrap = row(&list, 1);
        for _ in 0..2 {
            let mut mouse = vec2(0.0, 0.0);
            let mut sounds = Vec::new();
            assert_eq!(
                list.update(&at(scrap, touch), &mut mouse, &mut sounds, &fixed),
                Out::Stay
            );
            assert_eq!(sounds.len(), 1, "a press clicks");
            assert_eq!(list.menu.value(), Some(&Course::Lesson(Lesson::Scrap)));
        }
    }
}

#[test]
fn start_starts_the_selected_row() {
    for touch in [false, true] {
        let mut list = LessonsScreen::open(&[], false);
        let fight = row(&list, 3);
        press(&mut list, &at(fight, touch));
        let mut mouse = vec2(0.0, 0.0);
        let mut sounds = Vec::new();
        assert_eq!(
            list.update(&at(start(), touch), &mut mouse, &mut sounds, &fixed),
            Out::Start(Course::Lesson(Lesson::Fight))
        );
        assert_eq!(sounds.len(), 1, "a press clicks");
    }
}

#[test]
fn a_press_that_leaves_start_starts_nothing() {
    let mut list = LessonsScreen::open(&[], false);
    let on = start();
    let off = row(&list, 1);
    assert_eq!(
        press(&mut list, &[touch_down(3, on), touch_up(3, off)]),
        Out::Stay
    );
    assert_eq!(list.menu.value(), Some(&Course::Lesson(Lesson::Command)));
}

#[test]
fn the_panel_seats_its_scene_and_every_story_on_every_target_window() {
    for (w, h) in [
        (1133.0, 744.0),
        (1210.0, 834.0),
        (1280.0, 800.0),
        (1366.0, 1024.0),
    ] {
        crate::render::set_viewport(w, h);
        let s = crate::render::ui_scale();
        let list = LessonsScreen::open(&[], false);
        let panel = panel(s, &fixed);
        let scene = scene_area(s, &fixed);
        let start = start_button(s, &fixed);
        for index in 0..Course::ALL.len() {
            let rect = list.menu.item_rect(index).expect("every row shows");
            assert!(
                rect.x >= 0.0 && rect.right() < panel.x,
                "row {index} at {w}"
            );
        }
        assert!(panel.right() <= w && panel.bottom() <= h, "panel at {w}");
        assert!(scene.h > 0.0 && scene.bottom() < start.y, "scene at {w}");
        assert!(
            start.x > panel.x && start.right() < panel.right() && start.bottom() < panel.bottom(),
            "Start at {w}"
        );
        assert!(start.h >= MIN_TOUCH_TARGET * s, "Start at {w}");
        let room = panel.w - 2.0 * PAD * s;
        for row in Course::ALL {
            let lines = briefing_lines(row.briefing(), s, &fixed);
            assert!(lines.len() > 1, "{} wraps at {w}", row.title());
            for line in &lines {
                let width = fixed(Face::Body, line, Type::Label.at(s));
                assert!(width <= room, "{} spills sideways at {w}", row.title());
            }
            let bottom = scene.bottom() + PAD * s + LINE * lines.len() as f32 * s;
            assert!(bottom <= start.y, "{} runs under Start at {w}", row.title());
        }
    }
}

#[test]
fn a_story_that_would_squeeze_the_scene_out_gets_the_panel_to_itself() {
    // 150% UI scale on the smallest target window wraps the longest story
    // past the room a readable scene needs.
    crate::render::set_viewport(1133.0, 744.0);
    crate::render::set_user_scale(crate::render::MAX_USER_SCALE);
    let s = crate::render::ui_scale();
    let scene = scene_area(s, &fixed);
    let start = start_button(s, &fixed);
    let panel = panel(s, &fixed);
    crate::render::set_user_scale(1.0);
    assert_eq!(scene.h, 0.0, "no sliver of scene");
    assert!(start.bottom() < panel.bottom() && panel.bottom() <= 744.0);
    for row in Course::ALL {
        let lines = briefing_lines(row.briefing(), s, &fixed);
        let bottom = scene.bottom() + PAD * s + LINE * lines.len() as f32 * s;
        assert!(bottom <= start.y, "{} runs under Start", row.title());
    }
}

#[test]
fn every_rows_opening_scene_frames_the_players_own_forces() {
    let size = vec2(498.0, 288.0);
    for row in Course::ALL {
        let game = opening_session(row, size).expect("every row has a scene");
        let camera = &game.presentation.camera;
        assert_eq!(camera.viewport(), size, "{}", row.title());
        assert!(camera.zoom >= LEAST_ZOOM, "{} draws markers", row.title());
        let human = game.presentation.human;
        let shown = |at: Vec2| {
            let p = camera.to_screen(at);
            (0.0..=size.x).contains(&p.x) && (0.0..=size.y).contains(&p.y)
        };
        assert!(
            game.state
                .units()
                .iter()
                .any(|unit| unit.player == human && shown(crate::game::world_vec(unit.pos))),
            "{} shows none of the player's units",
            row.title()
        );
    }
}
