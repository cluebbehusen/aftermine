//! Basic Training's lesson list: a row per lesson and one for the first
//! match, each marked once finished, and the BACK button home. Windowless
//! update; the coordinator starts what a row picks.

use crate::game::SoundKind;
use crate::menu::{Label, Menu};
use crate::tutorial::{Course, Lesson};
use aftermine_protocol::{Key, RawEvent};
use macroquad::prelude::Vec2;

/// What a lesson-list frame decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Out {
    /// Still choosing.
    Stay,
    /// Back to the front door.
    Leave,
    /// Start this lesson or the first match.
    Start(Course),
}

/// The lesson list.
pub struct LessonsScreen {
    /// One row per lesson, then the first match.
    pub menu: Menu<Course>,
    back: crate::button::BackButton,
}

impl LessonsScreen {
    /// Opens with the cursor on the first row not yet finished.
    pub fn open(done: &[Lesson], first_match_done: bool) -> Self {
        let finished = |row: Course| match row {
            Course::Lesson(lesson) => done.contains(&lesson),
            Course::FirstMatch => first_match_done,
        };
        let rows = Course::ALL.into_iter().map(|row| {
            (
                Label::Setting {
                    name: format!("{}. {}", row.number(), row.title()),
                    value: if finished(row) { "DONE" } else { "" }.into(),
                },
                row,
            )
        });
        let mut menu = Menu::rows("TUTORIAL", rows);
        menu.heading = crate::menu::Heading::Page { band: 0.0 };
        menu.select_where(|&row| !finished(row));
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

    /// The subtitle: what the row under the cursor teaches.
    pub fn subtitle(&self) -> &'static str {
        self.menu.value().map_or("", |row| row.summary())
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
