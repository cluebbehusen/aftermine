use super::*;
use crate::tests::support::keystroke;
use aftermine_protocol::Key;
use macroquad::prelude::vec2;

fn pick(home: &mut HomeScreen, row: usize) -> Out {
    home.menu.select(row);
    let mut mouse = vec2(0.0, 0.0);
    let mut sounds = Vec::new();
    home.update(
        &[
            RawEvent::KeyDown { key: Key::Enter },
            RawEvent::KeyUp { key: Key::Enter },
        ],
        &mut mouse,
        &mut sounds,
    )
}

#[test]
fn rows_mean_the_same_verbs_with_and_without_a_continue_row() {
    // The Continue row shifts every index below it, so each row must
    // still map to its own verb.
    let mut fresh = HomeScreen::with_resumable(false);
    assert_eq!(pick(&mut fresh, 0), Out::Play);
    assert_eq!(pick(&mut fresh, 1), Out::Join);
    assert_eq!(pick(&mut fresh, 3), Out::Replays);
    assert_eq!(pick(&mut fresh, 4), Out::Roster);
    assert_eq!(pick(&mut fresh, 5), Out::Settings);
    assert_eq!(pick(&mut fresh, 6), Out::Quit);

    let mut resumable = HomeScreen::with_resumable(true);
    assert_eq!(pick(&mut resumable, 0), Out::Continue);
    assert_eq!(pick(&mut resumable, 1), Out::Play);
    assert_eq!(pick(&mut resumable, 5), Out::Roster);
    assert_eq!(pick(&mut resumable, 7), Out::Quit);
}
#[test]
fn a_touch_only_door_offers_no_quit_and_keeps_its_verbs() {
    let mut door = HomeScreen::build(true, false);
    assert!(!door.menu.view().items.iter().any(|item| item == "Quit"));
    assert_eq!(pick(&mut door, 0), Out::Continue);
    assert_eq!(pick(&mut door, 6), Out::Settings);
}

#[test]
fn a_seat_offered_back_tops_the_door_until_the_offer_ends() {
    let mut home = HomeScreen::with_resumable(true);
    home.set_rejoin(Some("Rejoin Connor's match on Broad Front".to_owned()));
    assert_eq!(
        home.menu.view().items[0],
        "Rejoin Connor's match on Broad Front"
    );
    assert_eq!(pick(&mut home, 0), Out::Rejoin);
    assert_eq!(
        pick(&mut home, 1),
        Out::Continue,
        "the rows below keep their verbs"
    );
    assert!(home.menu.wide, "the long row fits inside the list");

    home.set_catalog(true);
    assert_eq!(
        home.menu.view().items[0],
        "Rejoin Connor's match on Broad Front",
        "a catalog refresh keeps the offer"
    );
    home.set_rejoin(None);
    assert_eq!(pick(&mut home, 0), Out::Continue);
    assert!(!home.menu.wide);
}

#[test]
fn the_first_scan_puts_an_untouched_cursor_on_continue() {
    let mut home = HomeScreen::open();
    assert_eq!(home.menu.value(), Some(&Out::Play));
    home.set_rejoin(Some("Rejoin Connor's match on Broad Front".to_owned()));
    assert_eq!(home.menu.value(), Some(&Out::Play), "an offer is no scan");
    home.set_catalog(true);
    assert_eq!(home.menu.value(), Some(&Out::Continue));
    home.set_catalog(true);
    assert_eq!(home.menu.value(), Some(&Out::Continue));

    let mut nothing = HomeScreen::open();
    nothing.set_catalog(false);
    assert_eq!(nothing.menu.value(), Some(&Out::Play));
    nothing.set_catalog(true);
    assert_eq!(
        nothing.menu.value(),
        Some(&Out::Play),
        "only the first scan moves the cursor"
    );
}

#[test]
fn a_placed_cursor_survives_the_first_scan_and_rescans() {
    let mut moved = HomeScreen::open();
    assert_eq!(press(&mut moved, Key::Down), Out::Stay);
    assert_eq!(moved.menu.value(), Some(&Out::Join));
    moved.set_catalog(true);
    assert_eq!(moved.menu.value(), Some(&Out::Join));
    moved.set_catalog(false);
    moved.set_catalog(true);
    assert_eq!(moved.menu.value(), Some(&Out::Join));

    for row in [Out::Play, Out::Replays] {
        let mut back = HomeScreen::returning_to(row);
        assert_eq!(back.menu.value(), Some(&row));
        back.set_catalog(true);
        assert_eq!(back.menu.value(), Some(&row), "Back lands where it left");
        back.set_catalog(true);
        assert_eq!(back.menu.value(), Some(&row));
    }
}

