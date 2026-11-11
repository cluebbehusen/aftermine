use super::*;
use crate::game::GameReplay;
use aftermine_sim::SIM_VERSION;
use std::cell::{Cell, RefCell};

fn stamp(size: u64, secs: u64) -> Stamp {
    Stamp {
        size,
        secs,
        nanos: 0,
    }
}

fn synced(size: u64, secs: u64, at: u64) -> Synced {
    Synced {
        stamp: Some(stamp(size, secs)),
        ticks: at,
        saved_at: at,
    }
}

fn delete_agreed(at: u64) -> Synced {
    Synced {
        stamp: None,
        ticks: at,
        saved_at: 0,
    }
}

fn changed(name: &str, at: u64) -> Changed {
    Changed {
        name: name.to_owned(),
        ticks: at,
        saved_at: at,
        staged: Some(PathBuf::from(format!("/staging/{name}"))),
    }
}

fn delete_record(name: &str, at: u64) -> Changed {
    Changed {
        name: name.to_owned(),
        ticks: at,
        saved_at: 0,
        staged: None,
    }
}

fn file_at(at: u64) -> Version {
    Version {
        ticks: at,
        live: true,
        saved_at: at,
    }
}

fn gone_at(at: u64) -> Version {
    Version {
        ticks: at,
        live: false,
        saved_at: 0,
    }
}

/// How a planning device keeps its files, and what kind of pass this is.
struct Kept<'a> {
    purgeable: bool,
    arriving: &'a [&'a str],
    deleted: &'a [&'a str],
    unreadable: &'a [&'a str],
    full: bool,
}

/// An iCloud Drive folder with every file on the device, in a full pass.
const DRIVE: Kept = Kept {
    purgeable: false,
    arriving: &[],
    deleted: &[],
    unreadable: &[],
    full: true,
};

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

fn plan_on(
    kept: &Kept,
    agreed: &[(&str, Synced)],
    local: &[(&str, Stamp)],
    changes: &Changes,
    times: &[(&str, u64)],
) -> Vec<Action> {
    let agreed = agreed
        .iter()
        .map(|(name, entry)| ((*name).to_owned(), *entry))
        .collect();
    let listing = Listing {
        found: local
            .iter()
            .map(|(name, stamp)| ((*name).to_owned(), *stamp))
            .collect(),
        arriving: names(kept.arriving),
        unreadable: names(kept.unreadable),
    };
    let times: BTreeMap<String, u64> = times
        .iter()
        .map(|(name, at)| ((*name).to_owned(), *at))
        .collect();
    let here = Here {
        listing: &listing,
        deleted: &names(kept.deleted),
        purgeable: kept.purgeable,
        full: kept.full,
    };
    plan(&agreed, &here, changes, &mut |name| {
        times.get(name).copied().map(file_at)
    })
}

fn plan_with(
    agreed: &[(&str, Synced)],
    local: &[(&str, Stamp)],
    changes: &Changes,
    times: &[(&str, u64)],
) -> Vec<Action> {
    plan_on(&DRIVE, agreed, local, changes, times)
}

#[test]
fn a_new_local_record_goes_up_and_a_new_remote_one_comes_down() {
    let actions = plan_with(
        &[],
        &[("autosaves/autosave-1.amsave", stamp(10, 100))],
        &Changes {
            changed: vec![changed("matches/match-2.amreplay", 200)],
            ..Changes::default()
        },
        &[("autosaves/autosave-1.amsave", 100)],
    );
    assert_eq!(
        actions,
        vec![
            Action::Download(changed("matches/match-2.amreplay", 200)),
            Action::Upload("autosaves/autosave-1.amsave".into()),
        ]
    );
}

#[test]
fn an_unchanged_local_copy_takes_the_remote_edit() {
    let name = "autosaves/autosave-1.amsave";
    let actions = plan_with(
        &[(name, synced(10, 100, 100))],
        &[(name, stamp(10, 100))],
        &Changes {
            changed: vec![changed(name, 300)],
            ..Changes::default()
        },
        &[],
    );
    assert_eq!(actions, vec![Action::Download(changed(name, 300))]);
}

#[test]
fn when_both_sides_changed_the_newer_save_wins() {
    let name = "autosaves/autosave-1.amsave";
    let agreed = [(name, synced(10, 100, 100))];
    let edited = [(name, stamp(12, 400))];
    let remote_newer = plan_with(
        &agreed,
        &edited,
        &Changes {
            changed: vec![changed(name, 500)],
            ..Changes::default()
        },
        &[(name, 400)],
    );
    assert_eq!(remote_newer, vec![Action::Download(changed(name, 500))]);
    let local_newer = plan_with(
        &agreed,
        &edited,
        &Changes {
            changed: vec![changed(name, 300)],
            ..Changes::default()
        },
        &[(name, 400)],
    );
    assert_eq!(
        local_newer,
        vec![
            Action::Keep(changed(name, 300)),
            Action::Upload(name.into())
        ]
    );
}

#[test]
fn the_same_save_on_both_sides_moves_no_file() {
    // iPad and Mac share one iCloud Drive folder: each finds the other's
    // upload already on disk.
    let name = "saves/save-0000012000.amsave";
    let actions = plan_with(
        &[],
        &[(name, stamp(10, 999))],
        &Changes {
            changed: vec![changed(name, 250)],
            ..Changes::default()
        },
        &[(name, 250)],
    );
    assert_eq!(
        actions,
        vec![
            Action::Keep(changed(name, 250)),
            Action::Adopt(name.into(), file_at(250))
        ]
    );
}

#[test]
fn a_rewritten_file_with_the_same_save_is_not_sent_again() {
    // A download into iCloud Drive reaches the other Mac or iPad as a new
    // file time; sending it back would bounce it between them for ever.
    let name = "autosaves/autosave-1.amsave";
    let actions = plan_with(
        &[(name, synced(10, 100, 100))],
        &[(name, stamp(10, 777))],
        &Changes::default(),
        &[(name, 100)],
    );
    assert_eq!(actions, vec![Action::Adopt(name.into(), file_at(100))]);
}

