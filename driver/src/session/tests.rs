use super::*;
use aftermine_sim::scenario::BotDifficulty;
use aftermine_sim::scenario::BotStance;
use aftermine_sim::{BuildingKind, Command, Event, PlayerId, UnitKind};
use chassis::grid::TilePos;

#[test]
fn replay_resume_keeps_pending_controller_events_and_recording() {
    let mut scenario = Scenario::skirmish();
    for seat in &mut scenario.players {
        seat.bot = true;
        seat.bot_config = Some(aftermine_sim::scenario::BotConfig::default());
    }
    scenario.players[0].bot_config = Some(aftermine_sim::scenario::BotConfig::new(
        BotDifficulty::default(),
        BotStance::default(),
        0,
    ));
    let mut original = Session::new(scenario).unwrap();
    for _ in 0..121 {
        original.step();
    }
    original
        .handle(Request::SendCommand {
            player: PlayerId(0),
            command: Command::Stop { units: vec![] },
        })
        .unwrap();
    original.step();
    let controllers = |session: &Session| {
        let checkpoint = aftermine_kit::checkpoint::SessionCheckpoint::capture(
            &session.scenario,
            &session.state,
            &session.bots,
            &session.pending,
            None,
        )
        .unwrap();
        serde_json::to_value(checkpoint).unwrap()["bots"].clone()
    };
    let pending = controllers(&original);
    assert_eq!(
        pending[0]["events"],
        serde_json::json!([{"event": "command_rejected", "reason": "no_valid_units"}])
    );
    let mut snapshot = original.recorder.clone();
    snapshot.meta.ticks = Some(original.state.current_tick());
    let mut resumed = Session::resume(snapshot).unwrap();
    assert_eq!(controllers(&resumed), pending);
    for _ in 0..240 {
        assert_eq!(original.step(), resumed.step());
        assert_eq!(original.state.hash(), resumed.state.hash());
    }
    assert_eq!(
        serde_json::to_vec(&original.recorder.commands).unwrap(),
        serde_json::to_vec(&resumed.recorder.commands).unwrap()
    );
    for seat in [PlayerId(0), PlayerId(1)] {
        assert!(
            original
                .recorder
                .commands
                .iter()
                .any(|timed| timed.command.player == seat)
        );
    }
}

fn human_scenario(name: &str) -> Scenario {
    let mut scenario = Scenario::skirmish();
    scenario.name = name.to_string();
    for player in &mut scenario.players {
        player.bot = false;
        player.bot_config = None;
    }
    scenario
}

