//! The setup screen resuming a saved LAN match, against real joiners over
//! loopback.

use super::*;
use crate::game::checkpoint::LanManifest;
use crate::netplay::{ClientLobby, Lobby};
use macroquad::prelude::vec2;
use oxide_sim::scenario::BotConfig;
use std::time::Instant;

const SAM: InstallId = InstallId::new(11);
const ALEX: InstallId = InstallId::new(12);

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

/// Broad Front saved 40 ticks in, hosted from North West: Sam played North
/// East and Alex South West, and South East is a bot. Its display order
/// groups the teams: seats 0, 2, 1, 3.
fn saved_match() -> LanSave {
    saved_with(&[])
}

/// The same match, with `surrendered` seats out of it when it was saved.
fn saved_with(surrendered: &[PlayerId]) -> LanSave {
    let path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../scenarios/broad-front.json"
    ));
    let mut scenario = Scenario::load(&path).expect("shipped map");
    for (seat, player) in scenario.players.iter_mut().enumerate() {
        player.bot = seat == 3;
        player.bot_config = player.bot.then_some(BotConfig::default());
    }
    scenario.players[0].name = "Host".to_owned();
    scenario.players[1].name = "Sam".to_owned();
    scenario.players[2].name = "Alex".to_owned();
    let mut game = Game::with_viewport(scenario.clone(), vec2(1280.0, 800.0)).unwrap();
    for &seat in surrendered {
        game.stage(oxide_sim::PlayerCommand {
            player: seat,
            command: oxide_sim::Command::Surrender,
        });
    }
    game.advance_ticks(40);
    let lan = LanManifest::new(
        &scenario,
        PlayerId(0),
        &[(PlayerId(1), SAM), (PlayerId(2), ALEX)],
    );
    game.capture_save()
        .for_lan(Vec::new(), lan)
        .checkpoint()
        .unwrap()
        .into_lan_save()
        .unwrap()
}

/// The resume screen hosting on loopback, over a New Match draft it gives
/// back when the resume ends.
fn resuming() -> (Wizard, NewMatchDraft) {
    let (wizard, draft) = resuming_on(saved_match(), "127.0.0.1:0");
    assert!(wizard.lan.is_some(), "{:?}", wizard.notice);
    (wizard, draft)
}

fn resuming_on(save: LanSave, bind: &str) -> (Wizard, NewMatchDraft) {
    let mut draft = NewMatchDraft {
        player_name: Some(name("Host")),
        seat_choice: 7,
        ..NewMatchDraft::default()
    };
    let wizard = Wizard::resume_on(save, &mut draft, bind);
    (wizard, draft)
}

fn join(wizard: &Wizard, player: &str, install: InstallId) -> Lobby {
    let address = wizard.lan.as_ref().expect("hosting").lobby.address();
    Lobby::Client(ClientLobby::new(
        address,
        &crate::build_identity().revision,
        name(player),
        install,
    ))
}

/// Polls the host and every joiner until `done`, failing after ten seconds.
fn settle(
    wizard: &mut Wizard,
    draft: &mut NewMatchDraft,
    clients: &mut [Lobby],
    done: impl Fn(&Wizard, &[Lobby]) -> bool,
) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        assert!(wizard.poll_lan(draft, Duration::ZERO).is_none());
        for client in clients.iter_mut() {
            assert!(client.poll(Duration::ZERO, vec2(1280.0, 800.0)).is_none());
        }
        if done(wizard, clients) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn member_at(wizard: &Wizard, seat: usize) -> Option<MemberId> {
    wizard.members()[seat]
}

