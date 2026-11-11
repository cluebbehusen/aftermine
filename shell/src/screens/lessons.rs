//! Basic Training's lesson list: a row per lesson and one for the first
//! match, each marked once finished, beside the selected row's opening scene,
//! its story and the button that starts it. Windowless update; the
//! coordinator starts what the screen picks. The panel measures text through
//! a supplied function, so input hit-tests the Start button the frame draws
//! and tests lay the panel out headless.

use crate::assets::Sprites;
use crate::game::{Game, SoundKind};
use crate::home_backdrop::LEAST_ZOOM;
use crate::input::InputState;
use crate::menu::{Label, Menu};
use crate::nav::Nav;
use crate::press::{Fed, Press};
use crate::render::hud::{Face, Measure, window_measure};
use crate::render::prim::stroke_rect;
use crate::theme::{BORDER_STRONG, MIN_TOUCH_TARGET, SURFACE_MENU, Stroke, TEXT_BODY, Type};
use crate::tutorial::{Course, Lesson, Tutorial};
use aftermine_protocol::{Key, RawEvent};
use anyhow::Result;
use macroquad::prelude::{
    Camera2D, DrawTextureParams, FilterMode, Rect, RenderTarget, Vec2, WHITE, draw_rectangle,
    draw_text, draw_texture_ex, pop_camera_state, push_camera_state, render_target,
    screen_dpi_scale, set_camera, vec2,
};

/// The panel's inset, in logical px at 1x.
const PAD: f32 = 16.0;
/// The distance between story baselines, in logical px at 1x.
const LINE: f32 = 28.0;
/// The Start button's width, in logical px at 1x.
const START_WIDTH: f32 = 104.0;
/// The scene's tallest height as a share of its width.
const SCENE_ASPECT: f32 = 0.6;
/// The least share of its tallest height a scene keeps. Below it the scene
/// shows too little to read, so the story has the panel to itself.
const SCENE_LEAST_SHARE: f32 = 1.0 / 3.0;
/// Tiles across the scene at the wide end of its zoom.
const SCENE_TILES_ACROSS: f32 = 20.0;

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
    start: Press<()>,
    /// The selected row's scene; the first draw for a row builds it.
    scene: Option<Box<OpeningScene>>,
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
        menu.shift = crate::menu::BESIDE_PAGE_SHIFT;
        menu.select_where(|&row| !finished(row));
        Self {
            menu,
            back: crate::button::BackButton::default(),
            start: Press::default(),
            scene: None,
        }
    }

    /// The story of the row under the cursor.
    pub fn briefing(&self) -> Option<&'static str> {
        self.menu.value().map(|row| row.briefing())
    }

    /// Applies a frame's events. Pressing a row only selects it, so mouse
    /// and touch can read its story first; Start and Enter start it.
    pub fn update(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
        measure: Measure,
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
        let start = start_button(crate::render::ui_scale(), measure);
        for event in &events {
            match self
                .start
                .feed(event, |p, _| start.contains(p).then_some(()))
            {
                Fed::Activated(()) => {
                    if let Some(&row) = self.menu.value() {
                        sounds.push((SoundKind::Click, None));
                        return Out::Start(row);
                    }
                }
                Fed::Held => {}
                Fed::Ignored => {
                    if let Some(choice) = self.menu.handle(std::slice::from_ref(event), mouse) {
                        sounds.push((SoundKind::Click, None));
                        if Nav::decode(event) == Some(Nav::Confirm) {
                            return Out::Start(choice.value);
                        }
                    }
                }
            }
        }
        Out::Stay
    }

    /// Draws the list, the selected row's scene and story, and the
    /// buttons.
    pub fn draw(&mut self, sprites: &Sprites, mouse: Vec2) {
        self.menu.draw(sprites, "");
        crate::button::draw_back(mouse);
        let s = crate::render::ui_scale();
        let measure: Measure = &window_measure;
        let panel = panel(s, measure);
        draw_rectangle(panel.x, panel.y, panel.w, panel.h, SURFACE_MENU);
        let scene = scene_area(s, measure);
        self.draw_scene(sprites, scene);
        stroke_rect(
            Rect::new(panel.x, panel.y, panel.w, panel.h),
            Stroke::Edge.at(s),
            BORDER_STRONG,
        );
        // Enter presses it from anywhere on the screen, so it always reads
        // as focused.
        crate::button::draw(start_button(s, measure), "Start", true, s);
        let Some(briefing) = self.briefing() else {
            return;
        };
        let mut y = scene.bottom() + (PAD + LINE * 0.75) * s;
        for line in briefing_lines(briefing, s, measure) {
            draw_text(&line, panel.x + PAD * s, y, Type::Label.at(s), TEXT_BODY);
            y += LINE * s;
        }
    }

    fn draw_scene(&mut self, sprites: &Sprites, area: Rect) {
        let Some(&row) = self.menu.value() else {
            return;
        };
        if area.h <= 0.0 {
            return;
        }
        let dpi = screen_dpi_scale();
        // On whole device pixels the canvas lands texel for pixel.
        let at = (vec2(area.x, area.y) * dpi).round() / dpi;
        let size = (vec2(area.w, area.h) * dpi).floor() / dpi;
        let scene = match &mut self.scene {
            Some(scene) if scene.row == row && scene.size == size => scene,
            slot => slot.insert(Box::new(OpeningScene::open(row, size, dpi))),
        };
        scene.draw(sprites, at);
    }
}

