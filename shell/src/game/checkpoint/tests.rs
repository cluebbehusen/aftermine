use super::*;
use crate::numeric::Fit;
use oxide_sim::scenario::BotConfig;

#[test]
fn a_save_capture_leaves_the_live_world_free_to_tick() {
    let mut game = Game::with_viewport(Scenario::skirmish(), vec2(1280.0, 720.0)).unwrap();
    game.advance_ticks(5);
    let capture = game.capture_save();
    game.do_tick();
    let saved = capture.checkpoint().unwrap().restore().unwrap().install();
    assert_eq!(saved.state.current_tick(), 5);
    assert_eq!(game.state.current_tick(), 6);
}

#[test]
fn checkpoint_preserves_pending_input_memory_and_statistics() {
    let mut scenario = Scenario::skirmish();
    scenario.players[1].bot = true;
    scenario.players[1].bot_config = Some(oxide_sim::scenario::BotConfig::default());
    let mut original = Game::with_viewport(scenario, vec2(1280.0, 720.0)).unwrap();
    original.advance_ticks(121);
    original.issue(Command::Stop {
        units: vec![original.state.units()[0].id],
    });
    let before = original.state.hash();
    let bytes = serde_json::to_vec(&original).unwrap();
    assert_eq!(before, original.state.hash());
    let mut restored: Game = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(before, restored.state.hash());
    assert_eq!(*original.pending, *restored.pending);
    assert!(restored.clock.paused);
    let start = restored.state.current_tick();
    assert_eq!(restored.recorder.start_tick(), start);
    assert!(restored.recorder.commands.is_empty());
    for _ in 0..240 {
        assert_eq!(original.do_tick().events, restored.do_tick().events);
        assert_eq!(original.state.hash(), restored.state.hash());
        assert_eq!(
            serde_json::to_vec(
                &original
                    .recorder
                    .commands
                    .iter()
                    .filter(|c| c.tick >= start)
                    .collect::<Vec<_>>()
            )
            .unwrap(),
            serde_json::to_vec(&restored.recorder.commands).unwrap()
        );
        assert_eq!(
            original.live_stats.snapshot(&original.state),
            restored.live_stats.snapshot(&restored.state)
        );
    }
    assert_eq!(
        original.presentation.boundary_fog,
        restored.presentation.boundary_fog
    );
    original.issue(Command::Surrender);
    original.do_tick();
    assert!(original.state.result().is_some());
    let finished: Game = serde_json::from_slice(&serde_json::to_vec(&original).unwrap()).unwrap();
    assert_eq!(original.state.hash(), finished.state.hash());
    assert_eq!(original.end_stats, finished.end_stats);
}

#[test]
fn an_opponent_seat_continues_through_save_and_replay_resume() {
    let mut scenario = Scenario::skirmish();
    scenario.players[1].bot = true;
    scenario.players[1].bot_config = Some(oxide_sim::scenario::BotConfig::new(
        oxide_sim::scenario::BotDifficulty::Standard,
        oxide_sim::scenario::BotStance::Balanced,
        17,
    ));
    let mut original = Game::with_viewport(scenario, vec2(1280.0, 720.0)).unwrap();
    assert_eq!(original.bots.len(), 1);
    original.advance_ticks(121);
    original.stage(oxide_sim::PlayerCommand {
        player: PlayerId(1),
        command: Command::Stop { units: Vec::new() },
    });
    original.do_tick();
    let mut replay = original.recorder.clone();
    replay.meta.ticks = Some(original.state.current_tick());
    let controllers = |game: &Game| {
        game.bots
            .iter()
            .map(|bot| serde_json::to_value(bot.checkpoint()).unwrap())
            .collect::<Vec<_>>()
    };
    let pending = controllers(&original);
    assert_eq!(
        pending[0]["events"],
        serde_json::json!([{"event": "command_rejected", "reason": "no_valid_units"}])
    );

    let saved: Game = serde_json::from_slice(&serde_json::to_vec(&original).unwrap()).unwrap();
    let resumed = Game::from_replay(replay).unwrap();
    let mut continuations = [saved, resumed];
    for game in &continuations {
        assert_eq!(controllers(game), pending);
    }
    for _ in 0..240 {
        let events = original.do_tick().events;
        for game in &mut continuations {
            assert_eq!(game.do_tick().events, events);
        }
    }
    for game in &continuations {
        assert_eq!(game.state.hash(), original.state.hash());
        assert_eq!(controllers(game), controllers(&original));
    }
    assert!(
        original
            .recorder
            .commands
            .iter()
            .any(|timed| timed.command.player == PlayerId(1)),
        "the opponent seat must actually play"
    );
}

