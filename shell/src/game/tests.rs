use super::*;
use oxide_sim::scenario::ScenarioMode;

/// The watch-back loop replays every re-executed tick through the seat
/// bots so they rebuild their cross-tick memory. Without it the resume
/// hash still matches (the recorded commands carry it) and only the
/// future diverges, which this test catches.
#[test]
fn a_resumed_session_plays_the_same_future_as_an_unsaved_one() {
    // Seat 1 is a bot whose memory the watch-back rebuilds.
    let mut scenario = oxide_sim::Scenario::skirmish();
    oxide_kit::bench::all_bots(&mut scenario);
    scenario.players[0].bot = false;
    scenario.players[0].bot_config = None;
    let mut original = Game::new(scenario).expect("game builds");
    original.advance_ticks(600);

    let mut snapshot = original.recorder.clone();
    snapshot.meta.ticks = Some(600);
    let mut resumed = Game::from_replay(snapshot).expect("the snapshot resumes");
    assert_eq!(
        original.hash_hex(),
        resumed.hash_hex(),
        "premise: the resume point itself matches"
    );

    original.advance_ticks(1_000);
    resumed.advance_ticks(1_000);
    assert_eq!(
        original.hash_hex(),
        resumed.hash_hex(),
        "a resumed session's future diverged from the unsaved one — \
             bot memory was not rebuilt by the watch-back"
    );
}

#[test]
fn multiple_bot_seats_rebuild_the_same_history_on_resume() {
    let mut scenario =
        oxide_sim::Scenario::from_json(include_str!("../../../scenarios/compass-grand.json"))
            .unwrap();
    oxide_kit::bench::all_bots(&mut scenario);
    scenario.players[0].bot = false;
    scenario.players[0].bot_config = None;
    let mut original = Game::new(scenario).unwrap();
    original.advance_ticks(180);
    let mut snapshot = original.recorder.clone();
    snapshot.meta.ticks = Some(180);
    let mut resumed = Game::from_replay(snapshot).unwrap();
    original.advance_ticks(120);
    resumed.advance_ticks(120);
    assert_eq!(
        serde_json::to_vec(&original.recorder.commands).unwrap(),
        serde_json::to_vec(&resumed.recorder.commands).unwrap()
    );
    assert_eq!(original.hash_hex(), resumed.hash_hex());
}

use oxide_sim::{Command, Scenario, Target, UnitKind};

#[test]
fn foundry_free_sandbox_accepts_local_input_and_restores_checkpoint() {
    let mut scenario = Scenario::skirmish();
    scenario.mode = oxide_sim::scenario::ScenarioMode::Sandbox;
    for row in &mut scenario.map {
        *row = row.replace(['1', '2'], ".");
    }
    for player in &mut scenario.players {
        player.bot = false;
        player.bot_config = None;
    }
    let mut original = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
    assert!(original.bots.is_empty());
    assert!(original.home_foundry().is_none());
    let unit = original
        .state
        .units()
        .iter()
        .find(|unit| unit.player == PlayerId(0))
        .unwrap()
        .id;
    original.issue(Command::Run {
        units: vec![unit],
        goal: chassis::grid::TilePos::new(8, 6),
        queue: false,
    });
    let mut resumed: Game =
        serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
    for game in [&mut original, &mut resumed] {
        assert!(game.state.accepts_commands(PlayerId(0)));
        let report = game.do_tick();
        assert!(
            !report
                .events
                .iter()
                .any(|event| matches!(event, Event::CommandRejected { .. }))
        );
        game.advance_ticks(120);
        assert!(game.state.result().is_none());
    }
    assert_eq!(original.hash_hex(), resumed.hash_hex());
}

