use super::*;
use crate::game::Game;
use crate::slide_motion::SlideMotion;
use aftermine_sim::Command;
use aftermine_sim::UnitKind;

fn production_game() -> Game {
    let mut scenario = Scenario::skirmish();
    for player in &mut scenario.players {
        player.bot_config = None;
    }
    scenario.units.clear();
    Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap()
}

#[test]
fn a_wreck_on_a_live_node_counts_toward_its_scrap() {
    assert_eq!(classify_salvage(0, 0), None);
    assert_eq!(classify_salvage(0, 30), Some(Salvage::Wreck(30)));
    assert_eq!(classify_salvage(400, 0), Some(Salvage::Scrap(400)));
    assert_eq!(classify_salvage(400, 30), Some(Salvage::Scrap(430)));
}

#[test]
fn newborn_collision_waits_for_an_interpolation_origin() {
    let mut game = production_game();
    let foundry = game
        .state
        .buildings()
        .iter()
        .find(|b| b.player == game.presentation.human)
        .unwrap()
        .id;
    for _ in 0..2 {
        game.issue(Command::Train {
            building: foundry,
            kind: UnitKind::Harvester,
        });
    }
    for _ in 0..600 {
        let report = game.do_tick();
        for event in report.events {
            let Event::UnitTrained { unit: id, .. } = event else {
                continue;
            };
            if !report
                .movement
                .iter()
                .any(|motion| motion.unit == id && motion.correction != chassis::fx::Vec2Fx::ZERO)
            {
                continue;
            }
            let unit = game.state.unit(id).unwrap();
            let born_at = world_vec(unit.pos);
            assert!(!game.presentation.prev_pos.contains_key(&id.0));
            assert!(!game.presentation.slide_motion.contains_key(&id.0));
            for alpha in [0.0, 0.5, 1.0] {
                assert_eq!(game.presentation.draw_pos(id, unit.pos, alpha), born_at);
            }
            game.do_tick();
            let unit = game.state.unit(id).unwrap();
            assert_eq!(game.presentation.draw_pos(id, unit.pos, 0.0), born_at);
            assert!(game.presentation.slide_motion.contains_key(&id.0));
            return;
        }
    }
    panic!("production never spawned a colliding unit");
}

#[test]
fn a_stationary_fixed_weapon_clears_lean_when_aiming_but_idle_lean_eases() {
    for aiming in [false, true] {
        let mut scenario = Scenario::skirmish();
        scenario.units = vec![aftermine_sim::scenario::UnitSpec {
            player: 0,
            kind: UnitKind::Bombard,
            x: 10,
            y: 10,
        }];
        let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
        let id = game.state.units()[0].id;
        let tick = game.state.current_tick();
        let mut slide = SlideMotion::new(tick.wrapping_sub(2));
        slide.observe(
            tick.wrapping_sub(1),
            0.0,
            vec2(0.11, 0.0),
            vec2(0.0, 0.11),
            0.11,
            true,
        );
        game.presentation.slide_motion.insert(id.0, slide);
        if aiming {
            game.presentation
                .aim_units
                .insert(id.0, (0.0, game.presentation.fx_clock));
        }
        game.presentation.observe_slides(&game.state, &[]);
        for alpha in [0.0, 0.5] {
            let yaw = game.presentation.slide_yaw(id, alpha, false);
            if aiming {
                assert_eq!(yaw, 0.0);
            } else {
                assert!(yaw > 0.0);
            }
        }
        assert_eq!(game.presentation.slide_yaw(id, 1.0, false), 0.0);
    }
}

#[test]
fn reduced_motion_suppresses_existing_lean_without_discarding_position_easing() {
    let mut game = production_game();
    let id = UnitId(99);
    let mut slide = SlideMotion::new(0);
    slide.observe(1, 0.0, vec2(0.11, 0.0), vec2(0.0, 0.11), 0.11, true);
    game.presentation.slide_motion.insert(id.0, slide);
    for alpha in [0.5, 1.0] {
        assert!(game.presentation.slide_yaw(id, alpha, false) > 0.0);
        assert_eq!(game.presentation.slide_yaw(id, alpha, true), 0.0);
        assert!(game.presentation.slide_motion[&id.0].lag(alpha).length() > 0.0);
    }
    assert!(game.presentation.slide_yaw(id, 1.0, false) > 0.0);
}

