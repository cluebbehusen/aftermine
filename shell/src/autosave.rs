//! Atomic player checkpoints and finished-match recordings.
//!
//! Failure is a first-class outcome here: a quit path that cannot
//! write its record must be able to say so before the process exits,
//! which is why [`SaveJob::run`] reports [`SaveOutcome`] and [`SaveError`]
//! instead of a bool.

use crate::game::Game;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How many autosaves survive rotation.
const KEEP_AUTOSAVES: usize = 5;
/// How many finished-match records survive rotation. A decided match
/// is a keepsake, so it gets a wider shelf than live sessions.
const KEEP_MATCHES: usize = 20;
/// A temp older than this is an orphan from a crashed save, not a
/// write in flight.
const TEMP_ORPHAN_AGE: Duration = Duration::from_secs(3600);
/// Invalid-JSON ownership marker held at a destination until its record
/// atomically replaces it.
const RESERVATION_MARKER_PREFIX: &str = "oxide-save-reservation:";

/// What a successful [`SaveJob::run`] call actually did.
#[derive(Debug)]
pub enum SaveOutcome {
    /// A record landed at this path. Runtime quit flows need only the
    /// success; filesystem contract tests inspect the exact destination.
    #[cfg_attr(not(test), allow(dead_code))]
    Wrote(PathBuf),
    /// An unstarted game has nothing worth a file.
    NothingToSave,
    /// This session already wrote its record.
    AlreadySaved,
}

/// Why a save could not land — own-machine facts carrying the path
/// that refused, so the failure is diagnosable and reportable.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    /// No resolvable platform data directory (no HOME/APPDATA).
    #[error("no writable data directory")]
    NoDataDir,
    /// The autosave directory could not be created.
    #[error("could not create {path}: {source}")]
    CreateDir {
        /// The directory that refused.
        path: PathBuf,
        /// The underlying filesystem error.
        source: std::io::Error,
    },
    /// The record itself failed to write (disk full, permissions).
    #[error("could not write {path}: {source}")]
    Write {
        /// The record that failed.
        path: PathBuf,
        /// The underlying checkpoint or recording error.
        source: anyhow::Error,
    },
}

impl SaveError {
    /// One ASCII sentence for the toast strip and the failure dialog.
    pub fn player_line(&self) -> String {
        match self {
            SaveError::NoDataDir => {
                "could not save: no writable data folder is available".to_string()
            }
            SaveError::CreateDir { .. } => {
                "could not save: unable to create the save folder".to_string()
            }
            SaveError::Write { .. } => "could not save: unable to write the save file".to_string(),
        }
    }
}

/// Owns an exclusively-created path until an atomic write replaces its
/// marker. A failed write removes the marker, while an error reported
/// after rename leaves the published record intact.
struct PathReservation {
    path: PathBuf,
    marker: Vec<u8>,
    armed: bool,
}

impl PathReservation {
    fn publish<E>(
        mut self,
        write: impl FnOnce(&Path) -> Result<(), E>,
    ) -> Result<PathBuf, (PathBuf, E)> {
        match write(&self.path) {
            Ok(()) => {
                self.armed = false;
                Ok(self.path.clone())
            }
            Err(source) => Err((self.path.clone(), source)),
        }
    }
}

impl Drop for PathReservation {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let still_our_marker = std::fs::metadata(&self.path)
            .is_ok_and(|metadata| metadata.len() == self.marker.len() as u64)
            && std::fs::read(&self.path).is_ok_and(|contents| contents == self.marker);
        if still_our_marker {
            std::fs::remove_file(&self.path).ok();
        }
    }
}

