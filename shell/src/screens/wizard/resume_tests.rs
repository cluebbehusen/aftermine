//! The setup screen resuming a saved LAN match, against real joiners over
//! loopback.

use super::*;
use crate::game::checkpoint::LanManifest;
use crate::netplay::{ClientLobby, Lobby};
use crate::tests::support::keystroke;
use aftermine_sim::scenario::BotConfig;
use macroquad::prelude::vec2;
use std::time::Instant;

const SAM: InstallId = InstallId::new(11);
const ALEX: InstallId = InstallId::new(12);

fn drive(w: &mut Wizard, draft: &mut NewMatchDraft, key: Key) -> Out {
    w.update(&keystroke(key), &mut vec2(0.0, 0.0), draft, &mut Vec::new())
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
    saved_pending(surrendered, Vec::new())
}

/// The same match, with `surrendered` seats out of it and `pending` the
/// orders its session had not sealed when it was saved.
fn saved_pending(surrendered: &[PlayerId], pending: Vec<aftermine_sim::PlayerCommand>) -> LanSave {
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
        game.stage(aftermine_sim::PlayerCommand {
            player: seat,
            command: aftermine_sim::Command::Surrender,
        });
    }
    game.advance_ticks(40);
    let lan = LanManifest::new(
        &scenario,
        PlayerId(0),
        &[(PlayerId(1), SAM), (PlayerId(2), ALEX)],
    );
    game.capture_save()
        .for_lan(pending, lan)
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
fn the_setup_hint_says_when_a_press_does_nothing_on_a_resume() {
    let (mut w, mut draft) = resuming();
    let order = seat_display_order(draft.scenario.as_deref().unwrap());
    let press_at = |w: &mut Wizard, draft: &NewMatchDraft, row: usize, cell: Cell| {
        w.cursor = SetupCursor::Seat { row, cell };
        w.cursor_press(draft, &w.open_flags(draft), &order)
    };
    for row in [0, 1, 3] {
        assert_eq!(
            press_at(&mut w, &draft, row, Cell::Seat),
            CursorPress::Nothing,
            "row {row}: the host, a missing player and a bot stay put"
        );
    }
    assert_eq!(
        press_at(&mut w, &draft, 1, Cell::Difficulty),
        CursorPress::BotForChair,
        "a missing player's chair can go to a bot"
    );
    w.cursor = SetupCursor::Switch {
        row: 0,
        cell: Cell::Seat,
    };
    assert_eq!(
        w.cursor_press(&draft, &w.open_flags(&draft), &order),
        CursorPress::Nothing,
        "the lobby stays open"
    );

    let mut clients = vec![join(&w, "Alex", ALEX)];
    settle(&mut w, &mut draft, &mut clients, |w, _| {
        member_at(w, 2).is_some()
    });
    assert_eq!(
        press_at(&mut w, &draft, 1, Cell::Seat),
        CursorPress::MovePlayer
    );
    assert_eq!(
        press_at(&mut w, &draft, 1, Cell::Difficulty),
        CursorPress::RemovePlayer
    );
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
        w.cursor = SetupCursor::Seat { row, cell };
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
    let before = w.cursor;
    w.cursor = SetupCursor::Switch {
        row: 0,
        cell: Cell::Seat,
    };
    drive(&mut w, &mut draft, Key::Enter);
    assert!(w.lan.is_some());
    w.cursor = before;

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
    w.cursor = SetupCursor::Seat {
        row: 1,
        cell: Cell::Seat,
    };
    drive(&mut w, &mut draft, Key::Enter);
    w.cursor = SetupCursor::Seat {
        row: 2,
        cell: w.cursor.cell(),
    };
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
    w.cursor = SetupCursor::Start {
        cell: w.cursor.cell(),
    };
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
    w.cursor = SetupCursor::Start {
        cell: w.cursor.cell(),
    };
    drive(&mut w, &mut draft, Key::Enter);
    assert!(
        w.notice
            .is_some_and(|notice| notice.to_string().contains("still open"))
    );
    w.cursor = SetupCursor::Seat {
        row: 2,
        cell: Cell::Difficulty,
    };
    drive(&mut w, &mut draft, Key::Enter);
    assert!(!draft.seats[1].open);
    w.cursor = SetupCursor::Start {
        cell: w.cursor.cell(),
    };
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
        w.cursor = SetupCursor::Seat {
            row,
            cell: Cell::Difficulty,
        };
        drive(&mut w, &mut draft, Key::Enter);
    }
    w.cursor = SetupCursor::Start {
        cell: w.cursor.cell(),
    };
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
            .is_some_and(|notice| notice.to_string().starts_with("Cannot host"))
    );
    let rows = w.ui_surface(&draft).1;
    assert!(rows[1].contains("(was Alex)"), "{rows:?}");
    w.cursor = SetupCursor::Start {
        cell: w.cursor.cell(),
    };
    assert_eq!(drive(&mut w, &mut draft, Key::Enter), Out::Stay);
    assert_eq!(
        w.notice.map(|notice| notice.to_string()).as_deref(),
        Some("Switch to LAN to resume this match"),
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
    w.cursor = SetupCursor::Start {
        cell: w.cursor.cell(),
    };
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

#[test]
fn a_player_dropped_before_the_save_is_out_once_the_resume_plays() {
    // Sam was dropped while the match waited, so his surrender had yet to
    // seal when it was saved.
    let surrender = aftermine_sim::PlayerCommand {
        player: PlayerId(1),
        command: aftermine_sim::Command::Surrender,
    };
    let save = saved_pending(&[], vec![surrender]);
    let saved_hash = save.host_game(&[]).unwrap().0.state.hash();
    let (mut w, mut draft) = resuming_on(save, "127.0.0.1:0");
    assert!(!draft.seats[1].open, "nobody waits for Sam's seat");
    let mut clients = vec![join(&w, "Alex", ALEX)];
    settle(&mut w, &mut draft, &mut clients, |w, _| {
        member_at(w, 2).is_some()
    });
    w.cursor = SetupCursor::Start {
        cell: w.cursor.cell(),
    };
    assert_eq!(drive(&mut w, &mut draft, Key::Enter), Out::Launch);
    w.start_resume(&draft, Duration::ZERO).unwrap();
    let mut machines = run_start(&mut w, &mut draft, &mut clients);
    for (game, _) in &machines {
        assert_eq!(
            game.state.hash(),
            saved_hash,
            "every machine starts on the saved world"
        );
    }

    play_on(&mut machines);
    for (game, _) in &machines {
        assert!(!game.state.accepts_commands(PlayerId(1)), "Sam is out");
        assert_eq!(game.state.hash(), machines[0].0.state.hash());
    }
}

fn sounds_of(w: &mut Wizard, draft: &mut NewMatchDraft, key: Key) -> Vec<SoundKind> {
    let mut sounds = Vec::new();
    w.update(&keystroke(key), &mut vec2(0.0, 0.0), draft, &mut sounds)
        .expect("update");
    sounds.into_iter().map(|(sound, _)| sound).collect()
}

#[test]
fn a_picked_up_player_cannot_be_dropped_on_a_fixed_seat() {
    // Rows run in display order: the host, Alex, Sam, then the bot.
    for (fixed, what) in [(3, "the bot's seat"), (0, "the host's seat")] {
        let (mut w, mut draft) = resuming();
        let mut clients = vec![join(&w, "Sam", SAM), join(&w, "Alex", ALEX)];
        settle(&mut w, &mut draft, &mut clients, |w, _| {
            member_at(w, 1).is_some() && member_at(w, 2).is_some()
        });
        let sam = member_at(&w, 1);
        w.cursor = SetupCursor::Seat {
            row: 2,
            cell: Cell::Seat,
        };
        drive(&mut w, &mut draft, Key::Enter);
        assert_eq!(w.moving, Some(1), "Sam is picked up");

        w.cursor = SetupCursor::Seat {
            row: fixed,

            cell: w.cursor.cell(),
        };
        assert!(
            sounds_of(&mut w, &mut draft, Key::Enter).contains(&SoundKind::Denied),
            "{what} refuses Sam"
        );
        assert_eq!(w.moving, Some(1), "Sam stays picked up over {what}");
        assert_eq!(member_at(&w, 1), sam);

        // Put back on his own seat, he starts there.
        w.cursor = SetupCursor::Seat {
            row: 2,
            cell: w.cursor.cell(),
        };
        drive(&mut w, &mut draft, Key::Enter);
        assert!(w.moving.is_none());
        w.cursor = SetupCursor::Start {
            cell: w.cursor.cell(),
        };
        assert_eq!(drive(&mut w, &mut draft, Key::Enter), Out::Launch);
        w.start_resume(&draft, Duration::ZERO).unwrap();
        let machines = run_start(&mut w, &mut draft, &mut clients);
        assert_eq!(machines[1].0.presentation.human, PlayerId(1));
    }
}