#[test]
fn the_door_names_the_build_by_version_and_short_commit() {
    use aftermine_kit::build::BuildIdentity;
    let commit = "228dd0e6ff22cd175b8f8ba27c718bcff896d6eb";
    assert_eq!(
        describe_build(&BuildIdentity::new("0.1.0", commit, "false")),
        "v0.1.0 (228dd0e)"
    );
    assert_eq!(
        describe_build(&BuildIdentity::new("0.1.0", commit, "true")),
        "v0.1.0 (228dd0e-dirty)"
    );
    assert_eq!(
        describe_build(&BuildIdentity::new("0.1.0", "unknown", "unknown")),
        "v0.1.0"
    );
}

#[test]
fn home_grows_only_on_windows_larger_than_its_layout() {
    assert_eq!(growth(vec2(1133.0, 744.0)), 1.0);
    assert_eq!(growth(vec2(1440.0, 900.0)), 1.0);
    let large = growth(vec2(2560.0, 1440.0));
    assert!((1.3..1.4).contains(&large), "{large}");
    assert_eq!(growth(vec2(9000.0, 9000.0)), 1.6);
}

fn press(home: &mut HomeScreen, key: Key) -> Out {
    let mut mouse = vec2(0.0, 0.0);
    let mut sounds = Vec::new();
    home.update(&keystroke(key), &mut mouse, &mut sounds)
}

#[test]
fn a_mistaken_press_on_the_missing_files_question_deletes_nothing() {
    let mut home = HomeScreen::with_resumable(true);
    home.set_missing(
        vec!["saves/a.amsave".to_owned(), "saves/b.amsave".to_owned()],
        0,
    );
    assert_eq!(home.mode_name(), "confirm_missing");
    home.age_question(2.0);
    assert_eq!(press(&mut home, Key::Enter), Out::BringBackMissing);
    assert_eq!(press(&mut home, Key::Escape), Out::BringBackMissing);
    assert_eq!(home.take_missing().len(), 2);
    assert_eq!(home.mode_name(), "home");
}

#[test]
fn deleting_missing_files_takes_a_deliberate_choice() {
    let mut home = HomeScreen::with_resumable(true);
    home.set_missing(vec!["saves/a.amsave".to_owned()], 0);
    home.age_question(2.0);
    assert_eq!(press(&mut home, Key::Down), Out::Stay);
    assert_eq!(press(&mut home, Key::Enter), Out::DeleteMissing);
}

#[test]
fn the_question_goes_away_once_nothing_is_missing_and_survives_a_rescan() {
    let mut home = HomeScreen::with_resumable(true);
    home.set_missing(vec!["saves/a.amsave".to_owned()], 0);
    home.set_catalog(true);
    assert_eq!(home.mode_name(), "confirm_missing");
    home.set_missing(Vec::new(), 0);
    assert_eq!(home.mode_name(), "home");
}

#[test]
fn presses_aimed_at_home_never_land_on_the_question() {
    let mut home = HomeScreen::with_resumable(true);
    home.set_missing(vec!["saves/a.amsave".to_owned()], 0);
    assert_eq!(press(&mut home, Key::Down), Out::Stay);
    assert_eq!(press(&mut home, Key::Enter), Out::Stay);
    home.age_question(2.0);
    assert_eq!(press(&mut home, Key::Enter), Out::BringBackMissing);
}

#[test]
fn the_missing_files_question_names_what_is_missing() {
    let now = 1_800_000_000;
    let subtitle = |names: &[&str]| {
        let names = names.iter().map(|name| (*name).to_owned()).collect();
        MissingQuestion::new(names, now).subtitle
    };
    assert_eq!(
        subtitle(&["saves/save-0000012000-1799996400.amsave"]),
        "A save from 1 h ago is missing from the Aftermine folder"
    );
    assert_eq!(
        subtitle(&["saves/lan-0000012000-1799996400-2.amsave"]),
        "A LAN save from 1 h ago is missing from the Aftermine folder"
    );
    assert_eq!(
        subtitle(&["saves/campaign.amsave"]),
        "The save campaign.amsave is missing from the Aftermine folder"
    );
    assert_eq!(
        subtitle(&["replays/final.amreplay"]),
        "The replay final.amreplay is missing from the Aftermine folder"
    );
    assert_eq!(
        subtitle(&["saves/a.amsave", "saves/b.amsave"]),
        "2 saves are missing from the Aftermine folder"
    );
    assert_eq!(
        subtitle(&["replays/a.amreplay", "replays/b.amreplay"]),
        "2 replays are missing from the Aftermine folder"
    );
    assert_eq!(
        subtitle(&["saves/a.amsave", "replays/b.amreplay"]),
        "2 saves and replays are missing from the Aftermine folder"
    );
}
