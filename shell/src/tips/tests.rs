use super::*;
use crate::tutorial::{Lesson, Tutorial};
use macroquad::prelude::vec2;
use oxide_sim::Command;

const W: usize = 60;
const H: usize = 20;
/// The player's Foundry anchor; the enemy's sits far out of sight.
const HOME: (usize, usize) = (4, 8);
const AWAY: (usize, usize) = (54, 8);
/// A scrap pile near the player's Foundry, in a Harvester's sight from
/// (8, 12).
const PILE: (usize, usize, char) = (12, 12, 's');

/// A two-seat match on open ground: `marks` adds map glyphs, `units`
/// places (seat, kind, x, y). Nobody runs a bot. One tick in, so sight
/// is up.
fn match_with(marks: &[(usize, usize, char)], scrap: u32, units: &[(u8, &str, i32, i32)]) -> Game {
    let mut rows = vec![vec!['.'; W]; H];
    rows[HOME.1][HOME.0] = '1';
    rows[AWAY.1][AWAY.0] = '2';
    for &(x, y, glyph) in marks {
        rows[y][x] = glyph;
    }
    let scenario: oxide_sim::Scenario = serde_json::from_value(serde_json::json!({
        "name": "Tip fixture",
        "map": rows.into_iter().map(String::from_iter).collect::<Vec<_>>(),
        "players": [
            {"name": "You", "scrap": scrap, "bot": false},
            {"name": "Them", "scrap": 0, "bot": false}
        ],
        "units": units
            .iter()
            .map(|(player, kind, x, y)| serde_json::json!({"player": player, "kind": kind, "x": x, "y": y}))
            .collect::<Vec<_>>(),
    }))
    .unwrap();
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
    game.do_tick();
    game
}

fn seat(game: &Game) -> Seat<'_> {
    Seat {
        state: &game.state,
        human: game.presentation.human,
        alerts: &game.presentation.alerts,
    }
}

/// A watch that has read `game` at its current tick.
fn watched(game: &Game) -> Watch {
    let mut watch = Watch::default();
    watch.update(&seat(game), game.state.current_tick());
    watch
}

/// Whether `tip` is due `after` ticks past the game's current tick, by a
/// watch that last read the game now.
fn due_after(game: &Game, watch: &Watch, tip: Tip, after: Tick) -> bool {
    tip.due(&seat(game), watch, game.state.current_tick() + after)
}

fn unit(game: &Game, player: u8, kind: UnitKind) -> UnitId {
    game.state
        .units()
        .iter()
        .find(|u| u.player == PlayerId(player) && u.kind == kind)
        .unwrap()
        .id
}

/// Rewrites the world through its serialized form, as a save could.
fn poke(game: &mut Game, edit: impl FnOnce(&mut serde_json::Value)) {
    let mut value = serde_json::to_value(&*game.state).unwrap();
    edit(&mut value);
    game.replace_state_after_jump(&serde_json::from_value(value).unwrap());
}

/// Sets the player's Foundry to `percent` of its health.
fn damage_foundry(game: &mut Game, percent: u32) {
    let max = BuildingKind::Foundry.base_stats().max_hp;
    poke(game, |value| {
        let foundry = value["buildings"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|b| b["player"] == 0)
            .unwrap();
        foundry["hp"] = serde_json::json!(max * percent / 100);
    });
}

fn place_wreck(game: &mut Game, x: usize, y: usize) {
    poke(game, |value| {
        value["map"]["grid"]["cells"][y * W + x]["wreck"] = serde_json::json!(20);
    });
}

#[test]
fn an_attack_alert_brings_the_attack_tip_and_an_assembly_alone_does_not() {
    let mut game = match_with(&[], 0, &[]);
    let watch = watched(&game);
    assert!(!due_after(&game, &watch, Tip::UnderAttack, 0));
    game.presentation
        .alerts
        .push((vec2(9.0, 9.0), 0.0, AlertKind::Assembly(UnitKind::Maw)));
    assert!(!due_after(&game, &watch, Tip::UnderAttack, 0));
    game.presentation
        .alerts
        .push((vec2(9.0, 9.0), 0.0, AlertKind::Attack));
    assert!(due_after(&game, &watch, Tip::UnderAttack, 0));
}

