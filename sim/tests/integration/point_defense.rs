//! Point defense: a Mantle shoots down hostile shells aimed near it, one
//! charge each, while hitscan fire passes through. Public API only.

use crate::common;
use common::{cmd, open_arena, players, run, run_until, unit};

use aftermine_sim::scenario::{PlayerSpec, UnitSpec};
use aftermine_sim::{
    AttackTarget, Command, Event, PlayerId, ProjectileKind, Scenario, State, UnitId, UnitKind,
};
use chassis::grid::TilePos;

fn range(units: Vec<UnitSpec>) -> Scenario {
    Scenario {
        players: players(300),
        ..open_arena(24, 16, units)
    }
}

fn attack(state: &mut State, player: u8, units: Vec<UnitId>, target: AttackTarget) {
    state.tick(&[cmd(
        player,
        Command::Attack {
            units,
            target,
            queue: false,
        },
    )]);
}

fn charges(state: &State, id: UnitId) -> u8 {
    state
        .unit(id)
        .and_then(|unit| unit.point_defense)
        .map_or(0, |held| held.charges)
}

fn intercepts(events: &[Event], defender: UnitId) -> usize {
    events
        .iter()
        .filter(
            |event| matches!(event, Event::ShellIntercepted { defender: d, .. } if *d == defender),
        )
        .count()
}

fn landed(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, Event::ShellLanded { .. }))
        .count()
}

#[test]
fn a_mantle_shoots_down_the_shell_aimed_at_its_ward() {
    let mut state = range(vec![
        unit(0, UnitKind::Bombard, 3, 4),
        unit(0, UnitKind::Sentinel, 4, 4),
        unit(1, UnitKind::Scuttler, 10, 4),
        unit(1, UnitKind::Mantle, 11, 5),
    ])
    .build()
    .unwrap();
    let ids: Vec<UnitId> = state.units().iter().map(|u| u.id).collect();
    let (bombard, scuttler, mantle) = (ids[0], ids[2], ids[3]);
    let hp = state.unit(scuttler).unwrap().hp;
    attack(&mut state, 0, vec![bombard], AttackTarget::Unit(scuttler));
    let mut events = run_until(&mut state, 300, |_, events| {
        events
            .iter()
            .any(|event| matches!(event, Event::ShellLaunched { .. }))
    });
    events.extend(run_until(&mut state, 100, |state, _| {
        state.shells().is_empty()
    }));
    assert_eq!(intercepts(&events, mantle), 1);
    assert_eq!(landed(&events), 0, "a stopped shell never lands");
    assert_eq!(state.unit(scuttler).unwrap().hp, hp);
    assert_eq!(charges(&state, mantle), 2);
    let Some(Event::ShellIntercepted { at, impact, .. }) = events
        .iter()
        .find(|event| matches!(event, Event::ShellIntercepted { .. }))
    else {
        unreachable!("counted above");
    };
    let reach = UnitKind::Mantle.stats().point_defense.unwrap().radius;
    let pos = state.unit(mantle).unwrap().pos;
    assert!(pos.dist_sq(*at) <= reach * reach);
    assert!(pos.dist_sq(*impact) <= reach * reach);
}

#[test]
fn one_bombard_never_outpaces_a_mantles_recharge() {
    let mut state = range(vec![
        unit(0, UnitKind::Bombard, 3, 4),
        unit(0, UnitKind::Sentinel, 4, 4),
        unit(1, UnitKind::Mantle, 10, 4),
    ])
    .build()
    .unwrap();
    let ids: Vec<UnitId> = state.units().iter().map(|u| u.id).collect();
    let (bombard, mantle) = (ids[0], ids[2]);
    attack(&mut state, 0, vec![bombard], AttackTarget::Unit(mantle));
    let events = run(&mut state, 1_200);
    assert!(intercepts(&events, mantle) >= 5, "the gun kept firing");
    assert_eq!(landed(&events), 0);
    let mantle = state.unit(mantle).unwrap();
    assert_eq!(mantle.hp, UnitKind::Mantle.stats().max_hp);
}