#[test]
fn checkpoint_rejects_invalid_shell_metadata_before_installation() {
    let game = Game::with_viewport(Scenario::skirmish(), vec2(1280.0, 720.0)).unwrap();
    let original = serde_json::to_value(&game).unwrap();
    let mut bad = original.clone();
    bad["human"] = serde_json::json!(255);
    assert!(serde_json::from_value::<Game>(bad).is_err());
    let mut bad = original;
    bad["boundary_fog"]["visible"] = serde_json::json!([{"x":-1,"y":-1}]);
    assert!(serde_json::from_value::<Game>(bad).is_err());
    assert_eq!(game.state.current_tick(), 0);
    assert!(game.pending.is_empty());
}

#[test]
fn checkpoint_requires_the_scenarios_default_local_seat() {
    for human in [0, 1] {
        let mut scenario = Scenario::skirmish();
        for (seat, player) in scenario.players.iter_mut().enumerate() {
            player.bot = seat != human;
            player.bot_config = player.bot.then_some(BotConfig::default());
        }
        let game = Game::with_viewport(scenario, vec2(1280.0, 720.0)).unwrap();
        let original = serde_json::to_value(&game).unwrap();
        let restored: Game = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(restored.presentation.human, PlayerId(human.fit::<u8>()));
        let mut bad = original;
        bad["human"] = serde_json::json!(1 - human);
        let error = serde_json::from_value::<Game>(bad).err().unwrap();
        assert!(error.to_string().contains("invalid local seat"));
    }
    for bots in [false, true] {
        let mut scenario = Scenario::skirmish();
        for player in &mut scenario.players {
            player.bot = bots;
            player.bot_config = None;
        }
        let state = scenario.build().unwrap();
        let stats = oxide_kit::stats::LiveMatchStats::new(&state);
        let session =
            SessionCheckpoint::capture(&scenario, &state, &[], &[], Some(&stats)).unwrap();
        let checkpoint = GameCheckpoint {
            session,
            human: PlayerId(0),
            concede_stats: None,
            boundary_fog: crate::boundary_fog::BoundaryFog::new(&state, PlayerId(0)),
            lan: None,
        };
        let game = checkpoint.restore().unwrap().install();
        assert_eq!(game.presentation.human, PlayerId(0));
    }
}