#[test]
fn deletions_cross_unless_the_other_side_saved_something_newer() {
    let gone_here = "autosaves/autosave-1.amsave";
    let gone_there = "autosaves/autosave-2.amsave";
    let edited_here = "autosaves/autosave-3.amsave";
    let actions = plan_on(
        &Kept {
            deleted: &[gone_here],
            ..DRIVE
        },
        &[
            (gone_here, synced(10, 100, 100)),
            (gone_there, synced(10, 100, 100)),
            (edited_here, synced(10, 100, 100)),
        ],
        &[(gone_there, stamp(10, 100)), (edited_here, stamp(11, 200))],
        &Changes {
            changed: vec![
                delete_record(gone_there, 101),
                delete_record(edited_here, 101),
            ],
            ..Changes::default()
        },
        &[(edited_here, 200)],
    );
    assert_eq!(
        actions,
        vec![
            Action::DeleteLocal(gone_there.into(), 101),
            Action::Keep(delete_record(edited_here, 101)),
            Action::Upload(edited_here.into()),
            Action::DeleteRemote(gone_here.into(), 101),
        ]
    );
}

#[test]
fn a_file_missing_from_icloud_drive_changes_nothing_until_the_player_answers() {
    let name = "saves/save-1.amsave";
    let agreed = [(name, synced(10, 100, 100))];
    assert_eq!(
        plan_with(&agreed, &[], &Changes::default(), &[]),
        vec![Action::Ask(name.into())],
    );
    let same = Changes {
        changed: vec![changed(name, 100)],
        ..Changes::default()
    };
    assert_eq!(
        plan_with(&agreed, &[], &same, &[]),
        vec![Action::Keep(changed(name, 100)), Action::Ask(name.into())],
        "its own copy coming round again is not news"
    );
    let newer = Changes {
        changed: vec![changed(name, 300)],
        ..Changes::default()
    };
    assert_eq!(
        plan_with(&agreed, &[], &newer, &[]),
        vec![Action::Download(changed(name, 300))],
        "a newer save is"
    );
    let deleted = Changes {
        changed: vec![delete_record(name, 101)],
        ..Changes::default()
    };
    assert_eq!(
        plan_with(&agreed, &[], &deleted, &[]),
        vec![Action::Adopt(name.into(), gone_at(101))],
        "and a delete record settles the question"
    );
}

#[test]
fn the_game_cleaning_up_after_itself_asks_nothing() {
    for name in ["autosaves/autosave-1.amsave", "matches/match-1.amreplay"] {
        let agreed = [(name, synced(10, 100, 100))];
        assert_eq!(plan_with(&agreed, &[], &Changes::default(), &[]), vec![]);
        let same = Changes {
            changed: vec![changed(name, 100)],
            ..Changes::default()
        };
        assert_eq!(
            plan_with(&agreed, &[], &same, &[]),
            vec![Action::Keep(changed(name, 100))],
            "it stays gone from this device"
        );
    }
}

#[test]
fn a_copy_of_a_save_the_game_deleted_goes_with_the_deletion() {
    // iCloud Drive can bring back a file the game has just deleted.
    let name = "saves/save-1.amsave";
    let actions = plan_on(
        &Kept {
            deleted: &[name],
            ..DRIVE
        },
        &[(name, synced(10, 100, 100))],
        &[(name, stamp(10, 777))],
        &Changes::default(),
        &[(name, 100)],
    );
    assert_eq!(
        actions,
        vec![
            Action::RemoveCopy(name.into()),
            Action::DeleteRemote(name.into(), 101)
        ]
    );
}

#[test]
fn a_file_older_than_the_agreed_save_is_replaced_not_sent() {
    // An iCloud Drive conflict can leave the older of two copies.
    let name = "autosaves/autosave-1.amsave";
    let agreed = [(name, synced(10, 100, 300))];
    let older = [(name, stamp(10, 999))];
    assert_eq!(
        plan_with(&agreed, &older, &Changes::default(), &[(name, 200)]),
        vec![Action::Refetch(name.into())]
    );
    let newer = Changes {
        changed: vec![changed(name, 300)],
        ..Changes::default()
    };
    assert_eq!(
        plan_with(&agreed, &older, &newer, &[(name, 200)]),
        vec![Action::Download(changed(name, 300))]
    );
}

#[test]
fn a_copy_never_synced_is_weighed_against_a_delete_record() {
    // iCloud Drive can bring a copy of a save another device has since
    // deleted: an older file goes, a newer one is a new save and goes up.
    let name = "saves/save-1.amsave";
    let delete = Changes {
        changed: vec![delete_record(name, 500)],
        ..Changes::default()
    };
    assert_eq!(
        plan_with(&[], &[(name, stamp(10, 999))], &delete, &[(name, 400)]),
        vec![Action::DeleteLocal(name.into(), 500)]
    );
    assert_eq!(
        plan_with(&[], &[(name, stamp(10, 999))], &delete, &[(name, 600)]),
        vec![
            Action::Keep(delete_record(name, 500)),
            Action::Upload(name.into())
        ]
    );
    assert_eq!(
        plan_with(&[], &[], &delete, &[]),
        vec![Action::Adopt(name.into(), gone_at(500))],
        "with no copy, it is only noted"
    );
}

#[test]
fn an_agreed_delete_outlasts_late_copies() {
    let name = "saves/save-1.amsave";
    let agreed = [(name, delete_agreed(500))];
    let quiet = Changes::default();
    assert_eq!(
        plan_with(&agreed, &[], &quiet, &[]),
        vec![],
        "nothing to do while the record stays deleted"
    );
    assert_eq!(
        plan_with(&agreed, &[(name, stamp(10, 999))], &quiet, &[(name, 400)]),
        vec![Action::DeleteLocal(name.into(), 500)],
        "a copy arriving late through iCloud Drive goes"
    );
    assert_eq!(
        plan_with(&agreed, &[(name, stamp(10, 999))], &quiet, &[(name, 600)]),
        vec![Action::Upload(name.into())],
        "a newer save under the name is a new save"
    );
    let older_upload = Changes {
        changed: vec![changed(name, 400)],
        ..Changes::default()
    };
    assert_eq!(
        plan_with(&agreed, &[], &older_upload, &[]),
        vec![
            Action::Keep(changed(name, 400)),
            Action::DeleteRemote(name.into(), 500)
        ],
        "an older upload landing after the delete does not undo it"
    );
    let newer_upload = Changes {
        changed: vec![changed(name, 600)],
        ..Changes::default()
    };
    assert_eq!(
        plan_with(&agreed, &[], &newer_upload, &[]),
        vec![Action::Download(changed(name, 600))],
        "a newer save outranks it"
    );
}

