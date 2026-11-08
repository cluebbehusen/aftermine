//! Mirrors this device's records through CloudKit, so Apple TV, which has no
//! iCloud Drive, shares saves with iPad and Mac. Each record file becomes
//! one CloudKit record named by its folder and file name. iPad and Mac
//! mirror their iCloud Drive folder, which carries Apple TV's records into
//! it; Apple TV mirrors its cache, which brings them back after tvOS clears
//! it.
//!
//! A manifest holds what this device last agreed with CloudKit: each name
//! with its file's size and modification time and the save time both sides
//! held. Against it, a pass tells an edit from a deletion on either side.
//! When both sides changed a record, the newer save wins; a named save
//! that collides with an unsynced one of the same name moves aside first,
//! so neither is lost. Only the save worker runs a pass, since every
//! CloudKit call waits on the network.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aftermine_icloud::cloudkit::{Changed, Changes};

/// How long one pass may spend moving files before it stops and leaves the
/// rest to the next one.
const BUDGET: Duration = Duration::from_secs(20);

/// A file's size and modification time: whether it changed since the
/// manifest saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Stamp {
    size: u64,
    secs: u64,
    nanos: u32,
}

impl Stamp {
    fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        let modified = metadata
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Some(Self {
            size: metadata.len(),
            secs: modified.as_secs(),
            nanos: modified.subsec_nanos(),
        })
    }
}

/// What both sides held for one record when they last agreed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Synced {
    stamp: Stamp,
    saved_at: u64,
}

/// What this device last agreed with CloudKit, and for which account and
/// folders: a different account or folder starts over.
#[derive(Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Manifest {
    account: String,
    folders: Vec<(String, PathBuf)>,
    token: Option<String>,
    synced: BTreeMap<String, Synced>,
}

/// One step a pass takes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Send the local file.
    Upload(String),
    /// Take the remote copy.
    Download(Changed),
    /// Both sides hold the same save: note the agreement, move nothing.
    Adopt(String, u64),
    /// A local named save of the same name, never synced, moves to a free
    /// name first; then the remote copy takes the name.
    MoveAsideThenDownload(Changed),
    /// Another device deleted it and this one never changed it.
    DeleteLocal(String),
    /// This device deleted it.
    DeleteRemote(String),
    /// Gone on both sides.
    Forget(String),
    /// The remote copy lost to a newer local one, which goes up instead.
    Keep(Changed),
}

/// How a local file stands against the manifest.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Local {
    /// Never agreed on.
    New,
    /// Unchanged since the agreement.
    Same,
    /// Rewritten with the same save, as a copy or download leaves it.
    Touched,
    /// A different save since the agreement.
    Edited,
}

/// What to do about each record, from the manifest, this device's files
/// and what CloudKit reported. `saved_at` reads a local file's save time,
/// and only runs for files that changed.
pub(crate) fn plan(
    synced: &BTreeMap<String, Synced>,
    local: &BTreeMap<String, Stamp>,
    changes: &Changes,
    saved_at: &mut impl FnMut(&str) -> Option<u64>,
) -> Vec<Action> {
    let mut times = Times {
        read: saved_at,
        known: BTreeMap::new(),
    };
    let standing = |name: &str, times: &mut Times<_>| -> Option<Local> {
        let stamp = local.get(name)?;
        Some(match synced.get(name) {
            None => Local::New,
            Some(entry) if entry.stamp == *stamp => Local::Same,
            Some(entry) if times.of(name) == Some(entry.saved_at) => Local::Touched,
            Some(_) => Local::Edited,
        })
    };
    let mut actions = Vec::new();
    let mut handled = std::collections::BTreeSet::new();
    for change in &changes.changed {
        handled.insert(change.name.clone());
        let agreed = synced.get(&change.name);
        let action = match standing(&change.name, &mut times) {
            None => match agreed {
                // Deleted here, but nothing changed there since: the
                // deletion goes up.
                Some(entry) if entry.saved_at == change.saved_at => {
                    actions.push(Action::Keep(change.clone()));
                    Action::DeleteRemote(change.name.clone())
                }
                // A change there outranks a deletion here.
                _ => Action::Download(change.clone()),
            },
            Some(Local::Same | Local::Touched) => {
                if agreed.is_some_and(|entry| entry.saved_at == change.saved_at) {
                    actions.push(Action::Keep(change.clone()));
                    Action::Adopt(change.name.clone(), change.saved_at)
                } else {
                    Action::Download(change.clone())
                }
            }
            Some(standing @ (Local::New | Local::Edited)) => {
                let mine = times.of(&change.name);
                if mine == Some(change.saved_at) {
                    actions.push(Action::Keep(change.clone()));
                    Action::Adopt(change.name.clone(), change.saved_at)
                } else if standing == Local::New && change.name.starts_with("saves/") {
                    Action::MoveAsideThenDownload(change.clone())
                } else if mine.is_some_and(|mine| mine > change.saved_at) {
                    actions.push(Action::Keep(change.clone()));
                    Action::Upload(change.name.clone())
                } else {
                    Action::Download(change.clone())
                }
            }
        };
        actions.push(action);
    }
    for name in &changes.deleted {
        if !handled.insert(name.clone()) {
            continue;
        }
        actions.push(match standing(name, &mut times) {
            None if synced.contains_key(name) => Action::Forget(name.clone()),
            None => continue,
            Some(Local::Same | Local::Touched) => Action::DeleteLocal(name.clone()),
            // A change here outranks a deletion there.
            Some(Local::New | Local::Edited) => Action::Upload(name.clone()),
        });
    }
    for name in local.keys() {
        if handled.contains(name) {
            continue;
        }
        match standing(name, &mut times) {
            Some(Local::New | Local::Edited) => actions.push(Action::Upload(name.clone())),
            Some(Local::Touched) => {
                if let Some(entry) = synced.get(name) {
                    actions.push(Action::Adopt(name.clone(), entry.saved_at));
                }
            }
            _ => {}
        }
    }
    for name in synced.keys() {
        if !handled.contains(name) && !local.contains_key(name) {
            actions.push(Action::DeleteRemote(name.clone()));
        }
    }
    actions
}

