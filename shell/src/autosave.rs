//! Atomic player checkpoints and finished-match recordings.
//!
//! Failure is a first-class outcome here: a quit path that cannot
//! write its record must be able to say so before the process exits,
//! which is why [`SaveJob::run`] reports a [`SaveError`] instead of a bool.

use crate::game::Game;
use crate::saves::RecordKind;
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
            SaveError::NoDataDir => "No writable data folder is available".to_string(),
            SaveError::CreateDir { .. } => "Unable to create the save folder".to_string(),
            SaveError::Write { .. } => "Unable to write the save file".to_string(),
        }
    }
}

/// Writes a named save to the first free `{prefix}-{tick}-{time}` path in
/// `dir`. The time keeps two devices saving at one tick from choosing a name
/// that means different saves. Each candidate is checked and written within
/// one coordinated access, so no coordinated writer takes the name between
/// the two; named saves run one at a time on the save worker. Collision
/// suffixes have no cutoff.
fn publish_named(
    dir: &Path,
    kind: RecordKind,
    tick: u64,
    write: impl FnOnce(&Path) -> anyhow::Result<()>,
) -> Result<PathBuf, SaveError> {
    let mut write = Some(write);
    let time = now_unix();
    let mut n = 0u64;
    loop {
        let (key, extension) = (kind.key(), kind.extension());
        let path = if n == 0 {
            dir.join(format!("{key}-{tick:010}-{time}.{extension}"))
        } else {
            dir.join(format!("{key}-{tick:010}-{time}-{n}.{extension}"))
        };
        let published = aftermine_apple_bridge::coordinate(
            &path,
            aftermine_apple_bridge::Access::Replace,
            |granted| match std::fs::symlink_metadata(granted) {
                Ok(_) => None,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let write = write.take().expect("only a free path is written");
                    Some(write(granted).map(|()| granted.to_owned()))
                }
                Err(error) => Some(Err(error.into())),
            },
        )
        .map_err(|source| SaveError::Write {
            path: path.clone(),
            source: source.into(),
        })?;
        if let Some(published) = published {
            return published.map_err(|source| SaveError::Write { path, source });
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
/// lands in the matches folder and removes the slot's autosaves, as a local
/// match's named save does from the saves folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SaveSlot(String);

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

    /// The slot of the autosave at `path` when it lies in one of the
    /// `autosaves` folders, so a continued match keeps its slot and replaces
    /// the file it came from.
    pub(crate) fn adopt(path: &Path, autosaves: &[PathBuf]) -> Option<Self> {
        let folder = path.parent()?;
        if !autosaves.iter().any(|autosaves| autosaves == folder) {
            return None;
        }
        let name = path.file_name()?.to_str()?;
        RecordKind::AUTOSAVES
            .into_iter()
            .find_map(|kind| {
                name.strip_prefix(kind.key())?
                    .strip_prefix('-')?
                    .strip_suffix(kind.extension())?
                    .strip_suffix('.')
            })
            .filter(|id| !id.is_empty())
            .map(|id| Self(id.to_owned()))
    }

    fn file(&self, kind: RecordKind) -> String {
        format!("{}-{}.{}", kind.key(), self.0, kind.extension())
    }

    /// Removes this slot's autosaves in `dir` other than one of `kept`.
    fn clear_autosaves(&self, dir: &Path, kept: Option<RecordKind>) {
        for kind in RecordKind::AUTOSAVES
            .into_iter()
            .filter(|kind| Some(*kind) != kept)
        {
            remove(&dir.join(self.file(kind)));
        }
    }

    /// Removes this slot's autosaves from `dir`, for a match its player
    /// abandoned or saved by name, or one whose autosave moved to another
    /// folder.
    fn discard(&self, dir: &Path) {
        self.clear_autosaves(dir, None);
    }
}

/// Deletes the autosaves of abandoned matches. The save worker runs it, so
/// a save still in flight cannot write a slot back after its deletion.
pub(crate) fn discard(slots: &[SaveSlot]) {
    discard_in(slots, &crate::paths::records().autosave_folders());
}

fn discard_in(slots: &[SaveSlot], folders: &[PathBuf]) {
    for folder in folders {
        for slot in slots {
            slot.discard(folder);
        }
    }
}

/// Removes a record while iCloud holds off syncing it; a missing file is
/// already gone.
pub(crate) fn remove(path: &Path) {
    crate::cloud_mirror::deleting(path);
    aftermine_apple_bridge::coordinate(path, aftermine_apple_bridge::Access::Delete, |granted| {
        std::fs::remove_file(granted)
    })
    .ok();
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

/// A match's slot and the content a save of it holds.
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
    rotate_kind(dir, RecordKind::Autosave, KEEP_AUTOSAVES);
    rotate_kind(dir, RecordKind::LanAutosave, KEEP_AUTOSAVES);
    rotate_kind(dir, RecordKind::Match, KEEP_MATCHES);
}

fn rotate_kind(dir: &Path, kind: RecordKind, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let prefix = format!("{}-", kind.key());
    let mut files: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(kind.extension()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix))
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
        remove(&files.remove(0));
    }
}