#[test]
fn local_sessions_accept_any_bot_roster_and_resume() {
    for (bot_flags, local_seat) in [
        ([true, true], PlayerId(0)),
        ([false, false], PlayerId(0)),
        ([true, false], PlayerId(1)),
    ] {
        let mut scenario = Scenario::skirmish();
        for (player, bot) in scenario.players.iter_mut().zip(bot_flags) {
            player.bot = bot;
            player.bot_config = bot.then(oxide_sim::scenario::BotConfig::default);
        }
        let mut original = Game::with_viewport(scenario, vec2(1280.0, 800.0))
            .expect("bot assignment does not constrain the local session");
        assert_eq!(original.presentation.human, local_seat);
        assert_eq!(
            original.bots.len(),
            bot_flags.into_iter().filter(|bot| *bot).count()
        );
        original.advance_ticks(180);
        let mut snapshot = original.recorder.clone();
        snapshot.meta.ticks = Some(180);
        let mut resumed = Game::from_replay(snapshot).expect("the session resumes");
        assert_eq!(resumed.presentation.human, local_seat);
        original.advance_ticks(120);
        resumed.advance_ticks(120);
        assert_eq!(original.hash_hex(), resumed.hash_hex());
        assert_eq!(
            serde_json::to_vec(&original.recorder.commands).unwrap(),
            serde_json::to_vec(&resumed.recorder.commands).unwrap()
        );
        if bot_flags == [false, false] {
            assert!(original.recorder.commands.is_empty());
        }
    }
}

#[test]
fn replay_without_duration_infers_its_tail_and_restores_finished_stats() {
    let scenario = Scenario::skirmish();
    let mut replay = GameReplay::new(SIM_VERSION, "test", scenario);
    replay.record(
        0,
        PlayerCommand {
            player: PlayerId(0),
            command: Command::Surrender,
        },
    );
    assert!(replay.meta.ticks.is_none());

    let game = Game::from_replay(replay).expect("legacy duration is inferred");
    assert_eq!(game.state.current_tick(), 1);
    assert!(game.state.result().is_some());
    assert_eq!(
        game.end_stats.as_ref().map(|stats| stats.final_tick),
        Some(1)
    );
}

#[test]
fn wall_clock_pause_and_hitch_rules_bound_simulation_debt() {
    let mut game = Game::with_viewport(
        Scenario::skirmish(),
        macroquad::prelude::vec2(1280.0, 800.0),
    )
    .expect("game");
    game.clock.paused = true;
    assert!(!game.advance_wall_clock(10.0, None));
    assert_eq!(game.state.current_tick(), 0);
    assert_eq!(game.clock.render_alpha(), 1.0);

    game.clock.paused = false;
    game.clock.speed = 64.0;
    assert!(!game.advance_wall_clock(1.0, None));
    assert_eq!(game.state.current_tick(), u64::from(MAX_TICKS_PER_FRAME));
    assert!(
        game.presentation.tick_fraction() <= 1.0,
        "excess hitch debt is dropped"
    );
}

#[test]
fn toast_history_keeps_only_the_three_newest_messages() {
    let mut game = Game::with_viewport(
        Scenario::skirmish(),
        macroquad::prelude::vec2(1280.0, 800.0),
    )
    .expect("game");
    for text in ["One", "Two", "Three", "Four"] {
        game.presentation.toast(text);
    }
    assert_eq!(
        game.presentation
            .toasts
            .iter()
            .map(|toast| toast.text.as_str())
            .collect::<Vec<_>>(),
        ["Two", "Three", "Four"]
    );
    // A lowercase fragment is shown as a sentence, so it repeats "Three".
    game.presentation.toast("three");
    assert_eq!(
        game.presentation
            .toasts
            .iter()
            .map(|toast| toast.text.as_str())
            .collect::<Vec<_>>(),
        ["Two", "Four", "Three"]
    );
    assert_eq!(game.presentation.toasts.last().unwrap().age, 0.0);
}

#[test]
fn articulated_hull_interpolates_the_short_turn_and_drops_on_seek() {
    let mut game = Game::with_viewport(Scenario::skirmish(), vec2(1280.0, 800.0)).unwrap();
    let id = game.state.units()[0].id;
    game.presentation.hull_heading.insert(id.0, (6.2, 0.1));
    assert!(
        (game.view().draw_hull_heading(id, 0.5) - (6.2 + angle_delta(6.2, 0.1) * 0.5)).abs() < 1e-6
    );
    assert!(angle_delta(6.2, 0.1) > 0.0);
    game.drop_presentation();
    let heading = f32::from(game.state.unit(id).unwrap().heading) * std::f32::consts::TAU / 256.0
        + std::f32::consts::FRAC_PI_2;
    assert_eq!(game.view().draw_hull_heading(id, 0.5), heading);
}

