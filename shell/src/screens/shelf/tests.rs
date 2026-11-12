use super::*;
use crate::saves::{RecordKind, Unavailable};
use crate::tests::support::{click, left_down, left_up, tap, touch_down, touch_move, touch_up};
use aftermine_protocol::PadButton;
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
    shelf.set_catalog(Vec::new(), 0);
    assert!(shelf.menu.view().items.is_empty());
    assert_eq!(drive(&mut shelf, Key::Enter), Out::Stay);
    for touch in [false, true] {
        let mut mouse = vec2(0.0, 0.0);
        let out = shelf.update(
            &crate::button::tests::press_back(touch),
            &mut mouse,
            &mut Vec::new(),
        );
        assert_eq!(out, Out::Home);
    }
}

#[test]
fn a_record_says_how_to_act_only_as_coaching() {
    let record = entry("done", true, RecordKind::Match, "/nowhere/m.json".into());
    let shelf = Shelf::from_entries(vec![record]);
    assert_eq!(shelf.status(), "", "an openable record's row says it all");
    assert_eq!(
        shelf.coaching().as_deref(),
        Some("{confirm} watches | {delete} deletes | {prev_tab}/{next_tab} switch tabs")
    );
}

#[test]
fn a_catalog_refresh_keeps_the_row_cursor_hidden_while_the_remote_is_on_the_tabs() {
    crate::platform::set_hands(crate::platform::Hands {
        pad: Some(aftermine_protocol::PadFamily::Remote),
        ..crate::platform::tests::ALL_HANDS[3]
    });
    let record = |name| {
        entry(
            name,
            true,
            RecordKind::Match,
            format!("/nowhere/{name}").into(),
        )
    };
    let mut shelf = Shelf::from_entries(vec![record("a")]);
    drive(&mut shelf, Key::Up);
    let focused = shelf.tabs.focused();
    shelf.set_catalog(vec![record("a"), record("b")], 0);
    let hidden = shelf.menu.cursor_hidden;
    crate::platform::set_hands(crate::platform::Hands::BUILD);
    assert!(focused, "up from the top row reaches the tabs");
    assert!(hidden);
}

#[test]
fn a_remote_always_hears_how_to_reach_the_tabs() {
    crate::platform::set_hands(crate::platform::Hands {
        pad: Some(aftermine_protocol::PadFamily::Remote),
        ..crate::platform::tests::ALL_HANDS[3]
    });
    let record = entry("done", true, RecordKind::Match, "/nowhere/m.json".into());
    let mut shelf = Shelf::from_entries(vec![record]);
    let coaching = shelf.coaching();
    shelf.set_catalog(Vec::new(), 0);
    let empty = shelf.coaching();
    crate::platform::set_hands(crate::platform::Hands::BUILD);
    assert_eq!(
        coaching.as_deref(),
        Some("{confirm} to watch | Swipe right to delete | Swipe up for tabs")
    );
    assert_eq!(empty.as_deref(), Some("Swipe up for tabs"));
}

#[test]
fn a_remote_swipes_right_onto_a_rows_delete_and_a_click_asks() {
    crate::platform::set_hands(crate::platform::Hands {
        pad: Some(aftermine_protocol::PadFamily::Remote),
        ..crate::platform::tests::ALL_HANDS[3]
    });
    let pad = |button| {
        crate::pad::menu_keys(
            &[
                RawEvent::PadDown { pad: 0, button },
                RawEvent::PadUp { pad: 0, button },
            ],
            true,
        )
    };
    let record = |name: &str| {
        entry(
            name,
            true,
            RecordKind::Match,
            format!("/nowhere/{name}.json").into(),
        )
    };
    let mut shelf = Shelf::from_entries(vec![record("a"), record("b")]);
    drive_events(&mut shelf, &pad(PadButton::DpadRight));
    let on_delete = (shelf.menu.on_action(), shelf.coaching());
    drive_events(&mut shelf, &pad(PadButton::DpadLeft));
    let back_on_row = shelf.menu.on_action();
    let watched = drive_events(&mut shelf, &pad(crate::pad::CONFIRM));
    drive_events(&mut shelf, &pad(PadButton::DpadRight));
    drive_events(&mut shelf, &pad(PadButton::DpadDown));
    let moved = (shelf.menu.on_action(), shelf.menu.selected);
    drive_events(&mut shelf, &pad(PadButton::DpadRight));
    let asked = drive_events(&mut shelf, &pad(crate::pad::CONFIRM));
    let question = shelf.view();
    crate::platform::set_hands(crate::platform::Hands::BUILD);
    assert_eq!(
        on_delete,
        (
            true,
            Some("{confirm} to delete | Swipe up for tabs".to_string())
        )
    );
    assert!(!back_on_row, "left goes back to the row");
    assert_eq!(watched, Out::Watch("/nowhere/a.json".into()));
    assert_eq!(moved, (false, 1), "down moves to the next row itself");
    assert_eq!(asked, Out::Stay, "a click on Delete only asks");
    assert_eq!(question.title, "DELETE THIS REPLAY?");
    assert_eq!(question.selected, 0, "Cancel waits under the cursor");
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
    opened.set_catalog(only_replays(), 0);
    assert_eq!(
        opened.tab(),
        Tab::Replays,
        "the first catalog decides where an untouched shelf opens"
    );
    let mut picked = Shelf::open();
    drive(&mut picked, Key::Q);
    drive(&mut picked, Key::Q);
    picked.set_catalog(only_replays(), 0);
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
    drive_events(&mut shelf, &tap(1, strip[0].center()));
    assert_eq!(shelf.tab(), Tab::Saves);
    drive_events(&mut shelf, &tap(1, strip[0].center()));
    assert_eq!(shelf.tab(), Tab::Saves, "the active tab stays put");
}

