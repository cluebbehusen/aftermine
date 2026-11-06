use super::*;

#[test]
fn bundle_probe_finds_mac_and_flat_ios_layouts() {
    let root = std::env::temp_dir().join(format!("aftermine-bundle-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let atlas = |dir: &Path| {
        let sprites = dir.join("assets/sprites");
        std::fs::create_dir_all(&sprites).unwrap();
        std::fs::write(sprites.join("atlas.png"), b"").unwrap();
    };
    let mac = root.join("Aftermine.app/Contents");
    std::fs::create_dir_all(mac.join("MacOS")).unwrap();
    atlas(&mac.join("Resources"));
    let found = bundle_resources_beside(&mac.join("MacOS")).expect("mac bundle");
    assert!(found.ends_with("MacOS/../Resources"));

    let ios = root.join("Aftermine-ios.app");
    atlas(&ios);
    assert_eq!(bundle_resources_beside(&ios), Some(ios.clone()));

    let workspace = root.join("target/debug");
    std::fs::create_dir_all(&workspace).unwrap();
    assert_eq!(bundle_resources_beside(&workspace), None);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn local_records_hang_off_the_device_roots() {
    let local = Folders::local();
    if let Some(data) = data_dir() {
        assert_eq!(local.autosaves, Some(data.join("autosaves")));
        assert_eq!(local.saves, Some(data.join("saves")));
        assert_eq!(local.matches, Some(data.join("matches")));
        assert_eq!(diagnostics_dir(), Some(data.join("diagnostics")));
    }
    // Test binaries never sit inside a bundle, so every documented
    // `driver` invocation keeps reading the local replays/ dir.
    assert_eq!(local.replays, PathBuf::from("replays"));
}

#[test]
fn without_icloud_records_stay_on_the_device() {
    let local = Folders::under(Path::new("/device"));
    let records = Records::with_cloud(None, local.clone());
    assert_eq!(records.autosaves(), local.autosaves);
    assert_eq!(records.saves(), local.saves);
    assert_eq!(records.matches(), local.matches);
    assert_eq!(records.listed(), local.all().collect::<Vec<_>>());
    assert_eq!(
        records.match_folders(),
        vec![PathBuf::from("/device/matches")]
    );
    assert_eq!(
        records.autosave_folders(),
        vec![PathBuf::from("/device/autosaves")]
    );
    assert!(records.cloud_folders().is_empty());
    // No lookup runs outside a packaged copy, so nothing waits on one.
    assert_eq!(
        super::records(),
        Records::with_cloud(None, Folders::local())
    );
}

#[test]
fn icloud_takes_new_records_and_the_shelf_keeps_the_old_ones() {
    let cloud = Path::new("/cloud/Documents");
    let records = Records::with_cloud(Some(cloud), Folders::under(Path::new("/device")));
    assert_eq!(records.autosaves(), Some(cloud.join("autosaves")));
    assert_eq!(records.saves(), Some(cloud.join("saves")));
    assert_eq!(records.matches(), Some(cloud.join("matches")));
    let names = |folders: Vec<PathBuf>| {
        folders
            .iter()
            .map(|folder| folder.display().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(records.cloud_folders()),
        [
            "/cloud/Documents/autosaves",
            "/cloud/Documents/matches",
            "/cloud/Documents/saves",
            "/cloud/Documents/replays",
        ]
    );
    assert_eq!(
        names(records.listed())[4..],
        [
            "/device/autosaves",
            "/device/matches",
            "/device/saves",
            "/device/replays",
        ]
    );
    assert_eq!(
        names(records.match_folders()),
        ["/cloud/Documents/matches", "/device/matches"]
    );
    assert_eq!(
        names(records.autosave_folders()),
        ["/cloud/Documents/autosaves", "/device/autosaves"]
    );
}

const CLOUD: &str = "/cloud/Documents";

#[test]
fn an_icloud_lookup_answers_whoever_waited_for_it() {
    static LOOKUP: Lookup = Lookup::new();
    assert_eq!(LOOKUP.answer(), None, "no lookup, no folder");
    LOOKUP.start(|| Some(PathBuf::from(CLOUD)));
    assert_eq!(LOOKUP.answer(), Some(PathBuf::from(CLOUD)));
    LOOKUP.start(|| None);
    let state = LOOKUP
        .answered
        .wait_while(LOOKUP.lock(), |state| state.looking)
        .unwrap();
    assert_eq!(state.folder, None, "a sign-out takes the folder away");
}

#[test]
fn a_lookup_that_cannot_answer_keeps_the_last_word() {
    static FAILED: Lookup = Lookup::new();
    static KEPT: Lookup = Lookup::new();
    FAILED.start(|| panic!("iCloud setup failed"));
    assert_eq!(FAILED.answer(), None);

    KEPT.start(|| Some(PathBuf::from(CLOUD)));
    assert_eq!(KEPT.answer(), Some(PathBuf::from(CLOUD)));
    KEPT.lock().looking = true;
    KEPT.give_up();
    assert_eq!(KEPT.answer(), Some(PathBuf::from(CLOUD)));
}
