use super::*;
use crate::game::Game;
use macroquad::prelude::Vec2;
use oxide_sim::{Event, PlayerId};

/// An open field of three seats with nobody's bot running, the camera over
/// its middle and the picture showing each tick whole.
fn field(units: &serde_json::Value, buildings: &serde_json::Value) -> (Game, UnitId) {
    let scenario = serde_json::from_value(serde_json::json!({
        "name": "Maw audio", "mode": "sandbox", "map": vec![".".repeat(40); 20],
        "players": [
            {"name": "West", "scrap": 0, "bot": false},
            {"name": "East", "scrap": 0, "bot": false},
            {"name": "North", "scrap": 0, "bot": false}
        ],
        "units": units,
        "buildings": buildings
    }))
    .unwrap();
    let mut game = Game::with_viewport(scenario, Vec2::new(1280.0, 800.0)).unwrap();
    game.clock.paused = true;
    game.presentation.camera.center = Vec2::new(12.0, 10.0);
    let maw = game
        .state
        .units()
        .iter()
        .find(|unit| unit.kind == UnitKind::Maw)
        .unwrap()
        .id;
    (game, maw)
}

fn grinding(game: &Game, maw: UnitId) -> bool {
    maw_motors(&game.view())
        .0
        .iter()
        .any(|motor| motor.key == maw)
}

fn humming(game: &Game, maw: UnitId) -> Option<f32> {
    maw_motors(&game.view())
        .1
        .iter()
        .find(|motor| motor.key == maw)
        .map(|motor| motor.gain)
}

/// Whether the frame's Maw loops raise the score, with no clips to play.
fn raises_score(game: &Game, running: bool) -> bool {
    update(
        &mut MawLoops::default(),
        &mut MawLoops::default(),
        &game.view(),
        running,
        (&[], &[]),
        (1.0, 1.0),
    )
}

fn cut(report: &oxide_sim::TickReport, maw: UnitId) -> bool {
    report
        .events
        .iter()
        .any(|event| matches!(event, Event::AttackHit { attacker, .. } if *attacker == maw))
}

#[test]
fn a_maw_s_drum_is_heard_from_its_first_hit_until_just_after_the_building_falls() {
    let (mut game, maw) = field(
        &serde_json::json!([{"player": 0, "kind": "maw", "x": 8, "y": 10}]),
        &serde_json::json!([{"player": 1, "kind": "fabricator", "x": 11, "y": 9}]),
    );
    let mut hits = 0;
    let mut last_hit = 0;
    while game
        .state
        .buildings()
        .iter()
        .any(|b| b.player == PlayerId(1))
    {
        assert!(game.state.current_tick() < 2_000, "the Maw never finished");
        game.presentation.sounds_pending.clear();
        let report = game.do_tick();
        let standing = game
            .state
            .buildings()
            .iter()
            .any(|b| b.player == PlayerId(1));
        if cut(&report, maw) {
            hits += 1;
            last_hit = report.tick;
            assert!(
                !standing || game.presentation.sounds_pending.is_empty(),
                "no report per hit: {:?}",
                game.presentation.sounds_pending
            );
        }
        assert_eq!(
            grinding(&game, maw),
            hits > 0,
            "the grind starts on the first hit and holds while it cuts"
        );
    }
    assert!(hits > 2);
    assert!(
        raises_score(&game, true),
        "a cut raises the score like a report"
    );
    assert!(!raises_score(&game, false), "nothing sounds while paused");
    while grinding(&game, maw) {
        game.do_tick();
        assert!(
            game.state.current_tick() <= last_hit + 16,
            "the grind dies away after the cut"
        );
    }
    for _ in 0..40 {
        game.do_tick();
        assert!(!grinding(&game, maw));
        assert_eq!(humming(&game, maw), None, "an unhurt Maw's kiln stays cold");
    }
}

#[test]
fn a_hostile_maw_is_heard_only_while_it_is_seen() {
    // East's Maw cuts North's Fabricator at the far side of the field,
    // where West has no eyes.
    let (mut game, maw) = field(
        &serde_json::json!([{"player": 1, "kind": "maw", "x": 30, "y": 10}]),
        &serde_json::json!([{"player": 2, "kind": "fabricator", "x": 33, "y": 9}]),
    );
    game.presentation.camera.center = Vec2::new(32.0, 10.0);
    while !cut(&game.do_tick(), maw) {
        assert!(game.state.current_tick() < 600, "the Maw never cut");
    }
    game.do_tick();
    assert!(!grinding(&game, maw), "unseen, it is unheard");
    game.presentation.overlay = true;
    assert!(grinding(&game, maw), "an all-seeing view hears it");
    game.presentation.overlay = false;
    game.presentation.human = PlayerId(2);
    assert!(grinding(&game, maw), "the Fabricator it cuts sees it");
    game.presentation.human = PlayerId(1);
    assert!(
        grinding(&game, maw),
        "its owner hears it anywhere it is seen"
    );
    game.presentation.camera.center = Vec2::new(-200.0, 10.0);
    assert!(
        !grinding(&game, maw),
        "a machine far off camera falls silent"
    );
}

#[test]
fn a_maw_s_kiln_hums_while_its_belt_runs_and_stops_with_the_belt() {
    let (game, maw) = field(
        &serde_json::json!([{"player": 0, "kind": "maw", "x": 12, "y": 10}]),
        &serde_json::json!([]),
    );
    // Wounded, with a little wreck ahead of it.
    let mut wire = serde_json::to_value(&*game.state).unwrap();
    for unit in wire["units"].as_array_mut().unwrap() {
        if unit["id"] == serde_json::json!(maw) {
            unit["hp"] = serde_json::json!(UnitKind::Maw.stats().max_hp - 20);
        }
    }
    wire["map"]["grid"]["cells"][10 * 40 + 18]["wreck"] = serde_json::json!(20);
    let state: oxide_sim::State = serde_json::from_value(wire).unwrap();
    let mut game = game;
    game.replace_state_after_jump(&state);
    game.issue(oxide_sim::Command::Run {
        units: vec![maw],
        goal: chassis::grid::TilePos::new(18, 10),
        queue: false,
    });

    while game.state.kiln_fuel(maw).is_none() {
        assert!(
            game.state.current_tick() < 600,
            "the Maw never reached the wreck"
        );
        assert_eq!(humming(&game, maw), None, "a cold kiln is silent");
        game.do_tick();
    }
    game.presentation.camera.center =
        game.presentation
            .draw_pos(maw, game.state.unit(maw).unwrap().pos, 1.0);
    let mut gains = Vec::new();
    while game.state.kiln_fuel(maw).is_some() {
        assert!(game.state.current_tick() < 1_200, "the kiln never ran dry");
        game.do_tick();
        gains.push(humming(&game, maw).unwrap_or(0.0));
        assert!(!grinding(&game, maw), "feeding is not cutting");
        assert!(!raises_score(&game, true), "nor does it raise the score");
    }
    assert!(
        gains[0] > 0.0 && gains[0] < gains[gains.len() - 1],
        "the hum swells as the belt starts: {gains:?}"
    );
    let dry = game.state.current_tick();
    while humming(&game, maw).is_some() {
        assert!(
            game.state.current_tick() < dry + 80,
            "the belt never stopped"
        );
        game.do_tick();
    }
    assert!(
        game.state.current_tick() > dry + 30,
        "the belt runs on, and the kiln hums, until its last load is in"
    );
}