#[test]
fn engines_heat_quickly_while_moving_and_cool_slowly() {
    let mut heat = 0.0;
    for _ in 0..20 {
        heat = super::next_engine_heat(heat, true);
    }
    assert!(heat > 0.9, "hot after a second of travel: {heat}");
    let mut cooling = heat;
    for _ in 0..20 {
        cooling = super::next_engine_heat(cooling, false);
    }
    assert!(
        cooling > 0.4,
        "still warm a second after stopping: {cooling}"
    );
    for _ in 0..200 {
        cooling = super::next_engine_heat(cooling, false);
    }
    assert_eq!(cooling, 0.0, "cold in the end");
}

/// A hostile Roc far outside West's sight, flying, with a drone docked.
fn hidden_roc() -> (aftermine_sim::State, Presentation, aftermine_sim::UnitId) {
    let mut scenario = aftermine_sim::Scenario::skirmish();
    scenario.units = vec![
        aftermine_sim::scenario::UnitSpec {
            player: 0,
            kind: UnitKind::Sentinel,
            x: 2,
            y: 2,
        },
        aftermine_sim::scenario::UnitSpec {
            player: 1,
            kind: UnitKind::Roc,
            x: 30,
            y: 18,
        },
    ];
    let game = Game::with_viewport(scenario, macroquad::prelude::vec2(1280.0, 800.0)).unwrap();
    let state = (*game.state).clone();
    let roc = state.units()[1].id;
    assert!(
        !state.vision(PlayerId(0)).sees_unit(&state.units()[1]),
        "premise: the Roc is out of sight"
    );
    (state, game.presentation, roc)
}

fn fly(
    state: &mut aftermine_sim::State,
    presentation: &mut Presentation,
    roc: aftermine_sim::UnitId,
) {
    let at = state.unit(roc).unwrap().tile();
    let commands = [aftermine_sim::PlayerCommand {
        player: PlayerId(1),
        command: Command::Run {
            units: vec![roc],
            goal: chassis::grid::TilePos::new(at.x - 4, at.y),
            queue: false,
        },
    }];
    for tick in 0..20 {
        presentation.remember_previous_tick(state);
        let report = state.tick(if tick == 0 { &commands } else { &[] });
        presentation.observe_tick(state, &report.events, &report.movement);
    }
}

#[test]
fn a_hidden_roc_keeps_no_engine_heat() {
    let (mut state, mut presentation, roc) = hidden_roc();
    fly(&mut state, &mut presentation, roc);
    assert_eq!(
        presentation.engine_heat(roc),
        0.0,
        "its flight stayed hidden"
    );
    presentation.human = PlayerId(1);
    fly(&mut state, &mut presentation, roc);
    assert!(
        presentation.engine_heat(roc) > 0.5,
        "its owner sees it run hot"
    );
}

#[test]
fn a_hidden_roc_s_bay_stays_at_rest() {
    let (mut state, mut presentation, roc) = hidden_roc();
    let mut value = serde_json::to_value(&state).unwrap();
    value["units"][1]["bay"]["docked"] = 2.into();
    state = serde_json::from_value(value).unwrap();
    // Seen once docked, as if it had been in sight earlier.
    presentation.human = PlayerId(1);
    presentation.observe_tick(&state, &[], &[]);
    presentation.human = PlayerId(0);
    presentation.remember_previous_tick(&state);
    let mut value = serde_json::to_value(&state).unwrap();
    value["units"][1]["bay"]["docked"] = 0.into();
    let launched: aftermine_sim::State = serde_json::from_value(value).unwrap();
    presentation.observe_tick(
        &launched,
        &[aftermine_sim::Event::DroneLaunched {
            host: roc,
            drone: aftermine_sim::UnitId(99),
            player: PlayerId(1),
        }],
        &[],
    );
    assert!(
        presentation.drone_launches.is_empty(),
        "no launch is recorded out of sight"
    );
    let docks = &presentation.dock_motion[&roc.0];
    assert_eq!(*docks, crate::dock_motion::DockMotion::settled(0));
}