/// Runs a started resume until the host and every joiner hold the match,
/// the host's first.
fn run_start(
    w: &mut Wizard,
    draft: &mut NewMatchDraft,
    clients: &mut [Lobby],
) -> Vec<(Game, Link)> {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    let mut host = None;
    let mut joined: Vec<Option<(Game, Link)>> = clients.iter().map(|_| None).collect();
    while host.is_none() || joined.iter().any(Option::is_none) {
        assert!(Instant::now() < deadline, "timed out");
        if host.is_none() {
            host = w.poll_lan(draft, Duration::ZERO);
        }
        for (client, game) in clients.iter_mut().zip(&mut joined) {
            if game.is_none() {
                *game = client.poll(Duration::ZERO, vec2(1280.0, 800.0));
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    std::iter::once(host).chain(joined).flatten().collect()
}

/// Plays the resumed match on, then pauses it and lets every machine catch
/// up to the host.
fn play_on(machines: &mut [(Game, Link)]) {
    let step = Duration::from_millis(50);
    let mut now = Duration::ZERO;
    let pump = |machines: &mut [(Game, Link)], now: Duration| {
        std::thread::sleep(std::time::Duration::from_millis(1));
        for (game, link) in machines.iter_mut() {
            assert_eq!(link.pump(game, now), None);
        }
    };
    for _ in 0..40 {
        now += step;
        pump(machines, now);
    }
    machines[0].0.clock.paused = true;
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while machines
        .iter()
        .any(|(game, _)| game.state.current_tick() != machines[0].0.state.current_tick())
    {
        assert!(Instant::now() < deadline, "timed out");
        now += step;
        pump(machines, now);
    }
}

#[test]
fn a_resume_reserves_each_players_seat_and_fixes_everything_else() {
    let (mut w, mut draft) = resuming();
    assert_eq!(draft.seat_choice, 0, "the host keeps its seat");
    assert_eq!(
        draft.seats.iter().map(|plan| plan.open).collect::<Vec<_>>(),
        [false, true, true, false],
        "only the players' seats wait"
    );
    let rows = w.ui_surface(&draft).1;
    assert!(rows[1].contains("(was Alex)"), "{rows:?}");
    assert!(rows[2].contains("(was Sam)"), "{rows:?}");

    // Teams and the bot stay as saved, and the host cannot leave its seat.
    let teams: Vec<usize> = draft.seats.iter().map(|plan| plan.team_choice).collect();
    let bot = draft.seats[3];
    for (row, cell) in [(3, Cell::Team), (3, Cell::Difficulty), (0, Cell::Seat)] {
        w.setup_sel = row;
        w.setup_cell = cell;
        drive(&mut w, &mut draft, Key::Enter);
    }
    assert_eq!(
        draft
            .seats
            .iter()
            .map(|plan| plan.team_choice)
            .collect::<Vec<_>>(),
        teams
    );
    assert_eq!(draft.seats[3], bot);
    assert_eq!(draft.seat_choice, 0);
    assert!(w.moving.is_none());

    // The switch cannot make it a local match.
    w.on_switch = true;
    drive(&mut w, &mut draft, Key::Enter);
    assert!(w.lan.is_some());
    w.on_switch = false;

    // Back leaves for Home and gives New Match its draft back.
    assert_eq!(drive(&mut w, &mut draft, Key::Escape), Out::Home);
    w.finish_resume(&mut draft);
    assert_eq!(draft.seat_choice, 7);
    assert!(draft.scenario.is_none());
}

#[test]
fn returning_players_find_their_seats_by_install_then_name_then_order() {
    let (mut w, mut draft) = resuming();
    // Sam's installation under another name still finds North East, though
    // South West comes first in display order.
    let mut clients = vec![join(&w, "Zed", SAM)];
    settle(&mut w, &mut draft, &mut clients, |w, c| {
        member_at(w, 1).is_some() && c[0].roster().len() == 4
    });
    let roster = clients[0].roster();
    assert_eq!(roster[1], "2. Zed (you)");
    assert_eq!(roster[2], "3. Was Alex", "{roster:?}");

    // Alex's name finds South West from another installation.
    clients.push(join(&w, "Alex", InstallId::new(99)));
    settle(&mut w, &mut draft, &mut clients, |w, _| {
        member_at(w, 2).is_some()
    });

    // With every reserved seat held, a newcomer is refused rather than
    // given the bot's seat.
    clients.push(join(&w, "Riley", InstallId::new(98)));
    settle(&mut w, &mut draft, &mut clients, |_, c| {
        c[2].failure().is_some()
    });
    assert!(member_at(&w, 3).is_none());

    // The host can still swap the two players.
    w.setup_sel = 1;
    w.setup_cell = Cell::Seat;
    drive(&mut w, &mut draft, Key::Enter);
    w.setup_sel = 2;
    drive(&mut w, &mut draft, Key::Enter);
    assert!(w.ui_surface(&draft).1[1].contains("(player Zed)"));
}

#[test]
fn a_shared_installation_falls_back_to_the_saved_names() {
    let (mut w, mut draft) = resuming();
    let mut clients = vec![join(&w, "Pat", ALEX)];
    settle(&mut w, &mut draft, &mut clients, |w, _| {
        member_at(w, 2).is_some()
    });
    // A second copy on Alex's machine shares its installation, so only its
    // name places it.
    clients.push(join(&w, "Sam", ALEX));
    settle(&mut w, &mut draft, &mut clients, |w, _| {
        member_at(w, 1).is_some()
    });
}

#[test]
fn a_resume_starts_every_machine_on_the_saved_world() {
    let (mut w, mut draft) = resuming();
    let mut clients = vec![join(&w, "Sam", SAM), join(&w, "Alex", ALEX)];
    settle(&mut w, &mut draft, &mut clients, |w, _| {
        member_at(w, 1).is_some() && member_at(w, 2).is_some()
    });
    w.setup_sel = 4;
    assert_eq!(drive(&mut w, &mut draft, Key::Enter), Out::Launch);
    assert!(w.hosting_players());
    w.start_resume(&draft, Duration::ZERO).unwrap();
    let mut machines = run_start(&mut w, &mut draft, &mut clients);
    let host = &machines[0].0;
    assert_eq!(host.state.current_tick(), 40);
    assert_eq!(host.net_role(), Some(NetRole::Host));
    for (joiner, _) in &machines[1..] {
        assert_eq!(joiner.state.current_tick(), 40);
        assert_eq!(joiner.state.hash(), host.state.hash());
    }
    assert_eq!(machines[1].0.presentation.human, PlayerId(1));
    assert_eq!(machines[2].0.presentation.human, PlayerId(2));
    assert_eq!(draft.seat_choice, 7, "New Match has its draft back");

    play_on(&mut machines);
    let host = &machines[0].0;
    assert!(host.state.current_tick() > 40, "the match plays on");
    for (joiner, _) in &machines[1..] {
        assert_eq!(joiner.state.hash(), host.state.hash());
    }
}

#[test]
fn a_missing_player_can_go_to_a_bot_and_the_host_can_resume_alone() {
    let (mut w, mut draft) = resuming();
    let mut clients = vec![join(&w, "Alex", ALEX)];
    settle(&mut w, &mut draft, &mut clients, |w, _| {
        member_at(w, 2).is_some()
    });
    // North East waits for Sam until the host hands it to a bot.
    w.setup_sel = 4;
    drive(&mut w, &mut draft, Key::Enter);
    assert!(
        w.notice
            .as_deref()
            .is_some_and(|notice| notice.contains("still open"))
    );
    w.setup_sel = 2;
    w.setup_cell = Cell::Difficulty;
    drive(&mut w, &mut draft, Key::Enter);
    assert!(!draft.seats[1].open);
    w.setup_sel = 4;
    assert_eq!(drive(&mut w, &mut draft, Key::Enter), Out::Launch);
    w.start_resume(&draft, Duration::ZERO).unwrap();
    let mut machines = run_start(&mut w, &mut draft, &mut clients);
    for (game, _) in &machines {
        assert!(game.scenario.players[1].bot, "a bot plays North East");
    }
    play_on(&mut machines);
    assert_eq!(machines[0].0.state.hash(), machines[1].0.state.hash());

    // With both players' seats given to bots, the host plays on alone.
    let (mut w, mut draft) = resuming();
    for row in [1, 2] {
        w.setup_sel = row;
        w.setup_cell = Cell::Difficulty;
        drive(&mut w, &mut draft, Key::Enter);
    }
    w.setup_sel = 4;
    assert_eq!(drive(&mut w, &mut draft, Key::Enter), Out::Launch);
    assert!(!w.hosting_players());
    let alone = w.resume_alone(&draft).unwrap();
    assert_eq!(alone.state.current_tick(), 40);
    assert_eq!(alone.net_role(), None);
    assert!(alone.scenario.players[1].bot && alone.scenario.players[2].bot);
}

#[test]
fn the_saved_players_keep_their_seats_while_the_lobby_cannot_open() {
    let (mut w, mut draft) = resuming_on(saved_match(), "not an address");
    assert!(w.lan.is_none());
    assert!(
        w.notice
            .as_deref()
            .is_some_and(|notice| notice.starts_with("Cannot host"))
    );
    let rows = w.ui_surface(&draft).1;
    assert!(rows[1].contains("(was Alex)"), "{rows:?}");
    w.setup_sel = 4;
    assert_eq!(drive(&mut w, &mut draft, Key::Enter), Out::Stay);
    assert_eq!(
        w.notice.as_deref(),
        Some("Players rejoin through the LAN lobby"),
        "nobody is handed to a bot behind the host's back"
    );
}

#[test]
fn leaving_a_resume_with_players_back_asks_first() {
    let (mut w, mut draft) = resuming();
    let mut clients = vec![join(&w, "Alex", ALEX)];
    settle(&mut w, &mut draft, &mut clients, |w, _| {
        member_at(w, 2).is_some()
    });
    assert_eq!(drive(&mut w, &mut draft, Key::Escape), Out::Stay);
    assert_eq!(w.mode_name(), "confirm_lobby");
    assert_eq!(
        drive(&mut w, &mut draft, Key::Enter),
        Out::Stay,
        "Cancel stays"
    );
    assert!(w.lan.is_some());
    drive(&mut w, &mut draft, Key::Escape);
    drive(&mut w, &mut draft, Key::Down);
    assert_eq!(drive(&mut w, &mut draft, Key::Enter), Out::Home);
}

#[test]
fn a_resume_without_a_player_who_was_out_still_saves_them() {
    let (mut w, mut draft) = resuming_on(saved_with(&[PlayerId(1)]), "127.0.0.1:0");
    assert!(
        !draft.seats[1].open,
        "Sam was out: nobody waits for that seat"
    );
    let mut clients = vec![join(&w, "Alex", InstallId::new(77))];
    settle(&mut w, &mut draft, &mut clients, |w, _| {
        member_at(w, 2).is_some()
    });
    w.setup_sel = 4;
    assert_eq!(drive(&mut w, &mut draft, Key::Enter), Out::Launch);
    w.start_resume(&draft, Duration::ZERO).unwrap();
    let machines = run_start(&mut w, &mut draft, &mut clients);
    let (host, link) = &machines[0];
    let saved =
        serde_json::to_value(link.capture_lan_save(host).unwrap().checkpoint().unwrap()).unwrap();
    assert_eq!(
        saved["lan"]["seats"],
        serde_json::json!([[1, SAM.to_hex()], [2, InstallId::new(77).to_hex()]]),
        "the next save still lists the player who was out"
    );
}
