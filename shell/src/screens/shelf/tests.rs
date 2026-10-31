use super::*;
use crate::saves::{RecordKind, Unavailable};
use macroquad::prelude::vec2;

fn entry(name: &str, compatible: bool, kind: RecordKind, path: std::path::PathBuf) -> ReplayEntry {
    ReplayEntry {
        path,
        title: name.to_string(),
        map: None,
        format: "1v1".to_string(),
        clock: "1:00".to_string(),
        saved_secs: 0,
        problem: (!compatible).then_some(Unavailable::OtherVersion),
        kind,
    }
}

/// One record of each tab: an autosave, a named save, and a match.
fn mixed() -> Vec<ReplayEntry> {
    vec![
        entry("live", true, RecordKind::Autosave, "/nowhere/a.json".into()),
        entry("named", true, RecordKind::Save, "/nowhere/s.json".into()),
        entry("done", true, RecordKind::Match, "/nowhere/m.json".into()),
    ]
}

/// Each row's title, without its chip and the columns after it.
fn titles(shelf: &Shelf) -> Vec<String> {
    shelf
        .menu
        .view()
        .items
        .iter()
        .map(|item| {
            let title = item.split(" | ").next().unwrap_or_default();
            title.split(" [").next().unwrap_or_default().to_string()
        })
        .collect()
}

fn drive_events(shelf: &mut Shelf, events: &[RawEvent]) -> Out {
    let mut mouse = vec2(0.0, 0.0);
    let mut sounds = Vec::new();
    shelf.update(events, &mut mouse, &mut sounds)
}

fn drive(shelf: &mut Shelf, key: Key) -> Out {
    drive_events(shelf, &[RawEvent::KeyDown { key }, RawEvent::KeyUp { key }])
}

fn tap(at: Vec2) -> Vec<RawEvent> {
    vec![
        RawEvent::TouchDown {
            id: 1,
            x: at.x,
            y: at.y,
        },
        RawEvent::TouchUp {
            id: 1,
            x: at.x,
            y: at.y,
        },
    ]
}

fn click(at: Vec2) -> Vec<RawEvent> {
    use aftermine_protocol::MouseButton;
    vec![
        RawEvent::MouseDown {
            button: MouseButton::Left,
            x: at.x,
            y: at.y,
        },
        RawEvent::MouseUp {
            button: MouseButton::Left,
            x: at.x,
            y: at.y,
        },
    ]
}

/// Moves the cursor to the titled row on the tab on show and activates
/// it.
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
    assert_eq!(shelf.status(), "", "an openable record's row says it all");
    assert_eq!(
        shelf.coaching().as_deref(),
        Some("{confirm} watches | {delete} twice deletes | {prev_tab}/{next_tab} switch tabs")
    );
    shelf.arming = shelf.menu.value().copied();
    assert!(
        shelf.status().contains("again to delete"),
        "an armed delete says so"
    );
    assert_eq!(shelf.coaching(), None);
}

#[test]
fn each_tab_lists_only_its_own_records() {
    let mut shelf = Shelf::from_entries(mixed());
    assert_eq!(shelf.tab(), Tab::Saves);
    assert_eq!(
        titles(&shelf),
        vec!["live", "named"],
        "autosaves and saves share the SAVES tab"
    );
    assert!(
        (0..shelf.menu.lines().len()).all(|row| !shelf.menu.is_header(row)),
        "the tabs replace the section headers"
    );
    drive(&mut shelf, Key::E);
    assert_eq!(shelf.tab(), Tab::Replays);
    assert_eq!(titles(&shelf), vec!["done"]);
}

#[test]
fn the_shelf_opens_on_saves_unless_only_replays_wait() {
    assert_eq!(Shelf::from_entries(mixed()).tab(), Tab::Saves);
    assert_eq!(
        Shelf::from_entries(Vec::new()).tab(),
        Tab::Saves,
        "an empty shelf opens on SAVES"
    );
    let only_replays = || {
        vec![entry(
            "done",
            true,
            RecordKind::Match,
            "/nowhere/m.json".into(),
        )]
    };
    assert_eq!(Shelf::from_entries(only_replays()).tab(), Tab::Replays);

    let mut opened = Shelf::open();
    opened.set_catalog(only_replays());
    assert_eq!(
        opened.tab(),
        Tab::Replays,
        "the first catalog decides where an untouched shelf opens"
    );
    let mut picked = Shelf::open();
    drive(&mut picked, Key::Q);
    drive(&mut picked, Key::Q);
    picked.set_catalog(only_replays());
    assert_eq!(
        picked.tab(),
        Tab::Saves,
        "a tab the player picked while the shelf looked stays picked"
    );
}

#[test]
fn tabs_step_by_key_both_ways_and_answer_clicks_and_taps() {
    crate::render::set_viewport(1280.0, 800.0);
    crate::render::set_user_scale(1.0);
    let mut shelf = Shelf::from_entries(mixed());
    for key in [Key::Q, Key::E] {
        let before = shelf.tab();
        assert_eq!(drive(&mut shelf, key), Out::Stay);
        assert_eq!(
            shelf.tab(),
            before.other(),
            "{key:?} steps to the other tab"
        );
    }
    let strip = crate::tabs::rects(Tab::ALL.len());
    drive_events(&mut shelf, &click(strip[1].center()));
    assert_eq!(shelf.tab(), Tab::Replays);
    drive_events(&mut shelf, &tap(strip[0].center()));
    assert_eq!(shelf.tab(), Tab::Saves);
    drive_events(&mut shelf, &tap(strip[0].center()));
    assert_eq!(shelf.tab(), Tab::Saves, "the active tab stays put");
}