#[test]
fn a_lan_checkpoint_binds_the_host_seat_and_lists_every_other_player() {
    let mut scenario = Scenario::from_json(include_str!(
        "../../../../fixtures/scenarios/twin-forges.json"
    ))
    .unwrap();
    for (seat, player) in scenario.players.iter_mut().enumerate() {
        player.bot = seat == 3;
        player.bot_config = player.bot.then_some(BotConfig::default());
    }
    let game = Game::with_viewport(scenario.clone(), vec2(1280.0, 720.0)).unwrap();
    let host = PlayerId(1);
    let installs = [
        (PlayerId(2), oxide_net::InstallId::new(12)),
        (PlayerId(0), oxide_net::InstallId::new(10)),
    ];
    let lan = LanManifest::new(&scenario, host, &installs);
    assert_eq!(lan.seats, vec![installs[1], installs[0]], "in seat order");
    let pending = vec![oxide_sim::PlayerCommand {
        player: PlayerId(2),
        command: Command::Surrender,
    }];
    let mut capture = game.capture_save().for_lan(pending.clone(), lan);
    capture.human = host;
    let checkpoint = serde_json::to_value(capture.checkpoint().unwrap()).unwrap();

    let restored: GameCheckpoint = serde_json::from_value(checkpoint.clone()).unwrap();
    assert!(restored.is_lan());
    let game = restored.restore().unwrap().install();
    assert_eq!(game.presentation.human, host, "any human seat can host");
    assert_eq!(
        game.pending.to_vec(),
        pending,
        "every seat's unsealed orders"
    );

    let refused = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut bad = checkpoint.clone();
        edit(&mut bad);
        let bad: GameCheckpoint = serde_json::from_value(bad).unwrap();
        bad.restore().err().unwrap().to_string()
    };
    assert!(
        refused(&|bad| bad["lan"]["seats"].as_array_mut().unwrap().truncate(1))
            .contains("LAN players do not match")
    );
    assert!(refused(&|bad| bad["human"] = serde_json::json!(3)).contains("invalid host seat"));
    assert!(
        refused(&|bad| bad["lan"] = serde_json::Value::Null).contains("invalid local seat"),
        "a local checkpoint keeps the default local seat"
    );
}

#[test]
fn a_lan_save_waits_only_for_players_still_in_the_match() {
    let mut scenario = Scenario::from_json(include_str!(
        "../../../../fixtures/scenarios/twin-forges.json"
    ))
    .unwrap();
    for (seat, player) in scenario.players.iter_mut().enumerate() {
        player.bot = seat == 3;
        player.bot_config = player.bot.then_some(BotConfig::default());
    }
    let mut game = Game::with_viewport(scenario.clone(), vec2(1280.0, 720.0)).unwrap();
    game.stage(oxide_sim::PlayerCommand {
        player: PlayerId(2),
        command: Command::Surrender,
    });
    game.advance_ticks(2);
    let orders = [PlayerId(0), PlayerId(1), PlayerId(2)].map(|player| oxide_sim::PlayerCommand {
        player,
        command: Command::Stop { units: Vec::new() },
    });
    let installs = [
        (PlayerId(1), oxide_net::InstallId::new(1)),
        (PlayerId(2), oxide_net::InstallId::new(2)),
    ];
    let lan = LanManifest::new(&scenario, PlayerId(0), &installs);
    let save = game
        .capture_save()
        .for_lan(orders.to_vec(), lan)
        .checkpoint()
        .unwrap()
        .into_lan_save()
        .unwrap();
    assert_eq!(
        save.players(),
        &installs[..1],
        "the surrendered player is out"
    );

    let (host, pending) = save
        .host_game(&[(PlayerId(1), BotDifficulty::Standard, BotStance::Balanced)])
        .unwrap();
    assert_eq!(host.net_role(), Some(network::NetRole::Host));
    assert!(!host.clock.paused, "a LAN match runs at once");
    assert!(host.scenario.players[1].bot, "the bot took the seat");
    assert_eq!(
        pending,
        vec![orders[0].clone()],
        "the replaced seat's orders go, and so do those of a player out of the match"
    );
    assert_eq!(
        host.pending.to_vec(),
        vec![orders[0].clone()],
        "the host projects its own"
    );
    assert!(
        save.host_game(&[(PlayerId(3), BotDifficulty::Standard, BotStance::Balanced)])
            .is_err(),
        "only a player's seat goes to a bot"
    );

    // Resuming alone hands the player out of the match a bot too, so the
    // host holds the only human seat a local checkpoint expects.
    let alone = save
        .local_game(&[(PlayerId(1), BotDifficulty::Standard, BotStance::Balanced)])
        .unwrap();
    assert!(alone.scenario.players[2].bot);
    assert_eq!(Game::local_seat(&alone.scenario), PlayerId(0));
    assert_eq!(alone.pending.to_vec(), vec![orders[0].clone()]);
}
