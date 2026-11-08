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
//! When both sides changed a record, the newer save wins, and an older copy
//! that lands over a newer one, as when two uploads cross, goes up again.
//! A named save that collides with an unsynced one of the same name moves
//! aside first, so neither is lost.
//!
//! A missing file counts as deleted, except where the system removes files
//! on its own, as tvOS clears its cache: there only a deletion the game made
//! goes up, and a file that vanished is fetched again. A file iCloud Drive
//! has not brought down yet is left alone until it arrives.
//!
//! Passes run on a thread of their own, since every CloudKit call waits on
//! the network and must never hold up a load or a save.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
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
    /// Synced records the game deleted here, until the deletion goes up or
    /// another device's copy replaces them.
    deleted: BTreeSet<String>,
}

impl Manifest {
    /// Notes the game's deletions of synced records among `paths`; whether
    /// any was new.
    fn note_deleted(&mut self, places: &Places, paths: &[PathBuf]) -> bool {
        let mut noted = false;
        for name in paths.iter().filter_map(|path| places.name_of(path)) {
            if self.synced.contains_key(&name) {
                noted |= self.deleted.insert(name);
            }
        }
        noted
    }

    /// Records `name` as gone from both sides.
    fn forget(&mut self, name: &str) {
        self.synced.remove(name);
        self.deleted.remove(name);
    }
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
    /// The remote copy lost to a newer local one, which goes up instead, or
    /// waits for a later pass.
    Keep(Changed),
    /// iCloud Drive is still bringing the file here: leave it, and fetch
    /// its change again once it has arrived.
    Wait(String),
    /// The file vanished without the game deleting it, as when tvOS clears
    /// cached files: forget it and fetch every record again, which brings
    /// it back.
    Refetch(String),
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

/// What this device holds, as a pass finds it.
pub(crate) struct Here<'a> {
    /// Each record file and its stamp.
    pub local: &'a BTreeMap<String, Stamp>,
    /// Records iCloud Drive holds for this device but has not brought down.
    pub arriving: &'a BTreeSet<String>,
    /// Synced records the game deleted here.
    pub deleted: &'a BTreeSet<String>,
    /// Whether the system removes files on its own, so that a missing file
    /// counts as deleted only when the game deleted it.
    pub purgeable: bool,
}

impl Here<'_> {
    /// Whether a synced record missing here was deleted, rather than
    /// removed by the system.
    fn deleted_here(&self, name: &str) -> bool {
        !self.purgeable || self.deleted.contains(name)
    }
}

