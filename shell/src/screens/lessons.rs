//! Basic Training's lesson list: a row per lesson, marked once finished,
//! and the BACK button home. Windowless update; the coordinator starts
//! the lesson a row picks.

use crate::game::SoundKind;
use crate::menu::{Label, Menu};
use crate::tutorial::Lesson;
use macroquad::prelude::Vec2;
use oxide_protocol::{Key, RawEvent};

/// What a lesson-list frame decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Out {
    /// Still choosing.
    Stay,
    /// Back to the front door.
    Leave,
    /// Start this lesson.
    Start(Lesson),
}

/// The lesson list.
pub struct LessonsScreen {
    /// One row per lesson.
    pub menu: Menu<Lesson>,
    back: crate::button::BackButton,
}

impl LessonsScreen {
    /// Opens with the cursor on the first lesson not yet finished.
    pub fn open(done: &[Lesson]) -> Self {
        let rows = Lesson::ALL.into_iter().map(|lesson| {
            (
                Label::Setting {
                    name: format!("{}. {}", lesson.number(), lesson.title()),
                    value: if done.contains(&lesson) { "DONE" } else { "" }.into(),
                },
                lesson,
            )
        });
        let mut menu = Menu::rows("BASIC TRAINING", rows);
        menu.select_where(|lesson| !done.contains(lesson));
        Self {
            menu,
            back: crate::button::BackButton::default(),
        }
    }

    /// The debug protocol's stable mode name.
    #[expect(
        clippy::unused_self,
        reason = "other screens answer this from their state"
    )]
    pub fn mode_name(&self) -> &'static str {
        "lessons"
    }

    /// The subtitle: what the lesson under the cursor teaches.
    pub fn subtitle(&self) -> &'static str {
        self.menu.value().map_or("", |lesson| lesson.summary())
    }

    /// Applies a frame's events.
    pub fn update(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Out {
        let (back, events) = self.back.route(events);
        if back {
            sounds.push((SoundKind::Click, None));
            return Out::Leave;
        }
        if events
            .iter()
            .any(|e| matches!(e, RawEvent::KeyDown { key: Key::Escape }))
        {
            return Out::Leave;
        }
        let Some(choice) = self.menu.handle(&events, mouse) else {
            return Out::Stay;
        };
        sounds.push((SoundKind::Click, None));
        Out::Start(choice.value)
    }

    /// Draws the list and its BACK button over the veiled backdrop.
    pub fn draw(&self, mouse: Vec2) {
        self.menu.draw(self.subtitle());
        crate::button::draw_back(mouse);
    }
}

#[cfg(test)]
mod tests;
