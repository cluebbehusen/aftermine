//! Tending: workers welding wounded own ground machines on their own, on a
//! patrol leg or on a trip out from an idle post. Headless scenarios
//! through the public API: who counts as a patient, the seven-tile reach,
//! the per-worker idle switch, the walk home, billing, the empty bank, and
//! that no other job is ever interrupted.

use crate::common;
use common::{cmd, open_arena, open_arena_with, run, run_until, unit};

use chassis::grid::TilePos;
use oxide_sim::command::RejectReason;
use oxide_sim::scenario::UnitSpec;
use oxide_sim::{Command, Event, Order, State, Target, UnitId, UnitKind, UnitRepairSource};
use serde_json::json;

fn build(units: Vec<UnitSpec>, scrap: u32) -> State {
    let mut scenario = open_arena(32, 18, units);
    for player in &mut scenario.players {
        player.scrap = scrap;
    }
    scenario.build().unwrap()
}

fn ids(state: &State) -> Vec<UnitId> {
    state.units().iter().map(|u| u.id).collect()
}

/// Rewrites one unit's serialized row, the way a save would carry it.
fn edit(state: &mut State, id: UnitId, poke: impl FnOnce(&mut serde_json::Value)) {
    let mut data = serde_json::to_value(&*state).unwrap();
    let row = data["units"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|u| u["id"] == json!(id))
        .unwrap();
    poke(row);
    *state = serde_json::from_value(data).unwrap();
}

fn set_hp(state: &mut State, id: UnitId, hp: u32) {
    edit(state, id, |row| row["hp"] = json!(hp));
}

fn max_hp(state: &State, id: UnitId) -> u32 {
    state.unit(id).unwrap().kind.stats().max_hp
}

fn stalls(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, Event::OrderStalled { .. }))
        .count()
}

fn welded_by(events: &[Event], welder: UnitId) -> u32 {
    events
        .iter()
        .filter_map(|event| match event {
            Event::UnitRepaired {
                source: UnitRepairSource::FieldWelder { unit },
                amount,
                ..
            } if *unit == welder => Some(*amount),
            _ => None,
        })
        .sum()
}

fn tending(state: &State, id: UnitId) -> bool {
    matches!(state.unit(id).unwrap().order, Order::Tend { .. })
}

fn idle_weld(units: Vec<UnitId>, enabled: bool) -> Command {
    Command::SetIdleWeld { units, enabled }
}

#[test]
fn an_idle_worker_welds_a_wounded_machine_near_its_post_and_walks_home() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 6, 8),
            unit(0, UnitKind::Sentinel, 10, 8),
        ],
        500,
    );
    let [welder, patient] = ids(&state)[..] else {
        unreachable!()
    };
    let post = state.unit(welder).unwrap().tile();
    let full = max_hp(&state, patient);
    set_hp(&mut state, patient, full - 30);
    let bank = state.player(oxide_sim::PlayerId(0)).scrap;

    let mut events = run_until(&mut state, 2_000, |s, _| {
        s.unit(patient).unwrap().hp == full
    });
    assert_eq!(welded_by(&events, welder), 30, "the welder did the work");
    assert!(
        state.player(oxide_sim::PlayerId(0)).scrap < bank,
        "the weld bills"
    );
    events.extend(run_until(&mut state, 600, |s, _| {
        s.unit(welder).unwrap().order == Order::Idle
    }));
    let unit = state.unit(welder).unwrap();
    assert_eq!(unit.tile(), post, "the trip ends back at the post");
    assert_eq!(stalls(&events), 0, "a tending walk reports nothing");
}

#[test]
fn a_patient_farther_than_seven_tiles_from_the_post_is_left_alone() {
    for (x, weld) in [(13, true), (14, false)] {
        let mut state = build(
            vec![
                unit(0, UnitKind::Harvester, 6, 8),
                unit(0, UnitKind::Sentinel, x, 8),
            ],
            500,
        );
        let [welder, patient] = ids(&state)[..] else {
            unreachable!()
        };
        let wounded = max_hp(&state, patient) - 30;
        set_hp(&mut state, patient, wounded);
        run(&mut state, 3);
        assert_eq!(
            tending(&state, welder),
            weld,
            "a patient {} tiles out",
            x - 6
        );
        if !weld {
            run(&mut state, 200);
            assert_eq!(state.unit(patient).unwrap().hp, wounded);
            assert_eq!(state.unit(welder).unwrap().order, Order::Idle);
        }
    }
}