#[test]
fn enemy_aircraft_bring_the_air_tip_only_in_sight_and_only_without_anti_air() {
    let due = |units: &[(u8, &str, i32, i32)]| {
        let game = match_with(&[], 0, units);
        due_after(&game, &watched(&game), Tip::EnemyAircraft, 0)
    };
    assert!(due(&[(0, "harvester", 8, 12), (1, "buzzard", 9, 9)]));
    assert!(
        !due(&[(0, "harvester", 8, 12), (1, "buzzard", 40, 9)]),
        "an aircraft out of sight stays hidden"
    );
    assert!(
        !due(&[(0, "flakhound", 8, 12), (1, "buzzard", 9, 9)]),
        "a Flakhound can shoot it"
    );
    assert!(
        !due(&[(0, "harvester", 8, 12), (1, "sentinel", 9, 9)]),
        "a ground machine is no aircraft"
    );

    let mut game = match_with(&[], 200, &[(0, "harvester", 8, 12), (1, "buzzard", 9, 9)]);
    let harvester = unit(&game, 0, UnitKind::Harvester);
    game.issue(Command::Build {
        units: vec![harvester],
        kind: BuildingKind::FlakTurret,
        anchor: TilePos::new(8, 14),
        queue: false,
        defer: false,
    });
    game.do_tick();
    assert!(
        !due_after(&game, &watched(&game), Tip::EnemyAircraft, 0),
        "a Flak Turret going up answers it"
    );
}

#[test]
fn a_harvester_idle_for_ten_seconds_brings_the_idle_tip() {
    let mut game = match_with(&[PILE], 0, &[(0, "harvester", 8, 12)]);
    let mut watch = watched(&game);
    assert!(!due_after(
        &game,
        &watch,
        Tip::IdleHarvesters,
        IDLE_WAIT - 1
    ));
    assert!(due_after(&game, &watch, Tip::IdleHarvesters, IDLE_WAIT));

    let harvester = unit(&game, 0, UnitKind::Harvester);
    game.issue(Command::Harvest {
        units: vec![harvester],
        node: TilePos::new(12, 12),
        queue: false,
    });
    game.do_tick();
    watch.update(&seat(&game), game.state.current_tick());
    assert!(
        !due_after(&game, &watch, Tip::IdleHarvesters, 10 * IDLE_WAIT),
        "a Harvester at work isn't idle"
    );
}

#[test]
fn the_idle_tip_needs_safe_scrap_in_sight_near_a_foundry() {
    let due = |marks: &[(usize, usize, char)], units: &[(u8, &str, i32, i32)]| {
        let game = match_with(marks, 0, units);
        due_after(&game, &watched(&game), Tip::IdleHarvesters, IDLE_WAIT)
    };
    let idle = (0, "harvester", 8, 12);
    assert!(due(&[PILE], &[idle]));
    assert!(!due(&[], &[idle]), "no scrap is left");
    assert!(
        !due(&[(30, 12, 's')], &[(0, "harvester", 28, 12)]),
        "the only pile in sight is far from the Foundry"
    );

    let hidden = (16, 1, 's');
    let game = match_with(&[hidden], 0, &[idle]);
    let human = game.presentation.human;
    assert!(!game.state.can_see(human, TilePos::new(16, 1)));
    assert!(
        !due(&[hidden], &[idle]),
        "a pile near the Foundry but out of sight doesn't count"
    );

    let guard = (1, "sentinel", 13, 12);
    let game = match_with(&[PILE], 0, &[idle, guard]);
    let sentinel = game.state.unit(unit(&game, 1, UnitKind::Sentinel)).unwrap();
    assert!(game.state.sees_unit(human, sentinel));
    assert!(
        !due(&[PILE], &[idle, guard]),
        "an enemy machine in sight guards the pile"
    );
}

#[test]
fn scrap_left_unspent_for_a_while_brings_the_spending_tip() {
    // Enough that the bank still counts once a Harvester is paid for.
    let mut game = match_with(&[], UNSPENT_BANK + UnitKind::Harvester.stats().cost, &[]);
    let mut watch = watched(&game);
    assert!(!due_after(
        &game,
        &watch,
        Tip::UnspentScrap,
        UNSPENT_WAIT - 1
    ));
    assert!(due_after(&game, &watch, Tip::UnspentScrap, UNSPENT_WAIT));

    let foundry = game.home_foundry().unwrap().id;
    game.issue(Command::Train {
        building: foundry,
        kind: UnitKind::Harvester,
    });
    game.do_tick();
    watch.update(&seat(&game), game.state.current_tick());
    assert!(
        !due_after(&game, &watch, Tip::UnspentScrap, 10 * UNSPENT_WAIT),
        "scrap in training is spent"
    );

    let game = match_with(&[], UNSPENT_BANK - 1, &[]);
    assert!(!due_after(
        &game,
        &watched(&game),
        Tip::UnspentScrap,
        10 * UNSPENT_WAIT
    ));
}

