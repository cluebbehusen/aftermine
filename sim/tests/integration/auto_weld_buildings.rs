//! Tending buildings: idle and patrolling workers repairing damaged own
//! structures on their own, ranked with wounded machines by the nearest
//! point of each. Headless scenarios through the public API: the footprint
//! reach, the shared switch, crew billing, the empty bank, and the
//! buildings tending leaves alone.

use crate::common;
use common::{building, cmd, open_arena, open_arena_with, run, run_until, unit};

use chassis::grid::TilePos;
use oxide_sim::scenario::{BuildingSpec, UnitSpec};
use oxide_sim::stats::BuildingKind;
use oxide_sim::{BuildingId, Command, Event, Order, PlayerId, State, Target, UnitId, UnitKind};
use serde_json::json;

fn build(units: Vec<UnitSpec>, buildings: Vec<BuildingSpec>, scrap: u32) -> State {
    let mut scenario = open_arena(32, 18, units);
    scenario.buildings.extend(buildings);
    for player in &mut scenario.players {
        player.scrap = scrap;
    }
    scenario.build().unwrap()
}

fn ids(state: &State) -> Vec<UnitId> {
    state.units().iter().map(|u| u.id).collect()
}

fn site(state: &State, kind: BuildingKind, x: i32, y: i32) -> BuildingId {
    state
        .buildings()
        .iter()
        .find(|b| b.kind == kind && b.anchor == TilePos::new(x, y))
        .unwrap()
        .id
}

/// Rewrites one building's hp, the way a save would carry it.
fn set_hp(state: &mut State, id: BuildingId, hp: u32) {
    let mut data = serde_json::to_value(&*state).unwrap();
    data["buildings"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|b| b["id"] == json!(id))
        .unwrap()["hp"] = json!(hp);
    *state = serde_json::from_value(data).unwrap();
}

fn hp(state: &State, id: BuildingId) -> u32 {
    state.building(id).unwrap().hp
}

fn max_hp(state: &State, id: BuildingId) -> u32 {
    state.building(id).unwrap().stats().max_hp
}

fn wound(state: &mut State, id: BuildingId, missing: u32) -> u32 {
    let wounded = max_hp(state, id) - missing;
    set_hp(state, id, wounded);
    wounded
}

fn patient(state: &State, welder: UnitId) -> Option<Target> {
    match state.unit(welder).unwrap().order {
        Order::Tend { weld, .. } => weld,
        _ => None,
    }
}

fn scrap(state: &State) -> u32 {
    state.player(PlayerId(0)).scrap
}

fn stalls(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, Event::OrderStalled { .. }))
        .count()
}

#[test]
fn an_idle_worker_repairs_a_damaged_building_near_its_post_and_walks_home() {
    let mut state = build(
        vec![unit(0, UnitKind::Harvester, 6, 8)],
        vec![building(0, BuildingKind::Turret, 10, 8)],
        500,
    );
    let welder = ids(&state)[0];
    let turret = site(&state, BuildingKind::Turret, 10, 8);
    let post = state.unit(welder).unwrap().tile();
    wound(&mut state, turret, 40);
    let bank = scrap(&state);

    let mut events = run_until(&mut state, 2_000, |s, _| hp(s, turret) == max_hp(s, turret));
    assert!(scrap(&state) < bank, "the repair bills");
    events.extend(run_until(&mut state, 600, |s, _| {
        s.unit(welder).unwrap().order == Order::Idle
    }));
    assert_eq!(state.unit(welder).unwrap().tile(), post);
    assert_eq!(stalls(&events), 0, "a tending walk reports nothing");
}

#[test]
fn tending_bills_a_building_exactly_as_a_repair_crew_does() {
    let scene = || {
        let mut state = build(
            vec![unit(0, UnitKind::Harvester, 6, 8)],
            vec![building(0, BuildingKind::Turret, 10, 8)],
            500,
        );
        let turret = site(&state, BuildingKind::Turret, 10, 8);
        wound(&mut state, turret, 60);
        (state, turret)
    };
    let (mut tended, turret) = scene();
    run_until(&mut tended, 2_000, |s, _| {
        hp(s, turret) == max_hp(s, turret)
    });

    let (crewed, _) = scene();
    let welder = ids(&crewed)[0];
    let mut crewed = common::idle_welds_off(&crewed, &[welder]);
    crewed.tick(&[cmd(
        0,
        Command::Repair {
            units: vec![welder],
            building: turret,
            queue: false,
        },
    )]);
    run_until(&mut crewed, 2_000, |s, _| {
        hp(s, turret) == max_hp(s, turret)
    });
    assert_eq!(scrap(&tended), scrap(&crewed), "the same hp costs the same");
}