#[test]
fn a_tab_opens_on_its_first_row_and_drops_an_armed_delete() {
    let mut shelf = Shelf::from_entries(mixed());
    drive(&mut shelf, Key::Down);
    assert_eq!(drive(&mut shelf, Key::X), Out::Stay);
    assert!(shelf.arming.is_some());
    drive(&mut shelf, Key::E);
    drive(&mut shelf, Key::Q);
    assert_eq!(shelf.menu.selected, 0, "a tab opens on its first row");
    assert_eq!(shelf.arming, None, "a switch disarms the delete");
    assert_eq!(
        drive(&mut shelf, Key::X),
        Out::Stay,
        "an X after the switch only arms again"
    );
}

#[test]
fn a_catalog_refresh_keeps_the_tab_and_the_focused_record() {
    let mut shelf = Shelf::from_entries(mixed());
    drive(&mut shelf, Key::Down);
    assert_eq!(titles(&shelf)[shelf.menu.selected], "named");
    let refreshed = || {
        let mut entries = mixed();
        entries.insert(
            0,
            entry("fresh", true, RecordKind::Save, "/nowhere/f.json".into()),
        );
        entries
    };
    shelf.set_catalog(refreshed());
    assert_eq!(titles(&shelf)[shelf.menu.selected], "named");

    drive(&mut shelf, Key::E);
    shelf.set_catalog(refreshed());
    assert_eq!(shelf.tab(), Tab::Replays, "a refresh keeps the tab on show");
}

#[test]
fn each_tab_reports_its_own_mode() {
    let mut shelf = Shelf::from_entries(mixed());
    assert_eq!(shelf.mode_name(), "saves");
    drive(&mut shelf, Key::E);
    assert_eq!(shelf.mode_name(), "replays");
}

#[test]
fn a_row_reads_its_title_chip_format_match_time_and_age() {
    let now = 1_784_721_600;
    let mut live = entry(
        "Long Causeway",
        true,
        RecordKind::Autosave,
        "/nowhere/a.json".into(),
    );
    live.format = "4-way FFA".to_string();
    live.clock = "28:58".to_string();
    live.saved_secs = now - 12 * 60;
    let mut named = entry(
        "before the push",
        true,
        RecordKind::LanSave,
        "/nowhere/s.json".into(),
    );
    named.map = Some("Bridge Town".to_string());
    named.format = "2v2".to_string();
    named.saved_secs = now - 3 * 60 * 60;
    let shelf = Shelf::at(vec![live, named], now, None);
    assert_eq!(
        shelf.menu.view().items,
        vec![
            "Long Causeway [AUTO] | 4-way FFA | 28:58 | 12 min ago",
            "before the push \u{b7} Bridge Town [LAN] | 2v2 | 1:00 | 3 h ago",
        ]
    );
}

#[test]
fn the_shelf_says_it_is_looking_until_the_catalog_lands() {
    let mut shelf = Shelf::open();
    assert!(
        shelf.status().starts_with("looking"),
        "an unscanned shelf is not an empty one: {}",
        shelf.status()
    );
    shelf.set_catalog(Vec::new());
    assert_eq!(shelf.status(), Tab::Saves.empty());
    drive(&mut shelf, Key::E);
    assert_eq!(
        shelf.status(),
        Tab::Replays.empty(),
        "each empty tab says how to fill it"
    );
}

#[test]
fn enter_loads_a_save_and_watches_a_match() {
    let mut shelf = Shelf::from_entries(mixed());
    assert_eq!(
        activate(&mut shelf, "live"),
        Out::Load("/nowhere/a.json".into()),
        "a resumable record's verb is Load, never a mid-match scout"
    );
    drive(&mut shelf, Key::E);
    assert_eq!(
        activate(&mut shelf, "done"),
        Out::Watch("/nowhere/m.json".into())
    );
}

#[test]
fn deletion_requires_two_presses_and_is_dispatched_to_the_owner() {
    let dir = std::env::temp_dir().join(format!("aftermine-shelf-{}", std::process::id()));
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
fn an_incompatible_record_refuses_its_verb_on_both_tabs() {
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
        shelf.status(),
        Unavailable::OtherVersion.reason(),
        "the focused record says why it will not open"
    );
    assert_eq!(
        shelf.coaching().as_deref(),
        Some("{delete} twice deletes | {prev_tab}/{next_tab} switch tabs")
    );
    drive(&mut shelf, Key::E);
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

#[test]
fn the_page_heading_stacks_title_strip_and_list() {
    for (width, height) in [(640.0, 400.0), (1180.0, 820.0), (1920.0, 1080.0)] {
        for scale in [
            crate::render::MIN_USER_SCALE,
            1.0,
            crate::render::MAX_USER_SCALE,
        ] {
            crate::render::set_viewport(width, height);
            crate::render::set_user_scale(scale);
            let s = crate::render::ui_scale();
            let shelf = Shelf::from_entries(mixed());
            let strip = crate::tabs::rects(Tab::ALL.len());
            let back = crate::button::corner_slot(0, s);
            let first = shelf.menu.item_rect(0).expect("the list opens on its top");
            let at = format!("{width}x{height} at {scale}");
            assert!(
                strip[0].y > crate::menu::page_title_baseline(s),
                "the strip hangs under the title, {at}"
            );
            assert!(
                strip.iter().all(|tab| !tab.overlaps(&back)),
                "the strip stays clear of BACK, {at}"
            );
            assert!(
                first.y >= strip[0].bottom(),
                "the list starts under the strip, {at}"
            );
        }
    }
    crate::render::set_user_scale(1.0);
}