fn scratch(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "aftermine-driver-session-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn failed_replacements_leave_state_and_staged_commands_intact() {
    let dir = scratch("failed-replace");
    let mut session = Session::new(human_scenario("original")).unwrap();
    session.handle(Request::AdvanceTicks { ticks: 2 }).unwrap();
    session
        .handle(Request::SendCommand {
            player: PlayerId(0),
            command: Command::Stop { units: Vec::new() },
        })
        .unwrap();
    let before_tick = session.state().current_tick();
    let before_hash = session.state().hash();

    let missing = dir.join("missing-scenario.json");
    assert!(
        session
            .handle(Request::LoadScenario {
                path: missing.to_string_lossy().into_owned(),
            })
            .is_err()
    );
    let malformed = dir.join("malformed-replay.json");
    std::fs::write(&malformed, b"not a replay").unwrap();
    assert!(
        session
            .handle(Request::LoadReplay {
                path: malformed.to_string_lossy().into_owned(),
            })
            .is_err()
    );
    assert_eq!(session.state().current_tick(), before_tick);
    assert_eq!(session.state().hash(), before_hash);

    session.handle(Request::PresentTicks { ticks: 1 }).unwrap();
    assert_eq!(session.status().recorded_commands, 1);
    assert_eq!(session.state().current_tick(), before_tick + 1);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn successful_scenario_replacement_resets_the_entire_session() {
    let dir = scratch("successful-replace");
    let replacement_path = dir.join("replacement.json");
    let replacement = human_scenario("replacement");
    std::fs::write(
        &replacement_path,
        serde_json::to_vec_pretty(&replacement).unwrap(),
    )
    .unwrap();

    let mut session = Session::new(human_scenario("original")).unwrap();
    session.handle(Request::AdvanceTicks { ticks: 4 }).unwrap();
    session
        .handle(Request::SendCommand {
            player: PlayerId(0),
            command: Command::Stop { units: Vec::new() },
        })
        .unwrap();

    assert!(matches!(
        session
            .handle(Request::LoadScenario {
                path: replacement_path.to_string_lossy().into_owned(),
            })
            .unwrap(),
        Reply::Ok
    ));
    let status = session.status();
    assert_eq!(status.scenario, "replacement");
    assert_eq!(status.tick, 0);
    assert_eq!(status.recorded_commands, 0);
    session.handle(Request::PresentTicks { ticks: 1 }).unwrap();
    assert_eq!(
        session.status().recorded_commands,
        0,
        "old pending work was discarded"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn replay_without_duration_resumes_through_its_last_command() {
    let mut replay = GameReplay::new(SIM_VERSION, "test", human_scenario("legacy"));
    replay.record(
        3,
        PlayerCommand {
            player: PlayerId(0),
            command: Command::Stop { units: Vec::new() },
        },
    );
    assert_eq!(replay.meta.ticks, None);
    let expected = runner::run_replay(&replay, None).unwrap();

    let session = Session::resume(replay).unwrap();
    assert_eq!(session.state().current_tick(), 4);
    assert_eq!(session.state().hash(), expected.hash());
    assert_eq!(session.status().recorded_commands, 1);
}

#[test]
fn commands_from_unknown_players_are_not_staged() {
    let mut session = Session::new(human_scenario("players")).unwrap();
    let error = session
        .handle(Request::SendCommand {
            player: PlayerId(99),
            command: Command::Stop { units: Vec::new() },
        })
        .unwrap_err();
    assert!(error.contains("no such player"));
    session.handle(Request::PresentTicks { ticks: 1 }).unwrap();
    assert_eq!(session.status().recorded_commands, 0);
}

/// Skirmish as a playtest for seat zero against a Prime bot, replaying to
/// `dir`.
fn playtest_session(dir: &std::path::Path, tick_limit: Option<u64>) -> Session {
    paced_session(dir, tick_limit, None, None)
}

/// [`playtest_session`] held to a pace.
fn paced_session(
    dir: &std::path::Path,
    tick_limit: Option<u64>,
    min_step: Option<u64>,
    orders_per_minute: Option<u32>,
) -> Session {
    let mut scenario = Scenario::skirmish();
    aftermine_kit::controller::seat_bots_around(
        &mut scenario,
        &[PlayerId(0)],
        BotDifficulty::Prime,
        BotStance::Balanced,
        7,
    )
    .unwrap();
    Session::playtest(
        scenario,
        Playtest {
            seats: vec![PlayerId(0)],
            tick_limit,
            replay_out: dir.join("replays/playtest.json"),
            min_step,
            orders_per_minute: orders_per_minute.map(|cap| cap.try_into().unwrap()),
        },
    )
    .unwrap()
}

#[test]
fn a_playtest_answers_only_what_its_seat_could_know_and_do() {
    let dir = scratch("playtest-refusals");
    let mut session = playtest_session(&dir, None);
    let path = dir.join("anything").to_string_lossy().into_owned();
    let refused = [
        Request::QueryState {
            filter: aftermine_protocol::StateFilter::default(),
        },
        Request::StateHash,
        Request::Screenshot {
            path: Some(path.clone()),
        },
        Request::PresentTicks { ticks: 1 },
        Request::LoadScenario { path: path.clone() },
        Request::LoadReplay { path: path.clone() },
        Request::SaveReplay { path },
        Request::QueryFogView {
            player: PlayerId(1),
        },
        Request::SendCommand {
            player: PlayerId(1),
            command: Command::Stop { units: Vec::new() },
        },
    ];
    for request in refused {
        let label = format!("{request:?}");
        assert!(session.handle(request).is_err(), "{label} was answered");
    }
    assert!(
        !dir.join("anything").exists(),
        "a refused request touched a file"
    );
    assert_eq!(session.state().current_tick(), 0);

    assert!(matches!(
        session.handle(Request::QueryFogView {
            player: PlayerId(0)
        }),
        Ok(Reply::Fog(_))
    ));
    let own = session
        .state()
        .units()
        .iter()
        .find(|unit| unit.player == PlayerId(0))
        .map(|unit| unit.id)
        .expect("the seat starts with units");
    session
        .handle(Request::SendCommand {
            player: PlayerId(0),
            command: Command::Stop { units: vec![own] },
        })
        .unwrap();
    session.handle(Request::AdvanceTicks { ticks: 60 }).unwrap();
    let Ok(Reply::Status(status)) = session.handle(Request::Status) else {
        panic!("a playtest reports its status");
    };
    assert!(
        session
            .recorder
            .commands
            .iter()
            .any(|timed| timed.command.player == PlayerId(1)),
        "the Prime bot played"
    );
    assert_eq!(
        status.recorded_commands, 1,
        "the status counts the seat's own commands, never the bot's"
    );
}

#[test]
fn a_playtest_stops_at_its_tick_limit_and_leaves_a_replay_of_every_advance() {
    let dir = scratch("playtest-limit");
    let mut session = playtest_session(&dir, Some(50));
    let Ok(Reply::Played(first)) = session.handle(Request::AdvanceTicks { ticks: 30 }) else {
        panic!("a playtest advances");
    };
    assert_eq!((first.ticks, first.tick), (30, 30));
    let Ok(Reply::Played(capped)) = session.handle(Request::AdvanceTicks { ticks: 30 }) else {
        panic!("a playtest advances to its limit");
    };
    assert_eq!((capped.ticks, capped.tick), (20, 50));
    assert!(session.handle(Request::AdvanceTicks { ticks: 1 }).is_err());
    assert_eq!(session.state().current_tick(), 50);

    let out = dir.join("replays/playtest.json");
    assert!(!out.with_extension("partial").exists());
    let replay = aftermine_kit::load_replay(&out).expect("the playtest replay loads");
    assert_eq!(replay.meta.ticks, Some(50));
    assert_eq!(
        replay.setup.players[1]
            .bot_config
            .map(|c| c.personality_seed),
        Some(8)
    );
    let resumed = Session::resume(replay).expect("the replay resumes");
    assert_eq!(resumed.state().hash(), session.state().hash());
}

#[test]
fn a_playtest_seat_must_be_one_no_bot_plays() {
    let dir = scratch("playtest-bot-seat");
    let scenario = Scenario::skirmish();
    assert!(scenario.players[1].bot);
    let playtest = |seat| Playtest {
        seats: vec![seat],
        tick_limit: None,
        replay_out: dir.join("replay.json"),
        min_step: None,
        orders_per_minute: None,
    };
    assert!(Session::playtest(scenario.clone(), playtest(PlayerId(1))).is_err());
    assert!(Session::playtest(scenario, playtest(PlayerId(7))).is_err());
}

#[test]
fn a_playtest_refuses_an_order_the_game_would_reject_with_its_reason() {
    let dir = scratch("playtest-rejections");
    let mut session = playtest_session(&dir, None);
    let foundry = session
        .state()
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0))
        .map(|building| building.id)
        .expect("the seat starts with a Foundry");
    let train = || Request::SendCommand {
        player: PlayerId(0),
        command: Command::Train {
            building: foundry,
            kind: UnitKind::Harvester,
        },
    };
    let bank = session.state().players()[0].scrap;
    let mut taken = 0;
    let refusal = loop {
        match session.handle(train()) {
            Ok(Reply::Ok) => taken += 1,
            Ok(other) => panic!("an order answers ok or a refusal, not {other:?}"),
            Err(refusal) => break refusal,
        }
        assert!(
            taken < 64,
            "the staged orders never ran out of scrap or room"
        );
    };
    assert!(
        refusal.contains("not enough scrap") || refusal.contains("queue full"),
        "{refusal}"
    );
    let refusal = session
        .handle(Request::SendCommand {
            player: PlayerId(0),
            command: Command::Stop { units: Vec::new() },
        })
        .expect_err("an order for no units is rejected");
    assert!(refusal.contains("no valid units"), "{refusal}");
    assert_eq!(session.pending.len(), taken);

    session.handle(Request::AdvanceTicks { ticks: 1 }).unwrap();
    let price = UnitKind::Harvester.stats().cost;
    assert_eq!(
        session.state().players()[0].scrap,
        bank - u32::try_from(taken).unwrap() * price,
        "every staged order was taken"
    );
}

/// The seat's first unit of `kind`.
fn own(session: &Session, kind: UnitKind) -> aftermine_sim::UnitId {
    session
        .state()
        .units()
        .iter()
        .find(|unit| unit.player == PlayerId(0) && unit.kind == kind)
        .map(|unit| unit.id)
        .expect("the seat starts with one")
}

#[test]
fn a_playtest_advance_stops_on_the_tick_the_match_ends_and_says_so() {
    let dir = scratch("playtest-result");
    let mut session = playtest_session(&dir, None);
    session
        .handle(Request::SendCommand {
            player: PlayerId(0),
            command: Command::Surrender,
        })
        .unwrap();
    let Ok(Reply::Played(played)) = session.handle(Request::AdvanceTicks { ticks: 200 }) else {
        panic!("a playtest advances");
    };
    assert_eq!(
        played.ticks, 1,
        "the advance stops on the tick the match ends"
    );
    assert!(played.result.is_some());
    assert!(
        played
            .log
            .iter()
            .any(|seen| matches!(seen.event, Event::GameOver { .. }) && seen.tick == 0),
        "{played:?}"
    );
    assert!(super::super::digest::played(&played).contains("you lost"));
    let error = session
        .handle(Request::AdvanceTicks { ticks: 1 })
        .unwrap_err();
    assert!(error.contains("the match is over"), "{error}");
}

#[test]
fn a_paced_playtest_holds_its_player_to_the_pace() {
    let dir = scratch("playtest-pace");
    let mut session = paced_session(&dir, Some(130), Some(100), Some(2));
    let error = session
        .handle(Request::AdvanceTicks { ticks: 40 })
        .unwrap_err();
    assert!(error.contains("at least 100 ticks"), "{error}");
    session
        .handle(Request::AdvanceTicks { ticks: 100 })
        .unwrap();
    // Short of the limit, the last step may be as short as what is left.
    session.handle(Request::AdvanceTicks { ticks: 30 }).unwrap();

    let mut session = paced_session(&dir, None, None, Some(2));
    let stop = |session: &Session| Request::SendCommand {
        player: PlayerId(0),
        command: Command::Stop {
            units: vec![own(session, UnitKind::Harvester)],
        },
    };
    session.handle(stop(&session)).unwrap();
    session.handle(stop(&session)).unwrap();
    let error = session.handle(stop(&session)).unwrap_err();
    assert!(error.contains("2 orders in any minute"), "{error}");
    assert!(error.contains("from tick 1200"), "{error}");

    // A few trains of one kind at one structure at once are one order.
    let mut session = paced_session(&dir, None, None, Some(2));
    let foundry = session
        .state()
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0))
        .map(|building| building.id)
        .expect("the seat starts with a Foundry");
    let train = Request::SendCommand {
        player: PlayerId(0),
        command: Command::Train {
            building: foundry,
            kind: UnitKind::Harvester,
        },
    };
    session.handle(train.clone()).unwrap();
    session.handle(train.clone()).unwrap();
    session.handle(stop(&session)).unwrap();
    let error = session.handle(train).unwrap_err();
    assert!(error.contains("2 orders in any minute"), "{error}");
}

