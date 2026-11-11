//! Mirrors this device's records through CloudKit, so Apple TV, which has no
//! iCloud Drive, shares saves with iPad and Mac. Each record file becomes
//! one CloudKit record named by its folder and file name. iPad and Mac
//! mirror their iCloud Drive folder, which carries Apple TV's records into
//! it; Apple TV mirrors its cache, which brings them back after tvOS clears
//! it.
//!
//! Every record has a version: how far its match has been played, in ticks, or,
//! once deleted, a delete record one tick past the save it deleted. A record
//! only gets further along by being played, so the most ticks wins, on every
//! device and in either direction, whatever the devices' clocks say. CloudKit
//! refuses anything behind what it holds, so devices agree whatever order
//! changes reach them in. A manifest holds what this device last agreed with
//! CloudKit, against which a pass tells what changed here.
//!
//! Only a player deletes from CloudKit: in the game, or by answering a question.
//! A device cannot tell a file deleted from an iCloud Drive folder from one
//! iCloud has not brought back yet, so a missing file there changes nothing
//! elsewhere. A save or replay, which only a player removes, waits for Home to
//! ask whether to delete it everywhere or bring it back; an autosave or finished
//! match, which the game cleans up on its own, just stays gone from this device.
//! On Apple TV, where the system clears files on its own, a missing file is
//! simply fetched again. CloudKit lacking a record deletes nothing either; only
//! a delete record does.
//!
//! Passes run on a thread of their own, since every CloudKit call waits on
//! the network and must never hold up a save. Files arrive only while Home or
//! the shelf is open or a load is waiting for them, never during a match.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
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

/// One state of a record. The most ticks wins. At the same tick a file beats a
/// delete record, so a tie never loses a save, and then the later save time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version {
    ticks: u64,
    live: bool,
    saved_at: u64,
}

/// What both sides held for one record when they last agreed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Synced {
    /// The file agreed on, or `None` for a delete record.
    stamp: Option<Stamp>,
    /// How far the save had been played, or the delete record's tick.
    ticks: u64,
    /// When the file was saved; nothing for a delete record.
    saved_at: u64,
}

impl Synced {
    fn version(self) -> Version {
        Version {
            ticks: self.ticks,
            live: self.stamp.is_some(),
            saved_at: self.saved_at,
        }
    }

    /// The delete record that outdates this save, and no save further along.
    fn deletion(self) -> Version {
        Version {
            ticks: self.ticks + 1,
            live: false,
            saved_at: 0,
        }
    }

    fn file(stamp: Stamp, version: Version) -> Self {
        Self {
            stamp: Some(stamp),
            ticks: version.ticks,
            saved_at: version.saved_at,
        }
    }
}

/// The version a change from CloudKit carries.
fn theirs(change: &Changed) -> Version {
    Version {
        ticks: change.ticks,
        live: change.staged.is_some(),
        saved_at: change.saved_at,
    }
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

/// A player's answer about a file missing from an iCloud Drive folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    /// Delete the record on every device.
    Delete(String),
    /// Fetch the record again.
    BringBack(String),
}

impl Manifest {
    /// Notes the game's deletions of synced files among `paths`; whether any
    /// was new.
    fn note_deleted(&mut self, places: &Places, paths: &[PathBuf]) -> bool {
        let mut noted = false;
        for name in paths.iter().filter_map(|path| places.name_of(path)) {
            if self
                .synced
                .get(&name)
                .is_some_and(|entry| entry.stamp.is_some())
            {
                noted |= self.deleted.insert(name);
            }
        }
        noted
    }

    /// Applies a player's answers: whether anything changed, and whether
    /// every record must be fetched again to bring one back.
    fn answer(&mut self, answers: Vec<Answer>) -> (bool, bool) {
        let (mut changed, mut refetch) = (false, false);
        for answer in answers {
            match answer {
                Answer::Delete(name) => {
                    if self
                        .synced
                        .get(&name)
                        .is_some_and(|entry| entry.stamp.is_some())
                    {
                        changed |= self.deleted.insert(name);
                    }
                }
                Answer::BringBack(name) => {
                    if self
                        .synced
                        .get(&name)
                        .is_some_and(|entry| entry.stamp.is_some())
                    {
                        self.synced.remove(&name);
                        self.deleted.remove(&name);
                        self.token = None;
                        changed = true;
                        refetch = true;
                    }
                }
            }
        }
        (changed, refetch)
    }