#[test]
fn massed_shells_spend_the_charges_and_the_rest_land() {
    let mut units = vec![unit(1, UnitKind::Mantle, 12, 7)];
    for y in 4..10 {
        units.push(unit(0, UnitKind::Bombard, 3, y));
    }
    units.push(unit(0, UnitKind::Sentinel, 6, 7));
    let mut state = range(units).build().unwrap();
    let mantle = state.units()[0].id;
    let guns: Vec<UnitId> = state.units()[1..7].iter().map(|u| u.id).collect();
    attack(&mut state, 0, guns, AttackTarget::Unit(mantle));
    let events = run(&mut state, 400);
    let stopped = intercepts(&events, mantle);
    assert!(stopped >= 3, "every charge fired: {stopped}");
    assert!(landed(&events) > 0, "six guns outrun three charges");
    assert!(state.unit(mantle).is_none_or(|unit| unit.hp < 350));
}

#[test]
fn gunfire_passes_through_point_defense() {
    let mut state = range(vec![
        unit(0, UnitKind::Lancer, 5, 4),
        unit(1, UnitKind::Scuttler, 10, 4),
        unit(1, UnitKind::Mantle, 10, 5),
    ])
    .build()
    .unwrap();
    let ids: Vec<UnitId> = state.units().iter().map(|u| u.id).collect();
    let (lancer, scuttler, mantle) = (ids[0], ids[1], ids[2]);
    let hp = state.unit(scuttler).unwrap().hp;
    attack(&mut state, 0, vec![lancer], AttackTarget::Unit(scuttler));
    let events = run_until(&mut state, 300, |state, _| {
        state.unit(scuttler).is_none_or(|unit| unit.hp < hp)
    });
    assert_eq!(intercepts(&events, mantle), 0);
    assert_eq!(charges(&state, mantle), 3);
}

#[test]
fn a_mantle_covers_its_teammates() {
    let mut scenario = range(vec![
        unit(0, UnitKind::Bombard, 3, 4),
        unit(0, UnitKind::Sentinel, 4, 4),
        unit(1, UnitKind::Scuttler, 10, 4),
        unit(2, UnitKind::Mantle, 11, 5),
    ]);
    scenario.players = vec![
        PlayerSpec {
            team: Some(0),
            ..scenario.players[0].clone()
        },
        PlayerSpec {
            team: Some(1),
            ..scenario.players[1].clone()
        },
        PlayerSpec {
            name: "Ally".into(),
            team: Some(1),
            ..scenario.players[1].clone()
        },
    ];
    scenario.map[12].replace_range(1..2, "3");
    let mut state = scenario.build().unwrap();
    let ids: Vec<UnitId> = state.units().iter().map(|u| u.id).collect();
    let (bombard, scuttler, mantle) = (ids[0], ids[2], ids[3]);
    attack(&mut state, 0, vec![bombard], AttackTarget::Unit(scuttler));
    let mut events = run_until(&mut state, 300, |_, events| {
        events
            .iter()
            .any(|event| matches!(event, Event::ShellLaunched { .. }))
    });
    events.extend(run_until(&mut state, 100, |state, _| {
        state.shells().is_empty()
    }));
    assert_eq!(intercepts(&events, mantle), 1);
    assert_eq!(state.unit(mantle).unwrap().player, PlayerId(2));
}

#[test]
fn a_condor_pass_costs_one_charge() {
    let mut state = Scenario {
        players: players(1_000),
        ..open_arena(
            24,
            12,
            vec![
                unit(0, UnitKind::Condor, 5, 4),
                unit(0, UnitKind::Kestrel, 13, 3),
                unit(1, UnitKind::Mantle, 16, 6),
            ],
        )
    }
    .build()
    .unwrap();
    let ids: Vec<UnitId> = state.units().iter().map(|u| u.id).collect();
    let (condor, mantle) = (ids[0], ids[2]);
    state.tick(&[]);
    attack(&mut state, 0, vec![condor], AttackTarget::Unit(mantle));
    let events = run_until(&mut state, 900, |_, events| {
        events.iter().any(|event| {
            matches!(
                event,
                Event::ShellIntercepted {
                    kind: ProjectileKind::Bomb,
                    ..
                }
            )
        })
    });
    assert_eq!(intercepts(&events, mantle), 1);
    assert_eq!(charges(&state, mantle), 2);
    assert_eq!(
        state.unit(mantle).unwrap().hp,
        UnitKind::Mantle.stats().max_hp
    );
}

