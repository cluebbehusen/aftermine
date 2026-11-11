//! The lesson card: heading, objective, the step's paragraphs, the coach
//! line, and the card's buttons; and the match tip card, which sits in the
//! same place. [`layout`] and [`tip_layout`] measure through a supplied
//! function, so input hit-tests the same rectangles the frame draws, and
//! tests lay the cards out headless.

use super::hud::{Face, HudEnv, Measure};
use crate::render::prim::stroke_rect;
use crate::theme::{
    EDGE_WARM, SURFACE_CARD, Stroke, TEXT_ACCENT, TEXT_BODY, TEXT_DANGER, TEXT_PRIMARY,
    TEXT_SECONDARY, Type,
};
use crate::tips::Tip;
use crate::tutorial::{CoachLine, Course, Tutorial};
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
    /// Starts the match that closes the course.
    FirstMatch,
    /// Leaves for the lesson list.
    Lessons,
    /// Closes the match tip.
    Dismiss,
}

impl CardButton {
    /// The controller button that presses it in pad mode, for the buttons
    /// a finished lesson offers.
    fn pad_button(self) -> Option<aftermine_protocol::PadButton> {
        use aftermine_protocol::PadButton;
        match self {
            Self::Next | Self::FirstMatch => Some(PadButton::South),
            Self::Lessons => Some(PadButton::East),
            Self::Fold | Self::Retry | Self::Dismiss => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Fold | Self::Dismiss => "",
            Self::Retry => "Retry",
            Self::Next => "Next lesson",
            Self::FirstMatch => "First match",
            Self::Lessons => "Lessons",
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

/// Lines stacked down a card from its top padding.
struct Column {
    x: f32,
    cursor: f32,
    lines: Vec<Line>,
}

impl Column {
    /// Sets `text` on the next baseline, `gap` below the line above.
    fn push(&mut self, text: String, face: Face, size: f32, color: Color, gap: f32) {
        self.cursor += gap + size;
        self.lines.push(Line {
            text,
            face,
            size,
            color,
            x: self.x,
            baseline: self.cursor,
        });
    }
}

/// One frame's card: where it sits, what it says, and what it answers.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CardLayout {
    /// The whole card; presses on it never reach the world.
    pub rect: Rect,
    /// The card's press targets, the corner toggle first.
    pub buttons: Vec<(Rect, CardButton)>,
    lines: Vec<Line>,
    folded: bool,
    /// The card's opacity, below 1 while a tip fades out.
    alpha: f32,
    /// Whose buttons the card's own buttons wear: the controller pad mode
    /// names, if any.
    pad: Option<aftermine_protocol::PadFamily>,
}

impl CardLayout {
    /// While pad mode names a controller, the card button one of its
    /// buttons presses: A for the next lesson or the first match and B for
    /// the lesson list, while the card offers them.
    pub(crate) fn pad_press(&self, pressed: aftermine_protocol::PadButton) -> Option<CardButton> {
        self.pad?;
        self.buttons
            .iter()
            .map(|(_, button)| *button)
            .find(|button| button.pad_button() == Some(pressed))
    }

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
}

/// The card's width at most, in logical px.
const CARD_W: f32 = 500.0;
/// The tip card's width at most, in logical px.
const TIP_W: f32 = 420.0;
/// The gap between a card and the window's side, in logical px, as the
/// performance panel keeps on the right.
const EDGE: f32 = 12.0;

/// Which card holds the top-left corner under the top bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CornerCard {
    Lesson,
    Tip,
}

impl CornerCard {
    /// The card's right edge in window px, whatever its height.
    pub(crate) fn right(self, env: HudEnv) -> f32 {
        let (x, _, w) = corner(
            match self {
                Self::Lesson => CARD_W,
                Self::Tip => TIP_W,
            },
            env,
        );
        x + w
    }
}

/// Where a card at most `width` wide sits: the top-left corner under the
/// top bar, narrowed to fit the window. Returns its x, y and width.
fn corner(width: f32, env: HudEnv) -> (f32, f32, f32) {
    let s = env.ui;
    let w = (width * s).min(env.viewport.x - 2.0 * EDGE * s);
    (EDGE * s, (crate::layout::TOP_BAR_H + 6.0) * s, w)
}

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
        ("{run}", Action::Run),
        ("{back}", Action::Back),
        ("{rally}", Action::SetRally),
        ("{assign}", Action::AssignGroup(1)),
        ("{recall}", Action::Slot(1)),
        ("{alert}", Action::JumpToLastAlert),
    ] {
        out = out.replace(token, &bindings.label(action));
    }
    out
}

