//! The 2D camera: world units (tiles) to screen pixels and back.
//!
//! Pure presentation — nothing here may influence the sim. All f32 math on
//! purpose; determinism is the sim's job.
//!
//! The camera never queries the window: the viewport is injected (once per
//! frame by the main loop), which keeps every method a pure function of
//! `Camera` state — and therefore unit-testable without a window.

use macroquad::prelude::{Vec2, vec2};

/// Zoom bounds in logical pixels per tile. macroquad's coordinate space
/// already absorbs the retina multiple in its backing store, so applying
/// dpi here would double-scale the world on high-dpi displays.
const ZOOM_MIN: f32 = 8.0;
const ZOOM_MAX: f32 = 96.0;
const ZOOM_DEFAULT: f32 = 32.0;

/// A pan/zoom camera over the tile grid.
pub struct Camera {
    /// World point at the viewport center.
    pub center: Vec2,
    /// Logical pixels per world unit.
    pub zoom: f32,
    target_zoom: f32,
    /// Cursor the current zoom glide is anchored on.
    zoom_anchor: Option<Vec2>,
    zoom_min: f32,
    zoom_max: f32,
    viewport: Vec2,
    map_size: Vec2,
    /// A decaying jolt, in logical pixels, from a nearby heavy blast. It
    /// moves the whole view, so screen and world points stay paired.
    shake: f32,
    shake_time: f32,
}

/// How fast a jolt dies away, per second.
const SHAKE_DECAY: f32 = 7.0;

impl Camera {
    /// A camera looking at `focus` on a `width`×`height`-tile map,
    /// rendered into a `viewport`-logical-pixel window.
    pub fn new(focus: Vec2, width: i32, height: i32, viewport: Vec2) -> Self {
        let mut camera = Self {
            center: focus,
            zoom: ZOOM_DEFAULT,
            target_zoom: ZOOM_DEFAULT,
            zoom_anchor: None,
            zoom_min: ZOOM_MIN,
            zoom_max: ZOOM_MAX,
            viewport,
            map_size: vec2(width as f32, height as f32),
            shake: 0.0,
            shake_time: 0.0,
        };
        camera.clamp();
        camera
    }

    /// Current viewport size in pixels.
    pub fn viewport(&self) -> Vec2 {
        self.viewport
    }

    /// Tracks a window resize (called once per frame); re-clamps so the
    /// view never strands outside the map.
    pub fn set_viewport(&mut self, viewport: Vec2) {
        if viewport != self.viewport {
            self.viewport = viewport;
            self.clamp();
        }
    }

    /// Screen pixels for a world point.
    pub fn to_screen(&self, world: Vec2) -> Vec2 {
        (world - self.center) * self.zoom + self.viewport * 0.5 + self.shake_offset()
    }

    /// World point under a screen pixel.
    pub fn to_world(&self, screen: Vec2) -> Vec2 {
        (screen - self.viewport * 0.5 - self.shake_offset()) / self.zoom + self.center
    }

    /// Jolts the view by up to `pixels`, keeping a stronger jolt underway.
    pub fn kick(&mut self, pixels: f32) {
        self.shake = self.shake.max(pixels);
    }

    fn shake_offset(&self) -> Vec2 {
        if self.shake <= 0.0 {
            return Vec2::ZERO;
        }
        // Two incommensurate rates read as a rattle, not a sway.
        vec2(
            (self.shake_time * 73.0).sin(),
            (self.shake_time * 59.0).cos(),
        ) * self.shake
    }

    /// Pans by a world-space delta.
    pub fn pan(&mut self, delta: Vec2) {
        self.center += delta;
        self.clamp();
    }

    /// Requests a zoom by wheel notches; the camera glides there over the
    /// next frames ([`Camera::update`]), keeping the world point under
    /// `cursor_px` stationary the whole way — smooth for trackpads and
    /// notchy wheels alike.
    pub fn zoom_at(&mut self, cursor_px: Vec2, notches: f32) {
        self.target_zoom =
            (self.target_zoom * 1.15f32.powf(notches)).clamp(self.zoom_min, self.zoom_max);
        self.zoom_anchor = Some(cursor_px);
    }

    /// Whether a zoom glide is underway.
    pub fn zooming(&self) -> bool {
        self.zoom != self.target_zoom
    }

    /// Advances the zoom glide. Call once per frame.
    pub fn update(&mut self, dt: f32) {
        if self.shake > 0.0 {
            self.shake_time += dt;
            self.shake *= (-SHAKE_DECAY * dt).exp();
            if self.shake < 0.25 {
                self.shake = 0.0;
                self.shake_time = 0.0;
            }
        }
        if self.zoom == self.target_zoom {
            self.zoom_anchor = None;
            return;
        }
        let anchor_px = self.zoom_anchor.unwrap_or(self.viewport * 0.5);
        let anchor_world = self.to_world(anchor_px);
        let t = (12.0 * dt).min(1.0);
        self.zoom += (self.target_zoom - self.zoom) * t;
        if (self.target_zoom - self.zoom).abs() < 0.01 {
            self.zoom = self.target_zoom;
            self.zoom_anchor = None;
        }
        let after = self.to_world(anchor_px);
        self.center += anchor_world - after;
        self.clamp();
    }

    /// The visible world rectangle `[min, max]`.
    pub fn world_rect(&self) -> (Vec2, Vec2) {
        let half = self.viewport * 0.5 / self.zoom;
        (self.center - half, self.center + half)
    }

    fn clamp(&mut self) {
        // Keep the view inside the map, with slack when zoomed far out.
        let half = self.viewport * 0.5 / self.zoom;
        let slack = vec2(2.0, 2.0);
        let lo = (half - slack).min(self.map_size * 0.5);
        let hi = (self.map_size - half + slack).max(self.map_size * 0.5);
        self.center = self.center.clamp(lo, hi);
    }
}

pub(crate) mod controls;

#[cfg(test)]
mod tests;
