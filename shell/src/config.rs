//! Persisted presentation config: bindings, volumes, UI scale, camera
//! feel, accessibility, touch timing, and window size.
//!
//! Nothing here may affect a running match. The config versions
//! independently of replays. Any read problem (missing file, old version,
//! parse error) falls back to defaults silently: a bad config file must
//! never keep the game from starting.

use crate::action::BindingMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Bumped when the shape changes incompatibly; mismatches reset to
/// defaults rather than guessing.
const CONFIG_VERSION: u32 = 1;

/// Optional player-facing frame timing, independent of the debug overlay.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceDisplay {
    /// No collection or display.
    #[default]
    Off,
    /// Smoothed frames per second.
    Fps,
    /// Frame intervals, CPU work, and recent hitch history.
    Detailed,
}

impl PerformanceDisplay {
    /// Every mode, from showing least to most.
    pub const ALL: [Self; 3] = [Self::Off, Self::Fps, Self::Detailed];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Fps => "FPS",
            Self::Detailed => "Detailed",
        }
    }
}

/// Mixer bus volumes, 0..=1, applied multiplicatively with each clip's
/// authored level.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Volumes {
    /// Everything.
    pub master: f32,
    /// Battle and world clips.
    pub effects: f32,
    /// Chrome sounds.
    pub ui: f32,
    /// Music and ambient beds.
    pub music: f32,
}

impl Default for Volumes {
    fn default() -> Self {
        Self {
            master: 1.0,
            effects: 1.0,
            ui: 1.0,
            music: 1.0,
        }
    }
}

/// Camera feel knobs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CameraPrefs {
    /// Keyboard pan speed multiplier.
    pub pan_speed: f32,
    /// Whether the pointer at a window edge pans.
    pub edge_pan: bool,
    /// Flip wheel-zoom direction.
    pub zoom_inverted: bool,
}

impl Default for CameraPrefs {
    fn default() -> Self {
        Self {
            pan_speed: 1.0,
            edge_pan: false,
            zoom_inverted: false,
        }
    }
}

/// Touch gesture preferences.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TouchPrefs {
    /// Two taps inside this window read as a double-tap.
    pub double_tap_ms: u32,
    /// A still finger held this long fires the context gesture.
    pub long_press_ms: u32,
}

impl Default for TouchPrefs {
    fn default() -> Self {
        Self {
            double_tap_ms: 300,
            long_press_ms: 350,
        }
    }
}

impl TouchPrefs {
    /// The invariant a hand-edited config cannot break: a lazy
    /// double-tap must never read as a long-press, so the press window
    /// always sits strictly above the tap window.
    pub fn clamped(self) -> Self {
        Self {
            double_tap_ms: self.double_tap_ms.clamp(50, 1000),
            long_press_ms: self
                .long_press_ms
                .clamp(50, 2000)
                .max(self.double_tap_ms.clamp(50, 1000) + 50),
        }
    }
}

/// Strategic marker transition and size in logical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MarkerPrefs {
    pub start: f32,
    pub end: f32,
    pub scale: f32,
}

impl Default for MarkerPrefs {
    fn default() -> Self {
        Self {
            start: 24.0,
            end: 16.0,
            scale: 1.0,
        }
    }
}

impl MarkerPrefs {
    pub fn clamped(self) -> Self {
        let finite = |value: f32, default: f32, min: f32, max: f32| {
            if value.is_finite() {
                value.clamp(min, max)
            } else {
                default
            }
        };
        let end = finite(self.end, 16.0, 8.0, 30.0);
        Self {
            start: finite(self.start, 24.0, end + 1.0, 40.0).max(end + 1.0),
            end,
            scale: finite(self.scale, 1.0, 0.75, 1.5),
        }
    }

    /// The timing presets as (start, end, name), from the latest
    /// transition to the earliest.
    pub const TIMINGS: [(f32, f32, &'static str); 3] = [
        (18.0, 10.0, "Later"),
        (24.0, 16.0, "Standard"),
        (30.0, 22.0, "Earlier"),
    ];

    /// The preset these endpoints match, if any.
    pub fn timing(self) -> Option<usize> {
        Self::TIMINGS.iter().position(|&(start, end, _)| {
            start.to_bits() == self.start.to_bits() && end.to_bits() == self.end.to_bits()
        })
    }

    pub fn timing_label(self) -> &'static str {
        self.timing()
            .map_or("Custom", |preset| Self::TIMINGS[preset].2)
    }

    /// Moves the endpoints to preset `index` of [`Self::TIMINGS`].
    pub fn set_timing(&mut self, index: usize) {
        (self.start, self.end, _) = Self::TIMINGS[index];
    }
}