#[test]
fn articulated_hulls_keep_authoritative_bearings_when_paused_or_jumped() {
    for kind in [UnitKind::Sentinel, UnitKind::Warden, UnitKind::Lancer] {
        let mut scenario = Scenario::skirmish();
        scenario.units[0].kind = kind;
        let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
        let id = game.state.units()[0].id;
        let assert_bearing = |game: &Game| {
            let expected = f32::from(game.state.unit(id).unwrap().heading) * std::f32::consts::TAU
                / 256.0
                + std::f32::consts::FRAC_PI_2;
            for alpha in [0.0, 0.5, 1.0] {
                assert_eq!(
                    game.view().draw_hull_heading(id, alpha),
                    expected,
                    "{kind:?}"
                );
            }
        };
        assert_bearing(&game);
        game.advance_ticks(1);
        assert_bearing(&game);
        let mut snapshot = serde_json::to_value(&*game.state).unwrap();
        snapshot["units"][0]["heading"] = serde_json::json!(96);
        game.replace_state_after_jump(&serde_json::from_value(snapshot).unwrap());
        assert_bearing(&game);
        game.drop_presentation();
        assert_bearing(&game);
    }
}

#[test]
fn cruising_aircraft_keep_authoritative_facing_across_timeline_jumps() {
    for kind in UnitKind::ALL
        .into_iter()
        .filter(|kind| kind.stats().cruise_turn_rate > 0)
    {
        let mut scenario = Scenario::skirmish();
        scenario.units[0].kind = kind;
        let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
        let id = game.state.units()[0].id;
        for heading in [0u8, 96, 254] {
            let mut snapshot = serde_json::to_value(&*game.state).unwrap();
            snapshot["units"][0]["heading"] = serde_json::json!(heading);
            game.replace_state_after_jump(&serde_json::from_value(snapshot).unwrap());
            let expected =
                f32::from(heading) * std::f32::consts::TAU / 256.0 + std::f32::consts::FRAC_PI_2;
            assert_eq!(
                game.presentation.facing.get(&id.0),
                Some(&expected),
                "{kind:?}"
            );
            for alpha in [0.0, 0.5, 1.0] {
                assert_eq!(game.presentation.draw_heading(id, heading, alpha), expected);
            }
        }
    }
}

fn head_on_pair() -> (Game, [UnitId; 2]) {
    let mut map = vec!["........................................"; 24];
    map[2] = "..1.....................................";
    let scenario = serde_json::from_value(serde_json::json!({
        "name": "Head-on pass", "map": map,
        "players": [{"name": "You", "scrap": 0, "bot": false}],
        "units": [
            {"player": 0, "kind": "sentinel", "x": 12, "y": 12},
            {"player": 0, "kind": "sentinel", "x": 22, "y": 12}
        ],
        "buildings": []
    }))
    .unwrap();
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
    let ids = [game.state.units()[0].id, game.state.units()[1].id];
    game.present_ticks(1);
    for (id, x) in [(ids[0], 22), (ids[1], 12)] {
        game.issue(Command::Run {
            units: vec![id],
            goal: chassis::grid::TilePos::new(x, 12),
            queue: false,
        });
    }
    (game, ids)
}

#[test]
fn collision_slides_are_drawn_eased_and_leaned_then_settle_on_the_simulation_pose() {
    let (mut game, ids) = head_on_pair();
    let (mut widest_lag, mut widest_lean) = (0.0_f32, 0.0_f32);
    for _ in 0..200 {
        game.present_ticks(1);
        for id in ids {
            let unit = game.state.unit(id).unwrap();
            let lag = world_vec(unit.pos) - game.presentation.draw_pos(id, unit.pos, 1.0);
            widest_lag = widest_lag.max(lag.length());
            widest_lean = widest_lean.max(game.presentation.slide_yaw(id, 1.0, false).abs());
        }
    }
    assert!(widest_lag > 0.02 && widest_lag <= crate::slide_motion::MAX_LAG + 1e-6);
    assert!(widest_lean > 0.05 && widest_lean <= crate::slide_motion::MAX_YAW + 1e-6);
    assert!(game.presentation.slide_motion.is_empty());
    for id in ids {
        let unit = game.state.unit(id).unwrap();
        assert_eq!(
            game.presentation.draw_pos(id, unit.pos, 1.0),
            world_vec(unit.pos)
        );
        assert_eq!(unit.order, oxide_sim::Order::Idle);
    }
}

#[test]
fn a_bulk_advance_drops_eased_slides() {
    let (mut game, _) = head_on_pair();
    while game.presentation.slide_motion.is_empty() {
        game.present_ticks(1);
        assert!(game.state.current_tick() < 200, "the pair never touched");
    }
    game.advance_ticks(1);
    assert!(game.presentation.slide_motion.is_empty());
}

