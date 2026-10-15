//! The Roc's drone bay: building drones one at a time, launching them on a
//! nearby enemy aircraft or a hit, fighting on a short leash, screening fire,
//! docking once calm, and falling with their host.

use crate::common::{building, cmd, open_arena, players, run, unit};
use chassis::grid::TilePos;
use oxide_sim::command::RejectReason;
use oxide_sim::scenario::UnitSpec;
use oxide_sim::stats::{BuildingKind, RECALL_TICKS};
use oxide_sim::{Command, Event, Rally, State, UnitId, UnitKind};

const WIDTH: usize = 30;
const HEIGHT: usize = 22;

fn staged(scrap: u32, units: Vec<UnitSpec>) -> State {
    let mut scenario = open_arena(WIDTH, HEIGHT, units);
    scenario.players = players(scrap);
    scenario.build().unwrap()
}

fn roc(state: &State) -> UnitId {
    state
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Roc && u.player.0 == 0)
        .unwrap()
        .id
}

fn drones(state: &State, host: UnitId) -> Vec<UnitId> {
    state
        .units()
        .iter()
        .filter(|u| u.host == Some(host))
        .map(|u| u.id)
        .collect()
}

/// `state` with the fields of `bay` forged into the bay of `host`.
fn with_bay(state: &State, host: UnitId, bay: &serde_json::Value) -> State {
    let mut value = serde_json::to_value(state).unwrap();
    let row = value["units"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|u| u["id"] == serde_json::json!(host))
        .unwrap();
    for (field, entry) in bay.as_object().unwrap() {
        row["bay"][field] = entry.clone();
    }
    serde_json::from_value(value).unwrap()
}

fn launched(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, Event::DroneLaunched { .. }))
        .count()
}

fn docked(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, Event::DroneDocked { .. }))
        .count()
}

#[test]
fn a_bay_builds_one_drone_at_a_time_to_six_while_it_refills() {
    let mut state = staged(1_000, vec![unit(0, UnitKind::Roc, 8, 10)]);
    let roc = roc(&state);
    let bank = state.player(oxide_sim::PlayerId(0)).scrap;
    let build = UnitKind::Drone.stats().train_ticks;
    run(&mut state, u64::from(build) + 1);
    let bay = state.unit(roc).unwrap().bay.unwrap();
    assert_eq!(bay.docked, 1, "one drone in {build} ticks");
    assert_eq!(bay.ordered, 1, "the next is already paid for");
    run(&mut state, 6 * u64::from(build));
    let bay = state.unit(roc).unwrap().bay.unwrap();
    assert_eq!((bay.docked, bay.ordered), (6, 0), "it stops at six");
    let cost = UnitKind::Drone.stats().cost;
    assert_eq!(state.player(oxide_sim::PlayerId(0)).scrap, bank - 6 * cost);

    let mut idle = staged(1_000, vec![unit(0, UnitKind::Roc, 8, 10)]);
    let roc = idle.units()[0].id;
    idle.tick(&[cmd(
        0,
        Command::SetDroneAutofill {
            units: vec![roc],
            enabled: false,
        },
    )]);
    run(&mut idle, u64::from(build) * 2);
    assert_eq!(idle.unit(roc).unwrap().bay.unwrap().docked, 0);
}

#[test]
fn ordering_counts_flying_drones_and_cancelling_refunds() {
    let state = staged(1_000, vec![unit(0, UnitKind::Roc, 8, 10)]);
    let roc = roc(&state);
    let mut state = with_bay(&state, roc, &serde_json::json!({"docked": 5}));
    let bank = state.player(oxide_sim::PlayerId(0)).scrap;
    let cost = UnitKind::Drone.stats().cost;
    state.tick(&[cmd(0, Command::OrderDrone { unit: roc })]);
    assert_eq!(state.player(oxide_sim::PlayerId(0)).scrap, bank - cost);
    let report = state.tick(&[cmd(0, Command::OrderDrone { unit: roc })]);
    assert!(
        report.events.iter().any(|e| matches!(
            e,
            Event::CommandRejected {
                reason: RejectReason::QueueFull,
                ..
            }
        )),
        "five docked and one ordered fill the bay"
    );
    state.tick(&[cmd(
        0,
        Command::CancelDrone {
            unit: roc,
            index: 0,
        },
    )]);
    let bay = state.unit(roc).unwrap().bay.unwrap();
    assert_eq!(bay.ordered, 0);
    assert!(!bay.autofill, "cancelling stops the refill");
    assert_eq!(state.player(oxide_sim::PlayerId(0)).scrap, bank);

    // Six in flight fill the bay as surely as six docked.
    let mut flying = with_bay(&state, roc, &serde_json::json!({"docked": 6, "alert": 60}));
    flying.tick(&[]);
    assert_eq!(drones(&flying, roc).len(), 6, "premise: all six launched");
    let report = flying.tick(&[cmd(0, Command::OrderDrone { unit: roc })]);
    assert!(report.events.iter().any(|e| matches!(
        e,
        Event::CommandRejected {
            reason: RejectReason::QueueFull,
            ..
        }
    )));
}