#[test]
fn a_refused_order_says_what_the_seat_can_tell_of_why() {
    let dir = scratch("playtest-why");
    let mut session = playtest_session(&dir, None);
    let harvesters: Vec<aftermine_sim::UnitId> = session
        .state()
        .units()
        .iter()
        .filter(|unit| unit.player == PlayerId(0) && unit.kind == UnitKind::Harvester)
        .map(|unit| unit.id)
        .collect();
    let foundry = session
        .state()
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0))
        .map(|building| building.anchor)
        .expect("the seat starts with a Foundry");
    let build_with = |harvester, kind, anchor, defer| Request::SendCommand {
        player: PlayerId(0),
        command: Command::Build {
            units: vec![harvester],
            kind,
            anchor,
            queue: false,
            defer,
        },
    };
    let build = |kind, anchor, defer| build_with(harvesters[0], kind, anchor, defer);
    let unseen = TilePos::new(
        session.state().map().width() - 1 - foundry.x,
        session.state().map().height() - 1 - foundry.y,
    );
    let error = session
        .handle(build(BuildingKind::Turret, unseen, false))
        .unwrap_err();
    assert!(error.contains("--defer"), "{error}");
    let error = session
        .handle(build(BuildingKind::Extractor, foundry.offset(-3, 0), false))
        .unwrap_err();
    assert!(error.contains("only on an Extractor frame"), "{error}");

    let scrap = session.state().player(PlayerId(0)).scrap;
    let fabricator = BuildingKind::Fabricator.base_stats().construction.cost;
    assert!(
        scrap >= fabricator && scrap < 2 * fabricator,
        "premise: {scrap}"
    );
    let (width, _) = BuildingKind::Fabricator.size();
    let open: Vec<TilePos> = (-12..12)
        .flat_map(|dx| (-12..12).map(move |dy| foundry.offset(dx, dy)))
        .filter(|&anchor| {
            session
                .state()
                .place_refusal(PlayerId(0), BuildingKind::Fabricator, anchor)
                .is_none()
        })
        .collect();
    let first = *open.first().expect("premise: room near the Foundry");
    let second = *open
        .iter()
        .find(|anchor| anchor.y == first.y && anchor.x >= first.x + width)
        .expect("premise: room for two");
    session
        .handle(build(BuildingKind::Fabricator, first, false))
        .unwrap();
    let error = session
        .handle(build_with(
            harvesters[1],
            BuildingKind::Fabricator,
            second,
            false,
        ))
        .unwrap_err();
    assert!(
        error.contains("the orders you already sent this step leave"),
        "{error}"
    );
}