#[test]
fn a_tab_opens_on_its_first_row() {
    let mut shelf = Shelf::from_entries(mixed());
    drive(&mut shelf, Key::Down);
    drive(&mut shelf, Key::E);
    drive(&mut shelf, Key::Q);
    assert_eq!(shelf.menu.selected, 0, "a tab opens on its first row");
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
    shelf.set_catalog(refreshed(), 0);
    assert_eq!(titles(&shelf)[shelf.menu.selected], "named");

    drive(&mut shelf, Key::E);
    shelf.set_catalog(refreshed(), 0);
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
    assert_eq!(
        shelf.status(),
        "Looking for saves and replays...",
        "an unscanned shelf is not an empty one"
    );
    assert_eq!(shelf.empty_message(), None);
    shelf.set_catalog(Vec::new(), 0);
    assert_eq!(shelf.status(), "");
    assert_eq!(shelf.empty_message(), Some("No saves yet"));
    drive(&mut shelf, Key::E);
    assert_eq!(shelf.status(), "");
    assert_eq!(shelf.empty_message(), Some("No replays yet"));
}

#[test]
fn the_shelf_counts_records_still_coming_from_icloud() {
    let mut shelf = Shelf::open();
    shelf.set_catalog(Vec::new(), 3);
    assert_eq!(
        shelf.status(),
        "Downloading 3 files from iCloud",
        "an empty tab is waiting, not empty"
    );
    assert_eq!(shelf.empty_message(), None);
    shelf.set_catalog(Vec::new(), 2);
    assert_eq!(shelf.status(), "Downloading 2 files from iCloud");
    let record = entry("done", true, RecordKind::Match, "/nowhere/m.json".into());
    shelf.set_catalog(vec![record], 1);
    assert_eq!(shelf.status(), "Downloading 1 file from iCloud");
    shelf.set_catalog(Vec::new(), 0);
    assert_eq!(shelf.empty_message(), Some("No saves yet"));
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
fn a_delete_asks_first_and_is_dispatched_to_the_owner() {
    let dir = std::env::temp_dir().join(format!("aftermine-shelf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("doomed.json");
    std::fs::write(&path, "{}").unwrap();
    let mut doomed = entry("doomed", true, RecordKind::Match, path.clone());
    doomed.map = Some("Crossroads".to_string());
    let mut shelf = Shelf::from_entries(vec![doomed]);
    assert_eq!(drive(&mut shelf, Key::X), Out::Stay, "X only asks");
    assert_eq!(shelf.mode_name(), "confirm_delete");
    let question = shelf.view();
    assert_eq!(question.title, "DELETE THIS REPLAY?");
    assert_eq!(question.items, ["Cancel", "Delete"]);
    assert_eq!(question.selected, 0, "Cancel waits under the cursor");
    assert_eq!(
        shelf.asking.as_ref().map(|ask| ask.subtitle.as_str()),
        Some("doomed \u{b7} Crossroads")
    );
    assert_eq!(
        drive(&mut shelf, Key::X),
        Out::Stay,
        "a second X never deletes"
    );
    assert_eq!(drive(&mut shelf, Key::Enter), Out::Stay, "Cancel keeps it");
    assert_eq!(shelf.mode_name(), "replays");
    assert!(path.exists());

    drive(&mut shelf, Key::X);
    drive(&mut shelf, Key::Down);
    assert_eq!(drive(&mut shelf, Key::Enter), Out::Delete(path.clone()));
    assert_eq!(
        shelf.mode_name(),
        "replays",
        "the answer closes the question"
    );
    assert!(path.exists(), "the worker owns disk operations");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn escape_and_back_keep_the_record_and_the_shelf() {
    let mut shelf = Shelf::from_entries(mixed());
    assert_eq!(shelf.view().title, "SAVES & REPLAYS");
    drive(&mut shelf, Key::X);
    assert_eq!(shelf.view().title, "DELETE THIS SAVE?");
    assert_eq!(drive(&mut shelf, Key::Escape), Out::Stay);
    assert_eq!(shelf.mode_name(), "saves");
    for touch in [false, true] {
        drive(&mut shelf, Key::X);
        let mut mouse = vec2(0.0, 0.0);
        let out = shelf.update(
            &crate::button::tests::press_back(touch),
            &mut mouse,
            &mut Vec::new(),
        );
        assert_eq!(out, Out::Stay, "BACK leaves the question, not the shelf");
        assert_eq!(shelf.mode_name(), "saves");
    }
}

#[test]
fn a_rows_delete_button_asks_by_click_or_tap_and_the_rest_of_the_row_opens_it() {
    crate::render::set_viewport(1180.0, 820.0);
    crate::render::set_user_scale(1.0);
    for touch in [false, true] {
        let press = |at: Vec2| if touch { tap(1, at) } else { click(at) };
        let mut shelf = Shelf::from_entries(mixed());
        let button = shelf.menu.action_rect(1).expect("every record row has one");
        let row = shelf.menu.item_rect(1).unwrap();
        assert!(row.contains(button.center()) && button.right() <= row.right());
        assert_eq!(drive_events(&mut shelf, &press(button.center())), Out::Stay);
        assert_eq!(
            shelf.asking.as_ref().map(|ask| ask.path.clone()),
            Some("/nowhere/s.json".into()),
            "the button asks about its own row"
        );
        let delete = shelf
            .asking
            .as_ref()
            .and_then(|ask| ask.menu.item_rect(1))
            .unwrap();
        assert_eq!(
            drive_events(&mut shelf, &press(delete.center())),
            Out::Delete("/nowhere/s.json".into())
        );
        let title = vec2(row.x + 40.0, row.center().y);
        assert_eq!(
            drive_events(&mut shelf, &press(title)),
            Out::Load("/nowhere/s.json".into())
        );
    }
}

#[test]
fn a_press_that_crosses_a_rows_delete_edge_does_nothing() {
    crate::render::set_viewport(1180.0, 820.0);
    crate::render::set_user_scale(1.0);
    let mut shelf = Shelf::from_entries(mixed());
    let button = shelf.menu.action_rect(1).unwrap().center();
    let row = shelf.menu.item_rect(1).unwrap();
    let title = vec2(row.x + 40.0, row.center().y);
    for (from, to) in [(button, title), (title, button)] {
        let mouse = [left_down(from), left_up(to)];
        let finger = [touch_down(1, from), touch_up(1, to)];
        for events in [&mouse, &finger] {
            assert_eq!(drive_events(&mut shelf, events), Out::Stay);
            assert!(shelf.asking.is_none(), "{from} to {to} asked");
        }
    }
}

#[test]
fn a_finger_dragging_from_a_delete_button_scrolls_instead() {
    crate::render::set_viewport(1180.0, 400.0);
    crate::render::set_user_scale(1.0);
    let records = (0..20).map(|n| {
        entry(
            &format!("save {n}"),
            true,
            RecordKind::Save,
            format!("/nowhere/{n}.json").into(),
        )
    });
    let mut shelf = Shelf::from_entries(records.collect());
    let start = shelf.menu.action_rect(1).unwrap().center();
    let mut events = vec![touch_down(1, start)];
    for step in 1..=8 {
        events.push(touch_move(1, vec2(start.x, start.y - 12.0 * step as f32)));
    }
    events.push(touch_up(1, vec2(start.x, start.y - 96.0)));
    assert_eq!(drive_events(&mut shelf, &events), Out::Stay);
    assert!(shelf.asking.is_none(), "a drag never asks");
    assert!(shelf.menu.visible_range()[0] > 0, "the drag scrolled");
}

#[test]
fn the_controllers_delete_button_asks_too() {
    let mut shelf = Shelf::from_entries(mixed());
    let events = crate::pad::menu_keys(
        &[RawEvent::PadDown {
            pad: 0,
            button: crate::pad::DELETE,
        }],
        true,
    );
    assert_eq!(drive_events(&mut shelf, &events), Out::Stay);
    assert_eq!(shelf.mode_name(), "confirm_delete");
}

#[test]
fn after_a_delete_the_cursor_stays_on_the_row_that_moved_up() {
    let saves = || {
        ["first", "doomed", "next"].map(|name| {
            entry(
                name,
                true,
                RecordKind::Save,
                format!("/nowhere/{name}.json").into(),
            )
        })
    };
    let mut shelf = Shelf::from_entries(saves().into());
    drive(&mut shelf, Key::Down);
    let mut rest: Vec<_> = saves().into();
    rest.remove(1);
    shelf.set_catalog(rest, 0);
    assert_eq!(titles(&shelf)[shelf.menu.selected], "next");
}

#[test]
fn a_refresh_keeps_a_question_only_while_its_record_is_listed() {
    let mut shelf = Shelf::from_entries(mixed());
    drive(&mut shelf, Key::X);
    let mut fresh = mixed();
    fresh.insert(
        0,
        entry("fresh", true, RecordKind::Save, "/nowhere/f.json".into()),
    );
    shelf.set_catalog(fresh, 0);
    assert_eq!(shelf.mode_name(), "confirm_delete");
    let mut gone = mixed();
    gone.remove(0);
    shelf.set_catalog(gone, 0);
    assert_eq!(
        shelf.mode_name(),
        "saves",
        "a record deleted elsewhere has nothing left to ask about"
    );
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
        "Made by a different version of Aftermine",
        "the focused record says why it will not open"
    );
    assert_eq!(
        shelf.coaching().as_deref(),
        Some("{delete} deletes | {prev_tab}/{next_tab} switch tabs")
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

#[test]
fn a_controller_in_use_hears_its_confirm_delete_and_bumpers() {
    let record = entry("done", true, RecordKind::Match, "/nowhere/m.json".into());
    let shelf = Shelf::from_entries(vec![record]);
    crate::platform::set_hands(crate::platform::Hands {
        pad: Some(aftermine_protocol::PadFamily::PlayStation),
        ..crate::platform::tests::ALL_HANDS[0]
    });
    let coaching = shelf
        .coaching()
        .map(|line| crate::menu::binding_hint(&line));
    crate::platform::set_hands(crate::platform::Hands::BUILD);
    assert_eq!(
        coaching.as_deref(),
        Some("Cross watches | Square deletes | L1/R1 switch tabs")
    );
}

#[test]
fn only_apple_tv_warns_that_records_may_not_last() {
    assert!(!standing_line(Platform::Tv).is_empty());
    assert_eq!(standing_line(Platform::Desktop), "");
    assert_eq!(standing_line(Platform::Touch), "");
}

#[test]
fn only_apple_tv_says_why_records_are_not_backing_up() {
    let mut shelf = Shelf::open();
    shelf.set_backup(Some(Trouble::NoAccount));
    assert!(
        shelf.status_on(Platform::Tv).starts_with("Looking"),
        "the scan comes first"
    );
    shelf.set_catalog(mixed(), 0);
    for (trouble, line) in [
        (
            Trouble::StorageFull,
            "iCloud storage is full, so saves and replays are not backed up",
        ),
        (
            Trouble::NoAccount,
            "Sign in to iCloud to back up saves and replays",
        ),
        (
            Trouble::Failing,
            "Could not back up saves and replays to iCloud",
        ),
    ] {
        shelf.set_backup(Some(trouble));
        assert_eq!(shelf.status_on(Platform::Tv), line);
        assert_eq!(shelf.status_on(Platform::Touch), "");
        assert_eq!(shelf.status_on(Platform::Desktop), "");
    }
    shelf.set_catalog(Vec::new(), 0);
    assert_eq!(
        shelf.status_on(Platform::Tv),
        "Could not back up saves and replays to iCloud",
        "an empty tab still hears it, and a refresh keeps it"
    );
    shelf.set_catalog(Vec::new(), 2);
    assert_eq!(
        shelf.status_on(Platform::Tv),
        "Downloading 2 files from iCloud",
        "files on their way say so first"
    );
    let old = entry("old", false, RecordKind::Save, "/nowhere/s.json".into());
    shelf.set_catalog(vec![old], 0);
    assert_eq!(
        shelf.status_on(Platform::Tv),
        "Made by a different version of Aftermine",
        "the focused record says why it will not open"
    );
    shelf.set_catalog(mixed(), 0);
    shelf.set_backup(None);
    assert_eq!(shelf.status_on(Platform::Tv), standing_line(Platform::Tv));
}