/// Local save times, each read at most once per plan.
struct Times<'a, F> {
    read: &'a mut F,
    known: BTreeMap<String, Option<u64>>,
}

impl<F: FnMut(&str) -> Option<u64>> Times<'_, F> {
    fn of(&mut self, name: &str) -> Option<u64> {
        if let Some(known) = self.known.get(name) {
            return *known;
        }
        let read = (self.read)(name);
        self.known.insert(name.to_owned(), read);
        read
    }
}

/// The CloudKit calls a pass makes, so tests can stand in for iCloud.
pub(crate) trait Remote {
    fn account(&self) -> Option<String>;
    fn fetch(&self, token: Option<&str>, staging: &Path) -> io::Result<Changes>;
    fn upload(&self, name: &str, path: &Path, saved_at: u64, size: u64) -> io::Result<()>;
    fn delete(&self, name: &str) -> io::Result<()>;
}

/// The app's iCloud account.
pub(crate) struct CloudKit;

impl Remote for CloudKit {
    fn account(&self) -> Option<String> {
        aftermine_icloud::cloudkit::account()
    }
    fn fetch(&self, token: Option<&str>, staging: &Path) -> io::Result<Changes> {
        aftermine_icloud::cloudkit::fetch(token, staging)
    }
    fn upload(&self, name: &str, path: &Path, saved_at: u64, size: u64) -> io::Result<()> {
        aftermine_icloud::cloudkit::upload(name, path, saved_at, size)
    }
    fn delete(&self, name: &str) -> io::Result<()> {
        aftermine_icloud::cloudkit::delete(name)
    }
}

/// How much of the mirror a pass covers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Both directions.
    Full,
    /// This device's changes only, with no files arriving: right after a
    /// save, while a match may still be running.
    Push,
}

/// How often Home and the shelf look for records other devices changed.
pub(crate) const EVERY: Duration = Duration::from_secs(30);

/// Runs a pass for this device against iCloud, reporting failures rather
/// than raising them: a mirror that cannot reach iCloud leaves records
/// where they are.
pub(crate) fn run(mode: Mode, cancelled: &impl Fn() -> bool) -> Outcome {
    let Some(places) = Places::here() else {
        return Outcome::default();
    };
    pass(&CloudKit, &places, mode, cancelled).unwrap_or_else(|error| {
        eprintln!("iCloud mirror: {error}");
        Outcome::default()
    })
}

/// What a pass left undone.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// Records from other devices still to arrive.
    pub pending: usize,
}

