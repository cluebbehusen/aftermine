use super::*;
use crate::netplay::ClientLobby;
use macroquad::prelude::vec2;

fn feed(screen: &mut LobbyScreen, events: &[RawEvent]) -> Out {
    screen.update(events, &mut vec2(0.0, 0.0), &mut Vec::new())
}

fn press(screen: &mut LobbyScreen, key: Key) -> Out {
    feed(
        screen,
        &[RawEvent::KeyDown { key }, RawEvent::KeyUp { key }],
    )
}

fn typed(text: &str) -> Vec<RawEvent> {
    text.chars().map(|ch| RawEvent::Text { ch }).collect()
}

fn join(name: &str, address: &str) -> Out {
    Out::Join {
        name: PlayerName::new(name).unwrap(),
        address: address.to_owned(),
    }
}

#[test]
fn a_new_player_types_a_name_then_an_address_and_joins_on_the_default_port() {
    crate::render::set_viewport(1280.0, 800.0);
    let mut screen = LobbyScreen::join(None, "");
    assert_eq!(screen.mode_name(), "join_match");
    assert_eq!(screen.keyboard(), Some(Keyboard::Text));
    feed(&mut screen, &typed("Alex Q. 1/2"));
    assert_eq!(press(&mut screen, Key::Enter), Out::Stay, "Enter moves on");
    assert_eq!(screen.keyboard(), Some(Keyboard::Address));
    feed(&mut screen, &typed("10.0.0.2 @"));
    assert_eq!(
        screen.menu.view().items,
        vec!["Your name: Alex Q. 1/2", "Host address: 10.0.0.2_"],
        "the address field drops what no address holds"
    );
    assert_eq!(
        press(&mut screen, Key::Enter),
        join("Alex Q. 1/2", "10.0.0.2:4200")
    );
    assert_eq!(press(&mut screen, Key::Escape), Out::Cancel);
}

#[test]
fn a_returning_player_starts_on_the_address_and_bad_input_says_why() {
    crate::render::set_viewport(1280.0, 800.0);
    let mut screen = LobbyScreen::join(Some("Connor"), "192.168.1");
    assert_eq!(screen.keyboard(), Some(Keyboard::Address));
    assert_eq!(press(&mut screen, Key::Enter), Out::Stay);
    let Face::Join { form, .. } = &screen.face else {
        panic!("the form stays up");
    };
    assert_eq!(
        form.layout(vec2(1280.0, 800.0), 1.0).fields.len(),
        2,
        "name and address share a row"
    );

    let mut screen = LobbyScreen::join(Some("   "), "connor-mbp:5000");
    assert_eq!(
        screen.keyboard(),
        Some(Keyboard::Text),
        "a blank name needs typing"
    );
    assert_eq!(press(&mut screen, Key::Tab), Out::Stay);
    assert_eq!(
        press(&mut screen, Key::Enter),
        Out::Stay,
        "the name is still blank"
    );
    assert_eq!(
        screen.keyboard(),
        Some(Keyboard::Text),
        "and takes focus back"
    );
    feed(&mut screen, &typed("Sam"));
    assert_eq!(press(&mut screen, Key::Tab), Out::Stay);
    assert_eq!(
        press(&mut screen, Key::Enter),
        join("Sam", "connor-mbp:5000")
    );

    let host = [63, 63, 63, 61].map(|len| "a".repeat(len)).join(".");
    let longest = format!("{host}:65535");
    assert_eq!(longest.len(), ADDRESS_MAX);
    let mut screen = LobbyScreen::join(Some("Connor"), &longest);
    assert_eq!(press(&mut screen, Key::Enter), join("Connor", &longest));
}

#[test]
fn a_failed_join_returns_to_the_form_with_the_reason_and_both_values() {
    crate::render::set_viewport(1280.0, 800.0);
    let unreachable = "127.0.0.1:1";
    let mut screen = LobbyScreen::waiting(Lobby::Client(ClientLobby::new(
        unreachable,
        "abc",
        PlayerName::new("Alex").unwrap(),
        aftermine_net::InstallId::new(1),
    )));
    assert_eq!(screen.keyboard(), None);
    assert_eq!(screen.mode_name(), "lobby");
    assert!(screen.subtitle().starts_with("Connecting to"));
    assert_eq!(
        screen.menu.view().items,
        vec!["Cancel"],
        "no roster before the room arrives"
    );
    assert_eq!(press(&mut screen, Key::Escape), Out::Cancel);
    assert_eq!(press(&mut screen, Key::Enter), Out::Cancel);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while screen.keyboard().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "the connect never failed"
        );
        assert!(screen.poll(Duration::ZERO, vec2(1280.0, 800.0)).is_none());
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        screen.menu.view().items,
        vec!["Your name: Alex", "Host address: 127.0.0.1:1_"]
    );
    assert!(screen.subtitle().starts_with("Cannot reach 127.0.0.1:1"));
    assert_eq!(press(&mut screen, Key::Enter), join("Alex", unreachable));
}

#[test]
fn a_typed_address_that_cannot_parse_opens_the_form_with_why() {
    let screen = LobbyScreen::failed("", "nope:0", "the port must be a number".to_owned());
    assert_eq!(screen.subtitle(), "the port must be a number");
    assert_eq!(screen.keyboard(), Some(Keyboard::Text));
    assert_eq!(LobbyScreen::join(None, "").subtitle(), "");
}

