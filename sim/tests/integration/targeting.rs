//! Victims a unit picks for itself: threats first, the danger a shot
//! removes, the half-second look, the volley skip, and fair ties. Public
//! API only.

use crate::common::*;
use aftermine_sim::{BuildingKind, Command, Event, State, Target, UnitId, UnitKind};
use chassis::grid::TilePos;

/// What `unit` is attacking, as its owner sees it.
fn attacking(state: &State, unit: UnitId) -> Option<Target> {
    let u = state.unit(unit)?;
    let aftermine_sim::Order::Attack { target, .. } = u.order else {
        return None;
    };
    state
        .attack_view(u.player, target)
        .and_then(|view| view.entity)
}

/// Stages a wound: `unit` at `hp`.
fn wound(state: &mut State, unit: UnitId, hp: u32) {
    let mut value = serde_json::to_value(&*state).unwrap();
    let row = value["units"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|u| u["id"] == serde_json::json!(unit))
        .unwrap();
    row["hp"] = serde_json::json!(hp);
    *state = serde_json::from_value(value).unwrap();
}

fn ids(state: &State) -> Vec<UnitId> {
    state.units().iter().map(|unit| unit.id).collect()
}

#[test]
fn a_unit_takes_a_machine_that_can_hit_it_before_a_nearer_harmless_one() {
    let mut state = open_arena(
        20,
        12,
        vec![
            unit(0, UnitKind::Lancer, 5, 5),
            unit(1, UnitKind::Harvester, 7, 5),
            // Four tiles off, inside its own reach of the first Lancer.
            unit(1, UnitKind::Lancer, 9, 5),
        ],
    )
    .build()
    .unwrap();
    let [lancer, _, threat] = ids(&state)[..] else {
        unreachable!()
    };
    state.tick(&[]);
    assert_eq!(attacking(&state, lancer), Some(Target::Unit(threat)));
}

#[test]
fn among_threats_a_unit_takes_the_one_a_shot_removes_most_danger_from() {
    // Both Sentinels can hit the first. The farther one has half its
    // health, so a shot at it removes twice the danger.
    let mut state = open_arena(
        20,
        12,
        vec![
            unit(0, UnitKind::Sentinel, 5, 5),
            unit(1, UnitKind::Sentinel, 7, 5),
            unit(1, UnitKind::Sentinel, 6, 7),
        ],
    )
    .build()
    .unwrap();
    let [sentinel, _, wounded] = ids(&state)[..] else {
        unreachable!()
    };
    wound(&mut state, wounded, 30);
    state.tick(&[]);
    assert_eq!(attacking(&state, sentinel), Some(Target::Unit(wounded)));
}

#[test]
fn machines_come_before_buildings_and_defenses_that_can_hit_first_among_them() {
    let scene = |units| {
        let mut scenario = open_arena(24, 16, units);
        scenario.buildings = vec![
            building(1, BuildingKind::Barricade, 12, 6),
            // Its gun reaches the Sentinel; the Barricade stands nearer.
            building(1, BuildingKind::Turret, 10, 9),
        ];
        scenario.build().unwrap()
    };
    let turret = |state: &State| {
        state
            .buildings()
            .iter()
            .find(|b| b.kind == BuildingKind::Turret)
            .unwrap()
            .id
    };

    let mut state = scene(vec![
        unit(0, UnitKind::Sentinel, 10, 6),
        unit(1, UnitKind::Harvester, 8, 6),
    ]);
    let [sentinel, harvester] = ids(&state)[..] else {
        unreachable!()
    };
    state.tick(&[]);
    assert_eq!(attacking(&state, sentinel), Some(Target::Unit(harvester)));

    let mut state = scene(vec![unit(0, UnitKind::Sentinel, 10, 6)]);
    let sentinel = ids(&state)[0];
    state.tick(&[]);
    assert_eq!(
        attacking(&state, sentinel),
        Some(Target::Building(turret(&state)))
    );
}

/// A Sentinel shooting a Harvester, and an enemy Sentinel walking in to hit
/// it from five tiles off.
fn interrupted(ordered: bool) -> (State, [UnitId; 3]) {
    let mut state = open_arena(
        24,
        12,
        vec![
            unit(0, UnitKind::Sentinel, 5, 5),
            unit(1, UnitKind::Harvester, 7, 5),
            unit(1, UnitKind::Sentinel, 10, 5),
        ],
    )
    .build()
    .unwrap();
    let [sentinel, harvester, attacker] = ids(&state)[..] else {
        unreachable!()
    };
    let mut commands = vec![cmd(
        1,
        Command::Attack {
            units: vec![attacker],
            target: Target::Unit(sentinel).into(),
            queue: false,
        },
    )];
    if ordered {
        commands.push(cmd(
            0,
            Command::Attack {
                units: vec![sentinel],
                target: Target::Unit(harvester).into(),
                queue: false,
            },
        ));
    }
    state.tick(&commands);
    assert_eq!(
        attacking(&state, sentinel),
        Some(Target::Unit(harvester)),
        "test premise: the Harvester is the only victim in reach"
    );
    (state, [sentinel, harvester, attacker])
}