/// A collision-free `{prefix}-{tick}` path in `dir` for a named save, reserved by
/// `create_new` (`O_EXCL`). Every successful return owns its candidate;
/// collision suffixes have no arbitrary cutoff that can bypass the
/// exclusive create.
fn free_path(dir: &Path, prefix: &str, tick: u64) -> Result<PathReservation, SaveError> {
    static RESERVATION_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut n = 0u64;
    loop {
        let path = if n == 0 {
            dir.join(format!("{prefix}-{tick:010}.oxsave"))
        } else {
            dir.join(format!("{prefix}-{tick:010}-{n}.oxsave"))
        };
        match std::fs::File::create_new(&path) {
            Ok(mut file) => {
                let nonce = RESERVATION_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let marker = format!(
                    "{RESERVATION_MARKER_PREFIX}{}:{nonce}\n",
                    std::process::id()
                )
                .into_bytes();
                if let Err(source) = file.write_all(&marker) {
                    drop(file);
                    std::fs::remove_file(&path).ok();
                    return Err(SaveError::Write {
                        path,
                        source: source.into(),
                    });
                }
                return Ok(PathReservation {
                    path,
                    marker,
                    armed: true,
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(SaveError::Write {
                    path,
                    source: source.into(),
                });
            }
        }
        n = n.checked_add(1).ok_or_else(|| SaveError::Write {
            path,
            source: std::io::Error::other("exhausted save path suffixes").into(),
        })?;
    }
}

/// Wall-clock provenance for record metadata — never consumed by any
/// sim path (the sim's ban is on state, not on metadata).
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// One match's place among the autosaves. Its autosave, LAN autosave and
/// finished record share the id, so each write replaces the last instead of
/// adding a file, and rotation keeps whole matches. The finished record
/// lands in the matches folder and removes the slot's autosaves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SaveSlot(String);

/// Autosave kinds; a slot holds at most one of them.
const AUTOSAVE_KINDS: [&str; 2] = ["autosave", "lan-autosave"];

impl SaveSlot {
    /// A new match's slot, distinct from every slot this process made.
    pub(crate) fn fresh() -> Self {
        static LAST: std::sync::Mutex<u64> = std::sync::Mutex::new(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_nanos()).unwrap_or(u64::MAX)
            });
        let mut last = LAST
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *last = now.max(last.saturating_add(1));
        Self(last.to_string())
    }

    /// The slot of the autosave at `path` when it lies in `autosaves`, so a
    /// continued match keeps overwriting the file it came from.
    pub(crate) fn adopt(path: &Path, autosaves: &Path) -> Option<Self> {
        if path.parent() != Some(autosaves) {
            return None;
        }
        let name = path.file_name()?.to_str()?;
        AUTOSAVE_KINDS
            .iter()
            .find_map(|kind| name.strip_prefix(kind)?.strip_prefix('-'))
            .and_then(|rest| rest.strip_suffix(".oxsave"))
            .filter(|id| !id.is_empty())
            .map(|id| Self(id.to_owned()))
    }

    fn file(&self, kind: &str) -> String {
        let extension = if kind == "match" {
            "oxreplay"
        } else {
            "oxsave"
        };
        format!("{kind}-{}.{extension}", self.0)
    }

    /// Removes this slot's autosaves in `dir` other than `kept`.
    fn clear_autosaves(&self, dir: &Path, kept: &str) {
        for kind in AUTOSAVE_KINDS.into_iter().filter(|kind| *kind != kept) {
            std::fs::remove_file(dir.join(self.file(kind))).ok();
        }
    }

    /// Removes this slot's autosaves from `dir`, for a match its player
    /// abandoned.
    fn discard(&self, dir: &Path) {
        self.clear_autosaves(dir, "");
    }
}

/// Deletes the autosaves of abandoned matches once the save worker is idle,
/// so a save still in flight cannot write a slot back after its deletion.
pub(crate) fn discard_when_idle(slots: &mut Vec<SaveSlot>, busy: bool, dir: Option<&Path>) {
    if busy || slots.is_empty() {
        return;
    }
    for slot in slots.drain(..) {
        if let Some(dir) = dir {
            slot.discard(dir);
        }
    }
}

/// What a save of a session holds: ticks only advance, and staged orders
/// only grow until a tick consumes them, so an equal key means nothing new.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SaveKey {
    tick: u64,
    pending: usize,
}

impl SaveKey {
    pub(crate) fn new(tick: u64, pending: usize) -> Self {
        Self { tick, pending }
    }
}

/// An unnamed save's slot and the content it wrote there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SaveMark {
    pub(crate) slot: SaveSlot,
    pub(crate) key: SaveKey,
}

/// Retention runs per record kind: live sessions and finished matches
/// each rotate against their own budget, so quick quits never evict
/// finished matches. Anything else in the directory, explicit saves
/// included, is never touched.
fn rotate(dir: &Path) {
    chassis::fsx::sweep_temps(dir, TEMP_ORPHAN_AGE);
    rotate_prefix(dir, "autosave-", KEEP_AUTOSAVES);
    rotate_prefix(dir, "lan-autosave-", KEEP_AUTOSAVES);
    rotate_prefix(dir, "match-", KEEP_MATCHES);
}

fn rotate_prefix(dir: &Path, prefix: &str, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str())
                == Some(if prefix == "match-" {
                    "oxreplay"
                } else {
                    "oxsave"
                })
        })
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix))
        })
        .collect();
    // Newest last by modification time; ties by name for determinism.
    files.sort_by_key(|p| {
        (
            std::fs::metadata(p).and_then(|m| m.modified()).ok(),
            p.clone(),
        )
    });
    while files.len() > keep {
        let oldest = files.remove(0);
        std::fs::remove_file(oldest).ok();
    }
}

/// Header-eligible autosaves Continue may resume, newest first. Loading
/// must still validate the payload.
pub(crate) fn candidates() -> Vec<PathBuf> {
    let entries = crate::saves::discover(|| false);
    crate::saves::continuable(&entries)
        .map(|entry| entry.path.clone())
        .collect()
}

pub(crate) enum SaveData {
    Checkpoint(crate::game::checkpoint::SaveCapture),
    /// A finished match's record and the keyframes kept beside it, whose
    /// encoding the worker waits out, as it does a resumed match's history
    /// check.
    Recording(
        oxide_kit::GameReplay,
        oxide_kit::keyframes::PendingKeyframes,
        Option<Box<oxide_kit::history::PendingHistory>>,
    ),
}

