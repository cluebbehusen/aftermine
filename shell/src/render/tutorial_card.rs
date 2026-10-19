//! The lesson card: heading, objective, the step's paragraphs, the coach
//! line, and the card's buttons. [`layout`] measures through a supplied
//! function, so input hit-tests the same rectangles the frame draws, and
//! tests lay the card out headless.

use super::hud::{Face, HudEnv, Measure};
use crate::theme::{
    EDGE_WARM, SURFACE_CARD, Stroke, TEXT_ACCENT, TEXT_BODY, TEXT_DANGER, TEXT_PRIMARY,
    TEXT_SECONDARY, Type,
};
use crate::tutorial::{CoachLine, Tutorial};
use macroquad::prelude::*;

/// A press target on the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CardButton {
    /// Folds the card to its heading and objective, or opens it.
    Fold,
    /// Starts the lesson over.
    Retry,
    /// Starts the next lesson.
    Next,
    /// Leaves for the lesson list.
    Lessons,
}

impl CardButton {
    fn label(self) -> &'static str {
        match self {
            Self::Fold => "",
            Self::Retry => "RETRY",
            Self::Next => "NEXT LESSON",
            Self::Lessons => "LESSONS",
        }
    }
}

/// One line of text on the card, at its baseline.
#[derive(Debug, Clone, PartialEq)]
struct Line {
    text: String,
    face: Face,
    size: f32,
    color: Color,
    x: f32,
    baseline: f32,
}

/// One frame's card: where it sits, what it says, and what it answers.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CardLayout {
    /// The whole card; presses on it never reach the world.
    pub rect: Rect,
    /// The card's press targets, the fold toggle first.
    pub buttons: Vec<(Rect, CardButton)>,
    lines: Vec<Line>,
    folded: bool,
}

impl CardLayout {
    /// The button whose drawn rectangle, padded to a fingertip for a
    /// finger, holds `point`.
    pub(crate) fn button_at(&self, point: Vec2, finger: bool, ui: f32) -> Option<CardButton> {
        self.buttons
            .iter()
            .find(|(rect, _)| {
                let rect = if finger {
                    crate::layout::touch_pad(*rect, ui)
                } else {
                    *rect
                };
                rect.contains(point)
            })
            .map(|(_, button)| *button)
    }

    /// Whether `point` lies on the card or on a padded button target that
    /// reaches past its edge.
    pub(crate) fn covers(&self, point: Vec2, finger: bool, ui: f32) -> bool {
        self.rect.contains(point) || (finger && self.button_at(point, true, ui).is_some())
    }

    /// Every line of text on the card, in reading order.
    #[cfg(test)]
    pub(crate) fn text(&self) -> Vec<&str> {
        self.lines.iter().map(|line| line.text.as_str()).collect()
    }
}

/// The card's width at most, in logical px.
const CARD_W: f32 = 500.0;

/// Expands the `{token}`s lesson copy uses into the player's bindings.
pub(crate) fn expand(text: &str, bindings: &crate::action::BindingMap) -> String {
    use crate::action::Action;
    let mut out = text.to_string();
    if out.contains("{pan}") {
        let pan = [
            Action::PanUp,
            Action::PanLeft,
            Action::PanDown,
            Action::PanRight,
        ]
        .map(|action| bindings.label(action))
        .join(" ");
        out = out.replace("{pan}", &pan);
    }
    for (token, action) in [
        ("{train}", Action::TrainSlot(0)),
        ("{idle}", Action::CycleIdleWorker),
        ("{build}", Action::ToggleBuildPalette),
        ("{hunt}", Action::Hunt),
        ("{back}", Action::Back),
    ] {
        out = out.replace(token, &bindings.label(action));
    }
    out
}