#[test]
fn a_badly_damaged_building_nobody_welds_brings_the_weld_tip_once_the_fight_is_over() {
    let mut game = match_with(&[], 100, &[(0, "harvester", 8, 12)]);
    damage_foundry(&mut game, 70);
    assert!(
        !due_after(&game, &watched(&game), Tip::WeldDamage, 0),
        "light damage can wait"
    );
    damage_foundry(&mut game, 50);
    assert!(due_after(&game, &watched(&game), Tip::WeldDamage, 0));

    let harvester = unit(&game, 0, UnitKind::Harvester);
    let foundry = game.home_foundry().unwrap().id;
    game.issue(Command::Repair {
        units: vec![harvester],
        building: foundry,
        queue: false,
    });
    game.do_tick();
    assert!(
        !due_after(&game, &watched(&game), Tip::WeldDamage, 0),
        "a Harvester is on it"
    );

    let mut game = match_with(
        &[],
        100,
        &[(0, "harvester", 8, 12), (1, "harvester", 9, 14)],
    );
    damage_foundry(&mut game, 50);
    let watch = watched(&game);
    assert!(
        !due_after(&game, &watch, Tip::WeldDamage, QUIET - 1),
        "an enemy machine is in sight"
    );
    assert!(due_after(&game, &watch, Tip::WeldDamage, QUIET));
}

#[test]
fn a_wreck_in_sight_near_the_base_brings_the_wreck_tip() {
    let mut game = match_with(&[], 0, &[(0, "harvester", 8, 12)]);
    assert!(!due_after(&game, &watched(&game), Tip::Wrecks, 0));
    place_wreck(&mut game, 10, 10);
    assert!(due_after(&game, &watched(&game), Tip::Wrecks, 0));

    let harvester = unit(&game, 0, UnitKind::Harvester);
    game.issue(Command::Harvest {
        units: vec![harvester],
        node: TilePos::new(10, 10),
        queue: false,
    });
    game.do_tick();
    assert!(
        !due_after(&game, &watched(&game), Tip::Wrecks, 0),
        "a Harvester is collecting it"
    );

    let mut game = match_with(&[], 0, &[(0, "harvester", 8, 12), (0, "surveyor", 30, 10)]);
    place_wreck(&mut game, 31, 10);
    assert!(
        !due_after(&game, &watched(&game), Tip::Wrecks, 0),
        "a wreck in sight far from any building is not near the base"
    );

    let mut game = match_with(&[], 0, &[(0, "surveyor", 8, 12)]);
    place_wreck(&mut game, 10, 10);
    assert!(
        !due_after(&game, &watched(&game), Tip::Wrecks, 0),
        "nobody can collect it"
    );
}

#[test]
fn an_unclaimed_frame_seen_near_the_foundry_brings_the_extractor_tip() {
    let frame = (10, 9);
    let mut game = match_with(&[(frame.0, frame.1, 'E')], 500, &[(0, "harvester", 8, 12)]);
    let now = game.state.current_tick();
    let watch = watched(&game);
    assert!(!due_after(
        &game,
        &watch,
        Tip::ExtractorFrame,
        FRAMES_AFTER - now - 1
    ));
    assert!(due_after(
        &game,
        &watch,
        Tip::ExtractorFrame,
        FRAMES_AFTER - now
    ));

    let harvester = unit(&game, 0, UnitKind::Harvester);
    game.issue(Command::Build {
        units: vec![harvester],
        kind: BuildingKind::Extractor,
        anchor: TilePos::new(10, 9),
        queue: false,
        defer: false,
    });
    game.do_tick();
    assert!(
        !due_after(&game, &watched(&game), Tip::ExtractorFrame, FRAMES_AFTER),
        "the player's own claim answers it"
    );

    let game = match_with(
        &[(30, 9, 'E')],
        500,
        &[(0, "harvester", 8, 12), (0, "surveyor", 30, 12)],
    );
    assert!(
        game.my_vision().explored(TilePos::new(30, 9)),
        "the far frame is seen"
    );
    assert!(
        !due_after(&game, &watched(&game), Tip::ExtractorFrame, FRAMES_AFTER),
        "a frame far from the Foundry is not near the base"
    );
}

#[test]
fn a_player_who_never_saw_an_enemy_building_is_told_to_scout() {
    let game = match_with(&[], 0, &[(0, "surveyor", 8, 12)]);
    let now = game.state.current_tick();
    let watch = watched(&game);
    assert!(!due_after(
        &game,
        &watch,
        Tip::FindEnemy,
        SCOUT_AFTER - now - 1
    ));
    assert!(
        due_after(&game, &watch, Tip::FindEnemy, SCOUT_AFTER - now),
        "an enemy base out of sight stays unknown"
    );

    let game = match_with(&[], 0, &[(0, "harvester", 8, 12)]);
    assert!(
        !due_after(&game, &watched(&game), Tip::FindEnemy, SCOUT_AFTER),
        "there is no Surveyor to send"
    );

    let mut game = match_with(&[], 0, &[(0, "surveyor", 50, 9)]);
    let mut watch = watched(&game);
    let surveyor = unit(&game, 0, UnitKind::Surveyor);
    game.issue(Command::Run {
        units: vec![surveyor],
        goal: TilePos::new(8, 12),
        queue: false,
    });
    for _ in 0..30 * SECOND {
        game.do_tick();
    }
    assert!(
        !game
            .state
            .can_see(game.presentation.human, TilePos::new(54, 8)),
        "the Surveyor has left"
    );
    watch.update(&seat(&game), game.state.current_tick());
    assert!(
        !due_after(&game, &watch, Tip::FindEnemy, SCOUT_AFTER),
        "the enemy base was found once"
    );
}