#[test]
fn an_advance_reports_the_seat_s_new_units() {
    let dir = scratch("playtest-trained");
    let mut session = playtest_session(&dir, None);
    let foundry = session
        .state()
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0))
        .map(|building| building.id)
        .expect("the seat starts with a Foundry");
    session
        .handle(Request::SendCommand {
            player: PlayerId(0),
            command: Command::Train {
                building: foundry,
                kind: UnitKind::Harvester,
            },
        })
        .unwrap();
    let Ok(Reply::Played(played)) = session.handle(Request::AdvanceTicks { ticks: 300 }) else {
        panic!("a playtest advances");
    };
    assert!(
        played.log.iter().any(|seen| matches!(
            seen.event,
            Event::UnitTrained {
                kind: UnitKind::Harvester,
                ..
            }
        )),
        "{played:?}"
    );
    assert!(super::super::digest::played(&played).contains("your harvester"));
}

/// Skirmish with both seats played by clients, replaying to `dir`.
fn two_seat_session(dir: &std::path::Path) -> Session {
    let mut scenario = Scenario::skirmish();
    aftermine_kit::controller::seat_bots_around(
        &mut scenario,
        &[PlayerId(0), PlayerId(1)],
        BotDifficulty::Prime,
        BotStance::Balanced,
        7,
    )
    .unwrap();
    Session::playtest(
        scenario,
        Playtest {
            seats: vec![PlayerId(0), PlayerId(1)],
            tick_limit: None,
            replay_out: dir.join("replays/match.json"),
            min_step: None,
            orders_per_minute: None,
        },
    )
    .unwrap()
}