/// A row's opening scene, drawn live so it always shows the current art. Its
/// session never ticks, so nothing in it moves or makes a sound.
struct OpeningScene {
    row: Course,
    /// The scene's logical size.
    size: Vec2,
    /// `None` when the session could not be built, which leaves the panel bare.
    game: Option<Game>,
    /// The session's own idle input, since the player's belongs to the window.
    input: InputState,
    canvas: RenderTarget,
}

impl OpeningScene {
    fn open(row: Course, size: Vec2, dpi: f32) -> Self {
        let game = opening_session(row, size)
            .inspect_err(|error| eprintln!("{} shows no scene: {error:#}", row.title()))
            .ok();
        let canvas = render_target(
            crate::numeric::to_u32((size.x * dpi).round()).max(1),
            crate::numeric::to_u32((size.y * dpi).round()).max(1),
        );
        canvas.texture.set_filter(FilterMode::Nearest);
        Self {
            row,
            size,
            game,
            input: InputState::new(),
            canvas,
        }
    }

    fn draw(&self, sprites: &Sprites, at: Vec2) {
        let Some(game) = &self.game else {
            return;
        };
        push_camera_state();
        set_camera(&Camera2D {
            target: self.size * 0.5,
            zoom: vec2(2.0 / self.size.x, 2.0 / self.size.y),
            render_target: Some(self.canvas.clone()),
            ..Default::default()
        });
        crate::render::draw_world(&game.view(), sprites, &self.input);
        pop_camera_state();
        draw_texture_ex(
            &self.canvas.texture,
            at.x,
            at.y,
            WHITE,
            DrawTextureParams {
                dest_size: Some(self.size),
                ..Default::default()
            },
        );
    }
}

/// A session of `row` as the player would find it on starting, for a view of
/// `size`. The first match's bot seed is unused, since this never ticks.
fn opening_session(row: Course, size: Vec2) -> Result<Game> {
    let mut game = match row {
        Course::Lesson(lesson) => Tutorial::begin_at(lesson, size)?,
        Course::FirstMatch => Game::with_viewport(crate::tutorial::first_match(None, 0)?, size)?,
    };
    let camera = &mut game.presentation.camera;
    let center = camera.center;
    camera.show(center, (size.x / SCENE_TILES_ACROSS).max(LEAST_ZOOM));
    Ok(game)
}

/// `briefing` wrapped to the panel's inset width.
fn briefing_lines(briefing: &str, s: f32, measure: Measure) -> Vec<String> {
    let size = Type::Label.at(s);
    let room = crate::menu::page_column(s).w - 2.0 * PAD * s;
    crate::render::wrap_words(briefing, |text| measure(Face::Body, text, size), room)
}

/// The height under the scene: the longest row's story, inset, with Start
/// on its own row below it so no line runs under the button.
fn text_height(s: f32, measure: Measure) -> f32 {
    let longest = Course::ALL
        .iter()
        .map(|row| briefing_lines(row.briefing(), s, measure).len())
        .max()
        .unwrap_or(0);
    (2.0 * PAD + LINE * longest as f32 + MIN_TOUCH_TARGET) * s
}

/// The panel beside the list, one size for every row so Start holds still as
/// the cursor moves.
fn panel(s: f32, measure: Measure) -> Rect {
    let column = crate::menu::page_column(s);
    let text = text_height(s, measure);
    let tallest = column.w * SCENE_ASPECT;
    let room = column.h - text;
    let scene = if room >= tallest * SCENE_LEAST_SHARE {
        tallest.min(room)
    } else {
        0.0
    };
    Rect::new(column.x, column.y, column.w, scene + text)
}

/// The scene, flush with the top of the panel.
fn scene_area(s: f32, measure: Measure) -> Rect {
    let panel = panel(s, measure);
    Rect::new(panel.x, panel.y, panel.w, panel.h - text_height(s, measure))
}

/// The Start button, in the panel's bottom right corner.
fn start_button(s: f32, measure: Measure) -> Rect {
    let panel = panel(s, measure);
    Rect::new(
        panel.right() - (PAD + START_WIDTH) * s,
        panel.bottom() - (PAD + MIN_TOUCH_TARGET) * s,
        START_WIDTH * s,
        MIN_TOUCH_TARGET * s,
    )
}

#[cfg(test)]
mod tests;