#[test]
fn a_unit_drops_a_harmless_victim_it_chose_for_a_machine_hitting_it() {
    let (mut state, [sentinel, harvester, attacker]) = interrupted(false);
    run_until(&mut state, 200, |state, events| {
        let hit = events
            .iter()
            .any(|event| matches!(event, Event::AttackHit { attacker: a, .. } if *a == attacker));
        assert!(
            hit || attacking(state, sentinel) == Some(Target::Unit(harvester)),
            "it kept its victim until the attacker could hit it"
        );
        hit
    });
    let switch = aftermine_sim::stats::TARGET_RECHECK_TICKS;
    run_until(&mut state, switch, |state, _| {
        attacking(state, sentinel) == Some(Target::Unit(attacker))
    });
}

#[test]
fn an_ordered_attack_keeps_its_victim() {
    let (mut state, [sentinel, harvester, attacker]) = interrupted(true);
    run_until(&mut state, 200, |state, events| {
        let hit = events
            .iter()
            .any(|event| matches!(event, Event::AttackHit { attacker: a, .. } if *a == attacker));
        hit || state.unit(harvester).is_none()
    });
    for _ in 0..2 * aftermine_sim::stats::TARGET_RECHECK_TICKS {
        if state.unit(harvester).is_none() {
            break;
        }
        assert_eq!(attacking(&state, sentinel), Some(Target::Unit(harvester)));
        state.tick(&[]);
    }
}

#[test]
fn a_unit_passes_over_a_victim_the_tick_has_already_killed() {
    // Two Lancers draw on the same wounded Harvester, which one slug kills.
    // The second of them to fire in that tick takes the other Harvester.
    let mut state = open_arena(
        24,
        12,
        vec![
            unit(0, UnitKind::Lancer, 5, 4),
            unit(0, UnitKind::Lancer, 5, 6),
            unit(1, UnitKind::Harvester, 8, 5),
            unit(1, UnitKind::Harvester, 9, 5),
        ],
    )
    .build()
    .unwrap();
    let [first, second, doomed, other] = ids(&state)[..] else {
        unreachable!()
    };
    wound(&mut state, doomed, 20);
    let point = state.unit(doomed).unwrap().pos;
    face_toward(&mut state, first, point);
    face_toward(&mut state, second, point);
    state.tick(&[]);
    assert_eq!(attacking(&state, first), Some(Target::Unit(doomed)));
    assert_eq!(attacking(&state, second), Some(Target::Unit(doomed)));
    let events = run_until(&mut state, 40, |state, _| {
        state.unit(doomed).is_none_or(|harvester| harvester.hp == 0)
    });
    let shots_at = |victim: UnitId| {
        events
            .iter()
            .filter(|event| {
                matches!(event, Event::AttackHit { target: Some(t), .. } if *t == Target::Unit(victim))
            })
            .count()
    };
    assert_eq!(shots_at(doomed), 1, "one slug was enough");
    assert_eq!(
        shots_at(other),
        1,
        "the other slug went into the other Harvester"
    );
}

#[test]
fn exact_ties_turn_with_the_field() {
    // A Sentinel faces east with two Harvesters at the same distance, one
    // either side of its nose. Turning the whole field half round, and
    // listing the Harvesters the other way, picks the turned twin of the
    // same Harvester.
    let (width, height) = (24, 14);
    let turned = |x: i32, y: i32| (width - 1 - x, height - 1 - y);
    let scene = |turn: bool| {
        let at = |x, y| if turn { turned(x, y) } else { (x, y) };
        let (sx, sy) = at(11, 7);
        let (ax, ay) = at(13, 6);
        let (bx, by) = at(13, 8);
        let mut units = vec![unit(0, UnitKind::Sentinel, sx, sy)];
        let harvesters = [
            unit(1, UnitKind::Harvester, ax, ay),
            unit(1, UnitKind::Harvester, bx, by),
        ];
        if turn {
            units.extend(harvesters.into_iter().rev());
        } else {
            units.extend(harvesters);
        }
        let mut state = open_arena(24, 14, units).build().unwrap();
        let sentinel = state.units()[0].id;
        let ahead = state.unit(sentinel).unwrap().pos
            + chassis::fx::Vec2Fx::new(
                chassis::fx::Fx::from_num(if turn { -1 } else { 1 }),
                chassis::fx::Fx::ZERO,
            );
        face_toward(&mut state, sentinel, ahead);
        state.tick(&[]);
        let picked = attacking(&state, sentinel).expect("it picked one");
        let Target::Unit(picked) = picked else {
            unreachable!()
        };
        let tile = state.unit(picked).unwrap().tile();
        if turn {
            let (x, y) = turned(tile.x, tile.y);
            TilePos::new(x, y)
        } else {
            tile
        }
    };
    assert_eq!(scene(false), scene(true));
}