#[test]
fn reach_is_seven_tiles_to_the_nearest_point_of_the_footprint() {
    // A Fabricator anchored at x = 13 has its centre 7.5 tiles from the
    // post's centre but its near edge 6.5 tiles away; at x = 14 the edge
    // too is out of reach.
    for (x, repairs) in [(13, true), (14, false)] {
        let mut state = build(
            vec![unit(0, UnitKind::Harvester, 6, 8)],
            vec![building(0, BuildingKind::Fabricator, x, 8)],
            500,
        );
        let welder = ids(&state)[0];
        let works = site(&state, BuildingKind::Fabricator, x, 8);
        let wounded = wound(&mut state, works, 40);
        run(&mut state, 3);
        assert_eq!(
            patient(&state, welder),
            repairs.then_some(Target::Building(works)),
            "a Fabricator anchored at x = {x}"
        );
        if !repairs {
            run(&mut state, 200);
            assert_eq!(hp(&state, works), wounded);
            assert_eq!(state.unit(welder).unwrap().order, Order::Idle);
        }
    }
}

#[test]
fn the_nearer_of_a_machine_and_a_building_goes_first_either_way() {
    for (turret_x, sentinel_x, first_building) in [(9, 12, true), (12, 8, false)] {
        let mut state = build(
            vec![
                unit(0, UnitKind::Harvester, 6, 8),
                unit(0, UnitKind::Sentinel, sentinel_x, 8),
            ],
            vec![building(0, BuildingKind::Turret, turret_x, 8)],
            500,
        );
        let [welder, sentinel] = ids(&state)[..] else {
            unreachable!()
        };
        let turret = site(&state, BuildingKind::Turret, turret_x, 8);
        wound(&mut state, turret, 30);
        let full = state.unit(sentinel).unwrap().kind.stats().max_hp;
        let mut data = serde_json::to_value(&state).unwrap();
        data["units"][1]["hp"] = json!(full - 20);
        let mut state: State = serde_json::from_value(data).unwrap();
        run(&mut state, 2);
        let expected = if first_building {
            Target::Building(turret)
        } else {
            Target::Unit(sentinel)
        };
        assert_eq!(patient(&state, welder), Some(expected));
        run_until(&mut state, 4_000, |s, _| {
            hp(s, turret) == max_hp(s, turret) && s.unit(sentinel).unwrap().hp == full
        });
    }
}

