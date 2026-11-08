use super::*;
use crate::game::GameReplay;
use aftermine_sim::SIM_VERSION;
use std::cell::RefCell;
use std::collections::BTreeSet;

fn stamp(size: u64, secs: u64) -> Stamp {
    Stamp {
        size,
        secs,
        nanos: 0,
    }
}

fn synced(size: u64, secs: u64, saved_at: u64) -> Synced {
    Synced {
        stamp: stamp(size, secs),
        saved_at,
    }
}

fn changed(name: &str, saved_at: u64) -> Changed {
    Changed {
        name: name.to_owned(),
        saved_at,
        staged: PathBuf::from(format!("/staging/{name}")),
    }
}

/// How a planning device keeps its files.
struct Kept<'a> {
    purgeable: bool,
    arriving: &'a [&'a str],
    deleted: &'a [&'a str],
}

/// An iCloud Drive folder with every file on the device.
const DRIVE: Kept = Kept {
    purgeable: false,
    arriving: &[],
    deleted: &[],
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
    let local = local
        .iter()
        .map(|(name, stamp)| ((*name).to_owned(), *stamp))
        .collect();
    let times: BTreeMap<String, u64> = times
        .iter()
        .map(|(name, at)| ((*name).to_owned(), *at))
        .collect();
    let here = Here {
        local: &local,
        arriving: &names(kept.arriving),
        deleted: &names(kept.deleted),
        purgeable: kept.purgeable,
    };
    plan(&agreed, &here, changes, &mut |name| {
        times.get(name).copied()
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
            Action::Adopt(name.into(), 250)
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
    assert_eq!(actions, vec![Action::Adopt(name.into(), 100)]);
}

#[test]
fn deletions_cross_unless_the_other_side_changed_the_record() {
    let gone_here = "autosaves/autosave-1.amsave";
    let gone_there = "autosaves/autosave-2.amsave";
    let edited_here = "autosaves/autosave-3.amsave";
    let actions = plan_with(
        &[
            (gone_here, synced(10, 100, 100)),
            (gone_there, synced(10, 100, 100)),
            (edited_here, synced(10, 100, 100)),
        ],
        &[(gone_there, stamp(10, 100)), (edited_here, stamp(11, 200))],
        &Changes {
            deleted: vec![gone_there.into(), edited_here.into()],
            ..Changes::default()
        },
        &[(edited_here, 200)],
    );
    assert_eq!(
        actions,
        vec![
            Action::DeleteLocal(gone_there.into()),
            Action::Upload(edited_here.into()),
            Action::DeleteRemote(gone_here.into()),
        ]
    );
}

#[test]
fn a_change_there_outranks_a_deletion_here() {
    let name = "autosaves/autosave-1.amsave";
    let actions = plan_with(
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
fn a_named_save_colliding_with_an_unsynced_one_moves_aside() {
    let name = "saves/save-0000012000.amsave";
    let actions = plan_with(
        &[],
        &[(name, stamp(10, 100))],
        &Changes {
            changed: vec![changed(name, 300)],
            ..Changes::default()
        },
        &[(name, 100)],
    );
    assert_eq!(
        actions,
        vec![Action::MoveAsideThenDownload(changed(name, 300))]
    );
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
fn where_the_system_removes_files_only_the_games_deletions_go_up() {
    let removed = "autosaves/autosave-1.amsave";
    let deleted = "autosaves/autosave-2.amsave";
    let agreed = [
        (removed, synced(10, 100, 100)),
        (deleted, synced(10, 100, 100)),
    ];
    let cache = Kept {
        purgeable: true,
        arriving: &[],
        deleted: &[deleted],
    };
    assert_eq!(
        plan_on(&cache, &agreed, &[], &Changes::default(), &[]),
        vec![
            Action::Refetch(removed.into()),
            Action::DeleteRemote(deleted.into()),
        ]
    );
    // A removed file that comes back in a fetch is downloaded, not deleted.
    let refetched = Changes {
        changed: vec![changed(removed, 100)],
        ..Changes::default()
    };
    assert_eq!(
        plan_on(&cache, &agreed[..1], &[], &refetched, &[]),
        vec![Action::Download(changed(removed, 100))]
    );
    assert_eq!(
        plan_on(&DRIVE, &agreed[..1], &[], &refetched, &[]),
        vec![
            Action::Keep(changed(removed, 100)),
            Action::DeleteRemote(removed.into())
        ]
    );
}

#[test]
fn a_file_icloud_drive_has_not_brought_down_is_left_alone() {
    let waiting = "saves/save-1.amsave";
    let changed_there = "saves/save-2.amsave";
    let deleted_there = "saves/save-3.amsave";
    let actions = plan_on(
        &Kept {
            purgeable: false,
            arriving: &[waiting, changed_there, deleted_there],
            deleted: &[],
        },
        &[
            (waiting, synced(10, 100, 100)),
            (changed_there, synced(10, 100, 100)),
            (deleted_there, synced(10, 100, 100)),
        ],
        &[],
        &Changes {
            changed: vec![changed(changed_there, 300)],
            deleted: vec![deleted_there.into()],
            ..Changes::default()
        },
        &[],
    );
    assert_eq!(
        actions,
        vec![
            Action::Keep(changed(changed_there, 300)),
            Action::Wait(changed_there.into()),
            Action::Wait(deleted_there.into()),
        ]
    );
}

/// An iCloud account in memory: each record's bytes and save time, and a
/// log of changes a token counts into.
#[derive(Default)]
struct FakeCloud {
    records: RefCell<BTreeMap<String, (Vec<u8>, u64)>>,
    log: RefCell<Vec<String>>,
    account: RefCell<Option<String>>,
    /// Records the next fetch fails to copy down.
    missed: RefCell<BTreeSet<String>>,
}

impl FakeCloud {
    fn signed_in() -> Self {
        let cloud = Self::default();
        *cloud.account.borrow_mut() = Some("me".into());
        cloud
    }
}

impl Remote for FakeCloud {
    fn account(&self) -> Option<String> {
        self.account.borrow().clone()
    }
    fn fetch(&self, token: Option<&str>, staging: &Path) -> io::Result<Changes> {
        let from: usize = token.map_or(0, |token| token.parse().unwrap());
        let log = self.log.borrow();
        let names: BTreeSet<&String> = log[from..].iter().collect();
        let mut changes = Changes {
            token: Some(log.len().to_string()),
            ..Changes::default()
        };
        for name in names {
            if self.missed.borrow_mut().remove(name) {
                changes.failed += 1;
                continue;
            }
            match self.records.borrow().get(name) {
                Some((bytes, saved_at)) => {
                    let staged = staging.join(name.replace('/', "-"));
                    std::fs::write(&staged, bytes)?;
                    changes.changed.push(Changed {
                        name: name.clone(),
                        saved_at: *saved_at,
                        staged,
                    });
                }
                None => changes.deleted.push(name.clone()),
            }
        }
        Ok(changes)
    }
    fn upload(&self, name: &str, path: &Path, saved_at: u64, _size: u64) -> io::Result<()> {
        self.records
            .borrow_mut()
            .insert(name.to_owned(), (std::fs::read(path)?, saved_at));
        self.log.borrow_mut().push(name.to_owned());
        Ok(())
    }
    fn delete(&self, name: &str) -> io::Result<()> {
        self.records.borrow_mut().remove(name);
        self.log.borrow_mut().push(name.to_owned());
        Ok(())
    }
}

/// An Apple TV's folders, in a cache the system may clear.
fn tv() -> Places {
    Places {
        purgeable: true,
        ..device("tv")
    }
}

/// A device's folders under a fresh scratch directory.
fn device(name: &str) -> Places {
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
    }
}

/// Writes a record a pass can read the save time of.
fn write_record(places: &Places, name: &str, saved_at: u64, description: &str) -> PathBuf {
    let path = places.path(name).unwrap();
    let mut replay: GameReplay =
        chassis::replay::Replay::new(SIM_VERSION, "test", aftermine_sim::Scenario::skirmish());
    replay.meta.saved_at = Some(saved_at);
    replay.meta.description = Some(description.to_owned());
    replay.save(&path).unwrap();
    path
}

fn sync(cloud: &FakeCloud, places: &Places) -> Outcome {
    pass(cloud, places, Mode::Full, Vec::new).unwrap()
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

    std::fs::remove_file(&arrived).unwrap();
    sync(&cloud, &ipad);
    sync(&cloud, &tv);
    assert!(!saved.exists(), "the deletion crossed to the TV");
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
    assert!(cloud.records.borrow().contains_key(name));
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
    std::fs::remove_file(&deleted_path).unwrap();
    let outcome = pass(&cloud, &tv, Mode::Full, || vec![deleted_path.clone()]).unwrap();
    assert!(outcome.again, "a removed file asks for every record again");
    assert!(cloud.records.borrow().contains_key(removed));
    assert!(!cloud.records.borrow().contains_key(deleted));
    sync(&cloud, &tv);
    assert!(removed_path.exists());
    assert!(!deleted_path.exists());
}

#[test]
fn a_file_waiting_on_icloud_drive_is_not_deleted() {
    let cloud = FakeCloud::signed_in();
    let ipad = device("ipad");
    let name = "saves/save-1.json";
    let path = write_record(&ipad, name, 1_000, "evicted");
    sync(&cloud, &ipad);
    // iOS frees space by swapping the file for a placeholder.
    std::fs::remove_file(&path).unwrap();
    std::fs::write(path.with_file_name(".save-1.json.icloud"), b"").unwrap();
    sync(&cloud, &ipad);
    assert!(cloud.records.borrow().contains_key(name));
}

#[test]
fn crossed_uploads_settle_on_the_newer_save() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let name = "autosaves/autosave-1.json";
    let path = write_record(&tv, name, 2_000, "newer");
    sync(&cloud, &tv);
    // Another device's older upload lands after this one's.
    cloud
        .records
        .borrow_mut()
        .insert(name.into(), (b"older".to_vec(), 1_000));
    cloud.log.borrow_mut().push(name.into());
    sync(&cloud, &tv);
    assert_eq!(cloud.records.borrow()[name].1, 2_000);
    assert!(std::fs::read_to_string(path).unwrap().contains("newer"));
}

#[test]
fn a_push_after_a_save_sends_without_taking_anything_in() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    write_record(&ipad, "saves/save-1.json", 1_000, "on the iPad");
    sync(&cloud, &ipad);
    write_record(&tv, "autosaves/autosave-2.json", 2_000, "mid-match");
    pass(&cloud, &tv, Mode::Push, Vec::new).unwrap();
    assert!(
        cloud
            .records
            .borrow()
            .contains_key("autosaves/autosave-2.json")
    );
    assert!(!tv.path("saves/save-1.json").unwrap().exists());
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
    assert!(
        other.records.borrow().contains_key(name),
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
fn a_colliding_named_save_keeps_both_copies() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    let name = "saves/save-1.json";
    write_record(&tv, name, 1_000, "the TV's");
    write_record(&ipad, name, 2_000, "the iPad's");
    sync(&cloud, &tv);
    sync(&cloud, &ipad);
    let kept = ipad.path("saves/save-1-1.json").unwrap();
    assert!(kept.exists(), "the iPad's own save moved aside");
    let incoming = std::fs::read_to_string(ipad.path(name).unwrap()).unwrap();
    assert!(incoming.contains("the TV's"));
    // The moved copy goes up on the next pass and reaches the TV.
    sync(&cloud, &ipad);
    sync(&cloud, &tv);
    assert!(tv.path("saves/save-1-1.json").unwrap().exists());
}

#[test]
fn a_save_moved_aside_never_takes_a_name_arriving_with_it() {
    let cloud = FakeCloud::signed_in();
    let tv = tv();
    let ipad = device("ipad");
    write_record(&tv, "saves/save-1.json", 1_000, "the TV's first");
    write_record(&tv, "saves/save-1-1.json", 1_100, "the TV's second");
    write_record(&ipad, "saves/save-1.json", 2_000, "the iPad's");
    sync(&cloud, &tv);
    sync(&cloud, &ipad);
    let read = |name: &str| std::fs::read_to_string(ipad.path(name).unwrap()).unwrap();
    assert!(read("saves/save-1.json").contains("the TV's first"));
    assert!(read("saves/save-1-1.json").contains("the TV's second"));
    assert!(read("saves/save-1-2.json").contains("the iPad's"));
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
