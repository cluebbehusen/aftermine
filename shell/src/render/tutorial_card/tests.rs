use super::*;
use crate::action::BindingMap;
use crate::input::Pointer;
use crate::platform::Hands;
use crate::tutorial::Lesson;

/// Every glyph half as wide as the size: deterministic and headless.
fn fixed(_: Face, text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * size * 0.5
}

fn env(w: f32) -> HudEnv {
    HudEnv {
        viewport: vec2(w, 800.0),
        ui: 1.0,
    }
}

const MOUSE: Hands = Hands {
    pointer: Pointer::Mouse,
    keys: true,
};

fn lesson(lesson: Lesson, at: usize) -> Tutorial {
    let game = Tutorial::begin_at(lesson, vec2(1280.0, 800.0)).unwrap();
    game.tutorial.unwrap().at_step(at)
}

fn lay(tutorial: &Tutorial, coach: Option<&CoachLine>, hands: Hands, w: f32) -> CardLayout {
    layout(
        tutorial,
        coach,
        &BindingMap::classic(),
        hands,
        env(w),
        &fixed,
    )
}

fn actions(card: &CardLayout) -> Vec<CardButton> {
    card.buttons.iter().map(|(_, button)| *button).collect()
}

#[test]
fn the_card_heads_its_step_and_keeps_every_line_inside_itself() {
    for w in [640.0, 1280.0] {
        let card = lay(&lesson(Lesson::Command, 0), None, MOUSE, w);
        let text = card.text();
        assert_eq!(text[0], "LESSON 1: COMMAND  |  STEP 1 OF 5");
        assert_eq!(text[1], "Move the camera");
        assert!(text.len() > 3, "the step's paragraphs follow");
        assert!(card.rect.x >= 0.0 && card.rect.right() <= w);
        for line in &card.lines {
            let width = fixed(line.face, &line.text, line.size);
            assert!(
                line.x >= card.rect.x && line.x + width <= card.rect.right(),
                "{w}: {} spills past the card",
                line.text
            );
            assert!(line.baseline <= card.rect.bottom());
        }
        assert_eq!(actions(&card), [CardButton::Fold]);
    }
}

