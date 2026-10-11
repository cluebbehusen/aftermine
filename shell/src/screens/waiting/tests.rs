use super::*;
use macroquad::prelude::vec2;

fn absent(seat: u8, name: &str, seconds: u64) -> Absent {
    Absent {
        seat: PlayerId(seat),
        name: name.to_owned(),
        waited: Duration::from_secs(seconds),
    }
}

fn press(screen: &mut WaitingScreen, key: Key) -> Out {
    screen.update(
        &[RawEvent::KeyDown { key }, RawEvent::KeyUp { key }],
        &mut vec2(0.0, 0.0),
        &mut Vec::new(),
    )
}

#[test]
fn the_host_can_drop_each_absent_seat_and_everyone_can_open_the_menu() {
    let host = WaitingScreen::open(vec![absent(1, "Alex", 42), absent(2, "Sam", 5)], true);
    assert_eq!(host.view().items, ["Drop Alex", "Drop Sam", "Menu"]);
    assert_eq!(host.subtitle(), "Waiting for Alex and Sam · 0:42");
    assert_eq!(host.mode_name(), "waiting");

    let client = WaitingScreen::open(vec![absent(1, "Alex", 65)], false);
    assert_eq!(client.view().items, ["Menu"]);
    assert_eq!(client.subtitle(), "Waiting for Alex · 1:05");
}

#[test]
fn dropping_asks_first_with_cancel_preselected() {
    let mut host = WaitingScreen::open(vec![absent(1, "Alex", 3)], true);
    assert_eq!(press(&mut host, Key::Enter), Out::Stay);
    assert_eq!(host.mode_name(), "confirm_drop");
    assert_eq!(host.view().title, "DROP ALEX?");
    assert_eq!(host.view().items, ["Cancel", "Drop Alex"]);
    assert_eq!(host.subtitle(), "Alex's seat surrenders");
    assert_eq!(press(&mut host, Key::Enter), Out::Stay, "Cancel");
    assert_eq!(host.mode_name(), "waiting");

    press(&mut host, Key::Enter);
    assert_eq!(press(&mut host, Key::Escape), Out::Stay);
    assert_eq!(host.mode_name(), "waiting", "Escape backs out");

    press(&mut host, Key::Enter);
    press(&mut host, Key::Down);
    assert_eq!(press(&mut host, Key::Enter), Out::Drop(PlayerId(1)));
    assert_eq!(host.mode_name(), "waiting");
}

#[test]
fn escape_or_the_menu_row_opens_the_match_menu() {
    let mut client = WaitingScreen::open(vec![absent(1, "Alex", 3)], false);
    assert_eq!(press(&mut client, Key::Escape), Out::Menu);
    assert_eq!(press(&mut client, Key::Enter), Out::Menu);
}

#[test]
fn a_confirmation_for_a_seat_no_longer_waited_on_closes() {
    let mut host = WaitingScreen::open(vec![absent(1, "Alex", 3), absent(2, "Sam", 3)], true);
    press(&mut host, Key::Enter);
    assert_eq!(host.mode_name(), "confirm_drop");
    host.refresh(vec![absent(2, "Sam", 4)]);
    assert_eq!(host.mode_name(), "waiting");
    assert_eq!(host.view().items, ["Drop Sam", "Menu"]);
}

#[test]
fn the_same_seats_keep_the_cursor_as_the_wait_grows() {
    let mut host = WaitingScreen::open(vec![absent(1, "Alex", 3), absent(2, "Sam", 3)], true);
    press(&mut host, Key::Down);
    assert_eq!(host.view().selected, 1);
    host.refresh(vec![absent(1, "Alex", 70), absent(2, "Sam", 9)]);
    assert_eq!(host.view().selected, 1);
    assert_eq!(host.subtitle(), "Waiting for Alex and Sam · 1:10");
}