    /// Records that both sides hold a delete record of tick `ticks` for `name`.
    fn agree_deleted(&mut self, name: &str, ticks: u64) {
        self.deleted.remove(name);
        self.synced.insert(
            name.to_owned(),
            Synced {
                stamp: None,
                ticks,
                saved_at: 0,
            },
        );
    }
}

/// One step a pass takes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Send the local file.
    Upload(String),
    /// Take the remote copy.
    Download(Changed),
    /// Both sides hold this version: note the agreement, move nothing.
    Adopt(String, Version),
    /// Remove the local file, which a delete record of this tick outdates.
    DeleteLocal(String, u64),
    /// Remove a local copy of a save the game deleted, which came back
    /// before the deletion went up.
    RemoveCopy(String),
    /// Send a delete record of this tick.
    DeleteRemote(String, u64),
    /// The remote change lost to a newer version here, or waits for a later
    /// pass.
    Keep(Changed),
    /// iCloud Drive is still bringing the file here: leave it, and fetch
    /// its change again once it has arrived.
    Wait(String),
    /// The file is not what was agreed and cannot be sent, as when tvOS clears
    /// cached files: forget it and fetch every record again, which brings
    /// the agreed copy back.
    Refetch(String),
    /// Ask the player about a save or replay missing from an iCloud Drive
    /// folder.
    Ask(String),
}

/// How a record stands on this device against the manifest.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mine {
    /// No file and no agreement, or a file that is not a record yet.
    Nothing,
    /// The file vanished where the system removes files on its own.
    Vanished,
    /// The file is gone from an iCloud Drive folder, which may mean deleted
    /// or not back yet. This holds what was agreed.
    Missing(Version),
    /// What both sides last agreed still holds.
    Agreed(Version),
    /// The agreed save, in a file rewritten as a copy or download leaves it.
    Touched(Version),
    /// A version CloudKit has not been told of: a new or edited file, or a
    /// deletion.
    Unsent(Version),
    /// A file behind the delete record both sides agreed on, as iCloud Drive
    /// can deliver late: the delete record stands.
    Outdated(Version),
    /// A copy of a save the game deleted, and the delete record yet to go up.
    Undeleted(Version),
    /// A file behind the save both sides agreed on, as an iCloud Drive conflict
    /// can leave: CloudKit would refuse it, and holds the one further along.
    Superseded,
}

impl Mine {
    fn version(self) -> Option<Version> {
        match self {
            Self::Nothing | Self::Vanished | Self::Missing(_) | Self::Superseded => None,
            Self::Agreed(version)
            | Self::Touched(version)
            | Self::Unsent(version)
            | Self::Outdated(version)
            | Self::Undeleted(version) => Some(version),
        }
    }
}

/// What a look through this device's folders found.
#[derive(Debug, Default)]
pub(crate) struct Listing {
    /// Each record file and its stamp.
    pub found: BTreeMap<String, Stamp>,
    /// Records this device cannot read yet, such as ones iCloud Drive has
    /// not brought down.
    pub arriving: BTreeSet<String>,
    /// Folders that could not be listed: their records may still be there.
    pub unreadable: BTreeSet<String>,
}

/// What this device holds and may do, as a pass finds it.
pub(crate) struct Here<'a> {
    pub listing: &'a Listing,
    /// Synced records the game deleted here.
    pub deleted: &'a BTreeSet<String>,
    /// Whether the system removes files on its own, so that a missing file is
    /// simply fetched again.
    pub purgeable: bool,
    /// Whether this pass fetched CloudKit's changes, so that fetching again
    /// could bring a file back.
    pub full: bool,
}

/// The folder part of a record's name.
fn folder_of(name: &str) -> &str {
    name.split_once('/').map_or(name, |(folder, _)| folder)
}

/// Whether only a player removes this record, so that it going missing is worth
/// asking about. The game cleans up autosaves and finished matches on its own.
fn player_kept(name: &str) -> bool {
    matches!(folder_of(name), "saves" | "replays")
}

