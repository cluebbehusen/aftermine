//! Persisted presentation config: bindings, volumes, UI scale, camera and
//! controller feel, accessibility, touch timing, and window size.
//!
//! Nothing here may affect a running match. The config versions
//! independently of replays. Any read problem (missing file, old version,
//! parse error) falls back to defaults silently: a bad config file must
//! never keep the game from starting. The settings that follow the player
//! also travel through iCloud; [`sync`] says which. Apple TV keeps the
//! config in user defaults, the one local store it never clears, and
//! every other platform in a file.

use crate::action::BindingMap;
use crate::platform::{PLATFORM, Platform};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Bumped when the shape changes incompatibly; mismatches reset to
/// defaults rather than guessing.
const CONFIG_VERSION: u32 = 1;

/// The names a player goes by until they give one, one per installation.
const DEFAULT_NAMES: [&str; 10] = [
    "Anvil", "Canary", "Flint", "Gravel", "Hopper", "Ingot", "Lantern", "Nickel", "Rivet", "Shale",
];

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

impl Volumes {
    /// Every bus within 0..=1, a non-finite one back at full.
    pub fn clamped(self) -> Self {
        let unit = |volume: f32| {
            if volume.is_finite() {
                volume.clamp(0.0, 1.0)
            } else {
                1.0
            }
        };
        Self {
            master: unit(self.master),
            effects: unit(self.effects),
            ui: unit(self.ui),
            music: unit(self.music),
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

/// How strongly the controller's reticle holds onto a unit or structure it
/// passes over, weakest first.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReticleSnap {
    Off,
    Light,
    #[default]
    Standard,
    Strong,
}

impl ReticleSnap {
    /// Every strength, weakest first.
    pub const ALL: [Self; 4] = [Self::Off, Self::Light, Self::Standard, Self::Strong];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Light => "Light",
            Self::Standard => "Standard",
            Self::Strong => "Strong",
        }
    }
}

/// Controller feel, apart from the keys' and mouse's.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PadPrefs {
    /// Left-stick pan speed multiplier.
    pub stick_speed: f32,
    /// Flip right-stick zoom direction.
    pub zoom_inverted: bool,
    /// How strongly the reticle holds onto what it passes over.
    pub snap: ReticleSnap,
}

impl Default for PadPrefs {
    fn default() -> Self {
        Self {
            stick_speed: 1.0,
            zoom_inverted: false,
            snap: ReticleSnap::default(),
        }
    }
}

impl PadPrefs {
    /// The stick speeds Settings steps through, slow to fast.
    pub const STICK_SPEEDS: [f32; 5] = [0.5, 0.75, 1.0, 1.5, 2.0];

    /// The stick speed within the range Settings steps through, a
    /// non-finite one back at the default.
    pub fn clamped(self) -> Self {
        let (slowest, fastest) = (Self::STICK_SPEEDS[0], Self::STICK_SPEEDS[4]);
        Self {
            stick_speed: if self.stick_speed.is_finite() {
                self.stick_speed.clamp(slowest, fastest)
            } else {
                1.0
            },
            ..self
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
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent player preferences"
)]
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
    /// Controller feel.
    pub pad: PadPrefs,
    /// Window size at startup, `WIDTHxHEIGHT`.
    pub window: (u32, u32),
    /// Accessibility: damp decorative animation (alert pulses, ping
    /// rings, muzzle flashes). Informational motion — unit movement,
    /// shell arcs — always stays.
    pub reduced_motion: bool,
    /// Accessibility: one ownership color each for you, your allies and your
    /// enemies, in place of a color per seat (base sprite art is untouched).
    pub team_colors: bool,
    /// Show the control-group column above the minimap. Hiding it
    /// leaves keyboard groups working.
    pub control_groups: bool,
    /// Show short tips during a match when something needs the player's
    /// attention. Basic Training's first match shows them either way.
    pub match_tips: bool,
    /// Touch gesture timing.
    pub touch: TouchPrefs,
    /// The host address the last LAN join used.
    pub last_join_address: Option<String>,
    /// The name the player gave, which other players see and which
    /// replaces the player's seat name; unset until they give one, and
    /// [`Config::name`] stands in a default meanwhile.
    pub player_name: Option<String>,
    /// This installation's identifier as hex, sent only to hosts it joins
    /// so a resumed match can seat the player again. It also picks the
    /// default name, so the launch makes it up once outside automation.
    pub install_id: Option<String>,
    /// The Basic Training lessons this player has finished, in course
    /// order.
    pub lessons_done: Vec<crate::tutorial::Lesson>,
    /// Whether this player has finished Basic Training's first match,
    /// whatever the result.
    pub first_match_done: bool,
    /// Whether this install has shown the report before its first Home.
    pub cold_open_seen: bool,
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
            pad: PadPrefs::default(),
            window: (1280, 800),
            reduced_motion: false,
            team_colors: false,
            control_groups: true,
            match_tips: false,
            touch: TouchPrefs::default(),
            last_join_address: None,
            player_name: None,
            install_id: None,
            lessons_done: Vec::new(),
            first_match_done: false,
            cold_open_seen: false,
        }
    }
}