#[test]
fn the_idle_switch_covers_buildings_and_a_patrol_always_repairs() {
    let mut state = build(
        vec![unit(0, UnitKind::Harvester, 6, 8)],
        vec![building(0, BuildingKind::Turret, 10, 8)],
        500,
    );
    let welder = ids(&state)[0];
    let turret = site(&state, BuildingKind::Turret, 10, 8);
    state.tick(&[cmd(
        0,
        Command::SetIdleWeld {
            units: vec![welder],
            enabled: false,
        },
    )]);
    let wounded = wound(&mut state, turret, 30);
    run(&mut state, 300);
    assert_eq!(
        hp(&state, turret),
        wounded,
        "a switched-off worker stays put"
    );
    assert_eq!(state.unit(welder).unwrap().order, Order::Idle);

    state.tick(&[cmd(
        0,
        Command::Patrol {
            units: vec![welder],
            waypoints: vec![TilePos::new(4, 10), TilePos::new(24, 10)],
        },
    )]);
    let events = run_until(&mut state, 2_000, |s, _| hp(s, turret) == max_hp(s, turret));
    let unit = state.unit(welder).unwrap();
    assert!(unit.looping, "the circuit survives the repair");
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
fn a_building_bound_for_salvage_is_no_patient() {
    let state = build(
        vec![
            unit(0, UnitKind::Harvester, 6, 8),
            unit(0, UnitKind::Harvester, 6, 12),
            unit(0, UnitKind::Sentinel, 6, 4),
        ],
        vec![building(0, BuildingKind::Turret, 10, 8)],
        500,
    );
    let [tender, stripper, sentinel] = ids(&state)[..] else {
        unreachable!()
    };
    let turret = site(&state, BuildingKind::Turret, 10, 8);
    let mut state = common::idle_welds_off(&state, &[stripper]);
    wound(&mut state, turret, 60);
    run_until(&mut state, 200, |s, _| {
        patient(s, tender) == Some(Target::Building(turret))
    });

    // A refused teardown changes nothing.
    let events = state
        .tick(&[cmd(
            0,
            Command::Salvage {
                units: vec![sentinel],
                building: turret,
                queue: false,
            },
        )])
        .events;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::CommandRejected { .. }))
    );
    assert_eq!(patient(&state, tender), Some(Target::Building(turret)));

    // An accepted one takes the building from its tender that same tick,
    // and a queued one keeps it from being taken on again.
    state.tick(&[cmd(
        0,
        Command::Run {
            units: vec![stripper],
            goal: TilePos::new(4, 14),
            queue: false,
        },
    )]);
    state.tick(&[cmd(
        0,
        Command::Salvage {
            units: vec![stripper],
            building: turret,
            queue: true,
        },
    )]);
    assert_eq!(patient(&state, tender), None);
    for _ in 0..20 {
        state.tick(&[]);
        assert_ne!(patient(&state, tender), Some(Target::Building(turret)));
    }
}

#[test]
fn foreign_hidden_unfinished_and_whole_buildings_are_no_patients() {
    let state = build(
        vec![
            unit(0, UnitKind::Harvester, 6, 8),
            unit(0, UnitKind::Harvester, 6, 13),
        ],
        vec![
            building(1, BuildingKind::Fabricator, 9, 7),
            building(0, BuildingKind::ScuttleCharge, 8, 5),
            building(0, BuildingKind::Turret, 8, 10),
        ],
        500,
    );
    let [welder, builder] = ids(&state)[..] else {
        unreachable!()
    };
    let enemy = site(&state, BuildingKind::Fabricator, 9, 7);
    let charge = site(&state, BuildingKind::ScuttleCharge, 8, 5);
    let whole = site(&state, BuildingKind::Turret, 8, 10);
    // An unfinished site its builder walked away from.
    let mut state = common::idle_welds_off(&state, &[builder]);
    state.tick(&[cmd(
        0,
        Command::Build {
            units: vec![builder],
            kind: BuildingKind::Barricade,
            anchor: TilePos::new(9, 12),
            queue: false,
            defer: false,
        },
    )]);
    run_until(&mut state, 400, |s, _| {
        s.buildings().iter().any(|b| {
            b.kind == BuildingKind::Barricade && b.construction_progress().is_some_and(|p| p > 0)
        })
    });
    state.tick(&[cmd(
        0,
        Command::Stop {
            units: vec![builder],
        },
    )]);
    let barricade = site(&state, BuildingKind::Barricade, 9, 12);
    assert!(!state.building(barricade).unwrap().built(), "premise");
    wound(&mut state, enemy, 30);
    wound(&mut state, charge, 5);
    for _ in 0..200 {
        state.tick(&[]);
        assert_eq!(state.unit(welder).unwrap().order, Order::Idle);
    }
    assert_eq!(hp(&state, whole), max_hp(&state, whole));
}

#[test]
fn a_building_no_route_reaches_is_never_taken_on() {
    let mut scenario = open_arena_with(32, 18, vec![unit(0, UnitKind::Harvester, 6, 8)], |rows| {
        // Rock walls in every tile around the Turret.
        for (y, row) in rows.iter_mut().enumerate().take(10).skip(7) {
            for (x, cell) in row.iter_mut().enumerate().take(12).skip(9) {
                if (x, y) != (10, 8) {
                    *cell = '#';
                }
            }
        }
    });
    scenario
        .buildings
        .push(building(0, BuildingKind::Turret, 10, 8));
    for player in &mut scenario.players {
        player.scrap = 500;
    }
    let mut state = scenario.build().unwrap();
    let welder = ids(&state)[0];
    let turret = site(&state, BuildingKind::Turret, 10, 8);
    wound(&mut state, turret, 30);
    for _ in 0..100 {
        state.tick(&[]);
        let unit = state.unit(welder).unwrap();
        assert_eq!(unit.order, Order::Idle);
        assert!(unit.path.is_none());
    }
}

