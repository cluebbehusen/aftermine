use super::*;
use crate::saves::{RecordKind, Unavailable};
use macroquad::prelude::vec2;

fn entry(name: &str, compatible: bool, kind: RecordKind, path: std::path::PathBuf) -> ReplayEntry {
    ReplayEntry {
        path,
        title: name.to_string(),
        detail: "Skirmish".to_string(),
        clock: "1:00".to_string(),
        saved_secs: 0,
        problem: (!compatible).then_some(Unavailable::OtherVersion),
        kind,
    }
}

/// Each line's leading text: a header's label, or a row's title.
fn titles(shelf: &Shelf) -> Vec<String> {
    shelf
        .menu
        .view()
        .items
        .iter()
        .map(|item| item.split(" | ").next().unwrap_or_default().to_string())
        .collect()
}

fn drive(shelf: &mut Shelf, key: Key) -> Out {
    let mut mouse = vec2(0.0, 0.0);
    let mut sounds = Vec::new();
    shelf.update(
        &[RawEvent::KeyDown { key }, RawEvent::KeyUp { key }],
        &mut mouse,
        &mut sounds,
    )
}

/// Moves the cursor to the titled row and activates it — by title, so
/// the tests survive the section headers shifting every index.
fn activate(shelf: &mut Shelf, title: &str) -> Out {
    let target = titles(shelf)
        .iter()
        .position(|t| t == title)
        .unwrap_or_else(|| panic!("no row titled {title} in {:?}", titles(shelf)));
    while shelf.menu.selected < target {
        drive(shelf, Key::Down);
    }
    while shelf.menu.selected > target {
        drive(shelf, Key::Up);
    }
    drive(shelf, Key::Enter)
}

#[test]
fn the_back_button_leaves_even_after_every_record_is_deleted() {
    // However the shelf is built (first open or post-delete rebuild), it
    // has an exit.
    let mut shelf = Shelf::from_entries(vec![entry(
        "done",
        true,
        RecordKind::Match,
        "/nowhere/m.json".into(),
    )]);
    shelf.set_catalog(Vec::new());
    assert!(shelf.menu.view().items.is_empty());
    assert_eq!(drive(&mut shelf, Key::Enter), Out::Stay);
    for touch in [false, true] {
        let mut mouse = vec2(0.0, 0.0);
        let out = shelf.update(
            &crate::button::press_back(touch),
            &mut mouse,
            &mut Vec::new(),
        );
        assert_eq!(out, Out::Home);
    }
}

#[test]
fn a_record_says_how_to_act_only_as_coaching() {
    let record = entry("done", true, RecordKind::Match, "/nowhere/m.json".into());
    let mut shelf = Shelf::from_entries(vec![record]);
    assert_eq!(shelf.subtitle(), "", "an openable record's row says it all");
    assert_eq!(
        shelf.coaching().as_deref(),
        Some("{confirm} watches | {delete} twice deletes")
    );
    shelf.arming = shelf.menu.value().copied();
    assert!(
        shelf.subtitle().contains("again to delete"),
        "an armed delete says so"
    );
    assert_eq!(shelf.coaching(), None);
}

#[test]
fn records_shelve_into_their_sections_and_the_cursor_skips_the_headers() {
    let mut shelf = Shelf::from_entries(vec![
        entry("live", true, RecordKind::Autosave, "/nowhere/a.json".into()),
        entry("named", true, RecordKind::Save, "/nowhere/s.json".into()),
        entry("done", true, RecordKind::Match, "/nowhere/m.json".into()),
    ]);
    assert_eq!(
        titles(&shelf),
        vec!["SAVES", "live", "named", "REPLAYS", "done"],
        "saves first, replays after"
    );
    assert!(shelf.menu.is_header(0) && shelf.menu.is_header(3));
    assert_eq!(shelf.menu.selected, 1, "the cursor opens on a real row");
    // Walking down never rests on the REPLAYS header.
    drive(&mut shelf, Key::Down);
    drive(&mut shelf, Key::Down);
    assert_eq!(titles(&shelf)[shelf.menu.selected], "done");
}

#[test]
fn a_row_reads_its_title_kind_match_time_and_age() {
    let now = 1_784_721_600;
    let mut live = entry(
        "Long Causeway",
        true,
        RecordKind::Autosave,
        "/nowhere/a.json".into(),
    );
    live.detail = "Autosave".to_string();
    live.clock = "28:58".to_string();
    live.saved_secs = now - 12 * 60;
    let shelf = Shelf::at(vec![live], now);
    assert_eq!(
        shelf.menu.view().items[1],
        "Long Causeway | Autosave | 28:58 | 12 min ago"
    );
}

#[test]
fn the_shelf_says_it_is_looking_until_the_catalog_lands() {
    let mut shelf = Shelf::open();
    assert!(
        shelf.subtitle().starts_with("looking"),
        "an unscanned shelf is not an empty one: {}",
        shelf.subtitle()
    );
    shelf.set_catalog(Vec::new());
    assert!(shelf.subtitle().starts_with("nothing recorded yet"));
}

#[test]
fn enter_loads_a_save_and_watches_a_match() {
    let mut shelf = Shelf::from_entries(vec![
        entry("live", true, RecordKind::Autosave, "/nowhere/a.json".into()),
        entry("done", true, RecordKind::Match, "/nowhere/m.json".into()),
    ]);
    assert_eq!(
        activate(&mut shelf, "live"),
        Out::Load("/nowhere/a.json".into()),
        "a resumable record's verb is Load, never a mid-match scout"
    );
    assert_eq!(
        activate(&mut shelf, "done"),
        Out::Watch("/nowhere/m.json".into())
    );
}

#[test]
fn deletion_requires_two_presses_and_is_dispatched_to_the_owner() {
    let dir = std::env::temp_dir().join(format!("oxide-shelf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("doomed.json");
    std::fs::write(&path, "{}").unwrap();
    let mut shelf =
        Shelf::from_entries(vec![entry("doomed", true, RecordKind::Match, path.clone())]);
    assert_eq!(drive(&mut shelf, Key::X), Out::Stay, "first X only arms");
    assert!(path.exists(), "arming deletes nothing");
    assert_eq!(drive(&mut shelf, Key::X), Out::Delete(path.clone()));
    assert!(path.exists(), "the worker owns disk operations");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_incompatible_record_refuses_its_verb_in_both_sections() {
    let old_save = entry(
        "old-save",
        false,
        RecordKind::Save,
        "/nowhere/s.json".into(),
    );
    let old_match = entry(
        "old-match",
        false,
        RecordKind::Match,
        "/nowhere/m.json".into(),
    );
    let new = entry("new", true, RecordKind::Match, "/nowhere/new.json".into());
    let mut shelf = Shelf::from_entries(vec![old_save, old_match, new]);
    assert_eq!(
        activate(&mut shelf, "old-save"),
        Out::Stay,
        "an incompatible save refuses to load"
    );
    assert_eq!(
        shelf.subtitle(),
        Unavailable::OtherVersion.reason(),
        "the focused record says why it will not open"
    );
    assert_eq!(shelf.coaching().as_deref(), Some("{delete} twice deletes"));
    assert_eq!(
        activate(&mut shelf, "old-match"),
        Out::Stay,
        "an incompatible match refuses to watch"
    );
    assert_eq!(
        activate(&mut shelf, "new"),
        Out::Watch("/nowhere/new.json".into())
    );
}