#[test]
fn at_the_same_second_a_file_beats_a_delete() {
    let name = "autosaves/autosave-1.amsave";
    assert_eq!(
        plan_on(
            &Kept {
                deleted: &[name],
                ..DRIVE
            },
            &[(name, synced(10, 100, 100))],
            &[],
            &Changes {
                changed: vec![changed(name, 101)],
                ..Changes::default()
            },
            &[]
        ),
        vec![Action::Download(changed(name, 101))]
    );
    assert_eq!(
        plan_with(
            &[],
            &[(name, stamp(10, 999))],
            &Changes {
                changed: vec![delete_record(name, 500)],
                ..Changes::default()
            },
            &[(name, 500)]
        ),
        vec![
            Action::Keep(delete_record(name, 500)),
            Action::Upload(name.into())
        ]
    );
}

#[test]
fn a_change_there_outranks_a_deletion_here() {
    let name = "autosaves/autosave-1.amsave";
    let actions = plan_on(
        &Kept {
            deleted: &[name],
            ..DRIVE
        },
        &[(name, synced(10, 100, 100))],
        &[],
        &Changes {
            changed: vec![changed(name, 300)],
            ..Changes::default()
        },
        &[],
    );
    assert_eq!(actions, vec![Action::Download(changed(name, 300))]);
}

#[test]
fn an_older_copy_over_this_devices_own_goes_up_again() {
    // Two uploads crossed: the older one landed last.
    let name = "autosaves/autosave-1.amsave";
    let actions = plan_with(
        &[(name, synced(10, 100, 500))],
        &[(name, stamp(10, 100))],
        &Changes {
            changed: vec![changed(name, 300)],
            ..Changes::default()
        },
        &[],
    );
    assert_eq!(
        actions,
        vec![
            Action::Keep(changed(name, 300)),
            Action::Upload(name.into())
        ]
    );
}

#[test]
fn where_the_system_removes_files_a_missing_one_is_simply_fetched_again() {
    let removed = "autosaves/autosave-1.amsave";
    let deleted = "autosaves/autosave-2.amsave";
    let agreed = [
        (removed, synced(10, 100, 100)),
        (deleted, synced(10, 100, 100)),
    ];
    let cache = Kept {
        purgeable: true,
        deleted: &[deleted],
        ..DRIVE
    };
    assert_eq!(
        plan_on(&cache, &agreed, &[], &Changes::default(), &[]),
        vec![
            Action::Refetch(removed.into()),
            Action::DeleteRemote(deleted.into(), 101),
        ]
    );
    let refetched = Changes {
        changed: vec![changed(removed, 100)],
        ..Changes::default()
    };
    assert_eq!(
        plan_on(&cache, &agreed[..1], &[], &refetched, &[]),
        vec![Action::Download(changed(removed, 100))]
    );
    assert_eq!(
        plan_on(
            &Kept {
                full: false,
                ..cache
            },
            &agreed,
            &[],
            &Changes::default(),
            &[]
        ),
        vec![Action::DeleteRemote(deleted.into(), 101)],
        "a push sends the deletion and fetches nothing"
    );
}

#[test]
fn a_file_icloud_drive_has_not_brought_down_is_left_alone() {
    let waiting = "saves/save-1.amsave";
    let changed_there = "saves/save-2.amsave";
    let deleted_there = "saves/save-3.amsave";
    let actions = plan_on(
        &Kept {
            arriving: &[waiting, changed_there, deleted_there],
            ..DRIVE
        },
        &[
            (waiting, synced(10, 100, 100)),
            (changed_there, synced(10, 100, 100)),
            (deleted_there, synced(10, 100, 100)),
        ],
        &[],
        &Changes {
            changed: vec![
                changed(changed_there, 300),
                delete_record(deleted_there, 300),
            ],
            ..Changes::default()
        },
        &[],
    );
    assert_eq!(
        actions,
        vec![
            Action::Keep(changed(changed_there, 300)),
            Action::Wait(changed_there.into()),
            Action::Keep(delete_record(deleted_there, 300)),
            Action::Wait(deleted_there.into()),
        ]
    );
}

#[test]
fn a_rewrite_of_another_size_in_the_same_second_is_a_new_save() {
    let name = "autosaves/autosave-1.amsave";
    let actions = plan_with(
        &[(name, synced(10, 100, 100))],
        &[(name, stamp(12, 100))],
        &Changes::default(),
        &[(name, 100)],
    );
    assert_eq!(actions, vec![Action::Upload(name.into())]);
}

#[test]
fn a_folder_that_cannot_be_read_deletes_nothing() {
    let name = "saves/save-1.amsave";
    let unreadable = Kept {
        unreadable: &["saves"],
        ..DRIVE
    };
    let agreed = [(name, synced(10, 100, 100))];
    assert_eq!(
        plan_on(&unreadable, &agreed, &[], &Changes::default(), &[]),
        vec![]
    );
    let changes = Changes {
        changed: vec![changed(name, 100)],
        ..Changes::default()
    };
    assert_eq!(
        plan_on(&unreadable, &agreed, &[], &changes, &[]),
        vec![Action::Keep(changed(name, 100)), Action::Wait(name.into())],
        "its changes wait until the folder can be read"
    );
}

/// A record as CloudKit holds it.
pub(super) struct Stored {
    /// The file, or `None` for a delete record.
    pub(super) bytes: Option<Vec<u8>>,
    pub(super) ticks: u64,
    pub(super) saved_at: u64,
}

impl Stored {
    fn version(&self) -> Version {
        Version {
            ticks: self.ticks,
            live: self.bytes.is_some(),
            saved_at: self.saved_at,
        }
    }
}

