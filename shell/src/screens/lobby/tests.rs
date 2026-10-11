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
