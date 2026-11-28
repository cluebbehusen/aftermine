//! Direct rounds out of the fog: a team that sees where a round lands but not
//! the gun that fired it remembers where the round came into its sight, the
//! way it came, and what fired it, and never where the gun stood.

use crate::common;
use common::{building_id, cmd, open_arena_with, orders, run_until, unit};

use aftermine_sim::observation::fog_shots;
use aftermine_sim::scenario::{BuildingSpec, PlayerSpec, ScenarioMode, UnitSpec};
use aftermine_sim::stats::{BuildingKind, FOG_SHOT_MEMORY_TICKS};
use aftermine_sim::vision::Shooter;
use aftermine_sim::{Event, PlayerId, Scenario, State, Target, UnitKind};
use chassis::compass::CompassPoint;
use chassis::fx::Vec2Fx;
use chassis::grid::TilePos;

/// West's Barricade, which sees a tile around it, among `units`, far out
/// of its Foundry's sight.
fn barricaded(units: Vec<UnitSpec>) -> State {
    let mut scenario = open_arena_with(32, 12, units, |_| {});
    scenario.buildings = vec![BuildingSpec {
        player: 0,
        kind: BuildingKind::Barricade,
        x: 14,
        y: 5,
    }];
    scenario.build().unwrap()
}

/// Has the first unit of `state` attack West's Barricade, and runs to its
/// first round: the muzzle and the impact point.
fn first_round(state: &mut State) -> (Vec2Fx, Vec2Fx) {
    let gun = state.units()[0].id;
    let wall = building_id(state, |b| b.kind == BuildingKind::Barricade);
    state.tick(&[cmd(1, orders::attack([gun], Target::Building(wall)))]);
    let events = run_until(state, 400, |_, events| {
        events
            .iter()
            .any(|event| matches!(event, Event::AttackHit { .. }))
    });
    events
        .iter()
        .find_map(|event| match event {
            Event::AttackHit {
                attacker_pos,
                target_pos,
                ..
            } => Some((*attacker_pos, *target_pos)),
            _ => None,
        })
        .expect("the gun fired")
}

#[test]
fn a_round_from_out_of_sight_is_remembered_where_it_came_into_sight() {
    let mut state = barricaded(vec![unit(1, UnitKind::Lancer, 19, 5)]);
    let (muzzle, aim) = first_round(&mut state);
    assert!(
        !state.can_see(PlayerId(0), TilePos::containing(muzzle)),
        "premise: West does not see the Lancer"
    );
    let marks = fog_shots(&state, PlayerId(0));
    assert_eq!(marks.len(), 1, "{marks:?}");
    let mark = marks[0];
    assert_eq!(mark.impact, TilePos::containing(aim));
    assert_eq!(mark.shooter, Shooter::Unit(UnitKind::Lancer));
    assert_eq!(mark.from, CompassPoint::of(muzzle - aim));
    assert_eq!(mark.at, state.current_tick() - 1);
    assert_eq!(
        Some(mark.entry),
        chassis::path::first_along(muzzle, aim, |tile| state.can_see(PlayerId(0), tile)),
        "it enters sight at the first seen tile of its line"
    );
    assert!(
        fog_shots(&state, PlayerId(1)).is_empty(),
        "the shooter's own seat remembers nothing"
    );
}

#[test]
fn a_round_from_a_gun_in_sight_leaves_no_mark() {
    let mut state = barricaded(vec![
        unit(1, UnitKind::Lancer, 19, 5),
        unit(0, UnitKind::Harvester, 20, 8),
    ]);
    let (muzzle, _) = first_round(&mut state);
    assert!(
        state.can_see(PlayerId(0), TilePos::containing(muzzle)),
        "premise: West's Harvester watches the Lancer"
    );
    assert!(fog_shots(&state, PlayerId(0)).is_empty());
}

#[test]
fn rounds_along_one_line_coalesce_and_are_forgotten_once_they_stop() {
    let mut state = barricaded(vec![unit(1, UnitKind::Lancer, 19, 5)]);
    let lancer = state.units()[0].id;
    first_round(&mut state);
    run_until(&mut state, 400, |_, events| {
        events
            .iter()
            .any(|event| matches!(event, Event::AttackHit { .. }))
    });
    let marks = fog_shots(&state, PlayerId(0));
    assert_eq!(
        marks.len(),
        1,
        "two rounds on one line are one mark: {marks:?}"
    );
    assert_eq!(marks[0].at, state.current_tick() - 1, "dated by the later");
    state.tick(&[cmd(1, orders::run([lancer], TilePos::new(28, 5)))]);
    for _ in 0..FOG_SHOT_MEMORY_TICKS {
        state.tick(&[]);
    }
    assert!(fog_shots(&state, PlayerId(0)).is_empty());
}

#[test]
fn teammates_remember_the_same_rounds() {
    let seat = |name: &str, team| PlayerSpec {
        name: name.into(),
        team: Some(team),
        scrap: 0,
        bot: false,
        bot_config: None,
    };
    let mut map = vec!["########################".to_string()];
    map.push("#1..................3..#".into());
    for _ in 0..9 {
        map.push("#......................#".into());
    }
    map.push("#2..................4..#".into());
    map.push("#......................#".into());
    map.push("########################".into());
    let state = Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Match,
        name: "fog-shots".into(),
        map,
        players: vec![
            seat("North West", 0),
            seat("South West", 0),
            seat("South East", 1),
            seat("North East", 1),
        ],
        units: vec![unit(2, UnitKind::Lancer, 17, 6)],
        buildings: vec![BuildingSpec {
            player: 0,
            kind: BuildingKind::Barricade,
            x: 12,
            y: 6,
        }],
        meta: None,
    }
    .build()
    .unwrap();
    let mut state = state;
    let lancer = state
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Lancer)
        .unwrap()
        .id;
    let wall = building_id(&state, |b| b.kind == BuildingKind::Barricade);
    state.tick(&[cmd(2, orders::attack([lancer], Target::Building(wall)))]);
    run_until(&mut state, 400, |_, events| {
        events
            .iter()
            .any(|event| matches!(event, Event::AttackHit { .. }))
    });
    let west = fog_shots(&state, PlayerId(0));
    assert!(!west.is_empty(), "the Lancer fired out of West's sight");
    assert_eq!(west, fog_shots(&state, PlayerId(1)));
    assert!(fog_shots(&state, PlayerId(3)).is_empty());
}
