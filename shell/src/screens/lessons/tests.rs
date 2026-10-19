use super::*;
use macroquad::prelude::vec2;

fn press(list: &mut LessonsScreen, events: &[RawEvent]) -> Out {
    let mut mouse = vec2(0.0, 0.0);
    let mut sounds = Vec::new();
    list.update(events, &mut mouse, &mut sounds)
}

fn key(key: Key) -> [RawEvent; 2] {
    [RawEvent::KeyDown { key }, RawEvent::KeyUp { key }]
}

#[test]
fn the_list_marks_finished_lessons_and_opens_on_the_next_one() {
    let fresh = LessonsScreen::open(&[]);
    assert_eq!(fresh.menu.title, "BASIC TRAINING");
    assert_eq!(fresh.menu.view().items, ["1. Command: ", "2. Scrap: "]);
    assert_eq!(fresh.menu.value(), Some(&Lesson::Command));
    assert_eq!(fresh.subtitle(), Lesson::Command.summary());

    let next = LessonsScreen::open(&[Lesson::Command]);
    assert_eq!(next.menu.view().items, ["1. Command: DONE", "2. Scrap: "]);
    assert_eq!(next.menu.value(), Some(&Lesson::Scrap));
    assert_eq!(next.subtitle(), Lesson::Scrap.summary());

    let graduate = LessonsScreen::open(&Lesson::ALL);
    assert_eq!(graduate.menu.value(), Some(&Lesson::Command));
}

#[test]
fn a_row_starts_its_lesson_and_back_or_escape_leaves() {
    let mut list = LessonsScreen::open(&[]);
    assert_eq!(press(&mut list, &key(Key::Down)), Out::Stay);
    assert_eq!(
        press(&mut list, &key(Key::Enter)),
        Out::Start(Lesson::Scrap)
    );
    assert_eq!(press(&mut list, &key(Key::Escape)), Out::Leave);
    for touch in [false, true] {
        let mut list = LessonsScreen::open(&[]);
        assert_eq!(
            press(&mut list, &crate::button::press_back(touch)),
            Out::Leave
        );
    }
    assert_eq!(LessonsScreen::open(&[]).mode_name(), "lessons");
}