#[test]
fn players_who_share_a_match_fill_every_seat() {
    let dir = scratch("playtest-three-seats");
    let mut scenario = Scenario::skirmish();
    let third = scenario.players[1].clone();
    scenario.players.push(third);
    aftermine_kit::controller::seat_bots_around(
        &mut scenario,
        &[PlayerId(0), PlayerId(1)],
        BotDifficulty::Prime,
        BotStance::Balanced,
        7,
    )
    .unwrap();
    let error = Session::playtest(
        scenario,
        Playtest {
            seats: vec![PlayerId(0), PlayerId(1)],
            tick_limit: None,
            replay_out: dir.join("replays/match.json"),
            min_step: None,
            orders_per_minute: None,
        },
    )
    .err()
    .expect("a bot would sit beside the players");
    assert!(error.to_string().contains("fill every seat"), "{error}");
}

#[test]
fn two_players_move_the_clock_together_by_the_shorter_step() {
    let dir = scratch("playtest-two-seats");
    let mut session = two_seat_session(&dir);
    let error = session
        .handle_from(Some(PlayerId(0)), Request::AdvanceTicks { ticks: 100 })
        .unwrap_err();
    assert!(error.contains("waiting for the other player"), "{error}");
    assert_eq!(
        session.state().current_tick(),
        0,
        "one seat alone moves nothing"
    );
    let Reply::Played(view) = session
        .handle_from(Some(PlayerId(1)), Request::AdvanceTicks { ticks: 60 })
        .unwrap()
    else {
        panic!("the second seat's advance runs the step")
    };
    assert_eq!((view.player, view.ticks, view.tick), (PlayerId(1), 60, 60));
    let waited = session
        .take_unread(PlayerId(0))
        .expect("seat 0's report is kept");
    assert_eq!((waited.player, waited.ticks), (PlayerId(0), 60));
    assert!(session.take_unread(PlayerId(0)).is_none(), "collected once");
}