pub(crate) struct SaveJob {
    data: Option<SaveData>,
    meta: chassis::replay::ReplayMeta,
    dir: Option<PathBuf>,
    /// Where the match's autosaves live, which an unnamed save replaces.
    autosaves: Option<PathBuf>,
    /// Where an unnamed save lands and what it holds; a named save gets a
    /// fresh file in the saves folder instead.
    mark: Option<SaveMark>,
    already_saved: bool,
}

impl SaveJob {
    pub(crate) fn capture(game: &Game, name: Option<&str>) -> Self {
        let named = name.is_some();
        debug_assert!(
            !(named && game.net_role().is_some()),
            "LAN matches offer no named save"
        );
        debug_assert!(
            !(named && game.tutorial.is_some()),
            "lessons offer no named save"
        );
        let already_saved = game.is_saved();
        // A lesson restarts in seconds, so it is never saved.
        let needed = game.tutorial.is_none()
            && (named
                || (!already_saved
                    && (game.state.current_tick() != 0 || !game.pending.is_empty())));
        // A LAN match cannot resume on one machine, so it keeps the
        // watch-only record a decided match keeps.
        let recording = game.state.result().is_some() || game.net_role().is_some();
        let mut meta = game.recorder.meta.clone();
        meta.ticks = Some(game.state.current_tick());
        meta.saved_at = Some(now_unix());
        meta.kind = Some(
            if named {
                "save"
            } else if recording {
                "match"
            } else {
                "autosave"
            }
            .into(),
        );
        if let Some(name) = name {
            meta.description = Some(name.into());
        }
        let data = needed.then(|| {
            if !named && recording {
                let mut replay = game.recorder.clone();
                replay.meta = meta.clone();
                SaveData::Recording(
                    replay,
                    game.keyframes.pending(),
                    game.history.clone().map(Box::new),
                )
            } else {
                SaveData::Checkpoint(game.capture_save())
            }
        });
        Self {
            data,
            meta,
            dir: if named {
                crate::paths::saves_dir()
            } else if recording {
                crate::paths::matches_dir()
            } else {
                crate::paths::autosave_dir()
            },
            autosaves: crate::paths::autosave_dir(),
            mark: (!named).then(|| SaveMark {
                slot: game.slot.clone(),
                key: game.save_key(),
            }),
            already_saved,
        }
    }

    /// A LAN host's save of its running match, `capture` taken from its
    /// link: by `name`, or among the autosaves when unnamed.
    pub(crate) fn lan(
        game: &Game,
        capture: crate::game::checkpoint::SaveCapture,
        name: Option<&str>,
    ) -> Self {
        let named = name.is_some();
        let mut meta = game.recorder.meta.clone();
        meta.ticks = Some(game.state.current_tick());
        meta.saved_at = Some(now_unix());
        meta.kind = Some(if named { "lan" } else { "lan-autosave" }.into());
        meta.description = name.map(Into::into);
        Self {
            data: Some(SaveData::Checkpoint(capture)),
            meta,
            dir: if named {
                crate::paths::saves_dir()
            } else {
                crate::paths::autosave_dir()
            },
            autosaves: crate::paths::autosave_dir(),
            mark: (!named).then(|| SaveMark {
                slot: game.slot.clone(),
                key: game.save_key(),
            }),
            already_saved: false,
        }
    }

    /// The slot and content an unnamed save leaves behind.
    pub(crate) fn mark(&self) -> Option<SaveMark> {
        self.mark.clone()
    }

    pub(crate) fn run(self) -> Result<SaveOutcome, SaveError> {
        let outcome = if let Some(data) = self.data {
            let dir = self.dir.ok_or(SaveError::NoDataDir)?;
            std::fs::create_dir_all(&dir).map_err(|source| SaveError::CreateDir {
                path: dir.clone(),
                source,
            })?;
            let kind = self.meta.kind.as_deref().unwrap();
            let meta = self.meta.clone();
            let write = |path: &Path| match data {
                SaveData::Checkpoint(capture) => {
                    crate::saved_game::write_capture(capture, meta, path)
                }
                SaveData::Recording(mut replay, mut keyframes, history) => {
                    if let Some(history) = history {
                        history.wait(&mut replay, &mut keyframes);
                    }
                    oxide_kit::save_recording(path, &replay, &keyframes.wait()).map_err(Into::into)
                }
            };
            let path = if let Some(mark) = &self.mark {
                let path = dir.join(mark.slot.file(kind));
                write(&path).map_err(|source| SaveError::Write {
                    path: path.clone(),
                    source,
                })?;
                if let Some(autosaves) = &self.autosaves {
                    mark.slot.clear_autosaves(autosaves, kind);
                }
                rotate(&dir);
                path
            } else {
                free_path(&dir, kind, self.meta.ticks.unwrap())?
                    .publish(write)
                    .map_err(|(path, source)| SaveError::Write { path, source })?
            };
            SaveOutcome::Wrote(path)
        } else if self.already_saved {
            SaveOutcome::AlreadySaved
        } else {
            SaveOutcome::NothingToSave
        };
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests;