#[test]
fn a_carried_mantle_freezes_its_recharge_and_keeps_it_through_saves() {
    let mut state = range(vec![
        unit(0, UnitKind::Mantle, 6, 6),
        unit(0, UnitKind::Skyhook, 7, 6),
    ])
    .build()
    .unwrap();
    let (mantle, sky) = (state.units()[0].id, state.units()[1].id);
    let mut doc = serde_json::to_value(&state).unwrap();
    doc["units"][0]["point_defense"] = serde_json::json!({"charges": 1, "recharge": 10});
    state = serde_json::from_value(doc).unwrap();
    state.tick(&[cmd(
        0,
        Command::Load {
            units: vec![mantle],
            transport: sky,
            queue: false,
        },
    )]);
    run_until(&mut state, 200, |state, _| state.unit(mantle).is_none());
    let aboard = state.unit(sky).unwrap().cargo[0].point_defense.unwrap();
    run(&mut state, 200);
    assert_eq!(
        state.unit(sky).unwrap().cargo[0].point_defense,
        Some(aboard),
        "nothing recharges aboard"
    );
    let restored: State = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    assert_eq!(restored.hash(), state.hash());
    state.tick(&[cmd(
        0,
        Command::Unload {
            transport: sky,
            at: TilePos::new(12, 6),
            queue: false,
        },
    )]);
    run_until(&mut state, 400, |state, _| state.unit(mantle).is_some());
    assert_eq!(
        state.unit(mantle).unwrap().point_defense,
        Some(aboard),
        "it sets down as it boarded"
    );
    state.tick(&[]);
    let walking = state.unit(mantle).unwrap().point_defense.unwrap();
    assert_eq!(
        (walking.charges, walking.recharge),
        (aboard.charges, aboard.recharge + 1),
        "and recharges again once walking"
    );
    let restored: State = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    assert_eq!(restored.hash(), state.hash());
}

#[test]
fn mirrored_point_defense_resolves_mirrored() {
    // 24x16 with the arena's Foundries rotated about the centre; every
    // staged machine is rotated the same way.
    let pair = |player: u8, kind: UnitKind, x: i32, y: i32| {
        [
            unit(player, kind, x, y),
            unit(1 - player, kind, 23 - x, 15 - y),
        ]
    };
    let mut units = Vec::new();
    for (player, kind, x, y) in [
        (0, UnitKind::Bombard, 3, 3),
        (0, UnitKind::Bombard, 3, 5),
        (0, UnitKind::Sentinel, 5, 4),
        (1, UnitKind::Mantle, 12, 4),
    ] {
        units.extend(pair(player, kind, x, y));
    }
    let mut state = range(units).build().unwrap();
    let id = |state: &State, player: u8, kind: UnitKind| -> Vec<UnitId> {
        state
            .units()
            .iter()
            .filter(|u| u.player == PlayerId(player) && u.kind == kind)
            .map(|u| u.id)
            .collect()
    };
    let west_guns = id(&state, 0, UnitKind::Bombard);
    let east_guns = id(&state, 1, UnitKind::Bombard);
    let west_mantle = id(&state, 0, UnitKind::Mantle)[0];
    let east_mantle = id(&state, 1, UnitKind::Mantle)[0];
    state.tick(&[
        cmd(
            0,
            Command::Attack {
                units: west_guns,
                target: AttackTarget::Unit(east_mantle),
                queue: false,
            },
        ),
        cmd(
            1,
            Command::Attack {
                units: east_guns,
                target: AttackTarget::Unit(west_mantle),
                queue: false,
            },
        ),
    ]);
    let events = run(&mut state, 900);
    assert!(intercepts(&events, east_mantle) > 0);
    assert_eq!(
        intercepts(&events, east_mantle),
        intercepts(&events, west_mantle)
    );
    let (west, east) = (
        state.unit(west_mantle).unwrap(),
        state.unit(east_mantle).unwrap(),
    );
    assert_eq!(west.hp, east.hp);
    assert_eq!(west.point_defense, east.point_defense);
}