/// What to do about each record, from the manifest, this device's files
/// and what CloudKit reported. `saved_at` reads a local file's save time,
/// and only runs for files that changed.
pub(crate) fn plan(
    synced: &BTreeMap<String, Synced>,
    here: &Here,
    changes: &Changes,
    saved_at: &mut impl FnMut(&str) -> Option<u64>,
) -> Vec<Action> {
    let mut times = Times {
        read: saved_at,
        known: BTreeMap::new(),
    };
    let standing = |name: &str, times: &mut Times<_>| -> Option<Local> {
        let stamp = here.local.get(name)?;
        Some(match synced.get(name) {
            None => Local::New,
            Some(entry) if entry.stamp == *stamp => Local::Same,
            Some(entry) if times.of(name) == Some(entry.saved_at) => Local::Touched,
            Some(_) => Local::Edited,
        })
    };
    let mut actions = Vec::new();
    let mut handled = BTreeSet::new();
    for change in &changes.changed {
        handled.insert(change.name.clone());
        if here.arriving.contains(&change.name) {
            actions.push(Action::Keep(change.clone()));
            actions.push(Action::Wait(change.name.clone()));
            continue;
        }
        let agreed = synced.get(&change.name);
        let action = match standing(&change.name, &mut times) {
            None => match agreed {
                // Deleted here, and nothing newer there: the deletion goes
                // up.
                Some(entry)
                    if here.deleted_here(&change.name) && change.saved_at <= entry.saved_at =>
                {
                    actions.push(Action::Keep(change.clone()));
                    Action::DeleteRemote(change.name.clone())
                }
                // A change there outranks a deletion here, and a file the
                // system removed comes back.
                _ => Action::Download(change.clone()),
            },
            Some(Local::Same | Local::Touched) => {
                match agreed.map_or(Ordering::Less, |entry| entry.saved_at.cmp(&change.saved_at)) {
                    Ordering::Equal => {
                        actions.push(Action::Keep(change.clone()));
                        Action::Adopt(change.name.clone(), change.saved_at)
                    }
                    // An older copy landed over the one this device sent,
                    // as when two uploads cross: this one goes up again.
                    Ordering::Greater => {
                        actions.push(Action::Keep(change.clone()));
                        Action::Upload(change.name.clone())
                    }
                    Ordering::Less => Action::Download(change.clone()),
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
        if here.arriving.contains(name) {
            actions.push(Action::Wait(name.clone()));
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
    for name in here.local.keys() {
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
        if handled.contains(name) || here.local.contains_key(name) || here.arriving.contains(name) {
            continue;
        }
        actions.push(if here.deleted_here(name) {
            Action::DeleteRemote(name.clone())
        } else {
            Action::Refetch(name.clone())
        });
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Both directions.
    Full,
    /// This device's changes only, with no files arriving: right after a
    /// save, while a match may still be running.
    Push,
}

/// How often Home and the shelf look for records other devices changed.
pub(crate) const EVERY: Duration = Duration::from_secs(30);

/// The mirror's thread, once started, and what it is asked to do.
static MIRROR: OnceLock<Shared> = OnceLock::new();

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    /// Records the last pass left on their way to this device.
    pending: AtomicUsize,
    /// How many passes added, replaced or removed a local file.
    changes: AtomicU64,
}

#[derive(Default)]
struct Queue {
    /// The pass wanted next.
    wanted: Option<Mode>,
    /// Records the game deleted since a pass last took them.
    deleted: Vec<PathBuf>,
}

impl Shared {
    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Asks for a pass; a full pass covers a push.
    fn want(&self, mode: Mode) {
        let mut queue = self.queue();
        if queue.wanted != Some(Mode::Full) {
            queue.wanted = Some(mode);
        }
        self.wake.notify_one();
    }
}

/// Starts the mirror's thread with a full pass. Only the first call does
/// anything.
pub(crate) fn start() {
    let mut first = false;
    let shared = MIRROR.get_or_init(|| {
        first = true;
        Shared::default()
    });
    if !first {
        return;
    }
    shared.want(Mode::Full);
    if let Err(error) = std::thread::Builder::new()
        .name("cloud-mirror".into())
        .spawn(|| serve(shared))
    {
        eprintln!("iCloud mirror: {error}");
    }
}

/// Asks the mirror for a pass, if it runs.
pub(crate) fn request(mode: Mode) {
    if let Some(shared) = MIRROR.get() {
        shared.want(mode);
    }
}

/// Tells the mirror the game is about to delete the record at `path`, so
/// the deletion goes up even where a missing file alone would not count.
/// Called before the file goes, so a pass never finds it missing unnoted.
pub(crate) fn deleting(path: &Path) {
    if let Some(shared) = MIRROR.get() {
        shared.queue().deleted.push(path.to_owned());
    }
}

/// Records the last pass left on their way to this device.
pub(crate) fn pending() -> usize {
    MIRROR.get().map_or(0, |shared| {
        shared.pending.load(std::sync::atomic::Ordering::Relaxed)
    })
}

/// How many passes added, replaced or removed a local file. A catalog
/// scanned before a change may be out of date.
pub(crate) fn changes() -> u64 {
    MIRROR.get().map_or(0, |shared| {
        shared.changes.load(std::sync::atomic::Ordering::Acquire)
    })
}

/// Runs the passes asked for, one at a time, reporting failures rather
/// than raising them: a mirror that cannot reach iCloud leaves records
/// where they are.
fn serve(shared: &'static Shared) {
    loop {
        let mode = {
            let mut queue = shared.queue();
            loop {
                if let Some(mode) = queue.wanted.take() {
                    break mode;
                }
                queue = shared
                    .wake
                    .wait(queue)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        let Some(places) = Places::here() else {
            continue;
        };
        let taken = || std::mem::take(&mut shared.queue().deleted);
        let outcome = pass(&CloudKit, &places, mode, taken).unwrap_or_else(|error| {
            eprintln!("iCloud mirror: {error}");
            Outcome::default()
        });
        shared
            .pending
            .store(outcome.pending, std::sync::atomic::Ordering::Relaxed);
        if outcome.changed {
            shared
                .changes
                .fetch_add(1, std::sync::atomic::Ordering::Release);
        }
        if outcome.again {
            shared.want(Mode::Full);
        }
    }
}

/// What a pass did and left undone.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// Records from other devices still to arrive.
    pub pending: usize,
    /// Whether it added, replaced or removed a local file.
    pub changed: bool,
    /// Whether another pass should follow at once: more changes wait, the
    /// pass ran out of time, or every record must be fetched again.
    pub again: bool,
}

/// Where a pass keeps its state and finds records.
pub(crate) struct Places {
    pub manifest: PathBuf,
    pub staging: PathBuf,
    pub folders: Vec<(&'static str, PathBuf)>,
    /// Whether the system may remove record files on its own.
    pub purgeable: bool,
}

impl Places {
    /// This device's: its writing folders, and its data folder for the
    /// manifest and staging. `None` without a data folder.
    pub(crate) fn here() -> Option<Self> {
        Some(Self {
            manifest: crate::paths::mirror_manifest()?,
            staging: crate::paths::mirror_staging()?,
            folders: crate::paths::records().mirrored(),
            purgeable: crate::paths::records_purgeable(),
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

    /// The record name of the file at `path`, if a mirrored folder holds it.
    fn name_of(&self, path: &Path) -> Option<String> {
        let dir = path.parent()?;
        let file = path.file_name()?.to_str()?;
        let (label, _) = self.folders.iter().find(|(_, folder)| folder == dir)?;
        Some(format!("{label}/{file}"))
    }

    /// Each record file with its stamp, and the records iCloud Drive has
    /// not brought down yet.
    fn local(&self) -> (BTreeMap<String, Stamp>, BTreeSet<String>) {
        let mut found = BTreeMap::new();
        let mut arriving = BTreeSet::new();
        for (label, dir) in &self.folders {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
                let Some(file) = path.file_name().and_then(|f| f.to_str()) else {
                    continue;
                };
                // iOS stands a hidden `.name.icloud` file in for a file
                // iCloud Drive has not brought down.
                if let Some(real) = file
                    .strip_prefix('.')
                    .and_then(|file| file.strip_suffix(".icloud"))
                {
                    if mirrored(real) {
                        arriving.insert(format!("{label}/{real}"));
                    }
                } else if !file.starts_with('.')
                    && mirrored(file)
                    && let Some(stamp) = Stamp::of(&path)
                {
                    found.insert(format!("{label}/{file}"), stamp);
                }
            }
        }
        (found, arriving)
    }
}

/// Whether a file of this name is a record the mirror carries.
fn mirrored(file: &str) -> bool {
    matches!(
        Path::new(file).extension().and_then(|e| e.to_str()),
        Some("json" | "amreplay" | "amsave")
    )
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
/// pushing, takes the game's deletions from `deleted`, decides with
/// [`plan`], and carries it out until the time budget runs out. The token
/// advances only once every remote change it covers is applied, so a
/// cut-short pass fetches them again.
pub(crate) fn pass(
    remote: &impl Remote,
    places: &Places,
    mode: Mode,
    deleted: impl FnOnce() -> Vec<PathBuf>,
) -> io::Result<Outcome> {
    let mut manifest = load(&places.manifest);
    let Some(account) = remote.account() else {
        // Deletions made while signed out still go up once signed in.
        if manifest.note_deleted(places, &deleted()) {
            save(&places.manifest, &manifest)?;
        }
        return Ok(Outcome::default());
    };
    let folders: Vec<(String, PathBuf)> = places
        .folders
        .iter()
        .map(|(label, dir)| ((*label).to_owned(), dir.clone()))
        .collect();
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
    // The game notes a deletion before the file goes, and the notes are
    // taken after the scan, so every file the scan finds missing that the
    // game deleted has its note.
    let (local, arriving) = places.local();
    manifest.note_deleted(places, &deleted());
    let mut read = |name: &str| places.path(name).and_then(|path| saved_at(&path));
    let here = Here {
        local: &local,
        arriving: &arriving,
        deleted: &manifest.deleted,
        purgeable: places.purgeable,
    };
    let actions = plan(&manifest.synced, &here, &changes, &mut read);
    let incoming: BTreeSet<String> = changes
        .changed
        .iter()
        .map(|change| change.name.clone())
        .collect();
    let started = Instant::now();
    let mut outcome = Outcome::default();
    let mut complete = changes.failed == 0;
    let mut refetch = false;
    for action in actions {
        let out_of_time = started.elapsed() > BUDGET;
        if out_of_time {
            complete = false;
            outcome.again = true;
        }
        refetch |= matches!(action, Action::Refetch(_));
        let writes = matches!(
            action,
            Action::Download(_) | Action::MoveAsideThenDownload(_) | Action::DeleteLocal(_)
        );
        match apply(
            remote,
            places,
            &mut manifest,
            action,
            out_of_time,
            &incoming,
        ) {
            Applied::Done => outcome.changed |= writes,
            Applied::Skipped { arriving } => {
                complete &= !arriving;
                outcome.pending += usize::from(arriving);
            }
        }
    }
    if refetch {
        manifest.token = None;
        outcome.again = true;
    } else if mode == Mode::Full && complete {
        manifest.token = changes.token;
        outcome.again |= changes.more;
    }
    if mode == Mode::Full {
        outcome.pending += usize::from(changes.more) + changes.failed;
    }
    save(&places.manifest, &manifest)?;
    std::fs::remove_dir_all(&places.staging).ok();
    Ok(outcome)
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
    incoming: &BTreeSet<String>,
) -> Applied {
    let arriving = matches!(
        action,
        Action::Download(_)
            | Action::MoveAsideThenDownload(_)
            | Action::DeleteLocal(_)
            | Action::Wait(_)
    );
    let skipped = Applied::Skipped { arriving };
    let moves_nothing = matches!(
        action,
        Action::Keep(_) | Action::Forget(_) | Action::Refetch(_)
    );
    if out_of_time && !moves_nothing {
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
            move_aside(places, &change.name, incoming, &manifest.synced)
                .and_then(|()| download(places, manifest, &change))
        }
        Action::DeleteLocal(name) => delete_local(places, manifest, &name),
        Action::DeleteRemote(name) => remote.delete(&name).map(|()| manifest.forget(&name)),
        Action::Forget(name) | Action::Refetch(name) => {
            manifest.forget(&name);
            Ok(())
        }
        Action::Keep(change) => {
            std::fs::remove_file(&change.staged).ok();
            Ok(())
        }
        Action::Wait(_) => return skipped,
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
    manifest.deleted.remove(&change.name);
    manifest.synced.insert(
        change.name.clone(),
        Synced {
            stamp,
            saved_at: change.saved_at,
        },
    );
    Ok(())
}

/// Renames the local file `name` to the first `-n` name beside it that no
/// file holds and no record arriving in this pass or already synced
/// claims, so a later download cannot land on it.
fn move_aside(
    places: &Places,
    name: &str,
    incoming: &BTreeSet<String>,
    synced: &BTreeMap<String, Synced>,
) -> io::Result<()> {
    let path = places
        .path(name)
        .ok_or_else(|| io::Error::other(format!("no folder for {name}")))?;
    let folder = name.split_once('/').map_or(name, |(folder, _)| folder);
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
        .map(|n| format!("{stem}-{n}.{extension}"))
        .find(|file| {
            let claimed = format!("{folder}/{file}");
            !incoming.contains(&claimed)
                && !synced.contains_key(&claimed)
                && !path.with_file_name(file).exists()
        })
        .map(|file| path.with_file_name(file))
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
    manifest.forget(name);
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
