use super::*;
use macroquad::prelude::vec2;
use oxide_protocol::Key;

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
fn the_door_names_the_build_by_version_and_short_commit() {
    use oxide_kit::build::BuildIdentity;
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
