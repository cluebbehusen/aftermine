use super::*;
use crate::game::Game;
use crate::tests::support::sandbox;
use aftermine_sim::{BuildingKind, Event, PlayerId};
use macroquad::prelude::Vec2;

/// An open field of three seats with nobody's bot running, the camera over
/// its middle and the picture showing each tick whole.
fn field(
    units: &[(u8, UnitKind, i32, i32)],
    buildings: &[(u8, BuildingKind, i32, i32)],
) -> (Game, UnitId) {
    let scenario = sandbox(40, 20, 3, units, buildings);
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
        .iter()
        .any(|motor| motor.key == maw)
}

fn cut(report: &aftermine_sim::TickReport, maw: UnitId) -> bool {
    report
        .events
        .iter()
        .any(|event| matches!(event, Event::AttackHit { attacker, .. } if *attacker == maw))
}

#[test]
fn a_maw_s_drum_is_heard_from_its_first_hit_until_just_after_the_building_falls() {
    let (mut game, maw) = field(
        &[(0, UnitKind::Maw, 8, 10)],
        &[(1, BuildingKind::Fabricator, 11, 9)],
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
        game.presentation.score_cues().guns > 0,
        "a cut counts toward the score's fight like a report"
    );
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
    }
}

#[test]
fn a_hostile_maw_is_heard_only_while_it_is_seen() {
    // East's Maw cuts North's Fabricator at the far side of the field,
    // where West has no eyes.
    let (mut game, maw) = field(
        &[(1, UnitKind::Maw, 30, 10)],
        &[(2, BuildingKind::Fabricator, 33, 9)],
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
fn a_maw_feeding_its_kiln_is_not_heard_as_cutting() {
    let (game, maw) = field(&[(0, UnitKind::Maw, 12, 10)], &[]);
    // Wounded, with a little wreck ahead of it.
    let mut wire = serde_json::to_value(&*game.state).unwrap();
    for unit in wire["units"].as_array_mut().unwrap() {
        if unit["id"] == serde_json::json!(maw) {
            unit["hp"] = serde_json::json!(UnitKind::Maw.stats().max_hp - 20);
        }
    }
    wire["map"]["grid"]["cells"][10 * 40 + 18]["wreck"] = serde_json::json!(20);
    let state: aftermine_sim::State = serde_json::from_value(wire).unwrap();
    let mut game = game;
    game.replace_state_after_jump(&state);
    game.issue(aftermine_sim::Command::Run {
        units: vec![maw],
        goal: chassis::grid::TilePos::new(18, 10),
        queue: false,
    });

    while game.state.kiln_fuel(maw).is_none() {
        assert!(
            game.state.current_tick() < 600,
            "the Maw never reached the wreck"
        );
        game.do_tick();
    }
    game.presentation.camera.center =
        game.presentation
            .draw_pos(maw, game.state.unit(maw).unwrap().pos, 1.0);
    while game.state.kiln_fuel(maw).is_some() {
        assert!(game.state.current_tick() < 1_200, "the kiln never ran dry");
        game.do_tick();
        assert!(!grinding(&game, maw), "feeding is not cutting");
        assert_eq!(
            game.presentation.score_cues().guns,
            0,
            "nor does it count toward the score's fight"
        );
    }
}