/// An iCloud account in memory: each record, and a log of changes a token
/// counts into.
#[derive(Default)]
pub(super) struct FakeCloud {
    pub(super) records: RefCell<BTreeMap<String, Stored>>,
    pub(super) log: RefCell<Vec<String>>,
    pub(super) account: RefCell<Option<String>>,
    /// Records the next fetch fails to copy down.
    pub(super) missed: RefCell<BTreeSet<String>>,
    /// Tokens older than this log position no longer work.
    pub(super) expired_before: Cell<usize>,
    /// The most records one fetch returns; zero for no limit.
    pub(super) batch: Cell<usize>,
    /// Whether the next fetch fails.
    pub(super) fail_fetch: Cell<bool>,
    /// Whether the next save fails.
    pub(super) fail_save: Cell<bool>,
    /// Records CloudKit will not take, as a file over its limits.
    pub(super) refused: RefCell<BTreeSet<String>>,
    /// Whether the next save lands only once [`Self::land`] runs.
    pub(super) delay_save: Cell<bool>,
    late: RefCell<Vec<(String, Stored)>>,
}

impl FakeCloud {
    pub(super) fn signed_in() -> Self {
        let cloud = Self::default();
        *cloud.account.borrow_mut() = Some("me".into());
        cloud
    }

    /// Lands every delayed save, oldest first.
    pub(super) fn land(&self) {
        let late = std::mem::take(&mut *self.late.borrow_mut());
        for (name, stored) in late {
            self.admit(&name, stored);
        }
    }

    /// Empties the database, as resetting the Development environment does.
    pub(super) fn wipe(&self) {
        self.records.borrow_mut().clear();
        self.late.borrow_mut().clear();
        // Every token given so far stops working; the entry keeps the next
        // one given apart from them.
        let mut log = self.log.borrow_mut();
        log.push(String::new());
        self.expired_before.set(log.len());
    }

    /// The save time of the file the cloud holds under `name`.
    pub(super) fn file(&self, name: &str) -> Option<u64> {
        self.records
            .borrow()
            .get(name)
            .filter(|stored| stored.bytes.is_some())
            .map(|stored| stored.saved_at)
    }

    /// Every file the cloud holds, with its save time.
    pub(super) fn files(&self) -> BTreeMap<String, u64> {
        self.records
            .borrow()
            .iter()
            .filter(|(_, stored)| stored.bytes.is_some())
            .map(|(name, stored)| (name.clone(), stored.saved_at))
            .collect()
    }

    /// Whether the cloud holds, or will hold, this save.
    pub(super) fn holds(&self, saved_at: u64) -> bool {
        self.files().values().any(|at| *at == saved_at)
            || self
                .late
                .borrow()
                .iter()
                .any(|(_, stored)| stored.bytes.is_some() && stored.saved_at == saved_at)
    }

    /// Takes a version unless one further along is already here, as the
    /// bridge's check and conditional save do; whether nothing stood in its way.
    fn admit(&self, name: &str, stored: Stored) -> bool {
        let held = self.records.borrow().get(name).map(Stored::version);
        match held.map(|held| held.cmp(&stored.version())) {
            Some(Ordering::Greater) => false,
            Some(Ordering::Equal) => true,
            _ => {
                self.records.borrow_mut().insert(name.to_owned(), stored);
                self.log.borrow_mut().push(name.to_owned());
                true
            }
        }
    }

    fn save(&self, name: &str, stored: Stored) -> io::Result<()> {
        if self.fail_save.take() || self.refused.borrow().contains(name) {
            return Err(io::Error::other("no network"));
        }
        if self.delay_save.take() {
            self.late.borrow_mut().push((name.to_owned(), stored));
            return Ok(());
        }
        if self.admit(name, stored) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "CloudKit holds a version further along",
            ))
        }
    }
}

impl Remote for FakeCloud {
    fn account(&self) -> Option<String> {
        self.account.borrow().clone()
    }
    fn fetch(&self, token: Option<&str>, staging: &Path) -> io::Result<Changes> {
        if self.fail_fetch.take() {
            return Err(io::Error::other("no network"));
        }
        let log = self.log.borrow();
        let asked: Option<usize> = token.map(|token| token.parse().unwrap());
        let restarted =
            asked.is_some_and(|from| from > log.len() || from < self.expired_before.get());
        let from = if restarted { 0 } else { asked.unwrap_or(0) };
        let mut names = BTreeSet::new();
        let mut end = from;
        while end < log.len() && (self.batch.get() == 0 || names.len() < self.batch.get()) {
            names.insert(&log[end]);
            end += 1;
        }
        let mut changes = Changes {
            token: Some(end.to_string()),
            more: end < log.len(),
            restarted,
            ..Changes::default()
        };
        for name in names {
            if self.missed.borrow_mut().remove(name) {
                changes.failed += 1;
                continue;
            }
            let records = self.records.borrow();
            let Some(stored) = records.get(name) else {
                continue;
            };
            let staged = match &stored.bytes {
                Some(bytes) => {
                    let staged = staging.join(name.replace('/', "-"));
                    std::fs::write(&staged, bytes)?;
                    Some(staged)
                }
                None => None,
            };
            changes.changed.push(Changed {
                name: name.clone(),
                ticks: stored.ticks,
                saved_at: stored.saved_at,
                staged,
            });
        }
        Ok(changes)
    }
    fn upload(&self, name: &str, path: &Path, ticks: u64, saved_at: u64) -> io::Result<()> {
        let bytes = Some(std::fs::read(path)?);
        self.save(
            name,
            Stored {
                bytes,
                ticks,
                saved_at,
            },
        )
    }
    fn delete(&self, name: &str, ticks: u64) -> io::Result<()> {
        self.save(
            name,
            Stored {
                bytes: None,
                ticks,
                saved_at: 0,
            },
        )
    }
}

/// The game, as far as a pass can tell.
pub(super) struct Game {
    pub(super) deleted: RefCell<Vec<PathBuf>>,
    pub(super) answers: RefCell<Vec<Answer>>,
    pub(super) arrivals: Cell<bool>,
}

impl Default for Game {
    fn default() -> Self {
        Self {
            deleted: RefCell::default(),
            answers: RefCell::default(),
            arrivals: Cell::new(true),
        }
    }
}

impl Host for Game {
    fn take_deleted(&self) -> Vec<PathBuf> {
        self.deleted.take()
    }
    fn take_answers(&self) -> Vec<Answer> {
        self.answers.take()
    }
    fn allows_arrivals(&self) -> bool {
        self.arrivals.get()
    }
}

