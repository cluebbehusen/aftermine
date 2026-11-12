use super::*;
use crate::game::Game;
use macroquad::prelude::Vec2;

#[test]
fn flight_envelope_is_sustained_until_arrival_including_short_flights() {
    for total in [1.0, 4.0, 20.0, 80.0] {
        let ignition = missile_ejection_ticks(total);
        assert_eq!(motor_envelope(ignition - 0.01, total), 0.0);
        assert!(motor_envelope(f32::midpoint(ignition, total), total) > 0.0);
        assert!(motor_envelope(total - 0.01, total) > 0.0);
        assert_eq!(motor_envelope(total, total), 0.0);
    }
}

#[test]
fn real_missile_launch_flight_impact_fog_and_reconstruction_follow_state() {
    let mut map = vec![".".repeat(80); 32];
    map[20].replace_range(8..9, "1");
    map[20].replace_range(65..66, "2");
    let scenario = serde_json::from_value(serde_json::json!({
        "name": "Rocket audio lifecycle", "map": map,
        "players": [
            {"name": "You", "bot": false},
            {"name": "Target", "bot": true}
        ],
        "units": [
            {"player": 0, "kind": "avalanche", "x": 20, "y": 10},
            {"player": 0, "kind": "harvester", "x": 29, "y": 14}
        ],
        "buildings": [{"player": 1, "kind": "fabricator", "x": 32, "y": 10}]
    }))
    .unwrap();
    let mut game = Game::with_viewport(scenario, Vec2::new(1280.0, 800.0)).unwrap();
    game.presentation.camera.center = Vec2::new(26.0, 12.0);
    for _ in 0..200 {
        game.do_tick();
        if !game.state.shells().is_empty() {
            break;
        }
    }
    assert!(
        game.presentation
            .sounds_pending
            .iter()
            .any(|(kind, _)| *kind == SoundKind::AvalancheFire)
    );
    assert!(
        !game
            .presentation
            .sounds_pending
            .iter()
            .any(|(kind, _)| *kind == SoundKind::RocketMotor)
    );
    let arrival = game.state.shells()[0].arrival;
    assert!(
        audible_motors(&game.view()).is_empty(),
        "ejection precedes ignition"
    );
    for _ in 0..4 {
        game.do_tick();
    }
    let original = audible_motors(&game.view());
    assert_eq!(original.len(), 1);

    game.presentation.human = aftermine_sim::PlayerId(1);
    assert!(
        audible_motors(&game.view()).is_empty(),
        "hidden hostile motor must not reveal its muzzle"
    );
    game.presentation.overlay = true;
    assert_eq!(audible_motors(&game.view()).len(), 1);
    game.presentation.overlay = false;
    game.presentation.human = aftermine_sim::PlayerId(0);
    game.presentation.camera.center = Vec2::new(75.0, 30.0);
    assert!(
        audible_motors(&game.view()).is_empty(),
        "distant motors must fall silent"
    );
    game.presentation.camera.center = Vec2::new(26.0, 12.0);

    let snapshot = (*game.state).clone();
    game.replace_state_after_jump(&snapshot);
    assert_eq!(audible_motors(&game.view())[0].key, original[0].key);
    while game.state.current_tick() <= arrival {
        game.presentation.sounds_pending.clear();
        game.do_tick();
        if game.state.current_tick() <= arrival {
            assert_eq!(
                audible_motors(&game.view()).len(),
                1,
                "motor died before arrival"
            );
        }
    }
    assert!(audible_motors(&game.view()).is_empty());
    assert!(
        game.presentation
            .sounds_pending
            .iter()
            .any(|(kind, _)| *kind == SoundKind::RocketImpact)
    );
    assert!(
        !game
            .presentation
            .sounds_pending
            .iter()
            .any(|(kind, _)| *kind == SoundKind::Artillery)
    );
}