/// Ticks `game` to `to`, reading tips after every tick, and returns each
/// tip with the tick it appeared on.
fn run(game: &mut Game, to: Tick, reduced: bool) -> Vec<(Tick, Tip)> {
    let mut appeared = Vec::new();
    while game.state.current_tick() < to {
        game.do_tick();
        let before = game.tips.showing();
        Tips::advance_in(game, true, reduced);
        if let Some(tip) = game.tips.showing()
            && before != Some(tip)
        {
            appeared.push((game.state.current_tick(), tip));
        }
    }
    appeared
}

#[test]
fn tips_wait_out_the_opening_keep_their_distance_and_show_once() {
    let mut game = match_with(&[PILE], 0, &[(0, "harvester", 8, 12)]);
    assert_eq!(run(&mut game, OPENING - 1, false), []);
    assert_eq!(
        run(&mut game, OPENING, false),
        [(OPENING, Tip::IdleHarvesters)]
    );
    game.presentation
        .alerts
        .push((vec2(9.0, 9.0), 0.0, AlertKind::Attack));
    assert_eq!(run(&mut game, OPENING + SHOWN - FADE, false), []);
    assert_eq!(game.tips.showing(), Some(Tip::IdleHarvesters));
    let now = game.state.current_tick();
    assert_eq!(game.tips.alpha(now, 0.0, false), 1.0);
    assert_eq!(
        game.tips.alpha(now + FADE / 2, 0.0, false),
        0.5,
        "the last second fades"
    );
    assert_eq!(run(&mut game, OPENING + SHOWN, false), []);
    assert_eq!(game.tips.showing(), None, "a tip leaves after its stay");
    assert_eq!(
        run(&mut game, OPENING + GAP, false),
        [(OPENING + GAP, Tip::UnderAttack)],
        "the next tip waits for the gap, and the Harvester still idle never shows again"
    );
    game.presentation.alerts.clear();
    assert_eq!(run(&mut game, OPENING + 3 * GAP, false), []);
}

#[test]
fn a_tip_fades_once_its_reason_passes_and_leaves_at_once_with_reduced_motion() {
    for reduced in [false, true] {
        let mut game = match_with(&[], 0, &[]);
        game.presentation
            .alerts
            .push((vec2(9.0, 9.0), 0.0, AlertKind::Attack));
        assert_eq!(
            run(&mut game, OPENING, reduced),
            [(OPENING, Tip::UnderAttack)]
        );
        game.presentation.alerts.clear();
        run(&mut game, OPENING + 1, reduced);
        let now = game.state.current_tick();
        if reduced {
            assert_eq!(game.tips.showing(), None);
        } else {
            assert_eq!(game.tips.showing(), Some(Tip::UnderAttack));
            assert_eq!(game.tips.alpha(now, 0.0, false), 1.0);
            assert_eq!(game.tips.alpha(now + FADE / 2, 0.0, false), 0.5);
            run(&mut game, now + FADE, reduced);
            assert_eq!(game.tips.showing(), None);
        }
    }
}

#[test]
fn a_closed_tip_stays_shown_and_still_holds_the_gap() {
    let mut game = match_with(&[PILE], 0, &[(0, "harvester", 8, 12)]);
    run(&mut game, OPENING, false);
    game.tips.dismiss();
    game.presentation
        .alerts
        .push((vec2(9.0, 9.0), 0.0, AlertKind::Attack));
    assert_eq!(
        run(&mut game, OPENING + GAP, false),
        [(OPENING + GAP, Tip::UnderAttack)]
    );
}

#[test]
fn tips_stay_away_when_turned_off_and_during_lessons() {
    let mut game = match_with(&[PILE], 0, &[(0, "harvester", 8, 12)]);
    run(&mut game, OPENING, false);
    assert_eq!(game.tips.showing(), Some(Tip::IdleHarvesters));
    Tips::advance_in(&mut game, false, false);
    assert_eq!(game.tips.showing(), None, "turning tips off hides one");
    for _ in 0..GAP {
        game.do_tick();
        Tips::advance_in(&mut game, false, false);
        assert_eq!(game.tips.showing(), None);
    }

    let mut game = Tutorial::begin_at(Lesson::Scrap, vec2(1280.0, 800.0)).unwrap();
    assert_eq!(run(&mut game, OPENING + GAP, false), []);
}