#[test]
fn a_seen_enemy_aircraft_near_the_roc_launches_every_docked_drone() {
    let state = staged(
        0,
        vec![
            unit(0, UnitKind::Roc, 8, 10),
            unit(1, UnitKind::Skyhook, 13, 10),
        ],
    );
    let roc = roc(&state);
    let mut state = with_bay(&state, roc, &serde_json::json!({"docked": 6}));
    let report = state.tick(&[]);
    assert_eq!(launched(&report.events), 6);
    assert_eq!(drones(&state, roc).len(), 6);
    assert_eq!(state.unit(roc).unwrap().bay.unwrap().docked, 0);

    let far = staged(
        0,
        vec![
            unit(0, UnitKind::Roc, 8, 10),
            unit(1, UnitKind::Skyhook, 20, 10),
        ],
    );
    let mut far = with_bay(&far, roc, &serde_json::json!({"docked": 6}));
    let events = run(&mut far, 60);
    assert_eq!(launched(&events), 0, "an aircraft past the launch radius");
}

#[test]
fn drones_fight_only_inside_the_leash_and_hold_their_ring_otherwise() {
    // An unarmed enemy Skyhook five tiles out: inside the launch radius,
    // outside the leash.
    let state = staged(
        0,
        vec![
            unit(0, UnitKind::Roc, 8, 10),
            unit(1, UnitKind::Skyhook, 13, 10),
        ],
    );
    let roc = roc(&state);
    let mut state = with_bay(&state, roc, &serde_json::json!({"docked": 3}));
    let events = run(&mut state, 200);
    assert_eq!(launched(&events), 3);
    let shots = |events: &[Event]| {
        events
            .iter()
            .filter(|e| {
                matches!(e, Event::AttackHit { attacker_kind, .. } if *attacker_kind == UnitKind::Drone)
            })
            .count()
    };
    assert_eq!(shots(&events), 0, "nothing inside the leash");
    let host = state.unit(roc).unwrap().pos;
    for drone in drones(&state, roc) {
        let distance = state.unit(drone).unwrap().pos.dist_sq(host);
        assert!(
            distance <= oxide_sim::stats::DRONE_LEASH * oxide_sim::stats::DRONE_LEASH,
            "a drone with nothing to fight keeps near its host"
        );
    }

    let near = staged(
        0,
        vec![
            unit(0, UnitKind::Roc, 8, 10),
            unit(1, UnitKind::Skyhook, 11, 10),
        ],
    );
    let mut near = with_bay(&near, roc, &serde_json::json!({"docked": 3}));
    let events = run(&mut near, 200);
    assert!(shots(&events) > 0, "an aircraft inside the leash is fought");
}

#[test]
fn a_hit_on_the_roc_launches_its_drones_and_fire_on_them_keeps_them_out() {
    let mut scenario = open_arena(WIDTH, HEIGHT, vec![unit(0, UnitKind::Roc, 8, 10)]);
    scenario.players = players(0);
    scenario
        .buildings
        .push(building(1, BuildingKind::FlakTurret, 12, 10));
    let state = scenario.build().unwrap();
    let roc = roc(&state);
    let mut state = with_bay(&state, roc, &serde_json::json!({"docked": 6}));
    let full = UnitKind::Roc.stats().max_hp;
    let mut first_hit = None;
    let mut launch = None;
    let (mut docks, mut fallen) = (0, 0);
    for _ in 0..200 {
        let report = state.tick(&[]);
        // The Roc's guns kill the turret before long; only docks while it
        // still fires count against the screen.
        let firing = state
            .buildings()
            .iter()
            .any(|b| b.kind == BuildingKind::FlakTurret);
        if firing {
            docks += docked(&report.events);
        }
        fallen += report
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Event::UnitDied {
                        kind: UnitKind::Drone,
                        ..
                    }
                )
            })
            .count();
        if state.unit(roc).is_some_and(|u| u.hp < full) && first_hit.is_none() {
            first_hit = Some(state.current_tick());
        }
        if launched(&report.events) > 0 && launch.is_none() {
            launch = Some(state.current_tick());
        }
    }
    let (Some(hit), Some(launch)) = (first_hit, launch) else {
        panic!("the flak hits the Roc and the drones launch: {first_hit:?} {launch:?}");
    };
    assert!(fallen > 0, "the drones draw the flak's fire");
    assert_eq!(docks, 0, "while the flak fires, none docks");
    assert!(launch >= hit, "drones launch on the hit, not before");
}