impl Here<'_> {
    /// Whether this device cannot tell if it holds the record right now.
    fn waiting(&self, name: &str) -> bool {
        self.listing.arriving.contains(name) || self.listing.unreadable.contains(folder_of(name))
    }
}

/// What to do about each record, from the manifest, this device's files
/// and what CloudKit reported. `read` finds a local file's version, and only
/// runs for files that changed.
pub(crate) fn plan(
    synced: &BTreeMap<String, Synced>,
    here: &Here,
    changes: &Changes,
    read: &mut impl FnMut(&str) -> Option<Version>,
) -> Vec<Action> {
    // Each name stands once: a changed one is handled, and the rest skips it.
    let standing = |name: &str, read: &mut dyn FnMut(&str) -> Option<Version>| -> Mine {
        let agreed = synced.get(name).copied();
        let noted = here.deleted.contains(name);
        let Some(stamp) = here.listing.found.get(name) else {
            return match agreed {
                None => Mine::Nothing,
                Some(entry) if entry.stamp.is_none() => Mine::Agreed(entry.version()),
                Some(entry) if noted => Mine::Unsent(entry.deletion()),
                Some(_) if here.purgeable => Mine::Vanished,
                Some(entry) => Mine::Missing(entry.version()),
            };
        };
        if let Some(entry) = agreed
            && entry.stamp == Some(*stamp)
        {
            return if noted {
                Mine::Undeleted(entry.deletion())
            } else {
                Mine::Agreed(entry.version())
            };
        }
        let Some(version) = read(name) else {
            return Mine::Nothing;
        };
        match agreed {
            Some(entry) if noted && entry.stamp.is_some() && version <= entry.version() => {
                Mine::Undeleted(entry.deletion())
            }
            // A different size is a different save even at the same version.
            Some(entry)
                if entry.version() == version
                    && entry.stamp.is_some_and(|s| s.size == stamp.size) =>
            {
                Mine::Touched(version)
            }
            Some(entry) if entry.stamp.is_some() && version < entry.version() => Mine::Superseded,
            Some(entry) if entry.stamp.is_none() && version < entry.version() => {
                Mine::Outdated(entry.version())
            }
            _ => Mine::Unsent(version),
        }
    };
    let send = |name: &str, version: Version| {
        if version.live {
            Action::Upload(name.to_owned())
        } else {
            Action::DeleteRemote(name.to_owned(), version.ticks)
        }
    };
    let mut actions = Vec::new();
    let mut handled = BTreeSet::new();
    for change in &changes.changed {
        let name = &change.name;
        handled.insert(name.clone());
        if here.waiting(name) {
            actions.push(Action::Keep(change.clone()));
            actions.push(Action::Wait(name.clone()));
            continue;
        }
        let mine = standing(name, read);
        let incoming = theirs(change);
        let on_disk = here.listing.found.contains_key(name);
        // Nothing new about a file the player has yet to decide on.
        if let Mine::Missing(agreed) = mine
            && agreed >= incoming
        {
            actions.push(Action::Keep(change.clone()));
            if player_kept(name) {
                actions.push(Action::Ask(name.clone()));
            }
            continue;
        }
        let stale_copy = matches!(mine, Mine::Outdated(_) | Mine::Undeleted(_));
        match mine.version().map(|version| version.cmp(&incoming)) {
            Some(Ordering::Equal) => {
                actions.push(Action::Keep(change.clone()));
                actions.push(if stale_copy {
                    Action::DeleteLocal(name.clone(), incoming.ticks)
                } else {
                    Action::Adopt(name.clone(), incoming)
                });
            }
            Some(Ordering::Greater) => {
                actions.push(Action::Keep(change.clone()));
                if stale_copy {
                    actions.push(Action::RemoveCopy(name.clone()));
                }
                actions.extend(mine.version().map(|version| send(name, version)));
            }
            _ if incoming.live => actions.push(Action::Download(change.clone())),
            Some(_) if on_disk => actions.push(Action::DeleteLocal(name.clone(), incoming.ticks)),
            _ => actions.push(Action::Adopt(name.clone(), incoming)),
        }
    }
    let rest: BTreeSet<&String> = here
        .listing
        .found
        .keys()
        .chain(synced.keys())
        .filter(|name| !handled.contains(*name) && !here.waiting(name))
        .collect();
    for name in rest {
        match standing(name, read) {
            Mine::Touched(version) => actions.push(Action::Adopt(name.clone(), version)),
            Mine::Outdated(version) => {
                actions.push(Action::DeleteLocal(name.clone(), version.ticks));
            }
            Mine::Undeleted(version) => {
                actions.push(Action::RemoveCopy(name.clone()));
                actions.push(send(name, version));
            }
            Mine::Unsent(version) => actions.push(send(name, version)),
            Mine::Vanished | Mine::Superseded if here.full => {
                actions.push(Action::Refetch(name.clone()));
            }
            Mine::Missing(_) if player_kept(name) => actions.push(Action::Ask(name.clone())),
            _ => {}
        }
    }
    actions
}