#[test]
fn the_idle_switch_is_per_worker_and_starts_on() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 6, 4),
            unit(0, UnitKind::Excavator, 6, 12),
            unit(0, UnitKind::Sentinel, 9, 4),
            unit(0, UnitKind::Sentinel, 9, 12),
        ],
        500,
    );
    let [kept, switched, near_kept, near_switched] = ids(&state)[..] else {
        unreachable!()
    };
    assert!(state.unit(kept).unwrap().idle_weld());
    assert!(state.unit(switched).unwrap().idle_weld());
    state.tick(&[cmd(0, idle_weld(vec![switched], false))]);
    assert!(!state.unit(switched).unwrap().idle_weld());
    for patient in [near_kept, near_switched] {
        let wounded = max_hp(&state, patient) - 30;
        set_hp(&mut state, patient, wounded);
    }
    run(&mut state, 300);
    assert_eq!(state.unit(near_kept).unwrap().hp, max_hp(&state, near_kept));
    assert_eq!(
        state.unit(near_switched).unwrap().hp,
        max_hp(&state, near_switched) - 30,
        "a switched-off worker stays at its post"
    );
    assert_eq!(state.unit(switched).unwrap().order, Order::Idle);
}

#[test]
fn switching_idle_welding_off_mid_trip_sends_the_worker_home() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 6, 8),
            unit(0, UnitKind::Sentinel, 12, 8),
        ],
        500,
    );
    let [welder, patient] = ids(&state)[..] else {
        unreachable!()
    };
    let post = state.unit(welder).unwrap().tile();
    let wounded = max_hp(&state, patient) - 30;
    set_hp(&mut state, patient, wounded);
    run_until(&mut state, 100, |s, _| {
        s.unit(welder).unwrap().tile().x >= post.x + 2
    });
    state.tick(&[cmd(0, idle_weld(vec![welder], false))]);
    let events = run_until(&mut state, 600, |s, _| {
        s.unit(welder).unwrap().order == Order::Idle
    });
    assert_eq!(state.unit(welder).unwrap().tile(), post);
    assert_eq!(state.unit(patient).unwrap().hp, wounded, "no weld landed");
    assert_eq!(stalls(&events), 0);
}

#[test]
fn the_idle_switch_refuses_a_selection_with_any_foreign_or_unfit_unit() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 6, 8),
            unit(0, UnitKind::Sentinel, 10, 8),
            unit(1, UnitKind::Harvester, 20, 8),
        ],
        500,
    );
    let [worker, sentinel, foreign] = ids(&state)[..] else {
        unreachable!()
    };
    for (units, reason) in [
        (vec![], RejectReason::NoValidUnits),
        (vec![worker, foreign], RejectReason::NoValidUnits),
        (vec![worker, sentinel], RejectReason::InvalidTarget),
    ] {
        let mut quiet = state.clone();
        quiet.tick(&[]);
        let events = state.tick(&[cmd(0, idle_weld(units, false))]).events;
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::CommandRejected { reason: r, .. } if *r == reason)),
            "{reason:?}"
        );
        assert!(state.unit(worker).unwrap().idle_weld(), "all or none");
        assert_eq!(state.hash(), quiet.hash(), "a refusal changes nothing");
    }
}

#[test]
fn a_patrolling_worker_welds_what_it_passes_and_keeps_its_circuit() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 4, 8),
            unit(0, UnitKind::Sentinel, 14, 10),
        ],
        500,
    );
    let [welder, patient] = ids(&state)[..] else {
        unreachable!()
    };
    // The idle switch governs the post, never the patrol.
    state.tick(&[cmd(0, idle_weld(vec![welder], false))]);
    let full = max_hp(&state, patient);
    set_hp(&mut state, patient, full - 30);
    state.tick(&[cmd(
        0,
        Command::Patrol {
            units: vec![welder],
            waypoints: vec![TilePos::new(4, 8), TilePos::new(24, 8)],
        },
    )]);
    let events = run_until(&mut state, 2_000, |s, _| {
        s.unit(patient).unwrap().hp == full
    });
    assert_eq!(welded_by(&events, welder), 30);
    let unit = state.unit(welder).unwrap();
    assert!(unit.looping, "the circuit survives the weld");
    assert_eq!(unit.queue.len(), 1);
    assert!(
        std::iter::once(&unit.order)
            .chain(&unit.queue)
            .all(|order| matches!(order, Order::Tend { .. })),
        "{:?} then {:?}",
        unit.order,
        unit.queue
    );
    assert_eq!(stalls(&events), 0);
}