fn config_path() -> Option<PathBuf> {
    crate::paths::config_dir().map(|d| d.join("config.json"))
}

/// The user defaults key that holds the config on Apple TV.
const DEFAULTS_KEY: &str = "config";

impl Config {
    /// Loads the persisted config, or defaults on any trouble at all.
    pub fn load() -> Self {
        if PLATFORM == Platform::Tv {
            Self::from_text(aftermine_icloud::defaults::get(DEFAULTS_KEY).as_deref())
        } else {
            Self::load_from(config_path())
        }
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
        let text = path.and_then(|path| std::fs::read_to_string(path).ok());
        Self::from_text(text.as_deref())
    }

    /// The config `text` holds, or defaults when there is none, when it
    /// does not parse, or when another config version wrote it.
    fn from_text(text: Option<&str>) -> Self {
        let Some(text) = text else {
            return Self::default();
        };
        match serde_json::from_str::<Self>(text) {
            Ok(config) if config.version == CONFIG_VERSION => config.sanitized(),
            _ => Self::default(),
        }
    }

    /// Brings every field a hand edit, or another device, could have put
    /// out of range back into it, each on its own: one bad field never
    /// resets the others.
    fn sanitized(mut self) -> Self {
        if !self.bindings.valid() {
            self.bindings = BindingMap::classic();
        }
        self.bindings.keep_offered();
        self.window = Self::sane_window(self.window);
        self.ui_scale = Self::sane_ui_scale(self.ui_scale);
        self.touch = self.touch.clamped();
        self.pad = self.pad.clamped();
        self.markers = self.markers.clamped();
        self.volumes = self.volumes.clamped();
        // A name that no longer passes is forgotten.
        self.player_name = self
            .player_name
            .and_then(|name| aftermine_net::PlayerName::new(&name).ok())
            .map(String::from);
        self.install_id = self
            .install_id
            .filter(|id| aftermine_net::InstallId::from_hex(id).is_some());
        self.lessons_done = in_course_order(&self.lessons_done);
        self
    }

    /// The name the player goes by: the one they gave, or else this
    /// installation's default, so nothing waits on typing. The default is
    /// never stored as theirs, so it never syncs over a name given on
    /// another device.
    pub fn name(&self) -> aftermine_net::PlayerName {
        self.player_name
            .as_deref()
            .and_then(|name| aftermine_net::PlayerName::new(name).ok())
            .unwrap_or_else(|| self.default_name())
    }

    /// This installation's default name, picked by its install ID; the
    /// first one while it has none, as an automation run does until it
    /// joins a lobby.
    fn default_name(&self) -> aftermine_net::PlayerName {
        let pick = self
            .install_id
            .as_deref()
            .and_then(|hex| u128::from_str_radix(hex, 16).ok())
            .map_or(0, |bits| bits % DEFAULT_NAMES.len() as u128);
        let name = DEFAULT_NAMES[usize::try_from(pick).unwrap_or_default()];
        aftermine_net::PlayerName::new(name).expect("default names are valid")
    }

    /// Keeps `name` as the player's own unless it is the name they already
    /// go by, so confirming a default leaves it a default; whether it
    /// changed.
    pub fn set_name(&mut self, name: &aftermine_net::PlayerName) -> bool {
        if *name == self.name() {
            return false;
        }
        self.player_name = Some(name.to_string());
        true
    }

    /// This installation's identifier, made up on first use; the caller
    /// saves the config to keep a new one.
    pub fn install_id(&mut self) -> aftermine_net::InstallId {
        if let Some(id) = self
            .install_id
            .as_deref()
            .and_then(aftermine_net::InstallId::from_hex)
        {
            return id;
        }
        let id = aftermine_net::InstallId::new(crate::entropy::bits());
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

    /// Marks the first match finished; returns whether it was not already.
    pub fn finish_first_match(&mut self) -> bool {
        !std::mem::replace(&mut self.first_match_done, true)
    }

    /// Persists to user defaults on Apple TV, and elsewhere to the config
    /// file atomically (temp + rename), creating the directory.
    pub fn save(&self) -> std::io::Result<()> {
        if PLATFORM == Platform::Tv {
            aftermine_icloud::defaults::set(DEFAULTS_KEY, &self.text());
            return Ok(());
        }
        let Some(path) = config_path() else {
            return Ok(()); // headless CI without HOME: nothing to do
        };
        self.save_to(&path)
    }

    fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        let text = self.text();
        chassis::fsx::write_atomic(path, |writer| writer.write_all(text.as_bytes()))
    }

    fn text(&self) -> String {
        serde_json::to_string_pretty(self).expect("config serializes")
    }
}

/// Each lesson of `done` once, in course order.
fn in_course_order(done: &[crate::tutorial::Lesson]) -> Vec<crate::tutorial::Lesson> {
    crate::tutorial::Lesson::ALL
        .into_iter()
        .filter(|lesson| done.contains(lesson))
        .collect()
}

pub mod sync;

#[cfg(test)]
mod tests;