fn rotor_game(kind: UnitKind) -> Game {
    let mut map = vec!["........................................"; 24];
    map[2] = "..1................................2....";
    let scenario = serde_json::from_value(serde_json::json!({
        "name": "Rotor turning", "map": map,
        "players": [
            {"name": "You", "scrap": 0, "bot": false},
            {"name": "Target", "scrap": 0, "bot": true}
        ],
        "units": [{"player": 0, "kind": kind, "x": 12, "y": 12}],
        "buildings": []
    }))
    .unwrap();
    Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap()
}

#[test]
fn rotor_hulls_ease_reversals_and_finish_turning_after_stopping() {
    let mut first_turns = Vec::new();
    for kind in [UnitKind::Buzzard, UnitKind::Skyhook] {
        let mut game = rotor_game(kind);
        let id = game.state.units()[0].id;
        game.present_ticks(1);
        game.issue(Command::Run {
            units: vec![id],
            goal: chassis::grid::TilePos::new(28, 12),
            queue: false,
        });
        game.present_ticks(20);
        let before = game.view().draw_hull_heading(id, 1.0);
        let position = game.state.unit(id).unwrap().pos;
        game.issue(Command::Run {
            units: vec![id],
            goal: chassis::grid::TilePos::new(8, 12),
            queue: false,
        });
        game.present_ticks(1);
        assert!(game.state.unit(id).unwrap().pos.x < position.x, "{kind:?}");
        let turn = angle_delta(before, game.view().draw_hull_heading(id, 1.0)).abs();
        assert!(turn > 0.1 && turn < 0.7, "{kind:?}: {turn}");
        first_turns.push(turn);
        assert!((game.view().draw_hull_heading(id, 0.0) - before).abs() < 1e-6);
        game.issue(Command::Stop { units: vec![id] });
        game.present_ticks(1);
        let stopped = game.state.unit(id).unwrap().pos;
        game.present_ticks(20);
        assert_eq!(game.state.unit(id).unwrap().pos, stopped, "{kind:?}");
        assert!(
            angle_delta(
                game.view().draw_hull_heading(id, 1.0),
                -std::f32::consts::FRAC_PI_2
            )
            .abs()
                < 1e-5
        );
    }
    assert!((first_turns[0] - 0.3).abs() < 1e-6);
    assert!((first_turns[1] - 0.25).abs() < 1e-6);
    assert!(first_turns[1] < first_turns[0]);
}

#[test]
fn draw_positions_interpolate_known_units_and_fall_back_for_new_ones() {
    let mut game = Game::with_viewport(
        Scenario::skirmish(),
        macroquad::prelude::vec2(1280.0, 800.0),
    )
    .expect("game");
    let unit_id = game.state.units()[0].id;
    let current = game.state.units()[0].pos;
    let now = world_vec(current);
    game.presentation
        .prev_pos
        .insert(unit_id.0, now - macroquad::prelude::vec2(2.0, 4.0));
    assert_eq!(
        game.presentation.draw_pos(unit_id, current, 0.5),
        now - macroquad::prelude::vec2(1.0, 2.0)
    );
    assert_eq!(
        game.presentation.draw_pos(UnitId(u32::MAX), current, 0.5),
        now
    );
}

#[test]
fn externally_driven_tick_fractions_are_clamped_to_one_frame() {
    let mut game = Game::with_viewport(
        Scenario::skirmish(),
        macroquad::prelude::vec2(1280.0, 800.0),
    )
    .expect("game");
    game.presentation.set_tick_fraction(2.0);
    assert_eq!(game.presentation.tick_fraction(), 1.0);
    game.presentation.set_tick_fraction(-1.0);
    assert_eq!(game.presentation.tick_fraction(), 0.0);
}

#[test]
fn admitted_attack_alert_queues_one_protected_audio_cue() {
    let mut game = Game::with_viewport(
        Scenario::skirmish(),
        macroquad::prelude::vec2(1280.0, 800.0),
    )
    .expect("skirmish builds");
    game.presentation.sounds_pending.clear();

    game.presentation
        .raise_alert(macroquad::prelude::vec2(10.0, 10.0));
    game.presentation
        .raise_alert(macroquad::prelude::vec2(11.0, 11.0));

    assert_eq!(
        game.presentation.sounds_pending,
        vec![(SoundKind::Alert, None)],
        "the region gate must admit one alert cue rather than one per hit"
    );
    assert!(
        game.presentation.toasts.is_empty(),
        "the top-bar badge speaks for the alert"
    );
}