/// Where a pass keeps its state and finds records.
pub(crate) struct Places {
    pub manifest: PathBuf,
    pub staging: PathBuf,
    pub folders: Vec<(&'static str, PathBuf)>,
}

impl Places {
    /// This device's: its writing folders, and its data folder for the
    /// manifest and staging. `None` without a data folder.
    pub(crate) fn here() -> Option<Self> {
        Some(Self {
            manifest: crate::paths::mirror_manifest()?,
            staging: crate::paths::mirror_staging()?,
            folders: crate::paths::records().mirrored(),
        })
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        let (folder, file) = name.split_once('/')?;
        if file.is_empty() || file.contains('/') || file.starts_with('.') {
            return None;
        }
        self.folders
            .iter()
            .find(|(label, _)| *label == folder)
            .map(|(_, dir)| dir.join(file))
    }

    fn local(&self) -> BTreeMap<String, Stamp> {
        let mut found = BTreeMap::new();
        for (label, dir) in &self.folders {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
                let mirrored = matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("json" | "amreplay" | "amsave")
                );
                if let (true, Some(file), Some(stamp)) = (
                    mirrored,
                    path.file_name().and_then(|f| f.to_str()),
                    Stamp::of(&path),
                ) {
                    found.insert(format!("{label}/{file}"), stamp);
                }
            }
        }
        found
    }
}

/// When the record at `path` was saved, in Unix seconds: its own record of
/// it, or its modification time. `None` for a file that is not a record,
/// such as one still being written.
fn saved_at(path: &Path) -> Option<u64> {
    let info = crate::saved_game::inspect(path).ok()?;
    info.meta.saved_at.or_else(|| {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs())
    })
}

/// Runs one pass against `remote`: fetches what changed elsewhere unless
/// pushing, decides with [`plan`], and carries it out until `cancelled`
/// says stop or the time budget runs out. The token advances only once
/// every remote change it covers is applied, so a cut-short pass fetches
/// them again.
pub(crate) fn pass(
    remote: &impl Remote,
    places: &Places,
    mode: Mode,
    cancelled: &impl Fn() -> bool,
) -> io::Result<Outcome> {
    let Some(account) = remote.account() else {
        return Ok(Outcome::default());
    };
    let folders: Vec<(String, PathBuf)> = places
        .folders
        .iter()
        .map(|(label, dir)| ((*label).to_owned(), dir.clone()))
        .collect();
    let mut manifest = load(&places.manifest);
    if manifest.account != account || manifest.folders != folders {
        manifest = Manifest {
            account,
            folders,
            ..Manifest::default()
        };
    }
    std::fs::remove_dir_all(&places.staging).ok();
    std::fs::create_dir_all(&places.staging)?;
    let changes = match mode {
        Mode::Full => remote.fetch(manifest.token.as_deref(), &places.staging)?,
        Mode::Push => Changes::default(),
    };
    let local = places.local();
    let mut read = |name: &str| places.path(name).and_then(|path| saved_at(&path));
    let actions = plan(&manifest.synced, &local, &changes, &mut read);
    let started = Instant::now();
    let mut pending = 0;
    let mut complete = true;
    for action in actions {
        let out_of_time = cancelled() || started.elapsed() > BUDGET;
        if out_of_time {
            complete = false;
        }
        match apply(remote, places, &mut manifest, action, out_of_time) {
            Applied::Done => {}
            Applied::Skipped { arriving } => {
                complete &= !arriving;
                pending += usize::from(arriving);
            }
        }
    }
    if complete && mode == Mode::Full {
        manifest.token = changes.token;
        pending += usize::from(changes.more);
    } else if mode == Mode::Full {
        pending += usize::from(changes.more);
    }
    save(&places.manifest, &manifest)?;
    std::fs::remove_dir_all(&places.staging).ok();
    Ok(Outcome { pending })
}

enum Applied {
    Done,
    /// Left for a later pass; `arriving` when it was a file on its way in.
    Skipped {
        arriving: bool,
    },
}

fn apply(
    remote: &impl Remote,
    places: &Places,
    manifest: &mut Manifest,
    action: Action,
    out_of_time: bool,
) -> Applied {
    let arriving = matches!(
        action,
        Action::Download(_) | Action::MoveAsideThenDownload(_) | Action::DeleteLocal(_)
    );
    let skipped = Applied::Skipped { arriving };
    if out_of_time && !matches!(action, Action::Keep(_) | Action::Forget(_)) {
        return skipped;
    }
    let result = match action {
        Action::Upload(name) => upload(remote, places, manifest, &name),
        Action::Download(change) => download(places, manifest, &change),
        Action::Adopt(name, saved_at) => {
            if let Some(stamp) = places.path(&name).and_then(|path| Stamp::of(&path)) {
                manifest.synced.insert(name, Synced { stamp, saved_at });
            }
            Ok(())
        }
        Action::MoveAsideThenDownload(change) => {
            move_aside(places, &change.name).and_then(|()| download(places, manifest, &change))
        }
        Action::DeleteLocal(name) => delete_local(places, manifest, &name),
        Action::DeleteRemote(name) => remote.delete(&name).map(|()| {
            manifest.synced.remove(&name);
        }),
        Action::Forget(name) => {
            manifest.synced.remove(&name);
            Ok(())
        }
        Action::Keep(change) => {
            std::fs::remove_file(&change.staged).ok();
            Ok(())
        }
    };
    match result {
        Ok(()) => Applied::Done,
        Err(error) => {
            eprintln!("iCloud mirror: {error}");
            skipped
        }
    }
}

