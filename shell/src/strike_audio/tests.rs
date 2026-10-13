use super::*;
use crate::game::{Effect, Game};
use macroquad::prelude::Vec2;

#[test]
fn a_warhead_motor_rises_under_the_launch_and_dies_as_the_dive_begins() {
    assert_eq!(warhead_envelope(100.0, 100.0, 0.0), 0.0);
    assert!(warhead_envelope(110.0, 100.0, 0.05) > 0.0);
    assert_eq!(
        warhead_envelope(140.0, 100.0, 0.5),
        1.0,
        "full through the cruise"
    );
    let fading = warhead_envelope(400.0, 100.0, 0.83);
    assert!(fading > 0.0 && fading < 1.0, "fades as it burns out");
    assert_eq!(
        warhead_envelope(420.0, 100.0, 0.9),
        0.0,
        "silent through the dive"
    );
}

#[test]
fn an_interceptor_motor_sounds_only_during_its_climb() {
    assert_eq!(interceptor_envelope(-0.1), 0.0);
    assert_eq!(interceptor_envelope(0.0), 0.0);
    assert_eq!(interceptor_envelope(0.5), 1.0);
    assert_eq!(interceptor_envelope(1.0), 0.0);
}

/// West's loaded Spire on an open field with nobody else's eyes, the camera
/// over it.
fn spire_game() -> (Game, oxide_sim::BuildingId) {
    let scenario = serde_json::from_value(serde_json::json!({
        "name": "Strike audio", "mode": "sandbox", "map": vec![".".repeat(70); 20],
        "players": [
            {"name": "West", "scrap": 0, "bot": false},
            {"name": "East", "scrap": 0, "bot": false}
        ],
        "units": [],
        "buildings": [{"player": 0, "kind": "spire", "x": 4, "y": 8}]
    }))
    .unwrap();
    let mut game = Game::with_viewport(scenario, Vec2::new(1280.0, 800.0)).unwrap();
    let mut wire = serde_json::to_value(&*game.state).unwrap();
    let spire = wire["buildings"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|b| b["kind"] == serde_json::json!("spire"))
        .unwrap();
    spire["magazine"]["loaded"] = serde_json::json!(1);
    let id = oxide_sim::BuildingId(u32::try_from(spire["id"].as_u64().unwrap()).unwrap());
    let state: oxide_sim::State = serde_json::from_value(wire).unwrap();
    game.replace_state_after_jump(&state);
    game.presentation.camera.center = Vec2::new(5.0, 9.0);
    (game, id)
}

#[test]
fn an_own_warhead_motor_follows_it_anywhere_and_a_hostile_one_only_in_sight() {
    let (mut game, spire) = spire_game();
    game.issue(oxide_sim::Command::Launch {
        building: spire,
        // Straight east, so the deck is already lined up.
        target: chassis::grid::TilePos::new(64, 9),
    });
    game.do_tick();
    assert_eq!(game.state.warheads().len(), 1);
    for _ in 0..6 {
        game.do_tick();
    }
    let near = warhead_motors(&game.view());
    assert_eq!(near.len(), 1, "seen over the silo's own sight");
    assert_eq!(near[0].key, game.state.warheads()[0].id);
    for _ in 0..80 {
        game.do_tick();
    }
    let line = game.state.known_warheads(game.presentation.human)[0]
        .line
        .unwrap();
    let tick = game.state.current_tick().saturating_sub(1) as f32;
    game.presentation.camera.center = warhead_track(line, tick).0;
    assert_eq!(
        warhead_motors(&game.view()).len(),
        1,
        "an own warhead is heard out of sight, as it is drawn"
    );
    game.presentation.human = oxide_sim::PlayerId(1);
    assert!(
        warhead_motors(&game.view()).is_empty(),
        "a hostile one is heard only over ground in sight"
    );
    game.presentation.overlay = true;
    assert_eq!(
        warhead_motors(&game.view()).len(),
        1,
        "the omniscient view hears it"
    );
}

fn interception(
    line: Option<oxide_sim::strike::WarheadLine>,
    pod: Option<Vec2>,
    at: Vec2,
    completed_tick: u64,
) -> Effect {
    Effect {
        kind: EffectKind::Interception {
            line,
            warhead_player: Some(oxide_sim::PlayerId(1)),
            warhead_hostile: true,
            pod,
            battery_player: oxide_sim::PlayerId(0),
            at,
            seed: 7,
            completed_tick,
            burst_heard: false,
            early: false,
        },
        age: 0.0,
    }
}

#[test]
fn a_stopped_warhead_keeps_its_motor_until_the_interceptor_reaches_it() {
    let (mut game, _) = spire_game();
    for _ in 0..40 {
        game.do_tick();
    }
    let now = game.state.current_tick();
    // A line over the silo's own sight, a short way into its flight.
    let line = oxide_sim::strike::WarheadLine {
        launch: chassis::fx::Vec2Fx::new(chassis::fx::Fx::lit("5"), chassis::fx::Fx::lit("9")),
        impact: chassis::fx::Vec2Fx::new(chassis::fx::Fx::lit("40"), chassis::fx::Fx::lit("9")),
        arrival: now + 150,
    };
    let at = warhead_track(line, now as f32).0;
    game.presentation.fx.push(interception(
        Some(line),
        Some(Vec2::new(6.0, 10.0)),
        at,
        now - 2,
    ));
    let flying = warhead_motors(&game.view());
    assert_eq!(flying.len(), 1);
    assert_eq!(
        flying[0].key,
        WarheadId(7),
        "the same voice as the warhead before it was stopped"
    );
    assert_eq!(
        interceptor_motors(&game.view()).len(),
        1,
        "the interceptor climbs"
    );
    for _ in 0..=u64::from(crate::numeric::to_u8(INTERCEPT_MEET / TICK_DT)) {
        game.do_tick();
    }
    assert!(
        warhead_motors(&game.view()).is_empty(),
        "silent once they meet"
    );
    assert!(interceptor_motors(&game.view()).is_empty());
}

#[test]
fn an_unseen_hostile_interceptor_climbs_in_silence_and_an_own_one_is_heard() {
    let (mut game, _) = spire_game();
    for _ in 0..5 {
        game.do_tick();
    }
    let now = game.state.current_tick();
    let far = Vec2::new(60.0, 9.0);
    game.presentation.camera.center = far;
    let mut hostile = interception(None, Some(far), far, now - 2);
    if let EffectKind::Interception { battery_player, .. } = &mut hostile.kind {
        *battery_player = oxide_sim::PlayerId(1);
    }
    game.presentation.fx.push(hostile);
    assert!(interceptor_motors(&game.view()).is_empty());
    game.presentation
        .fx
        .push(interception(None, Some(far), far, now - 2));
    assert_eq!(
        interceptor_motors(&game.view()).len(),
        1,
        "the viewer's own interceptor is heard out of sight"
    );
    game.presentation.fx.pop();
    game.presentation.overlay = true;
    assert_eq!(interceptor_motors(&game.view()).len(), 1);
}