/// An Apple TV's folders, in a cache the system may clear.
pub(super) fn tv() -> Places {
    Places {
        purgeable: true,
        ..device("tv")
    }
}

/// A device's folders under a fresh scratch directory.
pub(super) fn device(name: &str) -> Places {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "aftermine-mirror-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::remove_dir_all(&root).ok();
    let folders = ["autosaves", "matches", "saves", "replays"].map(|label| {
        let dir = root.join(label);
        std::fs::create_dir_all(&dir).unwrap();
        (label, dir)
    });
    Places {
        manifest: root.join("cloud-mirror.json"),
        staging: root.join("cloud-staging"),
        folders: folders.to_vec(),
        purgeable: false,
        dataless: |_| false,
    }
}

/// Writes a record a pass can read the save time of.
pub(super) fn write_record(
    places: &Places,
    name: &str,
    saved_at: u64,
    description: &str,
) -> PathBuf {
    let path = places.path(name).unwrap();
    let mut replay: GameReplay =
        chassis::replay::Replay::new(SIM_VERSION, "test", aftermine_sim::Scenario::skirmish());
    replay.meta.ticks = Some(0);
    replay.meta.saved_at = Some(saved_at);
    replay.meta.description = Some(description.to_owned());
    replay.save(&path).unwrap();
    path
}

fn sync(cloud: &FakeCloud, places: &Places) -> Outcome {
    pass(cloud, places, Mode::Full, &Game::default()).unwrap()
}

/// Writes a file with a modification time no other write shares, as a real
/// disk's timer would show given enough time between writes.
pub(super) fn put_file(path: &Path, bytes: &[u8], saved_at: u64) {
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let write = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = u32::try_from(write % 1_000_000).unwrap() * 1_000;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::new(saved_at, nanos))
        .unwrap();
}

/// Writes a save holding only its header, which is all a pass reads and
/// keeps thousands of them fast.
pub(super) fn write_save(path: &Path, ticks: u64, saved_at: u64) {
    let header = serde_json::json!({
        "version": 1,
        "meta": {
            "sim_version": aftermine_sim::SIM_VERSION,
            "build": "test",
            "kind": "save",
            "ticks": ticks,
            "saved_at": saved_at,
        },
        "map": "test",
        "seats": 1,
        "teams": 1,
        "compressed_bytes": 0,
        "decoded_bytes": 0,
    })
    .to_string();
    let mut bytes = b"AFTERSAV".to_vec();
    bytes.extend(u32::try_from(header.len()).unwrap().to_le_bytes());
    bytes.extend(header.as_bytes());
    put_file(path, &bytes, saved_at);
}

/// Deletes the file at `path` the way the game does, and syncs.
fn game_delete(cloud: &FakeCloud, places: &Places, path: &Path) -> Outcome {
    let game = Game::default();
    game.deleted.borrow_mut().push(path.to_owned());
    std::fs::remove_file(path).unwrap();
    pass(cloud, places, Mode::Full, &game).unwrap()
}

fn read_ticks(path: &Path) -> u64 {
    crate::saved_game::inspect(path)
        .unwrap()
        .meta
        .ticks
        .unwrap()
}

/// Syncs with the player's answer about `name`.
fn answer(cloud: &FakeCloud, places: &Places, answer: Answer) -> Outcome {
    let game = Game::default();
    game.answers.borrow_mut().push(answer);
    pass(cloud, places, Mode::Full, &game).unwrap()
}

#[test]
fn a_record_saved_on_one_device_arrives_on_another_and_a_deletion_follows() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    let name = "saves/save-1.json";
    let saved = write_record(&tv, name, 1_000, "from the TV");
    sync(&cloud, &tv);
    sync(&cloud, &ipad);
    let arrived = ipad.path(name).unwrap();
    assert_eq!(
        std::fs::read(&arrived).unwrap(),
        std::fs::read(&saved).unwrap()
    );
    assert_eq!(
        std::fs::metadata(&arrived).unwrap().modified().unwrap(),
        UNIX_EPOCH + Duration::from_secs(1_000),
        "a download takes its save time as its file time"
    );
    // Passes that find nothing new move nothing.
    sync(&cloud, &tv);
    sync(&cloud, &ipad);
    let uploads = cloud.log.borrow().len();
    sync(&cloud, &tv);
    sync(&cloud, &ipad);
    assert_eq!(cloud.log.borrow().len(), uploads, "nothing bounces back");

    game_delete(&cloud, &ipad, &arrived);
    sync(&cloud, &tv);
    assert!(!saved.exists(), "the deletion crossed to the TV");
}

#[test]
fn a_finder_delete_waits_for_the_players_answer() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    let name = "saves/save-1.json";
    let on_tv = write_record(&tv, name, 1_000, "kept");
    sync(&cloud, &tv);
    sync(&cloud, &ipad);
    let on_ipad = ipad.path(name).unwrap();

    std::fs::remove_file(&on_ipad).unwrap();
    let outcome = sync(&cloud, &ipad);
    assert_eq!(outcome.missing, vec![name.to_owned()]);
    assert!(!on_ipad.exists(), "nothing comes back unasked");
    assert_eq!(cloud.file(name), Some(1_000), "and nothing is deleted");

    let outcome = answer(&cloud, &ipad, Answer::BringBack(name.into()));
    assert!(outcome.again);
    sync(&cloud, &ipad);
    assert!(on_ipad.exists(), "bringing it back fetches it");

    std::fs::remove_file(&on_ipad).unwrap();
    sync(&cloud, &ipad);
    let outcome = answer(&cloud, &ipad, Answer::Delete(name.into()));
    assert!(outcome.missing.is_empty());
    assert_eq!(cloud.file(name), None, "deleting it reaches CloudKit");
    sync(&cloud, &tv);
    assert!(!on_tv.exists(), "and the TV");
}

#[test]
fn a_file_late_from_icloud_drive_needs_no_answer() {
    let cloud = FakeCloud::signed_in();
    let ipad = device("ipad");
    let name = "saves/save-1.json";
    let path = write_record(&ipad, name, 1_000, "on its way back");
    sync(&cloud, &ipad);
    sync(&cloud, &ipad);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(sync(&cloud, &ipad).missing, vec![name.to_owned()]);
    std::fs::write(&path, bytes).unwrap();
    assert!(sync(&cloud, &ipad).missing.is_empty());
    assert_eq!(cloud.file(name), Some(1_000));
}

