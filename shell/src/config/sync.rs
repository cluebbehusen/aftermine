//! Which settings follow the player across their Apple devices, and how
//! this device's copy meets iCloud's key-value store.
//!
//! Each synced field is one store key holding the field's JSON. Per key the
//! device remembers its base: the text it last agreed with the store. A
//! merge compares the local value, the base and the store's value:
//!
//! - local differs from the base: the player changed it here, so it is
//!   uploaded, the newest write;
//! - otherwise a store value that differs from the base came from another
//!   device, and is adopted if it passes the same checks as a loaded
//!   `config.json`; one that does not is ignored;
//! - with no base yet, at launch, the store's value wins, and a key the
//!   store lacks keeps the local value without uploading it. A launch can
//!   look before iCloud has delivered what another device stored, so
//!   nothing goes up until the player changes a setting here; a fresh
//!   install can never write its defaults over the player's settings.
//!
//! The local config file stays the device's own copy, so a build that may
//! not use the store, or a device signed out of iCloud, behaves as before.

use super::{CONFIG_VERSION, Config};
use aftermine_icloud::KeyValueStore;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;

/// A setting that follows the player through iCloud, one per Settings row
/// so that edits to two rows on two devices both survive. Every other
/// config field stays on the device, because it belongs to the device's
/// screen, touch or pointer hardware, network or installation; the tests
/// list them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SyncedField {
    Bindings,
    MasterVolume,
    EffectsVolume,
    InterfaceVolume,
    MusicVolume,
    /// Marker start and end together: a timing choice sets both, and the
    /// two must stay apart.
    MarkerTiming,
    MarkerSize,
    ReducedMotion,
    TeamColors,
    ControlGroups,
    MatchTips,
    PlayerName,
}

impl SyncedField {
    const ALL: [Self; 12] = [
        Self::Bindings,
        Self::MasterVolume,
        Self::EffectsVolume,
        Self::InterfaceVolume,
        Self::MusicVolume,
        Self::MarkerTiming,
        Self::MarkerSize,
        Self::ReducedMotion,
        Self::TeamColors,
        Self::ControlGroups,
        Self::MatchTips,
        Self::PlayerName,
    ];

    /// Where the setting sits in `config.json`: the field's name, then the
    /// part of it this key carries.
    fn name(self) -> &'static str {
        match self {
            Self::Bindings => "bindings",
            Self::MasterVolume => "volumes.master",
            Self::EffectsVolume => "volumes.effects",
            Self::InterfaceVolume => "volumes.ui",
            Self::MusicVolume => "volumes.music",
            Self::MarkerTiming => "markers.timing",
            Self::MarkerSize => "markers.scale",
            Self::ReducedMotion => "reduced_motion",
            Self::TeamColors => "team_colors",
            Self::ControlGroups => "control_groups",
            Self::MatchTips => "match_tips",
            Self::PlayerName => "player_name",
        }
    }

    /// The store key, under this settings format, so a build with another
    /// format never reads it.
    fn key(self) -> String {
        format!("config.{CONFIG_VERSION}.{}", self.name())
    }

    /// The setting's JSON as this build writes it. Serializing the value's
    /// own type keeps `f32` text exact; a `serde_json::Value` would widen
    /// it to `f64` and make an unchanged volume look changed.
    fn json(self, config: &Config) -> String {
        let markers = &config.markers;
        let text = match self {
            Self::Bindings => serde_json::to_string(&config.bindings),
            Self::MasterVolume => serde_json::to_string(&config.volumes.master),
            Self::EffectsVolume => serde_json::to_string(&config.volumes.effects),
            Self::InterfaceVolume => serde_json::to_string(&config.volumes.ui),
            Self::MusicVolume => serde_json::to_string(&config.volumes.music),
            Self::MarkerTiming => serde_json::to_string(&(markers.start, markers.end)),
            Self::MarkerSize => serde_json::to_string(&markers.scale),
            Self::ReducedMotion => serde_json::to_string(&config.reduced_motion),
            Self::TeamColors => serde_json::to_string(&config.team_colors),
            Self::ControlGroups => serde_json::to_string(&config.control_groups),
            Self::MatchTips => serde_json::to_string(&config.match_tips),
            Self::PlayerName => serde_json::to_string(&config.player_name),
        };
        text.expect("settings serialize")
    }

    /// Sets the setting from `text`; whether it parsed.
    fn read(self, config: &mut Config, text: &str) -> bool {
        fn into<T: DeserializeOwned>(field: &mut T, text: &str) -> bool {
            serde_json::from_str(text)
                .map(|value| *field = value)
                .is_ok()
        }
        let markers = &mut config.markers;
        match self {
            Self::Bindings => into(&mut config.bindings, text),
            Self::MasterVolume => into(&mut config.volumes.master, text),
            Self::EffectsVolume => into(&mut config.volumes.effects, text),
            Self::InterfaceVolume => into(&mut config.volumes.ui, text),
            Self::MusicVolume => into(&mut config.volumes.music, text),
            Self::MarkerTiming => serde_json::from_str(text)
                .map(|(start, end)| (markers.start, markers.end) = (start, end))
                .is_ok(),
            Self::MarkerSize => into(&mut markers.scale, text),
            Self::ReducedMotion => into(&mut config.reduced_motion, text),
            Self::TeamColors => into(&mut config.team_colors, text),
            Self::ControlGroups => into(&mut config.control_groups, text),
            Self::MatchTips => into(&mut config.match_tips, text),
            Self::PlayerName => into(&mut config.player_name, text),
        }
    }
}

