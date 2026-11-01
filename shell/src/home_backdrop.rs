//! The match that plays behind Home: one of a few bundled recordings, chosen
//! at random and started from its checkpoint.
//!
//! This is presentation only. It shares nothing with the player's match, makes
//! no sound and only runs while Home is on screen.

use crate::assets::Sprites;
use crate::frame_time::FrameTime;
use crate::input::InputState;
use crate::screens::playback::PlaybackSession;
use anyhow::{Context, Result};
use macroquad::models::{Mesh, Vertex, draw_mesh};
use macroquad::prelude::*;

/// The bundled recordings, which `aftermine-driver home-replays` writes.
const SCENES: [&str; 6] = [
    "assets/home/highwall.amreplay",
    "assets/home/crosswinds.amreplay",
    "assets/home/trident-plateau.amreplay",
    "assets/home/derelict-belt.amreplay",
    "assets/home/crossroads.amreplay",
    "assets/home/eight-pits.amreplay",
];

/// Tiles across the window at the wide end of the zoom.
const TILES_ACROSS: f32 = 56.0;
/// The widest a window may zoom out: below it, units draw as strategic
/// markers.
const LEAST_ZOOM: f32 = 25.0;
/// How far right of center the fight sits, as a share of the window, to clear
/// the menu.
const FIGHT_OFFSET: f32 = 0.18;
/// How far the view sways from the fight, in tiles along each axis.
const SWAY: f32 = 3.0;
/// Seconds over which the match fades in, and out before it starts over.
const FADE: f32 = 1.2;
/// Ticks before the end at which it starts to fade out.
const FADE_TICKS: f32 = FADE * 20.0;
/// The whole match's dimming, so the menu stays in front of it.
const DIM: f32 = 0.42;
/// The added dimming at the window's left edge, under the menu.
const LEFT_DIM: f32 = 0.90;
/// How far across the window the left dimming reaches.
const LEFT_REACH: f32 = 0.72;

/// Which scene to show. Automation always gets the first, so its frames repeat.
fn choose(automation: bool) -> usize {
    if automation {
        return 0;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos() as usize);
    now % SCENES.len()
}

/// Whether a match is playing behind Home.
pub(crate) enum HomeMatch {
    /// None yet: the next Home loads one.
    Unloaded,
    Playing(Box<HomeBackdrop>),
    /// None could be loaded, so Home stays plain until the app restarts.
    Refused,
}

impl HomeMatch {
    /// The match to show, loading one on first use.
    pub(crate) fn playing(&mut self, automation: bool) -> Option<&mut HomeBackdrop> {
        if matches!(self, Self::Unloaded) {
            *self = match HomeBackdrop::load(choose(automation)) {
                Ok(backdrop) => Self::Playing(Box::new(backdrop)),
                Err(error) => {
                    eprintln!("Home plays no match: {error:#}");
                    Self::Refused
                }
            };
        }
        match self {
            Self::Playing(backdrop) => Some(backdrop),
            Self::Unloaded | Self::Refused => None,
        }
    }

    /// Frees the match while a real one runs. A refusal stays refused.
    pub(crate) fn unload(&mut self) {
        if matches!(self, Self::Playing(_)) {
            *self = Self::Unloaded;
        }
    }
}

pub(crate) struct HomeBackdrop {
    session: PlaybackSession,
    checkpoint: u64,
    /// Seconds since this run of the scene began.
    age: f32,
    /// Where the view rests, and the window it was framed for.
    anchor: Vec2,
    framed_for: Vec2,
    /// Holding still for reduced motion.
    still: bool,
}

/// The view's offset from its anchor at `age` seconds. It only sways, so the
/// fight stays in view however long Home is left open.
fn sway(age: f32) -> Vec2 {
    vec2((age / 23.0).sin(), (age / 31.0).sin()) * SWAY
}

const NO_TIME: FrameTime = FrameTime {
    presentation: 0.0,
    raw: 0.0,
};