/// Lays the card out for `tutorial` over `game`. `coach` is the step's
/// live line, which the caller reads from the world. In pad mode (`hud`)
/// the card stands open with no fold, since a controller cannot fold it,
/// and offers only what a finished lesson's buttons do, each wearing the
/// button that presses it on the controller `hud` names; Restart in the
/// pause menu stands in for Retry.
pub(crate) fn layout(
    tutorial: &Tutorial,
    coach: Option<&CoachLine>,
    bindings: &crate::action::BindingMap,
    hands: crate::platform::Hands,
    env: HudEnv,
    measure: Measure,
    hud: crate::input::HudHands,
) -> CardLayout {
    let s = env.ui;
    let (x, y, w) = corner(CARD_W, env);
    let pad = 12.0 * s;
    let fold = 24.0 * s;
    let text_w = w - pad * 2.0;
    let lesson = tutorial.lesson();
    let steps = lesson.steps().len();
    let folded = tutorial.collapsed() && !hud.pad_mode();
    let mut column = Column {
        x: x + pad,
        cursor: y + pad,
        lines: Vec::new(),
    };

    let progress = if tutorial.finished() {
        "DONE".to_string()
    } else {
        format!("STEP {} OF {steps}", tutorial.step_index() + 1)
    };
    let small = Type::Small.at(s);
    column.push(
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
    column.push(
        title.to_string(),
        Face::Display,
        Type::Label.at(s),
        TEXT_PRIMARY,
        6.0 * s,
    );

    let body_size = Type::Body.at(s);
    let wrap = |text: &str| super::wrap_words(text, |t| measure(Face::Body, t, body_size), text_w);
    let mut buttons = Vec::new();
    if !hud.pad_mode() {
        buttons.push((
            Rect::new(x + w - pad - fold, y + 6.0 * s, fold, fold),
            CardButton::Fold,
        ));
    }
    if !folded {
        let paragraphs: Vec<String> = if let Some(step) = tutorial.current() {
            step.body(hands)
                .iter()
                .map(|text| expand(text, bindings))
                .collect()
        } else {
            let next = lesson.next();
            let kind = match next {
                Course::Lesson(_) => "Next lesson",
                Course::FirstMatch => "Next",
            };
            [
                lesson.outro().to_string(),
                format!("{kind}: {}.", next.title()),
                next.briefing().to_string(),
            ]
            .into_iter()
            .filter(|paragraph| !paragraph.is_empty())
            .collect()
        };
        let mut gap = 8.0 * s;
        for paragraph in &paragraphs {
            for (i, line) in wrap(paragraph).into_iter().enumerate() {
                column.push(
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
                column.push(
                    line,
                    Face::Body,
                    body_size,
                    color,
                    if i == 0 { 9.0 * s } else { 5.0 * s },
                );
            }
        }
        let actions: &[CardButton] = if tutorial.finished() {
            match lesson.next() {
                Course::Lesson(_) => &[CardButton::Next, CardButton::Lessons],
                Course::FirstMatch => &[CardButton::FirstMatch, CardButton::Lessons],
            }
        } else if matches!(coach, Some(CoachLine::Stuck)) && !hud.pad_mode() {
            &[CardButton::Retry]
        } else {
            &[]
        };
        if !actions.is_empty() {
            let h = 32.0 * s;
            let top = column.cursor + 12.0 * s;
            let mut left = x + pad;
            for action in actions {
                let mut label_w = measure(Face::Body, action.label(), body_size);
                if hud.names().is_some() {
                    label_w += measure(Face::Body, " ", body_size)
                        + crate::hint_line::icon_width(body_size);
                }
                let bw = (label_w + 28.0 * s).max(112.0 * s);
                buttons.push((Rect::new(left, top, bw, h), *action));
                left += bw + 8.0 * s;
            }
            column.cursor = top + h;
        }
    }
    let h = column.cursor + pad - y;
    CardLayout {
        rect: Rect::new(x, y, w, h),
        buttons,
        lines: column.lines,
        folded,
        alpha: 1.0,
        pad: hud.names(),
    }
}

/// Lays out the card for match tip `tip` at opacity `alpha`, where the
/// lesson card sits: its heading, what to do, and a close button. In pad
/// mode (`hud`) it has no close button: a tip closes itself in time.
pub(crate) fn tip_layout(
    tip: Tip,
    alpha: f32,
    bindings: &crate::action::BindingMap,
    hands: crate::platform::Hands,
    env: HudEnv,
    measure: Measure,
    hud: crate::input::HudHands,
) -> CardLayout {
    let s = env.ui;
    let (x, y, w) = corner(TIP_W, env);
    let pad = 12.0 * s;
    let close = 24.0 * s;
    let mut column = Column {
        x: x + pad,
        cursor: y + pad,
        lines: Vec::new(),
    };
    column.push(
        tip.title().to_string(),
        Face::Display,
        Type::Label.at(s),
        TEXT_PRIMARY,
        6.0 * s,
    );
    let body_size = Type::Body.at(s);
    let body = expand(tip.body(hands), bindings);
    let lines = super::wrap_words(&body, |t| measure(Face::Body, t, body_size), w - pad * 2.0);
    for (i, line) in lines.into_iter().enumerate() {
        column.push(
            line,
            Face::Body,
            body_size,
            TEXT_BODY,
            if i == 0 { 8.0 * s } else { 5.0 * s },
        );
    }
    let h = column.cursor + pad - y;
    let buttons = if hud.pad_mode() {
        Vec::new()
    } else {
        vec![(
            Rect::new(x + w - pad - close, y + 6.0 * s, close, close),
            CardButton::Dismiss,
        )]
    };
    CardLayout {
        rect: Rect::new(x, y, w, h),
        buttons,
        lines: column.lines,
        folded: false,
        alpha,
        pad: hud.names(),
    }
}

/// The card as this frame lays it out against the window's fonts.
pub(crate) fn current(
    tutorial: &Tutorial,
    game: &crate::game::Game,
    bindings: &crate::action::BindingMap,
    hud: crate::input::HudHands,
) -> CardLayout {
    layout(
        tutorial,
        tutorial.coach(game).as_ref(),
        bindings,
        crate::platform::hands(),
        HudEnv::current(),
        &super::hud::window_measure,
        hud,
    )
}

/// The match tip's card as this frame lays it out against the window's
/// fonts, while a tip shows.
pub(crate) fn current_tip(
    game: &crate::game::Game,
    bindings: &crate::action::BindingMap,
    hud: crate::input::HudHands,
) -> Option<CardLayout> {
    let tip = game.tips.showing()?;
    let alpha = game.tips.alpha(
        game.state.current_tick(),
        game.presentation.tick_fraction(),
        super::reduced_motion(),
    );
    Some(tip_layout(
        tip,
        alpha,
        bindings,
        crate::platform::hands(),
        HudEnv::current(),
        &super::hud::window_measure,
        hud,
    ))
}

/// Draws a laid-out card; `mouse` lights the button under the pointer.
pub(crate) fn draw(card: &CardLayout, sprites: &crate::assets::Sprites, mouse: Vec2) {
    let s = super::ui_scale();
    let r = card.rect;
    let fade = |color: Color| Color {
        a: color.a * card.alpha,
        ..color
    };
    draw_rectangle(r.x, r.y, r.w, r.h, fade(SURFACE_CARD));
    stroke_rect(
        Rect::new(r.x, r.y, r.w, r.h),
        Stroke::Edge.at(s),
        fade(EDGE_WARM),
    );
    for line in &card.lines {
        let color = fade(line.color);
        match line.face {
            Face::Display => {
                crate::typography::draw(&line.text, line.x, line.baseline, line.size, color);
            }
            Face::Body => {
                draw_text(&line.text, line.x, line.baseline, line.size, color);
            }
        }
    }
    let hovered = crate::platform::hands().pointer == crate::input::Pointer::Mouse;
    for (rect, button) in &card.buttons {
        let lit = hovered && crate::platform::hovers(*rect, mouse);
        if matches!(button, CardButton::Fold | CardButton::Dismiss) {
            let color = fade(if lit { TEXT_PRIMARY } else { TEXT_SECONDARY });
            let stroke = Stroke::Edge.at(s);
            // A hairline box at whole-pixel corners loses edges to
            // rasterization; an icon this small needs every edge.
            stroke_rect(Rect::new(rect.x, rect.y, rect.w, rect.h), stroke, color);
            let (cx, cy, arm) = (rect.center().x, rect.center().y, rect.w * 0.28);
            if *button == CardButton::Dismiss {
                let arm = arm * std::f32::consts::FRAC_1_SQRT_2;
                draw_line(cx - arm, cy - arm, cx + arm, cy + arm, stroke, color);
                draw_line(cx - arm, cy + arm, cx + arm, cy - arm, stroke, color);
                continue;
            }
            // A bar folds the card; a cross of bars opens it again.
            draw_line(cx - arm, cy, cx + arm, cy, stroke, color);
            if card.folded {
                draw_line(cx, cy - arm, cx, cy + arm, stroke, color);
            }
        } else if let (Some(family), Some(pressed)) = (card.pad, button.pad_button()) {
            // The button wears the controller button that presses it,
            // ahead of its words.
            crate::button::draw(*rect, "", false, s);
            let size = Type::Body.at(s);
            let gap = measure_text(" ", None, crate::numeric::font_size(size), 1.0).width;
            let words = measure_text(button.label(), None, crate::numeric::font_size(size), 1.0);
            let width = crate::hint_line::icon_width(size) + gap + words.width;
            let x = rect.x + (rect.w - width) * 0.5;
            let baseline = rect.y + rect.h * 0.62;
            let icon = crate::pad::button_icon(family, pressed);
            let x = x + crate::hint_line::draw_icon(sprites, icon, x, baseline, size, TEXT_PRIMARY);
            draw_text(button.label(), x + gap, baseline, size, TEXT_BODY);
        } else {
            crate::button::draw(*rect, button.label(), lit, s);
        }
    }
}

#[cfg(test)]
mod tests;
