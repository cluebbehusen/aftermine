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
fn the_list_marks_finished_rows_and_opens_on_the_next_one() {
    let fresh = LessonsScreen::open(&[], false);
    assert_eq!(fresh.menu.title, "BASIC TRAINING");
    assert_eq!(
        fresh.menu.view().items,
        [
            "1. Command: ",
            "2. Scrap: ",
            "3. Build: ",
            "4. Fight: ",
            "5. See and sustain: ",
            "6. First match: "
        ]
    );
    assert_eq!(fresh.menu.value(), Some(&Course::Lesson(Lesson::Command)));
    assert_eq!(fresh.subtitle(), Lesson::Command.summary());

    let next = LessonsScreen::open(&[Lesson::Command], false);
    assert_eq!(
        next.menu.view().items,
        [
            "1. Command: DONE",
            "2. Scrap: ",
            "3. Build: ",
            "4. Fight: ",
            "5. See and sustain: ",
            "6. First match: "
        ]
    );
    assert_eq!(next.menu.value(), Some(&Course::Lesson(Lesson::Scrap)));
    assert_eq!(next.subtitle(), Lesson::Scrap.summary());

    let lessons_done = LessonsScreen::open(&Lesson::ALL, false);
    assert_eq!(lessons_done.menu.value(), Some(&Course::FirstMatch));
    assert_eq!(lessons_done.subtitle(), Course::FirstMatch.summary());

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
fn a_row_starts_its_lesson_or_the_first_match_and_back_or_escape_leaves() {
    let mut list = LessonsScreen::open(&[], false);
    assert_eq!(press(&mut list, &key(Key::Down)), Out::Stay);
    assert_eq!(
        press(&mut list, &key(Key::Enter)),
        Out::Start(Course::Lesson(Lesson::Scrap))
    );
    for _ in 0..4 {
        press(&mut list, &key(Key::Down));
    }
    assert_eq!(
        press(&mut list, &key(Key::Enter)),
        Out::Start(Course::FirstMatch)
    );
    assert_eq!(press(&mut list, &key(Key::Escape)), Out::Leave);
    for touch in [false, true] {
        let mut list = LessonsScreen::open(&[], false);
        assert_eq!(
            press(&mut list, &crate::button::press_back(touch)),
            Out::Leave
        );
    }
    assert_eq!(LessonsScreen::open(&[], false).mode_name(), "lessons");
}