#[test]
fn presented_ticks_return_rejections_and_keep_their_toast() {
    let mut game = Game::with_viewport(
        Scenario::skirmish(),
        macroquad::prelude::vec2(1280.0, 800.0),
    )
    .expect("skirmish builds");
    let foreign = game
        .state
        .units()
        .iter()
        .find(|unit| unit.player != game.presentation.human)
        .expect("skirmish has an opponent")
        .id;
    game.issue(Command::Stop {
        units: vec![foreign],
    });

    let events = game.present_ticks(1);

    assert!(events.iter().any(|event| matches!(
        event,
        Event::CommandRejected { player, .. } if *player == game.presentation.human
    )));
    assert!(
        game.presentation
            .toasts
            .iter()
            .any(|toast| toast.text == "Nothing selected can do that"),
        "presentation-preserving steps must retain shell feedback"
    );
}

#[test]
fn presented_ticks_age_old_effects_and_leave_new_feedback_fresh() {
    let mut game = Game::with_viewport(
        Scenario::skirmish(),
        macroquad::prelude::vec2(1280.0, 800.0),
    )
    .expect("skirmish builds");
    game.presentation.fx.push(Effect {
        kind: EffectKind::Ping {
            at: macroquad::prelude::Vec2::ZERO,
            kind: PingKind::Move,
            queued: false,
        },
        age: 0.0,
    });

    game.present_ticks(4);

    let age = game
        .presentation
        .fx
        .iter()
        .find_map(|effect| matches!(effect.kind, EffectKind::Ping { .. }).then_some(effect.age))
        .expect("the half-second ping remains after four ticks");
    assert!(
        (age - 4.0 * TICK_DT).abs() < f32::EPSILON * 8.0,
        "each represented interval must age existing presentation: {age}"
    );

    game.present_ticks(6);
    assert!(
        !game
            .presentation
            .fx
            .iter()
            .any(|effect| matches!(effect.kind, EffectKind::Ping { .. })),
        "an effect must expire after enough presented sim time"
    );
}

#[test]
fn paused_wall_time_freezes_presentation() {
    let mut game = Game::with_viewport(
        Scenario::skirmish(),
        macroquad::prelude::vec2(1280.0, 800.0),
    )
    .expect("skirmish builds");
    game.presentation.fx.push(Effect {
        kind: EffectKind::Ping {
            at: macroquad::prelude::Vec2::ZERO,
            kind: PingKind::Move,
            queued: false,
        },
        age: 0.0,
    });

    game.clock.paused = true;
    game.update_wall_clock_fx(0.25);
    assert_eq!(
        game.presentation.fx_time(),
        0.0,
        "decorative animation must hold"
    );
    assert_eq!(
        game.presentation.fx[0].age, 0.0,
        "transient effects must hold too"
    );

    game.clock.paused = false;
    game.update_wall_clock_fx(0.25);
    assert_eq!(game.presentation.fx_time(), 0.25);
    assert_eq!(game.presentation.fx[0].age, 0.25);
}

#[test]
fn wall_clock_profile_bound_cannot_overshoot_inside_a_multi_tick_frame() {
    let mut game = Game::with_viewport(
        Scenario::skirmish(),
        macroquad::prelude::vec2(1280.0, 800.0),
    )
    .expect("skirmish builds");
    game.clock.speed = 8.0;

    assert!(game.advance_wall_clock(1.0, Some(5)));
    assert_eq!(game.state.current_tick(), 5);
    assert_eq!(game.presentation.tick_fraction(), 0.0);
}