#[test]
fn a_folder_that_vanished_deletes_nothing() {
    let cloud = FakeCloud::signed_in();
    let ipad = device("ipad");
    let name = "saves/save-1.json";
    let path = write_record(&ipad, name, 1_000, "kept");
    sync(&cloud, &ipad);
    sync(&cloud, &ipad);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    assert_eq!(sync(&cloud, &ipad).missing, vec![name.to_owned()]);
    assert_eq!(cloud.file(name), Some(1_000));
    answer(&cloud, &ipad, Answer::BringBack(name.into()));
    sync(&cloud, &ipad);
    assert!(path.exists());
}

#[test]
fn a_file_renamed_to_what_cloudkit_cannot_name_stays_on_the_device() {
    let cloud = FakeCloud::signed_in();
    let ipad = device("ipad");
    let path = write_record(&ipad, "saves/save-1.json", 1_000, "renamed");
    std::fs::rename(&path, path.with_file_name("sauvegardé.json")).unwrap();
    let long = format!("{}.json", "x".repeat(MAX_FILE_NAME));
    std::fs::write(path.with_file_name(long), b"{}").unwrap();
    sync(&cloud, &ipad);
    assert!(cloud.records.borrow().is_empty());
}

#[test]
fn a_cleared_cache_brings_records_back_instead_of_deleting_them() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let name = "autosaves/autosave-1.json";
    write_record(&tv, name, 1_000, "kept");
    sync(&cloud, &tv);
    // tvOS clears the whole cache: the records and the manifest beside them.
    let root = tv.manifest.parent().unwrap().to_owned();
    std::fs::remove_dir_all(&root).unwrap();
    for (_, dir) in &tv.folders {
        std::fs::create_dir_all(dir).unwrap();
    }
    sync(&cloud, &tv);
    assert!(tv.path(name).unwrap().exists());
    assert_eq!(cloud.file(name), Some(1_000));
}

#[test]
fn a_record_that_failed_to_arrive_is_fetched_again() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    let name = "matches/match-1.json";
    write_record(&tv, name, 1_000, "a match");
    sync(&cloud, &tv);
    cloud.missed.borrow_mut().insert(name.into());
    let outcome = sync(&cloud, &ipad);
    assert_eq!(outcome.pending, 1);
    assert!(!ipad.path(name).unwrap().exists());
    sync(&cloud, &ipad);
    assert!(ipad.path(name).unwrap().exists());
}

#[test]
fn a_file_the_system_removed_comes_back_and_a_deletion_the_game_made_goes_up() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let removed = "autosaves/autosave-1.json";
    let deleted = "autosaves/autosave-2.json";
    let removed_path = write_record(&tv, removed, 1_000, "removed by tvOS");
    let deleted_path = write_record(&tv, deleted, 1_000, "deleted by the game");
    // The second pass's token covers the first pass's uploads.
    sync(&cloud, &tv);
    sync(&cloud, &tv);
    std::fs::remove_file(&removed_path).unwrap();
    let outcome = game_delete(&cloud, &tv, &deleted_path);
    assert!(outcome.again, "a removed file asks for every record again");
    assert_eq!(cloud.file(removed), Some(1_000));
    assert_eq!(cloud.file(deleted), None);
    sync(&cloud, &tv);
    assert!(removed_path.exists());
    assert!(!deleted_path.exists());
}

#[test]
fn a_file_waiting_on_icloud_drive_is_not_missing() {
    let cloud = FakeCloud::signed_in();
    let ipad = device("ipad");
    let name = "saves/save-1.json";
    let path = write_record(&ipad, name, 1_000, "evicted");
    sync(&cloud, &ipad);
    // iOS frees space by swapping the file for a stand-in.
    std::fs::remove_file(&path).unwrap();
    std::fs::write(path.with_file_name(".save-1.json.icloud"), b"").unwrap();
    assert!(sync(&cloud, &ipad).missing.is_empty());
    assert_eq!(cloud.file(name), Some(1_000));
}

#[test]
fn a_dataless_file_on_a_mac_is_on_its_way_not_found() {
    let mac = Places {
        dataless: |path| path.ends_with("save-1.json"),
        ..device("mac")
    };
    let name = "saves/save-1.json";
    write_record(&mac, name, 1_000, "evicted");
    write_record(&mac, "saves/save-2.json", 1_000, "here");
    let listing = mac.local();
    assert!(listing.arriving.contains(name));
    assert!(!listing.found.contains_key(name));
    assert!(listing.found.contains_key("saves/save-2.json"));
}

#[test]
fn a_match_resumed_from_an_old_copy_cannot_replace_a_longer_session() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    let name = "autosaves/autosave-1.json";
    let on_tv = tv.path(name).unwrap();
    write_save(&on_tv, 100, 1_000);
    sync(&cloud, &tv);
    sync(&cloud, &ipad);
    // A long session on the iPad, then the TV plays on from where it stopped,
    // saving later by the clock.
    write_save(&ipad.path(name).unwrap(), 40_000, 2_000);
    sync(&cloud, &ipad);
    write_save(&on_tv, 160, 3_000);
    let in_match = Game::default();
    in_match.arrivals.set(false);
    pass(&cloud, &tv, Mode::Push, &in_match).unwrap();
    assert_eq!(cloud.file(name), Some(2_000), "CloudKit refuses it");
    // Back at Home, the TV takes the session further along.
    sync(&cloud, &tv);
    assert_eq!(read_ticks(&on_tv), 40_000);
    assert_eq!(cloud.file(name), Some(2_000));
}

#[test]
fn a_delete_of_an_old_copy_cannot_remove_a_longer_session() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    let name = "saves/save-1.json";
    let on_tv = tv.path(name).unwrap();
    write_save(&on_tv, 100, 1_000);
    sync(&cloud, &tv);
    sync(&cloud, &ipad);
    write_save(&ipad.path(name).unwrap(), 40_000, 2_000);
    sync(&cloud, &ipad);
    game_delete(&cloud, &tv, &on_tv);
    assert_eq!(cloud.file(name), Some(2_000));
    assert!(on_tv.exists(), "and the TV takes it");
    assert_eq!(read_ticks(&on_tv), 40_000);
}

