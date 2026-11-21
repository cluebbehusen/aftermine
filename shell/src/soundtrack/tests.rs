use super::*;

fn settle(layers: &mut Layers, scene: Scene, guns: usize, special: bool) -> Volumes {
    let mut volumes = layers.update(scene, guns, special, 0.0);
    for _ in 0..200 {
        volumes = layers.update(scene, guns, special, 0.1);
    }
    volumes
}

#[test]
fn a_skirmish_leaves_the_music_calm_and_a_real_fight_brings_the_fight_layer() {
    let mut layers = Layers::default();
    let skirmish = settle(&mut layers, Scene::Match, CALM_GUNS, false);
    assert_eq!((skirmish.calm, skirmish.fight), (1.0, 0.0));

    let middling = settle(&mut layers, Scene::Match, CALM_GUNS + 3, false);
    assert!(
        middling.fight > 0.0 && middling.fight < 1.0,
        "a middling fight blends the two"
    );
    assert!(
        (middling.calm + middling.fight - 1.0).abs() < 1e-6,
        "the two layers of one song share the level"
    );

    let battle = settle(&mut layers, Scene::Match, FIGHT_GUNS + 5, false);
    assert_eq!((battle.calm, battle.fight), (0.0, 1.0));
    assert!(!layers.calm());
    let after = settle(&mut layers, Scene::Match, 0, false);
    assert_eq!((after.calm, after.fight), (1.0, 0.0));
    assert!(layers.calm());
}

#[test]
fn the_fight_layer_rises_over_seconds_and_falls_slower() {
    let mut layers = Layers::default();
    let mut rise = 0.0;
    let mut volumes = layers.update(Scene::Match, FIGHT_GUNS, false, 0.1);
    while volumes.fight < 1.0 {
        rise += 0.1;
        volumes = layers.update(Scene::Match, FIGHT_GUNS, false, 0.1);
    }
    let mut fall = 0.0;
    while volumes.fight > 0.0 {
        fall += 0.1;
        volumes = layers.update(Scene::Match, 0, false, 0.1);
    }
    assert!((2.5..3.5).contains(&rise), "rises in {rise} s");
    assert!((5.5..6.5).contains(&fall), "falls in {fall} s");
}

#[test]
fn a_special_section_holds_the_fight_layer_without_a_fight() {
    let mut layers = Layers::default();
    let held = settle(&mut layers, Scene::Match, 0, true);
    assert_eq!((held.calm, held.fight), (0.0, 1.0));
}

#[test]
fn pause_ducks_the_music_without_silencing_it() {
    let mut layers = Layers::default();
    settle(&mut layers, Scene::Match, 0, false);
    let paused = settle(&mut layers, Scene::Pause, 0, false);
    assert_eq!(paused.calm, PAUSE_DUCK);
    assert_eq!(paused.solo, PAUSE_DUCK);
    let back = settle(&mut layers, Scene::Match, 0, false);
    assert_eq!(back.calm, 1.0);
}

#[test]
fn every_scene_has_its_music() {
    assert_eq!(Scene::Menu.role(), Role::Menu);
    assert_eq!(Scene::Match.role(), Role::Match);
    assert_eq!(Scene::Pause.role(), Role::Match, "pause keeps the song");
    assert_eq!(Scene::Result.role(), Role::Draw);
    assert_eq!(Scene::Victory.role(), Role::Victory);
    assert_eq!(Scene::Defeat.role(), Role::Defeat);
}

#[test]
fn a_stray_hit_is_no_siege_but_sustained_fire_is_and_it_fades() {
    let mut pressure = Pressure::default();
    pressure.structure_hit(0.0);
    assert!(!pressure.cues(0.0, false).siege);
    for step in 1..=8 {
        pressure.structure_hit(step as f32 * 0.5);
    }
    assert!(
        pressure.cues(4.0, false).siege,
        "two hits a second for four seconds"
    );
    assert!(
        !pressure.cues(4.0, true).siege,
        "a spectator defends no seat"
    );
    assert!(
        !pressure.cues(20.0, false).siege,
        "it passes once the guns stop"
    );
}