#[test]
fn playback_and_seeks_face_a_parked_airframe_by_its_heading() {
    let mut scenario = Scenario::skirmish();
    scenario.units = vec![oxide_sim::scenario::UnitSpec {
        player: 0,
        kind: oxide_sim::UnitKind::Condor,
        x: 20,
        y: 12,
    }];
    let mut game = Game::with_viewport(scenario, macroquad::prelude::vec2(1280.0, 800.0))
        .expect("the landing scenario builds");
    let condor = game.state.units()[0].id;
    for _ in 0..600 {
        game.advance_ticks(1);
        if game.state.unit(condor).is_some_and(oxide_sim::Unit::landed) {
            break;
        }
    }
    let parked = game.state.unit(condor).expect("the Condor survives");
    assert!(parked.landed(), "premise: the idle Condor parks itself");
    let expected =
        f32::from(parked.heading) * std::f32::consts::TAU / 256.0 + std::f32::consts::FRAC_PI_2;
    let snapshot = (*game.state).clone();

    game.replace_state_after_jump(&snapshot);
    assert_eq!(
        game.presentation.facing.get(&condor.0).copied(),
        Some(expected),
        "a seek shows the parked heading, not the default rotation"
    );

    game.presentation.facing.clear();
    game.presentation.remember_previous_tick(&game.state);
    game.presentation.observe_tick(&snapshot, &[], &[]);
    assert_eq!(
        game.presentation.facing.get(&condor.0).copied(),
        Some(expected),
        "playback faces the airframe as live play does"
    );
}

#[test]
fn bulk_advance_drops_old_timeline_aim_and_facing() {
    let mut game = Game::with_viewport(
        Scenario::skirmish(),
        macroquad::prelude::vec2(1280.0, 800.0),
    )
    .expect("skirmish builds");
    let unit = game.state.units()[0].id.0;
    let building = game.state.buildings()[0].id.0;
    game.presentation.facing.insert(unit, 1.25);
    game.presentation
        .aim_units
        .insert(unit, (2.5, game.presentation.fx_time()));
    game.presentation
        .aim_buildings
        .insert(building, (0.75, game.presentation.fx_time()));
    game.presentation
        .aim_building_targets
        .insert(building, Target::Unit(UnitId(unit)));

    game.advance_ticks(1);

    let expected =
        f32::from(game.state.unit(UnitId(unit)).unwrap().heading) * std::f32::consts::TAU / 256.0
            + std::f32::consts::FRAC_PI_2;
    assert_eq!(game.presentation.facing.get(&unit), Some(&expected));
    assert!(game.presentation.aim_units.is_empty());
    assert!(game.presentation.aim_buildings.is_empty());
    assert!(game.presentation.aim_building_targets.is_empty());
}

#[test]
fn a_decisive_concession_uses_the_result_flow_not_the_overlay() {
    // 1v1: the surrender decides the match on its own tick, so the
    // normal end-of-match banner takes over.
    let mut game = Game::with_viewport(
        Scenario::skirmish(),
        macroquad::prelude::vec2(1280.0, 800.0),
    )
    .expect("skirmish builds");
    game.issue(Command::Surrender);
    game.do_tick();
    assert!(game.state.player(game.presentation.human).resigned);
    assert!(game.state.result().is_some(), "a 1v1 concession decides");
    assert!(
        !game.presentation.conceded_banner,
        "no concede overlay over a result"
    );
    assert!(game.concede_stats.is_none());
    assert_eq!(
        game.end_stats.as_ref().map(|stats| stats.final_tick),
        Some(1),
        "the deciding tick leaves its report ready without replaying"
    );
}

/// A four-seat arena whose seats, in spawn order, are `(team, bot)`; the
/// first human seat plays locally.
fn concede_arena(seats: [(u8, bool); 4]) -> Game {
    let names = ["North West", "South West", "North East", "South East"];
    let players = names
        .iter()
        .zip(seats)
        .map(|(name, (team, bot))| oxide_sim::scenario::PlayerSpec {
            name: (*name).into(),
            team: Some(team),
            scrap: 100,
            bot,
            bot_config: bot.then_some(oxide_sim::scenario::BotConfig::default()),
        })
        .collect();
    let scenario = Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Match,
        name: "concede-arena".into(),
        map: vec![
            "####################".into(),
            "#1..............3..#".into(),
            "#..................#".into(),
            "#..................#".into(),
            "#..................#".into(),
            "#2..............4..#".into(),
            "#..................#".into(),
            "####################".into(),
        ],
        players,
        units: Vec::new(),
        buildings: Vec::new(),
        meta: None,
    };
    Game::with_viewport(scenario, macroquad::prelude::vec2(1280.0, 800.0))
        .expect("the concede arena builds")
}