#[test]
fn an_autosave_written_again_after_its_deletion_goes_up() {
    // A named save removes the match's autosave, and playing on writes it again.
    let cloud = FakeCloud::signed_in();
    let ipad = device("ipad");
    let mac = device("mac");
    let name = "autosaves/autosave-1.amsave";
    let on_ipad = ipad.path(name).unwrap();
    let on_mac = mac.path(name).unwrap();
    write_save(&on_ipad, 100, 1_000);
    sync(&cloud, &ipad);
    sync(&cloud, &mac);
    game_delete(&cloud, &ipad, &on_ipad);
    sync(&cloud, &mac);
    assert!(!on_mac.exists(), "the deletion reaches the other device");
    write_save(&on_ipad, 101, 2_000);
    sync(&cloud, &ipad);
    assert_eq!(cloud.file(name), Some(2_000), "it ties the delete and wins");
    sync(&cloud, &mac);
    assert_eq!(read_ticks(&on_mac), 101);
    // Written again before the deletion ever went up, it still goes up.
    let game = Game::default();
    game.deleted.borrow_mut().push(on_ipad.clone());
    std::fs::remove_file(&on_ipad).unwrap();
    write_save(&on_ipad, 200, 3_000);
    pass(&cloud, &ipad, Mode::Full, &game).unwrap();
    assert_eq!(read_ticks(&on_ipad), 200);
    assert_eq!(cloud.file(name), Some(3_000));
}

#[test]
fn a_refused_delete_finds_what_cloudkit_holds_instead_of_retrying_for_ever() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    let name = "saves/save-1.json";
    let on_tv = tv.path(name).unwrap();
    write_save(&on_tv, 100, 1_000);
    sync(&cloud, &tv);
    sync(&cloud, &tv);
    sync(&cloud, &ipad);
    // The iPad moves on, and the TV sees that while it holds a save further
    // still, which it fails to send and then deletes.
    write_save(&ipad.path(name).unwrap(), 500, 2_000);
    sync(&cloud, &ipad);
    write_save(&on_tv, 900, 3_000);
    cloud.fail_save.set(true);
    sync(&cloud, &tv);
    assert_eq!(cloud.file(name), Some(2_000));
    let outcome = game_delete(&cloud, &tv, &on_tv);
    assert!(outcome.again);
    sync(&cloud, &tv);
    assert_eq!(read_ticks(&on_tv), 500);
    assert_eq!(cloud.file(name), Some(2_000));
}

#[test]
fn a_late_older_save_never_replaces_a_newer_one() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    let name = "autosaves/autosave-1.json";
    write_record(&ipad, name, 1_000, "older, on a slow link");
    cloud.delay_save.set(true);
    sync(&cloud, &ipad);
    write_record(&tv, name, 2_000, "newer");
    sync(&cloud, &tv);
    cloud.land();
    assert_eq!(cloud.file(name), Some(2_000));
    sync(&cloud, &ipad);
    assert!(
        std::fs::read_to_string(ipad.path(name).unwrap())
            .unwrap()
            .contains("newer")
    );
}

/// Runs `arrive` once the pass has scanned the folders, as iCloud Drive might
/// deliver a file while a pass is under way.
struct ArrivesMidPass<F>(Cell<Option<F>>);

impl<F: FnOnce()> Host for ArrivesMidPass<F> {
    fn take_deleted(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    fn take_answers(&self) -> Vec<Answer> {
        Vec::new()
    }
    fn allows_arrivals(&self) -> bool {
        if let Some(arrive) = self.0.take() {
            arrive();
        }
        true
    }
}

#[test]
fn a_file_arriving_mid_pass_is_not_replaced_by_an_older_download() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    let agreed = "saves/save-agreed.amsave";
    let unseen = "saves/save-unseen.amsave";
    write_save(&tv.path(agreed).unwrap(), 50, 1_000);
    sync(&cloud, &tv);
    sync(&cloud, &ipad);
    write_save(&tv.path(agreed).unwrap(), 90, 2_000);
    write_save(&tv.path(unseen).unwrap(), 90, 2_000);
    sync(&cloud, &tv);

    let arrive = || {
        write_save(&ipad.path(agreed).unwrap(), 100, 3_000);
        write_save(&ipad.path(unseen).unwrap(), 100, 3_000);
    };
    pass(
        &cloud,
        &ipad,
        Mode::Full,
        &ArrivesMidPass(Cell::new(Some(arrive))),
    )
    .unwrap();
    assert_eq!(read_ticks(&ipad.path(agreed).unwrap()), 100);
    assert_eq!(read_ticks(&ipad.path(unseen).unwrap()), 100);

    sync(&cloud, &ipad);
    sync(&cloud, &tv);
    assert_eq!(read_ticks(&tv.path(agreed).unwrap()), 100);
    assert_eq!(read_ticks(&tv.path(unseen).unwrap()), 100);
}

#[test]
fn a_file_arriving_mid_pass_outlasts_an_older_delete_record() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    let name = "saves/save-1.amsave";
    write_save(&tv.path(name).unwrap(), 50, 1_000);
    sync(&cloud, &tv);
    sync(&cloud, &ipad);
    game_delete(&cloud, &tv, &tv.path(name).unwrap());

    let arrive = || write_save(&ipad.path(name).unwrap(), 100, 3_000);
    pass(
        &cloud,
        &ipad,
        Mode::Full,
        &ArrivesMidPass(Cell::new(Some(arrive))),
    )
    .unwrap();
    assert_eq!(read_ticks(&ipad.path(name).unwrap()), 100);

    sync(&cloud, &ipad);
    sync(&cloud, &tv);
    assert_eq!(read_ticks(&tv.path(name).unwrap()), 100);
}

