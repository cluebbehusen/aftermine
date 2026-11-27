//! Tending buildings: idle and patrolling workers repairing damaged own
//! structures on their own, ranked with wounded machines by the nearest
//! point of each. Headless scenarios through the public API: the footprint
//! reach, the shared switch, crew billing, the empty bank, the buildings
//! tending leaves alone, the crew cap and known danger.

use crate::common;
use common::{
    CommandExt, building, building_id, cmd, open_arena, open_arena_with, orders, run, run_until,
    unit,
};

use aftermine_sim::scenario::{BuildingSpec, UnitSpec};
use aftermine_sim::stats::BuildingKind;
use aftermine_sim::{BuildingId, Command, Event, Order, PlayerId, State, Target, UnitId, UnitKind};
use chassis::grid::TilePos;
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
    building_id(state, |b| b.kind == kind && b.anchor == TilePos::new(x, y))
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
    crewed.tick(&[cmd(0, orders::repair([welder], turret))]);
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
        orders::patrol([welder], [TilePos::new(4, 10), TilePos::new(24, 10)]),
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
        .tick(&[cmd(0, orders::salvage([sentinel], turret))])
        .events;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::CommandRejected { .. }))
    );
    assert_eq!(patient(&state, tender), Some(Target::Building(turret)));

    // An accepted one takes the building from its tender that same tick,
    // and a queued one keeps it from being taken on again.
    state.tick(&[cmd(0, orders::run([stripper], TilePos::new(4, 14)))]);
    state.tick(&[cmd(0, orders::salvage([stripper], turret).queued())]);
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
        orders::build([builder], BuildingKind::Barricade, TilePos::new(9, 12)),
    )]);
    run_until(&mut state, 400, |s, _| {
        s.buildings().iter().any(|b| {
            b.kind == BuildingKind::Barricade && b.construction_progress().is_some_and(|p| p > 0)
        })
    });
    state.tick(&[cmd(0, orders::stop([builder]))]);
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
    let view = aftermine_sim::observation::ObservationData::fog_honest(&state, PlayerId(0));
    let worker = view.my_units.iter().find(|u| u.id == welder).unwrap();
    assert!(!worker.idle);
    assert_eq!(
        worker.repair,
        Some(aftermine_sim::observation::RepairWork::Tending)
    );
    assert_eq!(view.repair_target(welder), Some(Target::Building(turret)));
}

/// The welders whose trip from an idle post holds `building`.
fn tenders(state: &State, building: BuildingId) -> Vec<UnitId> {
    state
        .units()
        .iter()
        .filter(|u| {
            !u.looping
                && matches!(u.order, Order::Tend { weld: Some(Target::Building(b)), .. } if b == building)
        })
        .map(|u| u.id)
        .collect()
}

/// Six idle Harvesters at staggered distances west of a Fabricator at
/// (12, 8) that misses most of its health.
fn crowd() -> (State, BuildingId) {
    let spots = [(10, 8), (10, 9), (9, 7), (9, 10), (8, 8), (7, 9)];
    let mut state = build(
        spots
            .iter()
            .map(|&(x, y)| unit(0, UnitKind::Harvester, x, y))
            .collect(),
        vec![building(0, BuildingKind::Fabricator, 12, 8)],
        2_000,
    );
    let fabricator = site(&state, BuildingKind::Fabricator, 12, 8);
    let missing = max_hp(&state, fabricator) * 3 / 4;
    wound(&mut state, fabricator, missing);
    (state, fabricator)
}

#[test]
fn at_most_four_workers_tend_one_structure_and_the_rest_wait() {
    let (mut state, fabricator) = crowd();
    let mut events = Vec::new();
    for _ in 0..40 {
        events.extend(run(&mut state, 1));
        assert!(
            tenders(&state, fabricator).len() <= 4 + 2,
            "joiners past the cap let go at once"
        );
    }
    let crew = tenders(&state, fabricator);
    assert_eq!(crew.len(), 4, "{crew:?}");
    let nearest: Vec<UnitId> = ids(&state)[..4].to_vec();
    let mut held = crew.clone();
    held.sort_unstable();
    assert_eq!(held, nearest, "the nearest four keep it");
    assert_eq!(stalls(&events), 0);

    // One of the crew sent off frees a place, which a waiting worker takes.
    let sent = crew[0];
    state.tick(&[cmd(0, orders::run([sent], TilePos::new(4, 14)))]);
    run(&mut state, 20);
    let crew = tenders(&state, fabricator);
    assert_eq!(crew.len(), 4, "{crew:?}");
    assert!(!crew.contains(&sent));
}

/// A West welder by a damaged Fabricator, an East Bombard ten tiles off
/// that sees nothing of West, and with `spotter` a West Surveyor that sees
/// the Bombard; `sentinels` West Sentinels stand by the Fabricator.
fn under_fire(spotter: bool, sentinels: i32) -> (State, UnitId, BuildingId) {
    let mut units = vec![
        unit(0, UnitKind::Harvester, 9, 9),
        unit(1, UnitKind::Bombard, 23, 9),
    ];
    if spotter {
        units.push(unit(0, UnitKind::Surveyor, 16, 4));
    }
    for x in 11..11 + sentinels {
        units.push(unit(0, UnitKind::Sentinel, x, 11));
    }
    let mut state = build(
        units,
        vec![building(0, BuildingKind::Fabricator, 12, 8)],
        500,
    );
    let fabricator = site(&state, BuildingKind::Fabricator, 12, 8);
    wound(&mut state, fabricator, 200);
    let welder = ids(&state)[0];
    (state, welder, fabricator)
}