/// The whole persisted surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Strategic marker timing and size.
    pub markers: MarkerPrefs,
    /// Optional performance HUD.
    pub performance_display: PerformanceDisplay,
    /// Shape version; mismatch resets to defaults.
    pub version: u32,
    /// The active binding profile.
    pub bindings: BindingMap,
    /// Bus volumes.
    pub volumes: Volumes,
    /// User UI scale factor, applied through `render::ui_scale`.
    pub ui_scale: f32,
    /// Camera feel.
    pub camera: CameraPrefs,
    /// Window size at startup, `WIDTHxHEIGHT`.
    pub window: (u32, u32),
    /// Accessibility: damp decorative animation (alert pulses, ping
    /// rings, muzzle flashes). Informational motion — unit movement,
    /// shell arcs — always stays.
    pub reduced_motion: bool,
    /// Accessibility: colorblind-safe ownership colors (base sprite art
    /// is untouched).
    pub colorblind: bool,
    /// Show the control-group column above the minimap. Hiding it
    /// leaves keyboard groups working.
    pub control_groups: bool,
    /// Touch gesture timing.
    pub touch: TouchPrefs,
    /// The host address the last LAN join used.
    pub last_join_address: Option<String>,
    /// The name other players see and that replaces the player's seat
    /// name; unset until the player first gives one.
    pub player_name: Option<String>,
    /// This installation's identifier as hex, sent only to hosts it joins
    /// so a resumed match can seat the player again; made up the first
    /// time it is needed.
    pub install_id: Option<String>,
    /// The Basic Training lessons this player has finished, in course
    /// order.
    pub lessons_done: Vec<crate::tutorial::Lesson>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            markers: MarkerPrefs::default(),
            performance_display: PerformanceDisplay::Off,
            version: CONFIG_VERSION,
            bindings: BindingMap::classic(),
            volumes: Volumes::default(),
            ui_scale: 1.0,
            camera: CameraPrefs::default(),
            window: (1280, 800),
            reduced_motion: false,
            colorblind: false,
            control_groups: true,
            touch: TouchPrefs::default(),
            last_join_address: None,
            player_name: None,
            install_id: None,
            lessons_done: Vec::new(),
        }
    }
}

fn config_path() -> Option<PathBuf> {
    crate::paths::config_dir().map(|d| d.join("config.json"))
}

impl Config {
    /// Loads the persisted config, or defaults on any trouble at all.
    pub fn load() -> Self {
        Self::load_from(config_path())
    }

    /// Clamps a persisted window size into the envelope the CLI enforces,
    /// so a hand-edited config cannot hand the native backend an
    /// i32-overflowing dimension.
    fn sane_window(window: (u32, u32)) -> (u32, u32) {
        (window.0.clamp(640, 16_384), window.1.clamp(400, 16_384))
    }

    /// Clamps a persisted UI scale into the range the Settings row steps
    /// through, so the row shows the scale the chrome actually uses.
    fn sane_ui_scale(scale: f32) -> f32 {
        if scale.is_finite() {
            scale.clamp(crate::render::MIN_USER_SCALE, crate::render::MAX_USER_SCALE)
        } else {
            1.0
        }
    }

    fn load_from(path: Option<PathBuf>) -> Self {
        let Some(path) = path else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_json::from_str::<Self>(&text) {
            Ok(mut config) if config.version == CONFIG_VERSION => {
                if !config.bindings.valid() {
                    config.bindings = BindingMap::classic();
                }
                config.window = Self::sane_window(config.window);
                config.ui_scale = Self::sane_ui_scale(config.ui_scale);
                config.touch = config.touch.clamped();
                config.markers = config.markers.clamped();
                // A hand-edited name that no longer passes is forgotten,
                // not allowed to reset everything else.
                config.player_name = config
                    .player_name
                    .and_then(|name| oxide_net::PlayerName::new(&name).ok())
                    .map(String::from);
                config.install_id = config
                    .install_id
                    .filter(|id| oxide_net::InstallId::from_hex(id).is_some());
                config.lessons_done = in_course_order(&config.lessons_done);
                config
            }
            _ => Self::default(),
        }
    }

    /// This installation's identifier, made up on first use; the caller
    /// saves the config to keep a new one.
    pub fn install_id(&mut self) -> oxide_net::InstallId {
        if let Some(id) = self
            .install_id
            .as_deref()
            .and_then(oxide_net::InstallId::from_hex)
        {
            return id;
        }
        let id = oxide_net::InstallId::new(crate::entropy::bits());
        self.install_id = Some(id.to_hex());
        id
    }

    /// Marks `lesson` finished; returns whether it was not already.
    pub fn finish_lesson(&mut self, lesson: crate::tutorial::Lesson) -> bool {
        if self.lessons_done.contains(&lesson) {
            return false;
        }
        self.lessons_done.push(lesson);
        self.lessons_done = in_course_order(&self.lessons_done);
        true
    }

    /// Persists atomically (temp + rename), creating the directory.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = config_path() else {
            return Ok(()); // headless CI without HOME: nothing to do
        };
        self.save_to(&path)
    }

    fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).expect("config serializes");
        chassis::fsx::write_atomic(path, |writer| writer.write_all(text.as_bytes()))
    }
}

/// Each lesson of `done` once, in course order.
fn in_course_order(done: &[crate::tutorial::Lesson]) -> Vec<crate::tutorial::Lesson> {
    crate::tutorial::Lesson::ALL
        .into_iter()
        .filter(|lesson| done.contains(lesson))
        .collect()
}

#[cfg(test)]
mod tests;