#[test]
fn an_empty_bank_never_sends_a_worker_out() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 6, 8),
            unit(0, UnitKind::Sentinel, 10, 8),
        ],
        0,
    );
    let [welder, patient] = ids(&state)[..] else {
        unreachable!()
    };
    let wounded = max_hp(&state, patient) - 30;
    set_hp(&mut state, patient, wounded);
    for _ in 0..300 {
        state.tick(&[]);
        assert_eq!(state.unit(welder).unwrap().order, Order::Idle);
    }
    assert_eq!(state.unit(patient).unwrap().hp, wounded);
}

#[test]
fn two_wounded_idle_workers_weld_each_other_to_full() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 6, 8),
            unit(0, UnitKind::Harvester, 9, 8),
        ],
        500,
    );
    let [a, b] = ids(&state)[..] else {
        unreachable!()
    };
    let full = max_hp(&state, a);
    set_hp(&mut state, a, full - 20);
    set_hp(&mut state, b, full - 20);
    run_until(&mut state, 3_000, |s, _| {
        s.unit(a).unwrap().hp == full && s.unit(b).unwrap().hp == full
    });
    run_until(&mut state, 600, |s, _| {
        s.unit(a).unwrap().order == Order::Idle && s.unit(b).unwrap().order == Order::Idle
    });
}

#[test]
fn foreign_airborne_and_healthy_machines_are_no_patients() {
    let mut scenario = open_arena_with(
        32,
        18,
        vec![
            unit(0, UnitKind::Harvester, 6, 8),
            unit(1, UnitKind::Harvester, 9, 8),
            unit(2, UnitKind::Sentinel, 6, 11),
            unit(0, UnitKind::Kestrel, 8, 6),
            unit(0, UnitKind::Sentinel, 4, 8),
        ],
        |rows| rows[1][27] = '3',
    );
    let mut ally = scenario.players[0].clone();
    ally.name = "Ally".into();
    scenario.players.push(ally);
    scenario.players[0].team = Some(0);
    scenario.players[2].team = Some(0);
    scenario.players[1].team = Some(1);
    for player in &mut scenario.players {
        player.scrap = 500;
    }
    let mut state = scenario.build().unwrap();
    let [welder, enemy, ally, flier, healthy] = ids(&state)[..] else {
        unreachable!()
    };
    // The enemy worker would otherwise weld its own wounded ally.
    state = common::idle_welds_off(&state, &[enemy]);
    for patient in [enemy, ally, flier] {
        let wounded = max_hp(&state, patient) - 10;
        set_hp(&mut state, patient, wounded);
    }
    // Airborne for these ticks: an idle Kestrel lands only later.
    for _ in 0..30 {
        state.tick(&[]);
        assert_eq!(state.unit(welder).unwrap().order, Order::Idle);
    }
    assert_eq!(
        state.unit(healthy).unwrap().hp,
        max_hp(&state, healthy),
        "premise: the own Sentinel was never hurt"
    );
}

#[test]
fn a_moving_patient_is_taken_on_only_once_it_stops() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 6, 8),
            unit(0, UnitKind::Sentinel, 20, 8),
        ],
        500,
    );
    let [welder, patient] = ids(&state)[..] else {
        unreachable!()
    };
    let wounded = max_hp(&state, patient) - 20;
    set_hp(&mut state, patient, wounded);
    // It walks past the post and on out of reach.
    state.tick(&[cmd(
        0,
        Command::Run {
            units: vec![patient],
            goal: TilePos::new(2, 10),
            queue: false,
        },
    )]);
    let mut stopped = false;
    for _ in 0..400 {
        state.tick(&[]);
        let unit = state.unit(patient).unwrap();
        if unit.path.is_some() || unit.drive_speed() != chassis::fx::Fx::ZERO {
            assert!(
                !matches!(state.unit(welder).unwrap().order,
                    Order::Tend { weld: Some(p), .. } if p == Target::Unit(patient) && !stopped),
                "a walking patient is not taken on"
            );
        } else {
            stopped = true;
        }
    }
    assert!(stopped, "premise: the patient arrived");
    run_until(&mut state, 1_000, |s, _| {
        s.unit(patient).unwrap().hp == max_hp(s, patient)
    });
}