fn upload(
    remote: &impl Remote,
    places: &Places,
    manifest: &mut Manifest,
    name: &str,
) -> io::Result<()> {
    let path = places
        .path(name)
        .ok_or_else(|| io::Error::other(format!("no folder for {name}")))?;
    let stamp = Stamp::of(&path).ok_or_else(|| io::Error::other(format!("{name} vanished")))?;
    // A file still being written is not a record yet.
    let saved_at =
        saved_at(&path).ok_or_else(|| io::Error::other(format!("{name} is not a record yet")))?;
    // Upload a copy, so a save replacing the file meanwhile cannot tear it.
    let copy = places
        .staging
        .join(format!("upload-{}", name.replace('/', "-")));
    aftermine_icloud::coordinate(&path, aftermine_icloud::Access::Read, |granted| {
        std::fs::copy(granted, &copy)
    })??;
    let sent = remote.upload(name, &copy, saved_at, stamp.size);
    std::fs::remove_file(&copy).ok();
    sent?;
    manifest
        .synced
        .insert(name.to_owned(), Synced { stamp, saved_at });
    Ok(())
}

fn download(places: &Places, manifest: &mut Manifest, change: &Changed) -> io::Result<()> {
    // A record this device has no folder for, or a name no folder could
    // hold, is passed over rather than retried forever.
    let Some(path) = places.path(&change.name) else {
        std::fs::remove_file(&change.staged).ok();
        return Ok(());
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    aftermine_icloud::coordinate(&path, aftermine_icloud::Access::Replace, |granted| {
        chassis::fsx::write_atomic(granted, |writer| {
            io::copy(&mut std::fs::File::open(&change.staged)?, writer).map(|_| ())
        })?;
        // The save time stands in for when the file changed, so autosave
        // rotation, which goes by modification time, orders downloaded
        // records as it orders this device's own.
        let when = UNIX_EPOCH + Duration::from_secs(change.saved_at);
        std::fs::File::options()
            .write(true)
            .open(granted)?
            .set_modified(when.min(SystemTime::now()))
    })??;
    std::fs::remove_file(&change.staged).ok();
    let stamp = Stamp::of(&path).ok_or_else(|| io::Error::other("downloaded file vanished"))?;
    manifest.synced.insert(
        change.name.clone(),
        Synced {
            stamp,
            saved_at: change.saved_at,
        },
    );
    Ok(())
}

/// Renames the local file `name` to the first free `-n` name beside it.
fn move_aside(places: &Places, name: &str) -> io::Result<()> {
    let path = places
        .path(name)
        .ok_or_else(|| io::Error::other(format!("no folder for {name}")))?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("save")
        .to_owned();
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("amsave")
        .to_owned();
    let free = (1..1000)
        .map(|n| path.with_file_name(format!("{stem}-{n}.{extension}")))
        .find(|candidate| !candidate.exists())
        .ok_or_else(|| io::Error::other("no free name"))?;
    aftermine_icloud::coordinate(&path, aftermine_icloud::Access::Delete, |granted| {
        std::fs::rename(granted, &free)
    })?
}

fn delete_local(places: &Places, manifest: &mut Manifest, name: &str) -> io::Result<()> {
    if let Some(path) = places.path(name) {
        aftermine_icloud::coordinate(&path, aftermine_icloud::Access::Delete, |granted| {
            match std::fs::remove_file(granted) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            }
        })??;
    }
    manifest.synced.remove(name);
    Ok(())
}

fn load(path: &Path) -> Manifest {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(path: &Path, manifest: &Manifest) -> io::Result<()> {
    let text = serde_json::to_vec(manifest).map_err(io::Error::other)?;
    chassis::fsx::write_atomic(path, |writer| io::Write::write_all(writer, &text))
}

#[cfg(test)]
mod tests;