/// A Join form listening on loopback, and an advertiser aimed at it.
fn listening(name: Option<&str>) -> (LobbyScreen, aftermine_net::Advertiser) {
    let scanner = aftermine_net::Scanner::bind_to("127.0.0.1:0", "abc").unwrap();
    let target = scanner.local_addr().unwrap();
    let screen = LobbyScreen::join(name, "").with_scanner(scanner);
    (
        screen,
        aftermine_net::Advertiser::with_target(target).unwrap(),
    )
}

fn beacon(commit: &str) -> aftermine_net::discovery::Beacon {
    aftermine_net::discovery::Beacon::new(
        commit,
        PlayerName::new("Connor").unwrap(),
        4321,
        "Broad Front",
        2,
        4,
    )
}

/// Polls `screen` until its report lists `rows` entries.
fn heard(screen: &mut LobbyScreen, rows: usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while screen.menu.view().items.len() < rows {
        assert!(std::time::Instant::now() < deadline, "no beacon arrived");
        screen.poll(Duration::ZERO, vec2(1280.0, 800.0));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn tap(at: Vec2) -> [RawEvent; 2] {
    [
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

#[test]
fn a_lobby_heard_on_the_network_lists_and_a_tap_joins_it() {
    crate::render::set_viewport(1280.0, 800.0);
    let (mut screen, mut advertiser) = listening(Some("Alex"));
    advertiser.poll(&beacon("abc"), Duration::ZERO);
    heard(&mut screen, 3);
    assert_eq!(
        screen.menu.view().items[2],
        "Match: Broad Front | Connor | 2 of 4 players"
    );
    let Face::Join { form, .. } = &screen.face else {
        panic!("the form is up");
    };
    let row = match_rows(form, 1)[0].center();
    assert_eq!(feed(&mut screen, &tap(row)), join("Alex", "127.0.0.1:4321"));
}

#[test]
fn the_keyboard_reaches_a_listed_lobby_from_the_last_field() {
    crate::render::set_viewport(1280.0, 800.0);
    let (mut screen, mut advertiser) = listening(Some("Alex"));
    advertiser.poll(&beacon("abc"), Duration::ZERO);
    heard(&mut screen, 3);
    assert_eq!(press(&mut screen, Key::Down), Out::Stay);
    assert_eq!(screen.menu.view().selected, 2, "Down leaves the address");
    press(&mut screen, Key::Up);
    assert_eq!(screen.menu.view().selected, 1, "Up returns to it");
    press(&mut screen, Key::Down);
    assert_eq!(feed(&mut screen, &typed("x")), Out::Stay);
    assert_eq!(
        press(&mut screen, Key::Enter),
        join("Alex", "127.0.0.1:4321")
    );
}

#[test]
fn a_tap_joins_the_lobby_it_pressed_even_if_the_list_reorders() {
    crate::render::set_viewport(1280.0, 800.0);
    let (mut screen, mut advertiser) = listening(Some("Alex"));
    advertiser.poll(&beacon("abc"), Duration::ZERO);
    heard(&mut screen, 3);
    let Face::Join { form, .. } = &screen.face else {
        panic!("the form is up");
    };
    let row = match_rows(form, 1)[0].center();
    let [down, up] = tap(row);
    assert_eq!(feed(&mut screen, &[down]), Out::Stay);

    // A host that sorts first arrives under the finger.
    let earlier = aftermine_net::discovery::Beacon::new(
        "abc",
        PlayerName::new("Avery").unwrap(),
        4322,
        "Broad Front",
        1,
        4,
    );
    advertiser.poll(&earlier, Duration::from_secs(1));
    heard(&mut screen, 4);
    assert_eq!(feed(&mut screen, &[up]), Out::Stay, "Connor's lobby moved");
}

#[test]
fn joining_a_listed_lobby_still_needs_a_name_and_other_builds_are_inert() {
    crate::render::set_viewport(1280.0, 800.0);
    let (mut screen, mut advertiser) = listening(None);
    advertiser.poll(&beacon("abc"), Duration::ZERO);
    heard(&mut screen, 3);
    let Face::Join { form, .. } = &screen.face else {
        panic!("the form is up");
    };
    let row = match_rows(form, 1)[0].center();
    let mut sounds = Vec::new();
    let out = screen.update(&tap(row), &mut vec2(0.0, 0.0), &mut sounds);
    assert_eq!(out, Out::Stay);
    assert!(sounds.iter().any(|(kind, _)| *kind == SoundKind::Denied));
    assert_eq!(
        screen.keyboard(),
        Some(Keyboard::Text),
        "the name takes focus"
    );

    let (mut screen, mut advertiser) = listening(Some("Alex"));
    advertiser.poll(&beacon("other"), Duration::ZERO);
    heard(&mut screen, 3);
    assert_eq!(
        screen.menu.view().items[2],
        "Match: Connor | different build"
    );
    let Face::Join { form, .. } = &screen.face else {
        panic!("the form is up");
    };
    let row = match_rows(form, 1)[0].center();
    assert_eq!(feed(&mut screen, &tap(row)), Out::Stay);
}