#[test]
fn an_empty_bank_sends_nobody_and_a_spent_one_lets_go_quietly() {
    let mut state = build(
        vec![unit(0, UnitKind::Harvester, 6, 8)],
        vec![building(0, BuildingKind::Turret, 10, 8)],
        0,
    );
    let welder = ids(&state)[0];
    let turret = site(&state, BuildingKind::Turret, 10, 8);
    let wounded = wound(&mut state, turret, 200);
    for _ in 0..300 {
        state.tick(&[]);
        assert_eq!(state.unit(welder).unwrap().order, Order::Idle);
    }
    assert_eq!(hp(&state, turret), wounded);

    // A few scrap buy a few hp, then the trip ends without a stall and
    // never starts again.
    let mut data = serde_json::to_value(&state).unwrap();
    data["players"][0]["scrap"] = json!(3);
    let mut state: State = serde_json::from_value(data).unwrap();
    let events = run_until(&mut state, 1_000, |s, _| {
        scrap(s) == 0 && s.unit(welder).unwrap().order == Order::Idle
    });
    assert!(hp(&state, turret) > wounded, "the bank bought some repair");
    assert_eq!(stalls(&events), 0);
    for _ in 0..200 {
        state.tick(&[]);
        assert_eq!(state.unit(welder).unwrap().order, Order::Idle);
    }
}

#[test]
fn mirrored_seats_take_on_mirrored_buildings() {
    // Each welder stands exactly between two damaged buildings, so only the
    // bearing from its own facing decides; spawned units face the map
    // centre, so the two seats' scenes are half-turns of each other.
    let mirror = |x: i32, y: i32| (31 - x, 17 - y);
    for kind in [BuildingKind::Turret, BuildingKind::Fabricator] {
        let (w, h) = kind.size();
        let anchors = [(8, 7 - h), (8, 10)];
        let mut buildings = Vec::new();
        for &(x, y) in &anchors {
            buildings.push(building(0, kind, x, y));
            let (mx, my) = mirror(x + w - 1, y + h - 1);
            buildings.push(building(1, kind, mx, my));
        }
        let (ex, ey) = mirror(8, 8);
        let mut state = build(
            vec![
                unit(0, UnitKind::Harvester, 8, 8),
                unit(1, UnitKind::Harvester, ex, ey),
            ],
            buildings,
            500,
        );
        let [west, east] = ids(&state)[..] else {
            unreachable!()
        };
        let damaged: Vec<BuildingId> = state
            .buildings()
            .iter()
            .filter(|b| b.kind == kind)
            .map(|b| b.id)
            .collect();
        for id in damaged {
            wound(&mut state, id, 20);
        }
        run(&mut state, 2);
        let anchor = |welder: UnitId| match patient(&state, welder) {
            Some(Target::Building(id)) => state.building(id).unwrap().anchor,
            other => panic!("{kind:?}: {other:?}"),
        };
        let (west_pick, east_pick) = (anchor(west), anchor(east));
        assert_eq!(
            (east_pick.x, east_pick.y),
            mirror(west_pick.x + w - 1, west_pick.y + h - 1),
            "{kind:?}"
        );
    }
}

#[test]
fn the_bot_view_reports_a_building_tender() {
    let mut state = build(
        vec![unit(0, UnitKind::Harvester, 6, 8)],
        vec![building(0, BuildingKind::Turret, 10, 8)],
        500,
    );
    let welder = ids(&state)[0];
    let turret = site(&state, BuildingKind::Turret, 10, 8);
    wound(&mut state, turret, 30);
    run(&mut state, 3);
    let view = oxide_sim::observation::ObservationData::fog_honest(&state, PlayerId(0));
    let worker = view.my_units.iter().find(|u| u.id == welder).unwrap();
    assert!(!worker.idle);
    assert_eq!(
        worker.repair,
        Some(oxide_sim::observation::RepairWork::Tending)
    );
    assert_eq!(view.repair_target(welder), Some(Target::Building(turret)));
}