/// Lays the card out for `tutorial` over `game`. `coach` is the step's
/// live line, which the caller reads from the world.
pub(crate) fn layout(
    tutorial: &Tutorial,
    coach: Option<&CoachLine>,
    bindings: &crate::action::BindingMap,
    hands: crate::platform::Hands,
    env: HudEnv,
    measure: Measure,
) -> CardLayout {
    let s = env.ui;
    let w = (CARD_W * s).min(env.viewport.x - 32.0 * s);
    let x = (env.viewport.x - w) * 0.5;
    let y = (crate::layout::TOP_BAR_H + 6.0) * s;
    let pad = 12.0 * s;
    let fold = 24.0 * s;
    let text_w = w - pad * 2.0;
    let lesson = tutorial.lesson();
    let steps = lesson.steps().len();
    let folded = tutorial.collapsed();
    let mut lines = Vec::new();
    let mut cursor = y + pad;
    let mut push = |text: String, face: Face, size: f32, color: Color, gap: f32| {
        cursor += gap + size;
        lines.push(Line {
            text,
            face,
            size,
            color,
            x: x + pad,
            baseline: cursor,
        });
    };

    let progress = if tutorial.finished() {
        "DONE".to_string()
    } else {
        format!("STEP {} OF {steps}", tutorial.step_index() + 1)
    };
    let small = Type::Small.at(s);
    push(
        format!(
            "LESSON {}: {}  |  {progress}",
            lesson.number(),
            lesson.title().to_uppercase()
        ),
        Face::Display,
        small,
        TEXT_ACCENT,
        0.0,
    );
    let title = tutorial
        .current()
        .map_or("Lesson complete", |step| step.title);
    push(
        title.to_string(),
        Face::Display,
        Type::Label.at(s),
        TEXT_PRIMARY,
        6.0 * s,
    );

    let body_size = Type::Body.at(s);
    let wrap = |text: &str| super::wrap_words(text, |t| measure(Face::Body, t, body_size), text_w);
    let mut buttons = vec![(
        Rect::new(x + w - pad - fold, y + 6.0 * s, fold, fold),
        CardButton::Fold,
    )];
    if !folded {
        let paragraphs: Vec<String> = if let Some(step) = tutorial.current() {
            step.body(hands)
                .iter()
                .map(|text| expand(text, bindings))
                .collect()
        } else {
            let mut outro = vec![lesson.outro().to_string()];
            if let Some(next) = lesson.next() {
                outro.push(format!(
                    "Next lesson: {}. You will {}.",
                    next.title(),
                    next.summary()
                ));
            }
            outro
        };
        let mut gap = 8.0 * s;
        for paragraph in &paragraphs {
            for (i, line) in wrap(paragraph).into_iter().enumerate() {
                push(
                    line,
                    Face::Body,
                    body_size,
                    TEXT_BODY,
                    if i == 0 { gap } else { 5.0 * s },
                );
            }
            gap = 9.0 * s;
        }
        if let Some(coach) = coach {
            let color = match coach {
                CoachLine::Status(_) => TEXT_ACCENT,
                CoachLine::Recovery(_) | CoachLine::Stuck => TEXT_DANGER,
            };
            for (i, line) in wrap(coach.text()).into_iter().enumerate() {
                push(
                    line,
                    Face::Body,
                    body_size,
                    color,
                    if i == 0 { 9.0 * s } else { 5.0 * s },
                );
            }
        }
        let actions: &[CardButton] = if tutorial.finished() {
            if lesson.next().is_some() {
                &[CardButton::Next, CardButton::Lessons]
            } else {
                &[CardButton::Lessons]
            }
        } else if matches!(coach, Some(CoachLine::Stuck)) {
            &[CardButton::Retry]
        } else {
            &[]
        };
        if !actions.is_empty() {
            let h = 32.0 * s;
            let top = cursor + 12.0 * s;
            let mut left = x + pad;
            for action in actions {
                let label_w = measure(Face::Body, action.label(), body_size);
                let bw = (label_w + 28.0 * s).max(112.0 * s);
                buttons.push((Rect::new(left, top, bw, h), *action));
                left += bw + 8.0 * s;
            }
            cursor = top + h;
        }
    }
    let h = cursor + pad - y;
    CardLayout {
        rect: Rect::new(x, y, w, h),
        buttons,
        lines,
        folded,
    }
}

/// The card as this frame lays it out against the window's fonts.
pub(crate) fn current(
    tutorial: &Tutorial,
    game: &crate::game::Game,
    bindings: &crate::action::BindingMap,
) -> CardLayout {
    layout(
        tutorial,
        tutorial.coach(game).as_ref(),
        bindings,
        crate::platform::hands(),
        HudEnv::current(),
        &super::hud::window_measure,
    )
}

/// Draws a laid-out card; `mouse` lights the button under the pointer.
pub(crate) fn draw(card: &CardLayout, mouse: Vec2) {
    let s = super::ui_scale();
    let r = card.rect;
    draw_rectangle(r.x, r.y, r.w, r.h, SURFACE_CARD);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, Stroke::Edge.at(s), EDGE_WARM);
    for line in &card.lines {
        match line.face {
            Face::Display => {
                crate::typography::draw(&line.text, line.x, line.baseline, line.size, line.color);
            }
            Face::Body => {
                draw_text(&line.text, line.x, line.baseline, line.size, line.color);
            }
        }
    }
    let hovered = crate::platform::hands().pointer == crate::input::Pointer::Mouse;
    for (rect, button) in &card.buttons {
        let lit = hovered && rect.contains(mouse);
        if *button == CardButton::Fold {
            draw_rectangle_lines(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                // A hairline box at whole-pixel corners loses edges to
                // rasterization; an icon this small needs every edge.
                Stroke::Edge.at(s),
                if lit { TEXT_PRIMARY } else { TEXT_SECONDARY },
            );
            // A bar folds the card; a cross of bars opens it again.
            let (cx, cy, arm) = (rect.center().x, rect.center().y, rect.w * 0.28);
            let color = if lit { TEXT_PRIMARY } else { TEXT_SECONDARY };
            draw_line(cx - arm, cy, cx + arm, cy, Stroke::Edge.at(s), color);
            if card.folded {
                draw_line(cx, cy - arm, cx, cy + arm, Stroke::Edge.at(s), color);
            }
        } else {
            crate::button::draw(*rect, button.label(), lit, s);
        }
    }
}

#[cfg(test)]
mod tests;