#[test]
fn a_team_concession_raises_the_surrender_overlay() {
    let mut game = concede_arena([(0, false), (0, true), (1, true), (1, true)]);
    assert!(
        !game.presentation.all_seeing(),
        "a seat in the match sees through its own fog"
    );
    game.issue(Command::Surrender);
    game.do_tick();
    assert!(game.state.player(game.presentation.human).resigned);
    assert!(
        game.state.result().is_none(),
        "the ally's Foundry keeps the match running"
    );
    assert_eq!(
        game.presentation.stance,
        Stance::Out,
        "fog lifts with only a bot ally playing on"
    );
    assert!(
        game.presentation.conceded_banner,
        "the undecided concession raises the overlay"
    );
    assert!(
        game.concede_stats.is_some(),
        "the exit offer carries the match-so-far numbers"
    );
    assert_eq!(
        game.concede_stats.as_ref().map(|stats| stats.final_tick),
        Some(1)
    );
    // The banner is a one-shot moment: later ticks never re-raise
    // a dismissed overlay.
    game.presentation.conceded_banner = false;
    game.do_tick();
    assert!(
        !game.presentation.conceded_banner,
        "spectating stays unobstructed"
    );
}

/// `concede_arena(seats)` after the local seat surrenders, in a local
/// session or on `net`'s side of a LAN one.
fn surrendered(seats: [(u8, bool); 4], net: Option<network::NetRole>) -> Game {
    let mut game = concede_arena(seats);
    game.net = net;
    let surrender = PlayerCommand {
        player: game.presentation.human,
        command: Command::Surrender,
    };
    game.execute(&[surrender]);
    assert!(game.state.result().is_none(), "the match plays on");
    game
}

#[test]
fn fog_holds_only_while_a_lan_teammate_plays_on() {
    let team = [(0, false), (0, false), (1, true), (1, true)];
    let mut lan = surrendered(team, Some(network::NetRole::Client));
    assert_eq!(
        lan.presentation.stance,
        Stance::Seat,
        "an out seat could tip off its LAN ally"
    );
    lan.mirror_bot(PlayerId(1), oxide_sim::scenario::BotConfig::default());
    assert_eq!(
        lan.presentation.stance,
        Stance::Out,
        "fog lifts the moment a bot takes the ally's seat"
    );
    assert_eq!(
        surrendered(team, None).presentation.stance,
        Stance::Out,
        "a local session's second human seat is passive"
    );
    let free_for_all = [(0, false), (1, false), (2, true), (3, true)];
    assert_eq!(
        surrendered(free_for_all, Some(network::NetRole::Client))
            .presentation
            .stance,
        Stance::Out,
        "nobody plays on for a free-for-all seat's side"
    );
}

#[test]
fn an_out_seat_keeps_its_lifted_fog_through_save_and_resume() {
    let mut game = concede_arena([(0, false), (1, true), (2, true), (3, true)]);
    game.issue(Command::Surrender);
    game.do_tick();
    assert!(game.state.result().is_none(), "three seats fight on");
    assert_eq!(game.presentation.stance, Stance::Out);

    let saved = game
        .capture_save()
        .checkpoint()
        .unwrap()
        .restore()
        .unwrap()
        .install();
    assert_eq!(saved.presentation.stance, Stance::Out, "a restored save");
    let mut record = game.recorder.clone();
    record.meta.ticks = Some(game.state.current_tick());
    let resumed = Game::from_replay(record).expect("the record resumes");
    assert_eq!(resumed.presentation.stance, Stance::Out, "a resumed record");
}

#[test]
fn angular_interpolation_crosses_zero_by_the_short_arc() {
    let mut game = Game::new(Scenario::skirmish()).unwrap();
    let id = game.state.units()[0].id;
    game.presentation.prev_heading.insert(id.0, 254);
    let expected = std::f32::consts::TAU + std::f32::consts::FRAC_PI_2;
    assert!((game.presentation.draw_heading(id, 2, 0.5) - expected).abs() < 1e-6);
    game.presentation.prev_heading.insert(id.0, 2);
    assert!(
        (game.presentation.draw_heading(id, 254, 0.5) - std::f32::consts::FRAC_PI_2).abs() < 1e-6
    );
}