/// Header-eligible records Continue may resume, newest first. Loading
/// must still validate the payload.
pub(crate) fn candidates() -> Vec<PathBuf> {
    crate::saves::discover_arrived(|| false)
        .continuable()
        .map(|entry| entry.path.clone())
        .collect()
}

pub(crate) enum SaveData {
    Checkpoint(crate::game::checkpoint::SaveCapture),
    /// A finished match's record and the keyframes kept beside it, whose
    /// encoding the worker waits out, as it does a resumed match's history
    /// check.
    Recording(
        aftermine_kit::GameReplay,
        aftermine_kit::keyframes::PendingKeyframes,
        Option<Box<aftermine_kit::history::PendingHistory>>,
    ),
}

/// Writes `path` whole while iCloud holds off syncing it.
fn replace(path: &Path, write: impl FnOnce(&Path) -> anyhow::Result<()>) -> anyhow::Result<()> {
    aftermine_apple_bridge::coordinate(path, aftermine_apple_bridge::Access::Replace, write)?
}

pub(crate) struct SaveJob {
    data: Option<SaveData>,
    meta: chassis::replay::ReplayMeta,
    kind: RecordKind,
    /// The match's slot and what this save holds. An unnamed save lands in
    /// the slot; a named save gets a fresh file in the saves folder and
    /// removes the slot's autosaves if it has a mark.
    mark: Option<SaveMark>,
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
        // A lesson restarts in seconds, so it is never saved.
        let needed = game.tutorial.is_none()
            && (named
                || (!game.is_saved()
                    && (game.state.current_tick() != 0 || !game.pending.is_empty())));
        // A LAN match resumes only through its host's LAN save
        // (`SaveJob::lan`), so any other save of one keeps the watch-only
        // record a decided match keeps.
        let recording = game.state.result().is_some() || game.net_role().is_some();
        let kind = if named {
            RecordKind::Save
        } else if recording {
            RecordKind::Match
        } else {
            RecordKind::Autosave
        };
        let mut meta = game.recorder.meta.clone();
        meta.ticks = Some(game.state.current_tick());
        meta.saved_at = Some(now_unix());
        meta.kind = Some(kind.key().into());
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
            kind,
            // A named save stands in for the match's autosave, but not for
            // the recording a decided or LAN match still owes.
            mark: (!(named && recording)).then(|| SaveMark {
                slot: game.slot.clone(),
                key: game.save_key(),
            }),
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
        let kind = if named {
            RecordKind::LanSave
        } else {
            RecordKind::LanAutosave
        };
        let mut meta = game.recorder.meta.clone();
        meta.ticks = Some(game.state.current_tick());
        meta.saved_at = Some(now_unix());
        meta.kind = Some(kind.key().into());
        meta.description = name.map(Into::into);
        Self {
            data: Some(SaveData::Checkpoint(capture)),
            meta,
            kind,
            mark: (!named).then(|| SaveMark {
                slot: game.slot.clone(),
                key: game.save_key(),
            }),
        }
    }

    /// The slot and content the match counts as saved once this lands.
    pub(crate) fn mark(&self) -> Option<SaveMark> {
        self.mark.clone()
    }

    /// Writes the record, returning where it landed; an unstarted game, or
    /// one whose record is already written, writes nothing. Where it lands
    /// may wait on the first iCloud lookup, so the save worker runs this.
    pub(crate) fn run(self) -> Result<Option<PathBuf>, SaveError> {
        let records = crate::paths::records();
        let dir = self.kind.folder(&records);
        self.run_in(dir, &records.autosave_folders())
    }

    /// Writes the record into `dir`, replacing the match's autosaves in
    /// every `autosaves` folder.
    pub(crate) fn run_in(
        self,
        dir: Option<PathBuf>,
        autosaves: &[PathBuf],
    ) -> Result<Option<PathBuf>, SaveError> {
        let Some(data) = self.data else {
            return Ok(None);
        };
        let dir = dir.ok_or(SaveError::NoDataDir)?;
        std::fs::create_dir_all(&dir).map_err(|source| SaveError::CreateDir {
            path: dir.clone(),
            source,
        })?;
        let kind = self.kind;
        let meta = self.meta.clone();
        let write = |path: &Path| match data {
            SaveData::Checkpoint(capture) => crate::saved_game::write_capture(capture, meta, path),
            SaveData::Recording(mut replay, mut keyframes, history) => {
                if let Some(history) = history {
                    history.wait(&mut replay, &mut keyframes);
                }
                aftermine_kit::save_recording(path, &replay, &keyframes.wait()).map_err(Into::into)
            }
        };
        let path = match &self.mark {
            Some(mark) if !kind.named() => {
                let path = dir.join(mark.slot.file(kind));
                replace(&path, write).map_err(|source| SaveError::Write {
                    path: path.clone(),
                    source,
                })?;
                for folder in autosaves {
                    if *folder == dir {
                        mark.slot.clear_autosaves(folder, Some(kind));
                    } else {
                        mark.slot.discard(folder);
                    }
                }
                rotate(&dir);
                path
            }
            mark => {
                let path = publish_named(&dir, kind, self.meta.ticks.unwrap(), write)?;
                if let Some(mark) = mark {
                    for folder in autosaves {
                        mark.slot.discard(folder);
                    }
                }
                path
            }
        };
        Ok(Some(path))
    }
}

#[cfg(test)]
mod tests;
