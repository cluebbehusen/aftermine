//! The setup screen hosting a LAN lobby, against real joiners over
//! loopback.

use super::*;
use crate::netplay::{ClientLobby, Lobby};
use macroquad::prelude::vec2;
use oxide_net::JoinMessage;
use std::io::Write;
use std::time::Instant;

fn press(key: Key) -> [RawEvent; 2] {
    [RawEvent::KeyDown { key }, RawEvent::KeyUp { key }]
}

fn drive(w: &mut Wizard, draft: &mut NewMatchDraft, key: Key) -> Out {
    w.update(&press(key), &mut vec2(0.0, 0.0), draft, &mut Vec::new())
        .expect("update")
}

fn name(name: &str) -> PlayerName {
    PlayerName::new(name).unwrap()
}

/// Broad Front's setup screen hosting on loopback, the player on North
/// West. Its display order groups the teams: seats 0, 2, 1, 3.
fn hosting() -> (Wizard, NewMatchDraft) {
    let path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../scenarios/broad-front.json"
    ));
    let mut draft = NewMatchDraft::default();
    draft.set_scenario(Scenario::load(&path).expect("shipped map"), Some(path));
    draft.player_name = Some(name("Host"));
    let mut wizard = Wizard::open(&draft);
    wizard.goto(Step::Setup, &mut draft);
    wizard.lan_bind = "127.0.0.1:0".to_owned();
    wizard.open_lan(&mut draft);
    assert!(wizard.lan.is_some(), "{:?}", wizard.notice);
    assert!(
        draft
            .seats
            .iter()
            .enumerate()
            .all(|(seat, plan)| plan.open == (seat != draft.seat_choice)),
        "every chair but the host's opens"
    );
    (wizard, draft)
}

fn join(wizard: &Wizard, player: &str) -> Lobby {
    let address = wizard.lan.as_ref().expect("hosting").lobby.address();
    Lobby::Client(ClientLobby::new(
        address,
        &crate::build_identity().revision,
        name(player),
        oxide_net::InstallId::new(1),
    ))
}

/// Polls the host and every joiner until `done`, failing after ten
/// seconds; returns any match that started on each side.
fn settle(
    wizard: &mut Wizard,
    draft: &mut NewMatchDraft,
    clients: &mut [Lobby],
    done: impl Fn(&Wizard, &NewMatchDraft, &[Lobby]) -> bool,
) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        assert!(wizard.poll_lan(draft, Duration::ZERO).is_none());
        for client in clients.iter_mut() {
            assert!(client.poll(Duration::ZERO, vec2(1280.0, 800.0)).is_none());
        }
        if done(wizard, draft, clients) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Taps the controller chip on the open card at display position `row`,
/// giving the chair back to its bot.
fn bot_chair(w: &mut Wizard, draft: &mut NewMatchDraft, row: usize) {
    let seat = seat_display_order(draft.scenario.as_deref().unwrap())[row];
    w.setup_sel = row;
    w.setup_cell = Cell::Difficulty;
    drive(w, draft, Key::Enter);
    assert!(!draft.seats[seat].open);
}

fn member_at(wizard: &Wizard, seat: usize) -> Option<MemberId> {
    wizard.members()[seat]
}

#[test]
fn joiners_take_open_chairs_then_bots_in_display_order_and_see_who_sits_where() {
    let (mut w, mut draft) = hosting();
    let difficulty = draft.seats[1].difficulty;
    let mut clients = vec![join(&w, "Alex")];
    settle(&mut w, &mut draft, &mut clients, |_, _, c| {
        c[0].roster().len() == 4
    });
    assert!(
        member_at(&w, 2).is_some(),
        "South West is first in display order"
    );
    let roster = clients[0].roster();
    assert_eq!(roster[0], "1. Host (host)");
    assert_eq!(roster[2], "3. Alex (you)");
    assert_eq!(
        [roster[1].as_str(), roster[3].as_str()],
        ["2. Open", "4. Open"]
    );
    assert!(w.ui_surface(&draft).1[1].contains("(player Alex)"));

    // An open chair goes before any bot.
    bot_chair(&mut w, &mut draft, 2);
    assert_eq!(
        draft.seats[1].difficulty, difficulty,
        "North East's bot is back"
    );
    clients.push(join(&w, "Sam"));
    settle(&mut w, &mut draft, &mut clients, |w, _, _| {
        member_at(w, 3).is_some()
    });
    assert!(member_at(&w, 1).is_none(), "North East keeps its bot");
    let hosting = w.hosting_line(&draft).unwrap();
    assert!(hosting.ends_with("3 of 3 players here"), "{hosting}");

    // With no chair open, a joiner replaces a bot.
    clients.push(join(&w, "Riley"));
    settle(&mut w, &mut draft, &mut clients, |w, _, _| {
        member_at(w, 1).is_some()
    });
    assert!(draft.seats[1].open);

    // A leaver's chair stays open.
    clients.pop();
    settle(&mut w, &mut draft, &mut clients, |w, _, _| {
        member_at(w, 1).is_none()
    });
    assert!(draft.seats[1].open);
    assert!(
        w.hosting_line(&draft)
            .unwrap()
            .ends_with("3 of 4 players here")
    );
}