impl HomeBackdrop {
    /// Opens scene `index` at its checkpoint, framed on its fiercest fight.
    fn load(index: usize) -> Result<Self> {
        let path = crate::assets::resource_root().join(SCENES[index]);
        let recording = aftermine_kit::load_recording(&path)
            .with_context(|| format!("loading {}", path.display()))?;
        Self::open(recording)
    }

    fn open(recording: aftermine_kit::Recording) -> Result<Self> {
        let (mut session, checkpoint) = PlaybackSession::from_checkpoint(recording)?;
        let viewport = crate::render::viewport();
        // The seek restores the stored world on its first step.
        session.advance_frame(NO_TIME, viewport);
        let mut backdrop = Self {
            session,
            checkpoint,
            age: 0.0,
            anchor: Vec2::ZERO,
            framed_for: viewport,
            still: false,
        };
        backdrop.frame(viewport);
        Ok(backdrop)
    }

    fn frame(&mut self, viewport: Vec2) {
        let zoom = (viewport.x / TILES_ACROSS).max(LEAST_ZOOM);
        let view = self.session.view();
        let map = view.state.map();
        let fallback = (map.width() / 2, map.height() / 2);
        let (x, y) =
            aftermine_kit::fights::fiercest(view.state).map_or(fallback, |fight| fight.center);
        let center = vec2(x as f32 - viewport.x * FIGHT_OFFSET / zoom, y as f32);
        self.framed_for = viewport;
        let camera = &mut self.session.presentation.camera;
        camera.show(center, zoom);
        // The camera stops at the map's edge, so a fight beside one rests there.
        self.anchor = camera.center;
    }

    /// Plays on, sways the view, and starts over at the end. When `still`, for
    /// reduced motion, it holds instead, since none of this motion informs
    /// anyone.
    pub(crate) fn advance(&mut self, dt: f32, still: bool) {
        self.age += dt;
        self.still = still;
        let viewport = crate::render::viewport();
        if self.session.ticks_left() == 0 {
            self.session.seek_to(self.checkpoint);
            self.session.advance_frame(NO_TIME, viewport);
            self.age = 0.0;
            self.frame(viewport);
        } else if viewport != self.framed_for {
            self.frame(viewport);
        }
        let time = if still {
            NO_TIME
        } else {
            FrameTime {
                presentation: dt,
                raw: dt,
            }
        };
        self.session.advance_frame(time, viewport);
        // Nothing plays these, so they would pile up for as long as Home shows.
        self.session.presentation.sounds_pending.clear();
        let center = if still {
            self.anchor
        } else {
            self.anchor + sway(self.age)
        };
        let zoom = self.session.presentation.camera.zoom;
        self.session.presentation.camera.show(center, zoom);
    }

    pub(crate) fn draw(&self, sprites: &Sprites, input: &InputState) {
        crate::render::draw_world(&self.session.view(), sprites, input);
        let (w, h) = (screen_width(), screen_height());
        let page = crate::theme::PAGE;
        let veil = |alpha: f32| Color::new(page.r, page.g, page.b, alpha);
        let fade_in = 1.0 - (self.age / FADE).clamp(0.0, 1.0);
        let fade_out = if self.still {
            0.0
        } else {
            1.0 - (self.session.ticks_left() as f32 / FADE_TICKS).clamp(0.0, 1.0)
        };
        let dim = DIM + (1.0 - DIM) * fade_in.max(fade_out);
        draw_rectangle(0.0, 0.0, w, h, veil(dim));
        let reach = LEFT_REACH * w;
        draw_mesh(&Mesh {
            vertices: vec![
                Vertex::new(0.0, 0.0, 0.0, 0.0, 0.0, veil(LEFT_DIM)),
                Vertex::new(reach, 0.0, 0.0, 0.0, 0.0, veil(0.0)),
                Vertex::new(reach, h, 0.0, 0.0, 0.0, veil(0.0)),
                Vertex::new(0.0, h, 0.0, 0.0, 0.0, veil(LEFT_DIM)),
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            texture: None,
        });
    }
}

#[cfg(test)]
mod tests;