/// The CloudKit calls a pass makes, so tests can stand in for iCloud.
pub(crate) trait Remote {
    fn account(&self) -> Option<String>;
    fn fetch(&self, token: Option<&str>, staging: &Path) -> io::Result<Changes>;
    fn upload(&self, name: &str, path: &Path, ticks: u64, saved_at: u64) -> io::Result<()>;
    fn delete(&self, name: &str, ticks: u64) -> io::Result<()>;
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
    fn upload(&self, name: &str, path: &Path, ticks: u64, saved_at: u64) -> io::Result<()> {
        aftermine_icloud::cloudkit::upload(name, path, ticks, saved_at)
    }
    fn delete(&self, name: &str, ticks: u64) -> io::Result<()> {
        aftermine_icloud::cloudkit::delete(name, ticks)
    }
}

/// What a pass asks of the game, so tests can stand in for it.
pub(crate) trait Host {
    /// Records the game deleted since a pass last asked.
    fn take_deleted(&self) -> Vec<PathBuf>;
    /// The player's answers since a pass last asked.
    fn take_answers(&self) -> Vec<Answer>;
    /// Whether files may arrive now. Checked before each one, so a player
    /// starting a match stops the arrivals of a pass already running.
    fn allows_arrivals(&self) -> bool;
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
    /// Whether Home or the shelf is open, so that files may arrive.
    arrivals: AtomicBool,
}

#[derive(Default)]
struct Queue {
    /// The pass wanted next.
    wanted: Option<Mode>,
    /// The pass under way.
    running: Option<Mode>,
    /// A push was asked for that no pass has taken up yet.
    unsent: bool,
    /// The pass under way took up a push.
    sending: bool,
    /// Records the game deleted since a pass last took them.
    deleted: Vec<PathBuf>,
    /// The player's answers since a pass last took them.
    answers: Vec<Answer>,
    /// Files the last pass found missing from an iCloud Drive folder.
    missing: Vec<String>,
}

impl Queue {
    /// Marks the pass under way done. A pass cut short leaves the push it
    /// took up to the pass that follows it.
    fn end_pass(&mut self, again: bool) {
        self.running = None;
        if std::mem::take(&mut self.sending) && again {
            self.unsent = true;
        }
    }
}

impl Shared {
    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn sent(&self) -> bool {
        let queue = self.queue();
        !queue.unsent && !queue.sending
    }

    /// Asks for a pass; a full pass covers a push.
    fn want(&self, mode: Mode) {
        let mut queue = self.queue();
        if queue.wanted != Some(Mode::Full) {
            queue.wanted = Some(mode);
        }
        if mode == Mode::Push {
            queue.unsent = true;
        }
        self.wake.notify_one();
    }
}