#[test]
fn the_host_moves_players_kicks_them_and_starts_only_with_every_chair_filled() {
    let (mut w, mut draft) = hosting();
    let mut clients = vec![join(&w, "Alex")];
    settle(&mut w, &mut draft, &mut clients, |w, _, _| {
        member_at(w, 2).is_some()
    });
    let alex = member_at(&w, 2).unwrap();

    // Pick Alex up from South West and drop them on North East's bot.
    bot_chair(&mut w, &mut draft, 2);
    w.setup_cell = Cell::Seat;
    w.setup_sel = 1;
    drive(&mut w, &mut draft, Key::Enter);
    assert_eq!(w.moving, Some(2));
    assert!(w.ui_surface(&draft).1[1].contains("moving"));
    w.setup_sel = 2;
    drive(&mut w, &mut draft, Key::Enter);
    assert_eq!(w.moving, None);
    assert_eq!(member_at(&w, 1), Some(alex));
    assert!(
        draft.seats[1].open && !draft.seats[2].open,
        "the bot took South West"
    );
    assert_eq!(draft.seats[1].team_choice, 2, "the seat kept its team");
    settle(&mut w, &mut draft, &mut clients, |_, _, c| {
        c[0].roster()
            .get(1)
            .is_some_and(|line| line == "2. Alex (you)")
    });

    // Escape drops a pick instead of leaving setup.
    w.setup_sel = 0;
    drive(&mut w, &mut draft, Key::Enter);
    assert_eq!(w.moving, Some(0));
    drive(&mut w, &mut draft, Key::Escape);
    assert_eq!((w.moving, w.step), (None, Step::Setup));

    // An empty open chair holds Start.
    w.setup_sel = 4;
    assert_eq!(drive(&mut w, &mut draft, Key::Enter), Out::Stay);
    assert_eq!(w.notice.as_deref(), Some("Seat 4 is still open"));
    drive(&mut w, &mut draft, Key::Enter);

    // Removing Alex asks first, with Cancel preselected.
    w.setup_sel = 2;
    w.setup_cell = Cell::Difficulty;
    drive(&mut w, &mut draft, Key::Enter);
    assert_eq!(w.mode_name(), "confirm_lobby");
    assert_eq!(w.ui_surface(&draft).0, "REMOVE ALEX?");
    drive(&mut w, &mut draft, Key::Enter);
    assert_eq!(member_at(&w, 1), Some(alex), "Cancel keeps them");
    drive(&mut w, &mut draft, Key::Enter);
    drive(&mut w, &mut draft, Key::Down);
    drive(&mut w, &mut draft, Key::Enter);
    assert_eq!(member_at(&w, 1), None);
    settle(&mut w, &mut draft, &mut clients, |_, _, c| {
        c[0].failure().is_some()
    });
    let (_, _, reason) = clients[0].failure().unwrap();
    assert_eq!(reason, "The host removed you from the lobby");
}