#[test]
fn no_other_job_is_interrupted() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 6, 4),
            unit(0, UnitKind::Harvester, 6, 12),
            unit(0, UnitKind::Sentinel, 9, 8),
        ],
        500,
    );
    let [runner, builder, patient] = ids(&state)[..] else {
        unreachable!()
    };
    let wounded = max_hp(&state, patient) - 20;
    set_hp(&mut state, patient, wounded);
    state.tick(&[
        cmd(
            0,
            Command::Run {
                units: vec![runner],
                goal: TilePos::new(12, 8),
                queue: false,
            },
        ),
        cmd(
            0,
            Command::Build {
                units: vec![builder],
                kind: oxide_sim::BuildingKind::Turret,
                anchor: TilePos::new(10, 13),
                queue: false,
                defer: false,
            },
        ),
    ]);
    for _ in 0..60 {
        state.tick(&[]);
        assert!(!tending(&state, runner) && !tending(&state, builder));
    }
    assert!(matches!(
        state.unit(builder).unwrap().order,
        Order::Build { .. }
    ));
}

#[test]
fn a_queued_command_takes_over_a_trip_from_an_idle_post() {
    for queue in [false, true] {
        let mut state = build(
            vec![
                unit(0, UnitKind::Harvester, 6, 8),
                unit(0, UnitKind::Sentinel, 12, 8),
            ],
            500,
        );
        let [welder, patient] = ids(&state)[..] else {
            unreachable!()
        };
        let wounded = max_hp(&state, patient) - 20;
        set_hp(&mut state, patient, wounded);
        run(&mut state, 3);
        assert!(tending(&state, welder), "premise: out on a trip");
        state.tick(&[cmd(
            0,
            Command::Run {
                units: vec![welder],
                goal: TilePos::new(6, 14),
                queue,
            },
        )]);
        let unit = state.unit(welder).unwrap();
        assert!(
            matches!(unit.order, Order::Run { .. }) && unit.queue.is_empty(),
            "queue {queue}: {:?} then {:?}",
            unit.order,
            unit.queue
        );
    }
}

#[test]
fn the_idle_switch_rides_along_in_a_transport() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 6, 8),
            unit(0, UnitKind::Skyhook, 8, 8),
        ],
        500,
    );
    let [worker, skyhook] = ids(&state)[..] else {
        unreachable!()
    };
    state.tick(&[cmd(0, idle_weld(vec![worker], false))]);
    state.tick(&[cmd(
        0,
        Command::Load {
            units: vec![worker],
            transport: skyhook,
            queue: false,
        },
    )]);
    run_until(&mut state, 600, |s, _| s.unit(worker).is_none());
    state.tick(&[cmd(
        0,
        Command::Unload {
            transport: skyhook,
            at: TilePos::new(16, 8),
            queue: false,
        },
    )]);
    run_until(&mut state, 1_200, |s, _| s.unit(worker).is_some());
    assert!(!state.unit(worker).unwrap().idle_weld());
}

#[test]
fn mirrored_seats_take_on_mirrored_patients() {
    // Each welder stands exactly between two wounded Sentinels, so only
    // the bearing from its own facing decides; spawned units face the map
    // centre, so the two seats' scenes are half-turns of each other.
    let mirror = |x: i32, y: i32| (31 - x, 17 - y);
    let west = [(8, 8), (8, 6), (8, 10)];
    let mut units = Vec::new();
    for (player, spots) in [(0, west), (1, west.map(|(x, y)| mirror(x, y)))] {
        units.push(unit(player, UnitKind::Harvester, spots[0].0, spots[0].1));
        for &(x, y) in &spots[1..] {
            units.push(unit(player, UnitKind::Sentinel, x, y));
        }
    }
    let mut state = build(units, 500);
    let all = ids(&state);
    for &patient in &[all[1], all[2], all[4], all[5]] {
        let wounded = max_hp(&state, patient) - 20;
        set_hp(&mut state, patient, wounded);
    }
    run(&mut state, 2);
    let pick = |welder: UnitId| match state.unit(welder).unwrap().order {
        Order::Tend {
            weld: Some(Target::Unit(patient)),
            ..
        } => state.unit(patient).unwrap().tile(),
        other => panic!("{other:?}"),
    };
    let (west_pick, east_pick) = (pick(all[0]), pick(all[3]));
    assert_eq!((east_pick.x, east_pick.y), mirror(west_pick.x, west_pick.y));
}