#[test]
fn a_seen_enemy_keeps_idle_workers_off_a_structure_but_one_in_fog_does_not() {
    let (mut seen, welder, fabricator) = under_fire(true, 0);
    assert!(seen.vision(PlayerId(0)).visible(TilePos::new(23, 9)));
    run(&mut seen, 20);
    assert_eq!(seen.unit(welder).unwrap().order, Order::Idle);

    let (mut fogged, welder, _) = under_fire(false, 0);
    assert!(!fogged.vision(PlayerId(0)).visible(TilePos::new(23, 9)));
    run(&mut fogged, 20);
    assert_eq!(
        patient(&fogged, welder),
        Some(Target::Building(fabricator)),
        "an enemy unseen holds nobody back"
    );
}

#[test]
fn armed_units_that_outweigh_the_enemy_let_idle_workers_weld() {
    for (sentinels, welds) in [(2, false), (3, true)] {
        let (mut state, welder, fabricator) = under_fire(true, sentinels);
        run(&mut state, 20);
        assert_eq!(
            patient(&state, welder) == Some(Target::Building(fabricator)),
            welds,
            "{sentinels} Sentinels"
        );
    }
}

#[test]
fn a_welder_at_work_stays_when_fire_is_seen() {
    // A Surveyor walks up to see the Bombard after the welder has started:
    // the welder stays on, while a worker idle from then on keeps off.
    let state = build(
        vec![
            unit(0, UnitKind::Harvester, 9, 9),
            unit(1, UnitKind::Bombard, 23, 9),
            unit(0, UnitKind::Surveyor, 4, 15),
            unit(0, UnitKind::Harvester, 9, 7),
        ],
        vec![building(0, BuildingKind::Fabricator, 12, 8)],
        500,
    );
    let [welder, _, surveyor, late] = ids(&state)[..] else {
        unreachable!()
    };
    let mut state = common::idle_welds_off(&state, &[late]);
    let fabricator = site(&state, BuildingKind::Fabricator, 12, 8);
    wound(&mut state, fabricator, 400);
    run(&mut state, 20);
    assert_eq!(patient(&state, welder), Some(Target::Building(fabricator)));

    state.tick(&[cmd(0, orders::run([surveyor], TilePos::new(16, 4)))]);
    run_until(&mut state, 600, |s, _| {
        s.unit(surveyor).unwrap().order == Order::Idle
    });
    assert!(state.vision(PlayerId(0)).visible(TilePos::new(23, 9)));
    state.tick(&[cmd(
        0,
        Command::SetIdleWeld {
            units: vec![late],
            enabled: true,
        },
    )]);
    run(&mut state, 20);
    assert_eq!(patient(&state, welder), Some(Target::Building(fabricator)));
    assert_eq!(state.unit(late).unwrap().order, Order::Idle);
}

#[test]
fn an_ordered_crew_or_a_patrol_welds_regardless() {
    // Six ordered to repair all weld, past the cap; a patrol welds on its
    // circuit with the enemy seen.
    let (state, fabricator) = crowd();
    let crew = ids(&state);
    let mut state = common::idle_welds_off(&state, &crew);
    state.tick(&[cmd(0, orders::repair(crew.clone(), fabricator))]);
    let events = run(&mut state, 60);
    let repairing = state
        .units()
        .iter()
        .filter(|u| matches!(u.order, Order::Repair { building } if building == fabricator))
        .count();
    assert_eq!(repairing, 6);
    assert_eq!(stalls(&events), 0);

    let (seen, welder, fabricator) = under_fire(true, 0);
    let mut seen = common::idle_welds_off(&seen, &[welder]);
    seen.tick(&[cmd(
        0,
        orders::patrol([welder], [TilePos::new(9, 9), TilePos::new(9, 12)]),
    )]);
    run(&mut seen, 40);
    assert_eq!(patient(&seen, welder), Some(Target::Building(fabricator)));
    assert!(seen.unit(welder).unwrap().looping);
}

#[test]
fn mirrored_seats_keep_mirrored_crews() {
    // Each seat's six workers stand at mirrored spots, their ids
    // interleaved: the same spots keep each Fabricator.
    let mirror = |x: i32, y: i32| (31 - x, 17 - y);
    let spots = [(10, 8), (10, 9), (9, 7), (9, 10), (8, 8), (7, 9)];
    let mut units = Vec::new();
    for &(x, y) in &spots {
        units.push(unit(0, UnitKind::Harvester, x, y));
        let (mx, my) = mirror(x, y);
        units.push(unit(1, UnitKind::Harvester, mx, my));
    }
    let (mx, my) = mirror(13, 9);
    let mut state = build(
        units,
        vec![
            building(0, BuildingKind::Fabricator, 12, 8),
            building(1, BuildingKind::Fabricator, mx, my),
        ],
        2_000,
    );
    let west = site(&state, BuildingKind::Fabricator, 12, 8);
    let east = site(&state, BuildingKind::Fabricator, mx, my);
    for id in [west, east] {
        let missing = max_hp(&state, id) * 3 / 4;
        wound(&mut state, id, missing);
    }
    run(&mut state, 40);
    let spots_of = |crew: &[UnitId]| -> Vec<usize> {
        let all = ids(&state);
        let mut spots: Vec<usize> = crew
            .iter()
            .map(|id| all.iter().position(|u| u == id).unwrap() / 2)
            .collect();
        spots.sort_unstable();
        spots
    };
    let (west_crew, east_crew) = (tenders(&state, west), tenders(&state, east));
    assert_eq!(west_crew.len(), 4);
    assert_eq!(spots_of(&west_crew), spots_of(&east_crew));
}