/// The text this device last agreed with the store, per synced field.
type Bases = BTreeMap<SyncedField, String>;

/// `config` with `field` taken from `text`, if `text` parses and the
/// load-time checks leave it as it is.
fn adopted(config: &Config, field: SyncedField, text: &str) -> Option<Config> {
    let mut candidate = config.clone();
    if !field.read(&mut candidate, text) {
        return None;
    }
    let read = field.json(&candidate);
    (field.json(&candidate.clone().sanitized()) == read).then_some(candidate)
}

/// What one merge decided.
#[derive(Debug, Default)]
struct Merge {
    /// Keys to write to the store, with their text.
    uploads: Vec<(SyncedField, String)>,
    /// Whether `config` took a value from the store.
    adopted: bool,
}

/// Merges every synced field of `config` with the store's text, which
/// `stored` reads, and records the agreement in `bases`.
fn merge(
    config: &mut Config,
    bases: &mut Bases,
    stored: impl Fn(SyncedField) -> Option<String>,
) -> Merge {
    let mut merge = Merge::default();
    for field in SyncedField::ALL {
        let local = field.json(config);
        match bases.get(&field).cloned() {
            // Changed here since the last agreement: the newest write.
            Some(base) if base != local => {
                bases.insert(field, local.clone());
                merge.uploads.push((field, local));
            }
            // Changed on another device since the last agreement.
            Some(base) => {
                if let Some(text) = stored(field)
                    && text != base
                    && let Some(next) = adopted(config, field, &text)
                {
                    merge.adopted |= field.json(&next) != local;
                    *config = next;
                    bases.insert(field, text);
                }
            }
            // The launch merge: the store's value wins, and where it has
            // none the local value stands without going up.
            None => {
                let taken = stored(field)
                    .and_then(|text| adopted(config, field, &text).map(|next| (text, next)));
                let agreed = if let Some((text, next)) = taken {
                    merge.adopted |= field.json(&next) != local;
                    *config = next;
                    text
                } else {
                    local
                };
                bases.insert(field, agreed);
            }
        }
    }
    merge
}

/// The synced fields changed on this device since the last agreement, now
/// recorded as agreed.
fn changed_here(config: &Config, bases: &mut Bases) -> Vec<(SyncedField, String)> {
    let mut uploads = Vec::new();
    for (&field, base) in bases.iter_mut() {
        let local = field.json(config);
        if *base != local {
            base.clone_from(&local);
            uploads.push((field, local));
        }
    }
    uploads
}

/// This device's side of the settings in iCloud.
#[derive(Debug)]
pub struct SettingsSync {
    /// The store, when this build may use it.
    store: Option<KeyValueStore>,
    /// The store's change count this device has caught up with.
    seen: u64,
    bases: Bases,
}

impl SettingsSync {
    /// Opens the store when `enabled` and runs the launch merge into
    /// `config`. Returns whether `config` changed.
    pub fn open(enabled: bool, config: &mut Config) -> (Self, bool) {
        let mut sync = Self {
            store: enabled.then(KeyValueStore::open).flatten(),
            seen: 0,
            bases: Bases::new(),
        };
        let changed = sync.catch_up(config);
        (sync, changed)
    }

    /// Takes in what other devices changed since the last look; whether
    /// `config` changed. Call it only where a changed setting cannot reach
    /// a match: on Home.
    pub fn pull(&mut self, config: &mut Config) -> bool {
        match &self.store {
            Some(store) if store.changes() != self.seen => self.catch_up(config),
            _ => false,
        }
    }

    /// Uploads the synced settings the player changed on this device.
    pub fn push(&mut self, config: &Config) {
        let Some(store) = &self.store else {
            return;
        };
        for (field, text) in changed_here(config, &mut self.bases) {
            store.set(&field.key(), &text);
        }
    }

    /// Merges the store's values into `config` and uploads what the merge
    /// decided; whether `config` changed.
    fn catch_up(&mut self, config: &mut Config) -> bool {
        let Some(store) = &self.store else {
            return false;
        };
        // Counted before reading, so a change that lands meanwhile is
        // taken in next time.
        self.seen = store.changes();
        let merge = merge(config, &mut self.bases, |field| store.get(&field.key()));
        for (field, text) in &merge.uploads {
            store.set(&field.key(), text);
        }
        merge.adopted
    }
}

#[cfg(test)]
mod tests;