#[test]
fn a_patient_no_route_reaches_is_never_taken_on() {
    let mut scenario = open_arena_with(
        32,
        18,
        vec![
            unit(0, UnitKind::Harvester, 6, 8),
            unit(0, UnitKind::Sentinel, 12, 8),
        ],
        |rows| {
            // A closed pen around the patient.
            for (y, row) in rows.iter_mut().enumerate().take(11).skip(6) {
                for (x, cell) in row.iter_mut().enumerate().take(15).skip(10) {
                    if y == 6 || y == 10 || x == 10 || x == 14 {
                        *cell = '#';
                    }
                }
            }
        },
    );
    for player in &mut scenario.players {
        player.scrap = 500;
    }
    let mut state = scenario.build().unwrap();
    let [welder, patient] = ids(&state)[..] else {
        unreachable!()
    };
    let wounded = max_hp(&state, patient) - 20;
    set_hp(&mut state, patient, wounded);
    for _ in 0..100 {
        state.tick(&[]);
        let unit = state.unit(welder).unwrap();
        assert_eq!(unit.order, Order::Idle);
        assert!(unit.path.is_none());
    }
}

#[test]
fn patrol_legs_follow_what_each_machine_can_do() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Sentinel, 6, 6),
            unit(0, UnitKind::Harvester, 6, 8),
            unit(0, UnitKind::Skyhook, 6, 10),
        ],
        500,
    );
    let [armed, worker, carrier] = ids(&state)[..] else {
        unreachable!()
    };
    state.tick(&[cmd(
        0,
        Command::Patrol {
            units: vec![armed, worker, carrier],
            waypoints: vec![TilePos::new(6, 8), TilePos::new(20, 8)],
        },
    )]);
    let legs = |id: UnitId| {
        let unit = state.unit(id).unwrap();
        std::iter::once(unit.order)
            .chain(unit.queue.iter().copied())
            .collect::<Vec<_>>()
    };
    assert!(legs(armed).iter().all(|o| matches!(o, Order::Hunt { .. })));
    assert!(
        legs(worker)
            .iter()
            .all(|o| matches!(o, Order::Tend { weld: None, .. }))
    );
    assert!(legs(carrier).iter().all(|o| matches!(o, Order::Run { .. })));
}

#[test]
fn a_broke_patrol_walks_on_past_the_wounded() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 4, 8),
            unit(0, UnitKind::Sentinel, 14, 9),
        ],
        0,
    );
    let [welder, patient] = ids(&state)[..] else {
        unreachable!()
    };
    let wounded = max_hp(&state, patient) - 20;
    set_hp(&mut state, patient, wounded);
    state.tick(&[cmd(
        0,
        Command::Patrol {
            units: vec![welder],
            waypoints: vec![TilePos::new(4, 8), TilePos::new(24, 8)],
        },
    )]);
    let far = run_until(&mut state, 1_000, |s, _| {
        assert!(
            matches!(
                s.unit(welder).unwrap().order,
                Order::Tend { weld: None, .. }
            ),
            "no patient without the scrap to pay"
        );
        s.unit(welder).unwrap().tile().x >= 23
    });
    assert_eq!(stalls(&far), 0);
    assert_eq!(state.unit(patient).unwrap().hp, wounded);
}

#[test]
fn the_bot_view_reports_a_trip_as_busy_tending() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 6, 8),
            unit(0, UnitKind::Sentinel, 12, 8),
        ],
        500,
    );
    let [welder, patient] = ids(&state)[..] else {
        unreachable!()
    };
    let wounded = max_hp(&state, patient) - 20;
    set_hp(&mut state, patient, wounded);
    run(&mut state, 3);
    let view = oxide_sim::observation::ObservationData::fog_honest(&state, oxide_sim::PlayerId(0));
    let worker = view.my_units.iter().find(|u| u.id == welder).unwrap();
    assert!(!worker.idle);
    assert_eq!(
        worker.repair,
        Some(oxide_sim::observation::RepairWork::Tending)
    );
    assert_eq!(
        view.repair_target(welder),
        Some(oxide_sim::Target::Unit(patient))
    );
}