#[test]
fn a_push_sends_new_files_and_deletions_and_takes_nothing_in() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    let doomed = "autosaves/autosave-1.json";
    let doomed_path = write_record(&tv, doomed, 500, "cleaned up mid-match");
    sync(&cloud, &tv);
    write_record(&ipad, "saves/save-1.json", 1_000, "on the iPad");
    sync(&cloud, &ipad);
    // During a match, with nothing allowed to arrive.
    let game = Game::default();
    game.arrivals.set(false);
    write_record(&tv, "autosaves/autosave-2.json", 2_000, "mid-match");
    game.deleted.borrow_mut().push(doomed_path.clone());
    std::fs::remove_file(doomed_path).unwrap();
    pass(&cloud, &tv, Mode::Push, &game).unwrap();
    assert_eq!(cloud.file("autosaves/autosave-2.json"), Some(2_000));
    assert_eq!(cloud.file(doomed), None);
    assert!(!tv.path("saves/save-1.json").unwrap().exists());
}

#[test]
fn a_file_that_will_not_upload_holds_up_nothing_else() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    let stuck = "matches/match-1.json";
    write_record(&ipad, stuck, 1_000, "too big for CloudKit");
    cloud.refused.borrow_mut().insert(stuck.into());
    write_record(&tv, "saves/save-1.json", 2_000, "from the TV");
    sync(&cloud, &tv);
    sync(&cloud, &ipad);
    assert!(ipad.path("saves/save-1.json").unwrap().exists());
    write_record(&tv, "saves/save-2.json", 3_000, "from the TV, later");
    sync(&cloud, &tv);
    let uploads = cloud.log.borrow().len();
    sync(&cloud, &ipad);
    assert!(ipad.path("saves/save-2.json").unwrap().exists());
    assert_eq!(cloud.log.borrow().len(), uploads);
    cloud.refused.borrow_mut().clear();
    sync(&cloud, &ipad);
    assert_eq!(cloud.file(stuck), Some(1_000), "and it goes when it can");
}

#[test]
fn an_older_local_file_never_overwrites_a_newer_one_in_a_later_batch() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    let name = "autosaves/autosave-9.json";
    write_record(&tv, "autosaves/autosave-1.json", 1_000, "first in the log");
    write_record(&tv, name, 5_000, "newer");
    sync(&cloud, &tv);
    write_record(&ipad, name, 2_000, "older");
    cloud.batch.set(1);
    let first = sync(&cloud, &ipad);
    assert!(first.again, "more changes wait");
    assert_eq!(cloud.file(name), Some(5_000));
    cloud.batch.set(0);
    sync(&cloud, &ipad);
    assert_eq!(cloud.file(name), Some(5_000));
    assert!(
        std::fs::read_to_string(ipad.path(name).unwrap())
            .unwrap()
            .contains("newer")
    );
}

#[test]
fn files_go_back_up_after_cloudkit_loses_everything() {
    let cloud = FakeCloud::signed_in();
    let ipad = device("ipad");
    let name = "saves/save-1.json";
    let path = write_record(&ipad, name, 1_000, "only here now");
    sync(&cloud, &ipad);
    sync(&cloud, &ipad);
    cloud.wipe();
    sync(&cloud, &ipad);
    assert!(path.exists());
    assert_eq!(cloud.file(name), Some(1_000));
}

#[test]
fn an_expired_token_starts_over_without_moving_or_losing_anything() {
    let cloud = FakeCloud::signed_in();
    let ipad = device("ipad");
    let tv = tv();
    let name = "saves/save-1.json";
    let path = write_record(&ipad, name, 1_000, "kept");
    sync(&cloud, &ipad);
    sync(&cloud, &ipad);
    write_record(&tv, "saves/save-2.json", 2_000, "from the TV");
    sync(&cloud, &tv);
    let uploads = cloud.log.borrow().len();
    cloud.expired_before.set(uploads);
    sync(&cloud, &ipad);
    assert!(path.exists());
    assert!(ipad.path("saves/save-2.json").unwrap().exists());
    assert_eq!(cloud.log.borrow().len(), uploads, "nothing was sent again");
}

#[test]
fn files_wait_for_home_and_arrive_once_it_opens() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    let name = "matches/match-1.json";
    write_record(&tv, name, 1_000, "a match");
    sync(&cloud, &tv);
    let game = Game::default();
    game.arrivals.set(false);
    let outcome = pass(&cloud, &ipad, Mode::Full, &game).unwrap();
    assert_eq!(outcome.pending, 1);
    assert!(
        !outcome.again,
        "waiting for Home is no reason to hurry back"
    );
    assert!(!ipad.path(name).unwrap().exists());
    game.arrivals.set(true);
    pass(&cloud, &ipad, Mode::Full, &game).unwrap();
    assert!(ipad.path(name).unwrap().exists());
}

#[test]
fn a_different_account_starts_the_manifest_over() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let name = "saves/save-1.json";
    write_record(&tv, name, 1_000, "mine");
    sync(&cloud, &tv);
    let other = FakeCloud::default();
    *other.account.borrow_mut() = Some("someone else".into());
    sync(&other, &tv);
    assert_eq!(
        other.file(name),
        Some(1_000),
        "the new account gets this device's records"
    );
    assert!(tv.path(name).unwrap().exists());
}

#[test]
fn a_signed_out_device_leaves_everything_alone() {
    let cloud = FakeCloud::default();
    let tv = tv();
    write_record(&tv, "saves/save-1.json", 1_000, "offline");
    assert_eq!(sync(&cloud, &tv), Outcome::default());
    assert!(cloud.records.borrow().is_empty());
    assert!(!tv.manifest.exists());
}

#[test]
fn a_deleted_save_stays_deleted_when_icloud_drive_delivers_a_late_copy() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let mac = device("mac");
    let name = "saves/save-1.json";
    let on_tv = write_record(&tv, name, 1_000, "made on the TV");
    sync(&cloud, &tv);
    // iCloud Drive carries an iPad's copy to the Mac, whose app is closed.
    let on_mac = mac.path(name).unwrap();
    std::fs::copy(&on_tv, &on_mac).unwrap();
    game_delete(&cloud, &tv, &on_tv);
    assert_eq!(cloud.file(name), None);
    sync(&cloud, &mac);
    assert!(!on_mac.exists(), "the Mac's copy is older than the delete");
    assert_eq!(cloud.file(name), None, "and does not bring it back");
}

#[test]
fn record_names_never_leave_their_folder() {
    let tv = tv();
    for name in [
        "saves/../escape.json",
        "nowhere/x.json",
        "saves/",
        "saves/.hidden",
        "x",
    ] {
        assert_eq!(tv.path(name), None, "{name}");
    }
}