#[test]
fn drones_dock_once_the_threat_is_gone_and_relaunch_whole() {
    let state = staged(
        0,
        vec![
            unit(0, UnitKind::Roc, 8, 10),
            unit(1, UnitKind::Skyhook, 11, 10),
        ],
    );
    let roc = roc(&state);
    let enemy = state
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Skyhook)
        .unwrap()
        .id;
    let mut state = with_bay(&state, roc, &serde_json::json!({"docked": 2}));
    state.tick(&[]);
    let flying = drones(&state, roc);
    assert_eq!(flying.len(), 2, "premise: launched");
    // The enemy carrier leaves; the drones come home once the alert lapses.
    state.tick(&[cmd(
        1,
        Command::Run {
            units: vec![enemy],
            goal: TilePos::new(27, 20),
            queue: false,
        },
    )]);
    let events = run(&mut state, u64::from(RECALL_TICKS) + 400);
    assert_eq!(docked(&events), 2, "both drones dock");
    assert!(drones(&state, roc).is_empty());
    assert_eq!(state.unit(roc).unwrap().bay.unwrap().docked, 2);
}

#[test]
fn drones_fall_with_their_host() {
    // The drones go after an enemy Skyhook west of the Roc, out of the reach
    // of a Flak Turret to its east, which holds its first shot while they
    // launch and head west, leaving the Roc nearest it.
    let mut scenario = open_arena(
        WIDTH,
        HEIGHT,
        vec![
            unit(0, UnitKind::Roc, 10, 10),
            unit(1, UnitKind::Skyhook, 7, 10),
        ],
    );
    scenario.players = players(0);
    scenario
        .buildings
        .push(building(1, BuildingKind::FlakTurret, 14, 10));
    let state = scenario.build().unwrap();
    let roc = roc(&state);
    let mut value = serde_json::to_value(&state).unwrap();
    for row in value["units"].as_array_mut().unwrap() {
        if row["id"] == serde_json::json!(roc) {
            row["bay"]["docked"] = serde_json::json!(3);
            row["bay"]["ordered"] = serde_json::json!(1);
            row["bay"]["progress"] = serde_json::json!(5);
            row["hp"] = serde_json::json!(1);
        }
    }
    for row in value["buildings"].as_array_mut().unwrap() {
        if row["kind"] == serde_json::json!("flak_turret") {
            row["cooldown"] = serde_json::json!(12);
        }
    }
    let mut state: State = serde_json::from_value(value).unwrap();
    let mut died = Vec::new();
    for _ in 0..300 {
        let report = state.tick(&[]);
        let fell = report
            .events
            .iter()
            .any(|e| matches!(e, Event::UnitDied { unit, .. } if *unit == roc));
        if fell {
            died = report
                .events
                .iter()
                .filter_map(|e| match e {
                    Event::UnitDied { kind, .. } => Some(*kind),
                    _ => None,
                })
                .collect();
            break;
        }
    }
    assert!(died.contains(&UnitKind::Roc), "premise: the Roc falls");
    let fallen_drones = died.iter().filter(|kind| **kind == UnitKind::Drone).count();
    assert_eq!(fallen_drones, 3, "its flying drones fall in the same tick");
    assert!(
        state.units().iter().all(|u| u.host.is_none()),
        "no drone outlives its host"
    );
    state.validate_invariants().unwrap();
}

#[test]
fn drones_take_no_commands_and_nothing_targets_them_as_a_friend() {
    let state = staged(
        100,
        vec![
            unit(0, UnitKind::Roc, 8, 10),
            unit(0, UnitKind::Sentinel, 4, 4),
            unit(1, UnitKind::Skyhook, 11, 10),
        ],
    );
    let roc = roc(&state);
    let sentinel = state
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Sentinel)
        .unwrap()
        .id;
    let mut state = with_bay(&state, roc, &serde_json::json!({"docked": 1}));
    state.tick(&[]);
    let drone = drones(&state, roc)[0];
    let report = state.tick(&[cmd(
        0,
        Command::Run {
            units: vec![drone],
            goal: TilePos::new(25, 20),
            queue: false,
        },
    )]);
    assert!(report.events.iter().any(|e| matches!(
        e,
        Event::CommandRejected {
            reason: RejectReason::NoValidUnits,
            ..
        }
    )));
    assert_eq!(state.unit(drone).unwrap().host, Some(roc));
    let report = state.tick(&[cmd(
        0,
        Command::Accompany {
            units: vec![sentinel],
            target: drone,
            queue: false,
        },
    )]);
    assert!(report.events.iter().any(|e| matches!(
        e,
        Event::CommandRejected {
            reason: RejectReason::InvalidTarget,
            ..
        }
    )));
    let _ = Rally::Unit(drone);
}

#[test]
fn a_bay_and_its_drones_round_trip_mid_flight() {
    let state = staged(
        1_000,
        vec![
            unit(0, UnitKind::Roc, 8, 10),
            unit(1, UnitKind::Skyhook, 11, 10),
        ],
    );
    let roc = roc(&state);
    let mut state = with_bay(
        &state,
        roc,
        &serde_json::json!({"docked": 2, "ordered": 1, "progress": 9}),
    );
    run(&mut state, 3);
    assert!(!drones(&state, roc).is_empty(), "premise: in flight");
    let json = serde_json::to_string(&state).unwrap();
    let back: State = serde_json::from_str(&json).unwrap();
    back.validate_invariants().unwrap();
    assert_eq!(back.hash(), state.hash());
}