#[test]
fn a_mounted_gun_turns_the_short_way_between_ticks() {
    let mut scenario = Scenario::skirmish();
    scenario.units.push(oxide_sim::scenario::UnitSpec {
        player: 0,
        kind: oxide_sim::UnitKind::Roc,
        x: 12,
        y: 10,
    });
    let game = Game::new(scenario).unwrap();
    let roc = game
        .state
        .units()
        .iter()
        .find(|unit| unit.kind == oxide_sim::UnitKind::Roc)
        .unwrap()
        .clone();
    let step = std::f32::consts::TAU / 256.0;
    let angle = |bearing: u8| f32::from(bearing) * step + std::f32::consts::FRAC_PI_2;
    let mut presentation = game.presentation;
    let current = roc.mount_bearing(0).unwrap();
    let drawn = presentation.draw_mount_heading(&roc, 0, 0.5).unwrap();
    assert!(
        (drawn - angle(current)).abs() < 1e-5,
        "a gun with no last tick draws where it is"
    );
    for previous in [current.wrapping_sub(6), current.wrapping_add(6)] {
        presentation
            .prev_mounts
            .insert(roc.id.0, [Some(previous), None]);
        let drawn = presentation.draw_mount_heading(&roc, 0, 0.5).unwrap();
        let turned = (drawn - angle(previous)) / step;
        assert!(
            (turned.abs() - 3.0).abs() < 1e-3,
            "half of a six-step traverse from {previous} to {current}, not {turned}"
        );
    }
}

fn crash_root(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "oxide-crash-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

/// The dump a panicking tick left, as its replay and manifest.
fn crash_dump(root: &std::path::Path) -> (GameReplay, serde_json::Value) {
    let dump = oxide_kit::diagnostics::newest_crash_dump(root).expect("the panic left a dump");
    let replay = oxide_kit::load_replay(dump.join("replay.json")).unwrap();
    let manifest =
        serde_json::from_slice(&std::fs::read(dump.join("crash.json")).unwrap()).unwrap();
    (replay, manifest)
}

#[test]
fn a_simulation_panic_dump_resumes_the_world_it_interrupted() {
    let root = crash_root("simulation");
    let mut game = Game::new(Scenario::skirmish()).unwrap();
    game.crash_dumps = Some(root.clone());
    game.advance_ticks(40);
    let train = Command::Train {
        building: oxide_sim::BuildingId(0),
        kind: UnitKind::Harvester,
    };
    game.issue(train.clone());
    let before = game.hash_hex();
    // A world shared outside the session makes the tick's exclusive borrow
    // panic after its batch was recorded and before the simulation ran.
    let held = Arc::clone(&game.state.0);
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| game.do_tick()));
    assert!(panicked.is_err());
    drop(held);

    let (replay, manifest) = crash_dump(&root);
    assert_eq!(manifest["phase"], "simulation");
    assert_eq!(manifest["ticks"], 40);
    assert!(
        manifest["message"]
            .as_str()
            .unwrap()
            .contains("retained the world")
    );
    assert_eq!(
        manifest["unfinished"][0]["command"],
        serde_json::to_value(&train).unwrap(),
        "the batch the simulation never finished"
    );
    let resumed = Game::from_replay(replay).unwrap();
    assert_eq!(resumed.hash_hex(), before);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_collection_panic_dump_ends_at_the_last_completed_tick() {
    let root = crash_root("collection");
    let mut game = Game::new(Scenario::skirmish()).unwrap();
    game.crash_dumps = Some(root.clone());
    game.advance_ticks(12);
    let before = game.hash_hex();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        game.guard_tick(|_| -> () { panic!("a bot exploded") });
    }));
    assert!(panicked.is_err());
    let (replay, manifest) = crash_dump(&root);
    assert_eq!(manifest["phase"], "collection");
    assert_eq!(manifest["message"], "a bot exploded");
    assert_eq!(manifest["unfinished"], serde_json::json!([]));
    assert_eq!(Game::from_replay(replay).unwrap().hash_hex(), before);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_panic_after_the_simulation_keeps_the_finished_tick() {
    let root = crash_root("after");
    let mut game = Game::new(Scenario::skirmish()).unwrap();
    game.crash_dumps = Some(root.clone());
    game.advance_ticks(7);
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        game.guard_tick(|game| {
            game.execute(&[]);
            panic!("presentation exploded");
        })
    }));
    assert!(panicked.is_err());
    let (replay, manifest) = crash_dump(&root);
    assert_eq!(manifest["phase"], "after_simulation");
    assert_eq!(replay.meta.ticks, Some(8));
    assert_eq!(
        Game::from_replay(replay).unwrap().hash_hex(),
        game.hash_hex()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_session_without_a_diagnostics_folder_leaves_no_dump() {
    let mut game = Game::new(Scenario::skirmish()).unwrap();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        game.guard_tick(|_| -> () { panic!("nowhere to write") });
    }));
    assert!(panicked.is_err());
}
