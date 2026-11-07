use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aftermine-icloud-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn an_unentitled_process_has_no_icloud_folder() {
    // Test binaries carry no iCloud entitlement, so even a Mac signed in to
    // iCloud answers with nothing.
    assert_eq!(documents(), None);
}

#[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
#[test]
fn only_apple_platforms_have_a_key_value_store() {
    assert!(KeyValueStore::open().is_none());
}

#[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
#[test]
fn only_apple_platforms_have_user_defaults() {
    defaults::set("aftermine.icloud-test", "dropped");
    assert_eq!(defaults::get("aftermine.icloud-test"), None);
}

#[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
#[test]
fn user_defaults_answer_without_writing() {
    // Only reads and a refused write, so a test run leaves no preferences
    // file behind.
    assert_eq!(defaults::get("aftermine.icloud-test-never-written"), None);
    defaults::set("bad\0key", "dropped");
    assert_eq!(defaults::get("bad\0key"), None);
}

#[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
#[test]
fn an_unentitled_store_keeps_working_on_this_device() {
    // A test binary carries no entitlement, like an unsigned build: the
    // store opens, takes reads and writes, and syncs nothing.
    let store = KeyValueStore::open().expect("Apple platforms have a store");
    store.set("aftermine.icloud-test", "kept");
    let _ = store.get("aftermine.icloud-test");
    assert!(store.get("aftermine.icloud-test-never-written").is_none());
    store.set("bad\0key", "dropped");
    let _ = store.changes();
}

#[test]
fn coordinated_accesses_run_on_ordinary_files() {
    let dir = scratch("coordinate");
    let file = dir.join("record.amsave");
    let granted = coordinate(&file, Access::Replace, |granted| {
        std::fs::write(granted, b"saved").unwrap();
        granted.to_owned()
    })
    .unwrap();
    assert_eq!(granted, file, "an unmoved file keeps its path");
    let read = coordinate(&file, Access::Read, |path| std::fs::read(path)).unwrap();
    assert_eq!(read.unwrap(), b"saved");
    coordinate(&file, Access::Delete, |path| std::fs::remove_file(path))
        .unwrap()
        .unwrap();
    assert!(!file.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_panic_in_a_coordinated_access_reaches_the_caller() {
    let dir = scratch("panic");
    let caught = std::panic::catch_unwind(|| {
        coordinate(&dir.join("record.amsave"), Access::Replace, |_| {
            panic!("save writer failed");
        })
    });
    let message = caught.expect_err("the panic resumes");
    assert_eq!(message.downcast_ref::<&str>(), Some(&"save writer failed"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn local_files_never_wait_on_a_download() {
    let dir = scratch("download");
    std::fs::write(dir.join("save-0000000001.amsave"), b"saved").unwrap();
    assert_eq!(download(&dir), 0);
    assert_eq!(download(&dir.join("missing")), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[test]
fn a_path_with_a_nul_byte_is_refused_before_any_access() {
    use std::os::unix::ffi::OsStrExt;
    let path = Path::new(std::ffi::OsStr::from_bytes(b"bad\0path"));
    let error = coordinate(path, Access::Read, |_| unreachable!()).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(download(path), 0);
}