impl Host for Shared {
    fn take_deleted(&self) -> Vec<PathBuf> {
        std::mem::take(&mut self.queue().deleted)
    }
    fn take_answers(&self) -> Vec<Answer> {
        std::mem::take(&mut self.queue().answers)
    }
    fn allows_arrivals(&self) -> bool {
        self.arrivals.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Starts the mirror's thread, which waits to be asked for a pass. Only the
/// first call does anything.
pub(crate) fn start() {
    let mut first = false;
    let shared = MIRROR.get_or_init(|| {
        first = true;
        Shared::default()
    });
    if !first {
        return;
    }
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

/// Whether a full pass is under way or about to start, so that another
/// device's newer copy may be on its way here.
pub(crate) fn syncing() -> bool {
    MIRROR.get().is_some_and(|shared| {
        let queue = shared.queue();
        queue.wanted == Some(Mode::Full) || queue.running == Some(Mode::Full)
    })
}

/// Whether every push asked for has run, so this device's changes have gone
/// up or failed to. A quit waits on this, briefly, so the save it just made
/// reaches the player's other devices.
pub(crate) fn sent() -> bool {
    MIRROR.get().is_none_or(Shared::sent)
}

/// Files missing from an iCloud Drive folder that the player has yet to
/// decide on.
pub(crate) fn missing() -> Vec<String> {
    MIRROR
        .get()
        .map_or_else(Vec::new, |shared| shared.queue().missing.clone())
}

/// Takes the player's decision on `names`: delete them on every device, or
/// bring them back.
pub(crate) fn answer_missing(names: &[String], delete: bool) {
    let Some(shared) = MIRROR.get() else {
        return;
    };
    {
        let mut queue = shared.queue();
        queue.missing.retain(|name| !names.contains(name));
        queue.answers.extend(names.iter().map(|name| {
            if delete {
                Answer::Delete(name.clone())
            } else {
                Answer::BringBack(name.clone())
            }
        }));
    }
    shared.want(Mode::Full);
}

/// Tells the mirror whether files may arrive: while Home or the shelf is
/// open or a load is waiting for them, never during a match.
pub(crate) fn allow_arrivals(allowed: bool) {
    if let Some(shared) = MIRROR.get() {
        shared
            .arrivals
            .store(allowed, std::sync::atomic::Ordering::Relaxed);
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

/// Runs the passes asked for, one at a time.
fn serve(shared: &'static Shared) {
    loop {
        serve_one(shared, &CloudKit, Places::here);
    }
}

/// Waits for a pass to be asked for and runs it, reporting a failure
/// rather than raising it: a mirror that cannot reach iCloud leaves records
/// where they are.
fn serve_one(shared: &Shared, remote: &impl Remote, places: impl FnOnce() -> Option<Places>) {
    let mode = {
        let mut queue = shared.queue();
        loop {
            if let Some(mode) = queue.wanted.take() {
                queue.running = Some(mode);
                queue.sending = std::mem::take(&mut queue.unsent);
                break mode;
            }
            queue = shared
                .wake
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner);
        }
    };
    let result = places().map(|places| pass(remote, &places, mode, shared));
    let outcome = match result {
        Some(Ok(outcome)) => Some(outcome),
        Some(Err(error)) => {
            eprintln!("iCloud mirror: {error}");
            None
        }
        None => None,
    };
    let again = {
        let mut queue = shared.queue();
        let again = match outcome {
            Some(outcome) => {
                // A push has not looked at CloudKit, and a question answered
                // while this pass ran stays answered.
                if mode == Mode::Full {
                    let answers = std::mem::take(&mut queue.answers);
                    queue.missing = outcome
                        .missing
                        .into_iter()
                        .filter(|name| {
                            !answers.iter().any(|answer| match answer {
                                Answer::Delete(asked) | Answer::BringBack(asked) => asked == name,
                            })
                        })
                        .collect();
                    queue.answers = answers;
                }
                shared
                    .pending
                    .store(outcome.pending, std::sync::atomic::Ordering::Relaxed);
                if outcome.changed {
                    shared
                        .changes
                        .fetch_add(1, std::sync::atomic::Ordering::Release);
                }
                outcome.again
            }
            None => false,
        };
        queue.end_pass(again);
        again
    };
    if again {
        shared.want(Mode::Full);
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
    /// Files missing from an iCloud Drive folder, for the player to decide on.
    pub missing: Vec<String>,
}

/// Where a pass keeps its state and finds records.
pub(crate) struct Places {
    pub manifest: PathBuf,
    pub staging: PathBuf,
    pub folders: Vec<(&'static str, PathBuf)>,
    /// Whether the system may remove record files on its own.
    pub purgeable: bool,
    /// Whether a file is one iCloud Drive has not brought down here yet.
    pub dataless: fn(&Path) -> bool,
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
            dataless: crate::paths::dataless,
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

    /// What the folders hold. Anything that cannot be read counts as
    /// unknown rather than missing, so a failed look never reads as a
    /// deletion.
    fn local(&self) -> Listing {
        let mut listing = Listing::default();
        for (label, dir) in &self.folders {
            let paths: io::Result<Vec<PathBuf>> = std::fs::read_dir(dir)
                .and_then(|entries| entries.map(|entry| Ok(entry?.path())).collect());
            let paths = match paths {
                Ok(paths) => paths,
                // A folder not there holds nothing; what it held counts as
                // missing, like any other file.
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(_) => {
                    listing.unreadable.insert((*label).to_owned());
                    continue;
                }
            };
            for path in paths {
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
                        listing.arriving.insert(format!("{label}/{real}"));
                    }
                } else if !file.starts_with('.') && mirrored(file) {
                    let name = format!("{label}/{file}");
                    // macOS keeps one under its real name, with no content.
                    match Stamp::of(&path).filter(|_| !(self.dataless)(&path)) {
                        Some(stamp) => {
                            listing.found.insert(name, stamp);
                        }
                        None => {
                            listing.arriving.insert(name);
                        }
                    }
                }
            }
        }
        listing
    }
}

/// The longest file name CloudKit's record names leave room for, after the
/// folder in front of it.
const MAX_FILE_NAME: usize = 240;

/// Whether a file of this name is a record the mirror carries. CloudKit's
/// record names are ASCII of limited length, so a file renamed to anything
/// else stays on this device.
fn mirrored(file: &str) -> bool {
    file.is_ascii()
        && file.len() <= MAX_FILE_NAME
        && matches!(
            Path::new(file).extension().and_then(|e| e.to_str()),
            Some("json" | "amreplay" | "amsave")
        )
}

/// The version of the record at `path`: how far it was played and when it was
/// saved, by its own record of it or `modified`, when the file last changed.
/// `None` for a file that is not a record, such as one still being written.
fn file_version(path: &Path, modified: u64) -> Option<Version> {
    let meta = crate::saved_game::inspect(path).ok()?.meta;
    Some(Version {
        ticks: meta.ticks.unwrap_or(0),
        live: true,
        saved_at: meta.saved_at.unwrap_or(modified),
    })
}

/// Runs one pass against `remote`: fetches what changed elsewhere unless
/// pushing, takes the game's deletions from `host`, decides with [`plan`],
/// and carries it out until the time budget runs out. The token advances
/// only once every remote change it covers is applied, so a cut-short pass
/// fetches them again.
pub(crate) fn pass(
    remote: &impl Remote,
    places: &Places,
    mode: Mode,
    host: &impl Host,
) -> io::Result<Outcome> {
    let mut manifest = load(&places.manifest);
    let folders: Vec<(String, PathBuf)> = places
        .folders
        .iter()
        .map(|(label, dir)| ((*label).to_owned(), dir.clone()))
        .collect();
    let Some(account) = remote.account() else {
        // What the player did while signed out still counts once signed in,
        // provided it was done to the folders the manifest speaks for.
        let deleted = host.take_deleted();
        let answers = host.take_answers();
        if manifest.folders == folders {
            let noted = manifest.note_deleted(places, &deleted);
            let (answered, _) = manifest.answer(answers);
            if noted || answered {
                save(&places.manifest, &manifest)?;
            }
        }
        return Ok(Outcome::default());
    };
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
    let listing = places.local();
    manifest.note_deleted(places, &host.take_deleted());
    let (_, mut refetch) = manifest.answer(host.take_answers());
    // Starting over, files agreed on may no longer be in CloudKit, so they
    // count as new. Deletions, agreed or yet to go up, still stand.
    if changes.restarted {
        let Manifest {
            synced, deleted, ..
        } = &mut manifest;
        synced.retain(|name, entry| entry.stamp.is_none() || deleted.contains(name));
    }
    let mut read = |name: &str| {
        let path = places.path(name)?;
        file_version(&path, Stamp::of(&path)?.secs)
    };
    let here = Here {
        listing: &listing,
        deleted: &manifest.deleted,
        purgeable: places.purgeable,
        full: mode == Mode::Full,
    };
    let actions = plan(&manifest.synced, &here, &changes, &mut read);
    let started = Instant::now();
    let mut outcome = Outcome::default();
    let mut complete = changes.failed == 0;
    for action in actions {
        if let Action::Ask(name) = action {
            outcome.missing.push(name);
            continue;
        }
        let out_of_time = started.elapsed() > BUDGET;
        if out_of_time {
            complete = false;
            outcome.again = true;
        }
        refetch |= matches!(action, Action::Refetch(_));
        let writes = matches!(
            action,
            Action::Download(_) | Action::DeleteLocal(..) | Action::RemoveCopy(_)
        );
        let step = Step {
            out_of_time,
            arrivals: host.allows_arrivals(),
            listing: &listing,
        };
        match apply(remote, places, &mut manifest, action, &step) {
            Applied::Done => outcome.changed |= writes,
            Applied::Behind => refetch = true,
            Applied::Skipped { arriving } => {
                complete &= !arriving;
                outcome.pending += usize::from(arriving);
            }
        }
    }
    if mode == Mode::Full {
        if complete {
            manifest.token = changes.token;
            outcome.again |= changes.more;
        }
        outcome.pending += usize::from(changes.more) + changes.failed;
    }
    if refetch {
        manifest.token = None;
        outcome.again = true;
    }
    save(&places.manifest, &manifest)?;
    std::fs::remove_dir_all(&places.staging).ok();
    Ok(outcome)
}

/// What decides whether one action runs now.
struct Step<'a> {
    out_of_time: bool,
    arrivals: bool,
    listing: &'a Listing,
}

enum Applied {
    Done,
    /// CloudKit holds something further along than a deletion this device
    /// asked for, so every record must be fetched again to find it.
    Behind,
    /// Left for a later pass; `arriving` when it was a file on its way in,
    /// whose change must be fetched again. A file going up needs no such
    /// thing: it stays unsent, so a later pass sends it anyway.
    Skipped {
        arriving: bool,
    },
}

fn apply(
    remote: &impl Remote,
    places: &Places,
    manifest: &mut Manifest,
    action: Action,
    step: &Step,
) -> Applied {
    let arriving = matches!(
        action,
        Action::Download(_) | Action::DeleteLocal(..) | Action::RemoveCopy(_) | Action::Wait(_)
    );
    let moves_nothing = matches!(action, Action::Keep(_) | Action::Refetch(_));
    let deleting = match &action {
        Action::DeleteRemote(name, _) => Some(name.clone()),
        _ => None,
    };
    if (step.out_of_time && !moves_nothing) || (arriving && !step.arrivals) {
        return Applied::Skipped { arriving };
    }
    let scanned = |name: &str| step.listing.found.get(name).copied();
    let result = match action {
        Action::Upload(name) => upload(remote, places, manifest, &name),
        Action::Download(change) => download(places, manifest, &change, scanned(&change.name)),
        // The stamp the scan saw: a save landing since then differs from it,
        // so the next pass still notices.
        Action::Adopt(name, version) if version.live => {
            if let Some(stamp) = step.listing.found.get(&name) {
                manifest.deleted.remove(&name);
                manifest.synced.insert(name, Synced::file(*stamp, version));
            }
            Ok(())
        }
        Action::Adopt(name, version) => {
            manifest.agree_deleted(&name, version.ticks);
            Ok(())
        }
        Action::DeleteLocal(name, ticks) => remove_local(places, &name, scanned(&name))
            .map(|()| manifest.agree_deleted(&name, ticks)),
        Action::RemoveCopy(name) => remove_local(places, &name, scanned(&name)),
        Action::DeleteRemote(name, ticks) => remote
            .delete(&name, ticks)
            .map(|()| manifest.agree_deleted(&name, ticks)),
        Action::Refetch(name) => {
            manifest.synced.remove(&name);
            manifest.deleted.remove(&name);
            Ok(())
        }
        Action::Keep(change) => {
            if let Some(staged) = &change.staged {
                std::fs::remove_file(staged).ok();
            }
            Ok(())
        }
        Action::Wait(_) => return Applied::Skipped { arriving },
        Action::Ask(_) => Ok(()),
    };
    match result {
        Ok(()) => Applied::Done,
        Err(error) => {
            eprintln!("iCloud mirror: {error}");
            if let (io::ErrorKind::AlreadyExists, Some(name)) = (error.kind(), deleting) {
                manifest.synced.remove(&name);
                manifest.deleted.remove(&name);
                return Applied::Behind;
            }
            Applied::Skipped { arriving }
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
    // Upload a copy, and describe that copy, so a save replacing the file
    // meanwhile can neither tear it nor give it another save's details.
    let copy = places
        .staging
        .join(format!("upload-{}", name.replace('/', "-")));
    let vanished = || io::Error::other(format!("{name} vanished"));
    let stamp = aftermine_icloud::coordinate(&path, aftermine_icloud::Access::Read, |granted| {
        let stamp = Stamp::of(granted).ok_or_else(vanished)?;
        std::fs::copy(granted, &copy)?;
        if Stamp::of(granted) == Some(stamp) {
            Ok(stamp)
        } else {
            Err(io::Error::other(format!("{name} changed while copying")))
        }
    })??;
    // A file still being written is not a record yet.
    let version = file_version(&copy, stamp.secs)
        .ok_or_else(|| io::Error::other(format!("{name} is not a record yet")))?;
    let sent = remote.upload(name, &copy, version.ticks, version.saved_at);
    std::fs::remove_file(&copy).ok();
    sent?;
    manifest.deleted.remove(name);
    manifest
        .synced
        .insert(name.to_owned(), Synced::file(stamp, version));
    Ok(())
}

/// Fails unless the file at `path` is as the pass's scan found it. iCloud
/// Drive can deliver a newer copy after the scan, which the plan never weighed,
/// so the pass leaves the file for the next one to weigh.
fn unchanged_since_scan(name: &str, path: &Path, scanned: Option<Stamp>) -> io::Result<()> {
    if Stamp::of(path) == scanned {
        Ok(())
    } else {
        Err(io::Error::other(format!("{name} changed since the scan")))
    }
}

fn download(
    places: &Places,
    manifest: &mut Manifest,
    change: &Changed,
    scanned: Option<Stamp>,
) -> io::Result<()> {
    let staged = change
        .staged
        .as_ref()
        .ok_or_else(|| io::Error::other(format!("{} has no file", change.name)))?;
    // A record this device has no folder for, or a name no folder could
    // hold, is passed over rather than retried forever.
    let Some(path) = places.path(&change.name) else {
        std::fs::remove_file(staged).ok();
        return Ok(());
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let stamp =
        aftermine_icloud::coordinate(&path, aftermine_icloud::Access::Replace, |granted| {
            unchanged_since_scan(&change.name, granted, scanned)?;
            chassis::fsx::write_atomic(granted, |writer| {
                io::copy(&mut std::fs::File::open(staged)?, writer).map(|_| ())
            })?;
            // The save time stands in for when the file changed, so autosave
            // rotation, which goes by modification time, orders downloaded
            // records as it orders this device's own.
            let when = UNIX_EPOCH + Duration::from_secs(change.saved_at);
            std::fs::File::options()
                .write(true)
                .open(granted)?
                .set_modified(when.min(SystemTime::now()))?;
            Stamp::of(granted).ok_or_else(|| io::Error::other("downloaded file vanished"))
        })??;
    std::fs::remove_file(staged).ok();
    manifest.deleted.remove(&change.name);
    manifest
        .synced
        .insert(change.name.clone(), Synced::file(stamp, theirs(change)));
    Ok(())
}

fn remove_local(places: &Places, name: &str, scanned: Option<Stamp>) -> io::Result<()> {
    if let Some(path) = places.path(name) {
        aftermine_icloud::coordinate(&path, aftermine_icloud::Access::Delete, |granted| {
            unchanged_since_scan(name, granted, scanned)?;
            match std::fs::remove_file(granted) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            }
        })??;
    }
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
mod simulation_tests;
#[cfg(test)]
mod tests;