#[test]
fn a_full_lobby_starts_on_every_machine_under_the_players_names() {
    let (mut w, mut draft) = hosting();
    let mut clients = vec![join(&w, "Alex")];
    settle(&mut w, &mut draft, &mut clients, |w, _, _| {
        member_at(w, 2).is_some()
    });
    bot_chair(&mut w, &mut draft, 2);
    bot_chair(&mut w, &mut draft, 3);
    w.setup_sel = 4;
    assert_eq!(drive(&mut w, &mut draft, Key::Enter), Out::Launch);
    assert!(w.hosting_players());
    let scenario = launch::hosted_scenario(&draft, 0x500, &w.member_names()).unwrap();
    w.start_lan(&draft, &scenario, Duration::ZERO).unwrap();
    assert!(w.hosting_line(&draft).unwrap().starts_with("starting"));

    // The start launches the setup it froze, so nothing changes it.
    let frozen = draft.seats.clone();
    w.setup_sel = 1;
    w.setup_cell = Cell::Difficulty;
    drive(&mut w, &mut draft, Key::Enter);
    assert_eq!(draft.seats, frozen);

    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    let (mut host, mut joiner) = (None, None);
    while host.is_none() || joiner.is_none() {
        assert!(Instant::now() < deadline, "timed out");
        if host.is_none() {
            host = w.poll_lan(&mut draft, Duration::ZERO);
        }
        if joiner.is_none() {
            joiner = clients[0].poll(Duration::ZERO, vec2(1280.0, 800.0));
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let ((host, _), (joiner, _)) = (host.unwrap(), joiner.unwrap());
    for game in [&host, &joiner] {
        let names: Vec<&str> = game
            .scenario
            .players
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names[0], "Host");
        assert_eq!(names[2], "Alex");
    }
    assert_eq!(host.state.hash(), joiner.state.hash());
    assert!(w.lan.is_none(), "the lobby handed its sockets to the match");
    assert!(draft.seats.iter().all(|plan| !plan.open));
}

#[test]
fn escape_calls_off_a_start_and_the_next_one_still_runs() {
    let (mut w, mut draft) = hosting();
    let mut clients = vec![join(&w, "Alex")];
    settle(&mut w, &mut draft, &mut clients, |w, _, _| {
        member_at(w, 2).is_some()
    });
    bot_chair(&mut w, &mut draft, 2);
    bot_chair(&mut w, &mut draft, 3);
    let scenario = launch::hosted_scenario(&draft, 0x500, &w.member_names()).unwrap();
    w.start_lan(&draft, &scenario, Duration::ZERO).unwrap();
    drive(&mut w, &mut draft, Key::Escape);
    let lan = w.lan.as_ref().expect("the lobby stays open");
    assert!(lan.starting.is_none());
    assert_eq!(w.step, Step::Setup);
    assert!(member_at(&w, 2).is_some(), "Alex keeps the seat");

    assert!(w.poll_lan(&mut draft, Duration::ZERO).is_none());
    w.start_lan(&draft, &scenario, Duration::ZERO).unwrap();
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    let (mut host, mut joiner) = (None, None);
    while host.is_none() || joiner.is_none() {
        assert!(Instant::now() < deadline, "timed out");
        if host.is_none() {
            host = w.poll_lan(&mut draft, Duration::ZERO);
        }
        if joiner.is_none() {
            joiner = clients[0].poll(Duration::ZERO, vec2(1280.0, 800.0));
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let ((host, _), (joiner, _)) = (host.unwrap(), joiner.unwrap());
    assert_eq!(host.state.hash(), joiner.state.hash());
}

#[test]
fn a_failed_start_frees_its_culprits_chairs_before_anything_can_start_again() {
    let (mut w, mut draft) = hosting();
    let address = w.lan.as_ref().unwrap().lobby.address().to_owned();
    let mut rogue = std::net::TcpStream::connect(address).unwrap();
    let revision = crate::build_identity().revision;
    let introduce = JoinMessage::Introduce {
        name: name("Rogue"),
        install: oxide_net::InstallId::new(2),
    };
    for message in [JoinMessage::hello(&revision), introduce] {
        writeln!(rogue, "{}", message.encode()).unwrap();
    }
    settle(&mut w, &mut draft, &mut [], |w, _, _| {
        member_at(w, 2).is_some()
    });
    bot_chair(&mut w, &mut draft, 2);
    bot_chair(&mut w, &mut draft, 3);
    let scenario = launch::hosted_scenario(&draft, 0x500, &w.member_names()).unwrap();
    w.start_lan(&draft, &scenario, Duration::ZERO).unwrap();
    let wrong = JoinMessage::Ready {
        hash: 0,
        attempt: 1,
    };
    writeln!(rogue, "{}", wrong.encode()).unwrap();
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while w.notice.is_none() {
        assert!(Instant::now() < deadline, "timed out");
        assert!(w.poll_lan(&mut draft, Duration::ZERO).is_none());
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(w.notice.as_deref(), Some("A player's game didn't match"));
    assert_eq!(
        member_at(&w, 2),
        None,
        "the same frame frees the chair, so Start cannot seat the culprit"
    );
}

#[test]
fn a_map_without_seats_cannot_host() {
    let mut scenario = Scenario::skirmish();
    scenario.players.clear();
    let mut draft = NewMatchDraft::default();
    draft.set_scenario(scenario, None);
    draft.player_name = Some(name("Host"));
    let mut w = Wizard::open(&draft);
    w.lan_bind = "127.0.0.1:0".to_owned();
    w.open_lan(&mut draft);
    assert!(w.lan.is_none());
    assert_eq!(w.notice.as_deref(), Some("This map has no seats"));
}

#[test]
fn an_empty_lobby_starts_a_local_match_and_closes_quietly() {
    let (mut w, mut draft) = hosting();
    for row in 1..4 {
        bot_chair(&mut w, &mut draft, row);
    }
    w.setup_sel = 4;
    assert_eq!(drive(&mut w, &mut draft, Key::Enter), Out::Launch);
    assert!(w.lan.is_none());
    assert!(!w.hosting_players());

    let (mut w, mut draft) = hosting();
    w.on_switch = true;
    drive(&mut w, &mut draft, Key::Enter);
    assert!(
        w.lan.is_none(),
        "switching to Local with nobody here asks nothing"
    );
    assert_eq!(w.ui_surface(&draft).1.last().unwrap(), "Mode: Local");
    assert!(draft.seats.iter().all(|plan| !plan.open), "the bots return");
}

#[test]
fn leaving_or_going_local_with_players_asks_first() {
    let (mut w, mut draft) = hosting();
    let mut clients = vec![join(&w, "Alex")];
    settle(&mut w, &mut draft, &mut clients, |w, _, _| {
        member_at(w, 2).is_some()
    });

    drive(&mut w, &mut draft, Key::Escape);
    assert_eq!(w.mode_name(), "confirm_lobby");
    assert_eq!(w.ui_surface(&draft).0, "CLOSE THE LOBBY?");
    drive(&mut w, &mut draft, Key::Escape);
    assert_eq!(w.mode_name(), "match_setup", "Escape keeps the lobby");

    w.on_switch = true;
    drive(&mut w, &mut draft, Key::Enter);
    assert_eq!(w.mode_name(), "confirm_lobby");
    drive(&mut w, &mut draft, Key::Down);
    drive(&mut w, &mut draft, Key::Enter);
    assert!(w.lan.is_none());
    assert_eq!(w.step, Step::Setup, "going local stays on setup");
    assert!(draft.seats.iter().all(|plan| !plan.open));
    settle(&mut w, &mut draft, &mut clients, |_, _, c| {
        c[0].failure().is_some()
    });
}

#[test]
fn hosting_without_a_name_asks_for_one_and_saves_it() {
    let (mut w, mut draft) = hosting();
    w.close_lan(&mut draft);
    draft.player_name = None;
    w.on_switch = true;
    drive(&mut w, &mut draft, Key::Enter);
    assert_eq!(w.mode_name(), "player_name");
    assert_eq!(w.keyboard(), Some(Keyboard::Text));
    let typed: Vec<RawEvent> = "Riley".chars().map(|ch| RawEvent::Text { ch }).collect();
    w.update(&typed, &mut vec2(0.0, 0.0), &mut draft, &mut Vec::new())
        .unwrap();
    drive(&mut w, &mut draft, Key::Enter);
    assert_eq!(w.take_saved_name(), Some(name("Riley")));
    assert_eq!(w.take_saved_name(), None);
    assert_eq!(draft.player_name, Some(name("Riley")));
    assert!(w.lan.is_some(), "the lobby opens once named");
    assert_eq!(w.keyboard(), None);

    w.close_lan(&mut draft);
    draft.player_name = None;
    w.on_switch = true;
    drive(&mut w, &mut draft, Key::Enter);
    drive(&mut w, &mut draft, Key::Escape);
    assert!(w.lan.is_none(), "cancelling the name stays local");
}

#[test]
fn a_taken_port_keeps_the_setup_local_and_says_why() {
    let (mut w, mut draft) = hosting();
    let taken = w.lan.as_ref().unwrap().lobby.address().to_owned();
    let mut other = Wizard::open(&draft);
    other.goto(Step::Setup, &mut draft);
    for plan in &mut draft.seats {
        plan.open = false;
    }
    other.lan_bind = taken;
    other.open_lan(&mut draft);
    assert!(other.lan.is_none());
    assert!(
        draft.seats.iter().all(|plan| !plan.open),
        "a lobby that never opened opens no chairs"
    );
    assert!(other.notice.as_deref().unwrap().starts_with("Cannot host"));
    w.close_lan(&mut draft);
}