#[test]
fn each_player_sees_and_commands_only_its_own_seat() {
    let dir = scratch("playtest-two-seats-refusals");
    let mut session = two_seat_session(&dir);
    let harvester = |session: &Session, seat: u8| {
        session
            .state()
            .units()
            .iter()
            .find(|unit| unit.player == PlayerId(seat) && unit.kind == UnitKind::Harvester)
            .unwrap()
            .id
    };
    let stop = |player: u8, unit| Request::SendCommand {
        player: PlayerId(player),
        command: Command::Stop { units: vec![unit] },
    };
    let theirs = harvester(&session, 1);
    let error = session
        .handle_from(Some(PlayerId(0)), stop(1, theirs))
        .unwrap_err();
    assert!(error.contains("seat 1 is not yours"), "{error}");
    let error = session
        .handle_from(
            Some(PlayerId(0)),
            Request::QueryFogView {
                player: PlayerId(1),
            },
        )
        .unwrap_err();
    assert!(error.contains("seat 1 is not yours"), "{error}");
    let mine = harvester(&session, 1);
    session
        .handle_from(Some(PlayerId(1)), stop(1, mine))
        .unwrap();
    session
        .handle_from(Some(PlayerId(0)), Request::AdvanceTicks { ticks: 1 })
        .unwrap_err();
    session
        .handle_from(Some(PlayerId(1)), Request::AdvanceTicks { ticks: 1 })
        .unwrap();
    let recorded = |session: &mut Session, seat: u8| {
        let Reply::Status(status) = session
            .handle_from(Some(PlayerId(seat)), Request::Status)
            .unwrap()
        else {
            panic!("a status")
        };
        status.recorded_commands
    };
    assert_eq!(recorded(&mut session, 1), 1);
    assert_eq!(
        recorded(&mut session, 0),
        0,
        "a seat counts only its own orders"
    );
}

#[test]
fn two_players_on_their_own_ports_get_their_reports_when_both_advance() {
    let dir = scratch("playtest-two-seats-server");
    let session = two_seat_session(&dir);
    let bind = || std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let (first, second) = (bind(), bind());
    let addresses = [first.local_addr().unwrap(), second.local_addr().unwrap()];
    std::thread::spawn(move || {
        serve_seats(
            vec![(PlayerId(0), first), (PlayerId(1), second)],
            Limits::default(),
            session,
        );
    });
    let advance = |address: std::net::SocketAddr, ticks: u64| {
        std::thread::spawn(move || {
            crate::client::Client::connect(&address.to_string())
                .unwrap()
                .call(Request::AdvanceTicks { ticks })
        })
    };
    let waiting = advance(addresses[0], 120);
    std::thread::sleep(Duration::from_millis(200));
    let second = advance(addresses[1], 80).join().unwrap().unwrap();
    let first = waiting.join().unwrap().unwrap();
    let ticks = |reply: Reply| match reply {
        Reply::Played(view) => (view.player, view.ticks),
        other => panic!("{other:?}"),
    };
    assert_eq!(ticks(first), (PlayerId(0), 80));
    assert_eq!(ticks(second), (PlayerId(1), 80));
}