#[test]
fn every_step_names_real_bindings_for_every_pair_of_hands() {
    for hands in crate::platform::ALL_HANDS {
        for lesson_kind in Lesson::ALL {
            for at in 0..lesson_kind.steps().len() {
                let card = lay(&lesson(lesson_kind, at), None, hands, 1280.0);
                for line in card.text() {
                    assert!(
                        !line.contains('{') && !line.contains('}'),
                        "{lesson_kind:?} step {at}: unexpanded token in {line}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_folded_card_keeps_its_heading_and_the_way_to_open_it() {
    let open = lay(&lesson(Lesson::Scrap, 2), None, MOUSE, 1280.0);
    let mut tutorial = lesson(Lesson::Scrap, 2);
    tutorial.toggle_collapsed();
    let folded = lay(&tutorial, None, MOUSE, 1280.0);
    assert_eq!(folded.text(), &open.text()[..2]);
    assert!(folded.rect.h < open.rect.h);
    assert_eq!(actions(&folded), [CardButton::Fold]);
    let fold = folded.buttons[0].0;
    assert!(folded.rect.contains(fold.center()));
}

#[test]
fn a_stuck_step_offers_a_retry_and_a_finished_lesson_the_next() {
    let stuck = lay(
        &lesson(Lesson::Command, 4),
        Some(&CoachLine::Stuck),
        MOUSE,
        1280.0,
    );
    assert_eq!(actions(&stuck), [CardButton::Fold, CardButton::Retry]);
    assert!(stuck.text().join(" ").contains(CoachLine::Stuck.text()));

    let status = CoachLine::Status(
        "A Harvester costs 50 scrap. You have 0, and 1 Harvester is collecting.".into(),
    );
    let paying = lay(&lesson(Lesson::Scrap, 1), Some(&status), MOUSE, 1280.0);
    assert_eq!(actions(&paying), [CardButton::Fold]);
    assert!(paying.text().join(" ").contains(status.text()));

    let done = lay(&lesson(Lesson::Command, 5), None, MOUSE, 1280.0);
    assert_eq!(
        actions(&done),
        [CardButton::Fold, CardButton::Next, CardButton::Lessons]
    );
    let text = done.text();
    assert_eq!(text[0], "LESSON 1: COMMAND  |  DONE");
    assert_eq!(text[1], "Lesson complete");
    assert!(
        text.iter()
            .any(|line| line.starts_with("Next lesson: Scrap."))
    );
    for (rect, _) in &done.buttons {
        assert!(done.rect.contains(rect.center()));
    }

    let last = Lesson::ALL[Lesson::ALL.len() - 1];
    let graduate = lay(&lesson(last, last.steps().len()), None, MOUSE, 1280.0);
    assert_eq!(
        actions(&graduate),
        [
            CardButton::Fold,
            CardButton::FirstMatch,
            CardButton::Lessons
        ]
    );
    assert!(
        graduate
            .text()
            .iter()
            .any(|line| line.starts_with("Next: First match."))
    );
}

#[test]
fn a_finger_reaches_a_button_just_past_its_drawn_edge() {
    let card = lay(&lesson(Lesson::Command, 0), None, MOUSE, 1280.0);
    let fold = card.buttons[0].0;
    let near = vec2(fold.right() + 4.0, fold.center().y);
    assert!(!fold.contains(near));
    assert_eq!(card.button_at(near, false, 1.0), None);
    assert_eq!(card.button_at(near, true, 1.0), Some(CardButton::Fold));
    assert_eq!(
        card.button_at(fold.center(), false, 1.0),
        Some(CardButton::Fold)
    );
    let body = vec2(card.rect.center().x, card.rect.bottom() - 2.0);
    assert!(card.covers(body, false, 1.0));
    assert_eq!(card.button_at(body, false, 1.0), None);
    let below = vec2(card.rect.center().x, card.rect.bottom() + 40.0);
    assert!(!card.covers(below, true, 1.0));
}

fn lay_tip(tip: Tip, hands: Hands, w: f32) -> CardLayout {
    tip_layout(tip, 1.0, &BindingMap::classic(), hands, env(w), &fixed)
}

#[test]
fn both_cards_sit_in_the_top_left_corner_under_the_bar() {
    let under_bar = crate::layout::TOP_BAR_H + 6.0;
    for w in [320.0, 640.0, 1280.0] {
        let cards = [
            lay(&lesson(Lesson::Build, 3), None, MOUSE, w),
            lay_tip(Tip::IdleHarvesters, MOUSE, w),
        ];
        for card in cards {
            assert_eq!((card.rect.x, card.rect.y), (EDGE, under_bar), "{w}");
            assert!(
                card.rect.right() <= w - EDGE,
                "{w}: the card fits the window"
            );
        }
    }
}

#[test]
fn the_tip_card_heads_its_tip_names_real_bindings_and_keeps_inside_itself() {
    for hands in crate::platform::ALL_HANDS {
        for tip in Tip::ALL {
            for w in [640.0, 1280.0] {
                let card = lay_tip(tip, hands, w);
                let text = card.text();
                assert_eq!(text[0], "MATCH TIP");
                assert_eq!(text[1], tip.title());
                assert!(text.len() > 2, "{tip:?}: the tip says what to do");
                assert!(card.rect.x >= 0.0 && card.rect.right() <= w);
                for line in &card.lines {
                    assert!(
                        !line.text.contains('{') && !line.text.contains('}'),
                        "{tip:?}: unexpanded token in {}",
                        line.text
                    );
                    let width = fixed(line.face, &line.text, line.size);
                    assert!(
                        line.x >= card.rect.x && line.x + width <= card.rect.right(),
                        "{w}: {} spills past the card",
                        line.text
                    );
                    assert!(line.baseline <= card.rect.bottom());
                }
                assert_eq!(actions(&card), [CardButton::Dismiss]);
                assert!(card.rect.contains(card.buttons[0].0.center()));
            }
        }
    }
    let keys = lay_tip(Tip::UnderAttack, MOUSE, 1280.0).text().join(" ");
    assert!(keys.contains("press Tab"), "{keys}");
}