#[test]
fn a_final_push_builds_like_a_siege() {
    let mut pressure = Pressure::default();
    for step in 0..10 {
        pressure.push_hit(step as f32 * 0.5);
    }
    let cues = pressure.cues(4.5, false);
    assert!(cues.final_push && !cues.siege);
}

#[test]
fn a_big_battle_needs_many_guns_heard_lately() {
    let mut pressure = Pressure::default();
    let gun = |id: u32| aftermine_sim::Target::Unit(aftermine_sim::UnitId(id));
    for id in 0..u32::try_from(BATTLE_GUNS).unwrap() - 1 {
        pressure.gun_heard(gun(id), 1.0);
        pressure.gun_heard(gun(id), 2.0);
    }
    let cues = pressure.cues(2.0, false);
    assert_eq!(cues.guns, BATTLE_GUNS - 1, "the same guns count once");
    assert!(!cues.big_battle);
    pressure.gun_heard(
        aftermine_sim::Target::Building(aftermine_sim::BuildingId(0)),
        2.0,
    );
    assert!(
        pressure.cues(2.0, false).big_battle,
        "a turret is a gun beside the units"
    );
    assert!(
        pressure.cues(2.0, true).big_battle,
        "a replay hears battles too"
    );
    let later = pressure.cues(2.0 + BATTLE_SECS, false);
    assert_eq!((later.guns, later.big_battle), (0, false));
    for id in 1000..1300 {
        pressure.gun_heard(gun(id), id as f32);
    }
    assert!(
        pressure.guns.len() <= 4 * BATTLE_GUNS + 1,
        "guns heard long ago are forgotten"
    );
}

#[test]
fn soundtrack_context_tracks_pause_victory_and_surrender() {
    let mut won = crate::game::Game::new(aftermine_sim::Scenario::skirmish()).expect("game");
    assert_eq!(match_scene(&won.view(), false), Scene::Match);
    assert_eq!(match_scene(&won.view(), true), Scene::Pause);
    won.state.tick(&[aftermine_sim::PlayerCommand {
        player: aftermine_sim::PlayerId(1),
        command: aftermine_sim::Command::Surrender,
    }]);
    assert_eq!(match_scene(&won.view(), false), Scene::Victory);

    let mut lost = crate::game::Game::new(aftermine_sim::Scenario::skirmish()).expect("game");
    lost.state.tick(&[aftermine_sim::PlayerCommand {
        player: lost.presentation.human,
        command: aftermine_sim::Command::Surrender,
    }]);
    assert_eq!(
        match_scene(&lost.view(), false),
        Scene::Defeat,
        "a surrender that loses the match scores as a defeat"
    );
}

#[test]
fn a_loading_screen_holds_the_music_and_a_new_match_starts_from_the_top() {
    let (mut held, mut fresh) = (Scene::Menu, false);
    assert_eq!(follow(&mut held, &mut fresh, None), (Scene::Menu, false));
    fresh = true;
    assert_eq!(
        follow(&mut held, &mut fresh, None),
        (Scene::Menu, false),
        "loading from Home keeps the menu music"
    );
    assert!(fresh, "the new match waits for its screen");
    assert_eq!(
        follow(&mut held, &mut fresh, Some(Scene::Match)),
        (Scene::Match, true)
    );
    assert_eq!(
        follow(&mut held, &mut fresh, Some(Scene::Match)),
        (Scene::Match, false)
    );
    assert_eq!(
        follow(&mut held, &mut fresh, Some(Scene::Pause)),
        (Scene::Pause, false)
    );
    assert_eq!(
        follow(&mut held, &mut fresh, None),
        (Scene::Pause, false),
        "saving from Pause keeps the match ducked"
    );
}
