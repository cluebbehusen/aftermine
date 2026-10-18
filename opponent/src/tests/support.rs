use super::*;
use crate::defenses;
use crate::expansion;
use crate::frame::gap;
use crate::investments::Investment;
use crate::memory::Memory;
use oxide_sim::Target;
use oxide_sim::observation::ObservationData;

/// `state` with `building` at `hp`.
fn damaged(state: &State, building: BuildingId, hp: u32) -> State {
    let mut value = serde_json::to_value(state).unwrap();
    let buildings = value["buildings"].as_array_mut().unwrap();
    let entry = buildings
        .iter_mut()
        .find(|entry| entry["id"] == building.0)
        .unwrap();
    entry["hp"] = hp.into();
    serde_json::from_value(value).unwrap()
}

/// `state` with each of `units` at `hp`.
fn all_wounded(state: &State, units: &[UnitId], hp: u32) -> State {
    units
        .iter()
        .fold(state.clone(), |state, unit| wounded(&state, *unit, hp))
}

fn repairs(commands: &[PlayerCommand]) -> Vec<(Vec<UnitId>, BuildingId)> {
    commands
        .iter()
        .filter_map(|command| match &command.command {
            Command::Repair {
                units, building, ..
            } => Some((units.clone(), *building)),
            _ => None,
        })
        .collect()
}

fn welds(commands: &[PlayerCommand]) -> Vec<(Vec<UnitId>, UnitId)> {
    commands
        .iter()
        .filter_map(|command| match &command.command {
            Command::RepairUnit { units, target, .. } => Some((units.clone(), *target)),
            _ => None,
        })
        .collect()
}

/// West Sentinel spots near home, clear of the arena's scrap.
const SQUAD: [(i32, i32); 8] = [
    (2, 8),
    (3, 8),
    (4, 8),
    (5, 8),
    (2, 10),
    (3, 10),
    (4, 10),
    (5, 10),
];

/// The arena with West's Sentinels on `SQUAD`, and their ids.
fn squad(scrap: u32) -> (Scenario, impl Fn(&State) -> Vec<UnitId>) {
    let mut scenario = arena(scrap);
    for (x, y) in SQUAD {
        scenario.units.push(unit(0, UnitKind::Sentinel, x, y));
    }
    let ids = |state: &State| SQUAD.iter().map(|(x, y)| at(state, *x, *y)).collect();
    (scenario, ids)
}

#[test]
fn a_fabricator_turns_two_open_harvester_slots_into_an_excavator() {
    let trained = |fabricator: bool| {
        let mut scenario = arena(1_000);
        scenario.units.extend(standing_army(0));
        if fabricator {
            scenario
                .buildings
                .push(building(0, BuildingKind::Fabricator, 3, 1));
        }
        let state = scenario.build().unwrap();
        trains(&seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default()))
            .into_iter()
            .map(|(_, kind)| kind)
            .collect::<Vec<_>>()
    };
    let without = trained(false);
    assert!(without.contains(&UnitKind::Harvester), "{without:?}");
    let with = trained(true);
    assert!(with.contains(&UnitKind::Excavator), "{with:?}");
    assert!(
        !with.contains(&UnitKind::Harvester),
        "the Excavator fills both open slots: {with:?}"
    );
}

#[test]
fn workers_weld_a_damaged_building_unless_an_enemy_stands_near() {
    let welded = |raider: bool| {
        let mut scenario = arena(200);
        if raider {
            scenario.units.push(unit(1, UnitKind::Sentinel, 10, 6));
        }
        let state = scenario.build().unwrap();
        let foundry = foundries(&state, PlayerId(0))[0];
        let max = BuildingKind::Foundry.base_stats().max_hp;
        let state = damaged(&state, foundry, max / 2);
        let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
        (foundry, repairs(&commands))
    };
    let (foundry, calm) = welded(false);
    let [(welders, patient)] = &calm[..] else {
        panic!("{calm:?}");
    };
    assert_eq!(*patient, foundry);
    assert_eq!(welders.len(), 1, "one worker at a time");
    let (_, raided) = welded(true);
    assert!(raided.is_empty(), "{raided:?}");
}

#[test]
fn each_damaged_building_gets_its_own_welder() {
    let mut scenario = arena(400);
    scenario.units.push(harvester(0, 5, 7));
    scenario.buildings.extend([
        building(0, BuildingKind::Fabricator, 3, 1),
        building(0, BuildingKind::Turret, 10, 7),
    ]);
    let state = scenario.build().unwrap();
    let own: Vec<BuildingId> = state
        .buildings()
        .iter()
        .filter(|building| building.player == PlayerId(0))
        .map(|building| building.id)
        .collect();
    assert_eq!(own.len(), 3, "premise");
    let state = own.iter().fold(state, |state, id| {
        let kind = state
            .buildings()
            .iter()
            .find(|building| building.id == *id)
            .unwrap()
            .kind;
        damaged(&state, *id, kind.base_stats().max_hp / 2)
    });
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    let mut patients: Vec<BuildingId> = repairs(&commands)
        .into_iter()
        .map(|(welders, patient)| {
            assert_eq!(welders.len(), 1, "{commands:?}");
            patient
        })
        .collect();
    patients.sort_unstable();
    assert_eq!(patients, own);
}

#[test]
fn idle_workers_keep_off_a_node_in_known_danger() {
    let nodes = |raider: bool| {
        let mut scenario = arena(0);
        if raider {
            scenario.units.push(unit(1, UnitKind::Sentinel, 9, 10));
        }
        let state = scenario.build().unwrap();
        let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
        harvests(&commands)
            .into_iter()
            .map(|(node, _)| node)
            .collect::<Vec<_>>()
    };
    let guarded = TilePos::new(7, 9);
    assert!(
        nodes(false).contains(&guarded),
        "premise: {:?}",
        nodes(false)
    );
    let sent = nodes(true);
    assert!(!sent.is_empty() && !sent.contains(&guarded), "{sent:?}");
}

#[test]
fn a_worker_away_from_home_runs_from_an_armed_enemy() {
    let mut scenario = arena(0);
    scenario
        .units
        .extend([harvester(0, 15, 9), unit(1, UnitKind::Sentinel, 18, 10)]);
    let state = scenario.build().unwrap();
    let worker = at(&state, 15, 9);
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    let (_, refuge) = runs(&commands)
        .into_iter()
        .find(|(units, _)| *units == [worker])
        .unwrap_or_else(|| panic!("{commands:?}"));
    assert_eq!(
        gap(TilePos::new(3, 5), (2, 2), refuge, (1, 1)),
        0,
        "beside the Foundry: {refuge:?}"
    );
}

#[test]
fn a_worker_away_from_home_runs_from_artillery_in_range() {
    let mut scenario = arena(0);
    scenario.units.extend([
        harvester(0, 15, 9),
        unit(1, UnitKind::Bombard, 15, 1),
        unit(0, UnitKind::Kestrel, 15, 3),
    ]);
    let state = scenario.build().unwrap();
    let worker = at(&state, 15, 9);
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    assert!(
        runs(&commands).iter().any(|(units, _)| *units == [worker]),
        "a Bombard eight tiles off still reaches it: {commands:?}"
    );
}

/// West's arena with Sentinels at `spots` and the scrap to pay for welds.
fn wounded_at(scrap: u32, spots: &[(i32, i32)], hp: u32) -> (Scenario, State, Vec<UnitId>) {
    let mut scenario = arena(scrap);
    for (x, y) in spots {
        scenario.units.push(unit(0, UnitKind::Sentinel, *x, *y));
    }
    let state = scenario.build().unwrap();
    let hurt: Vec<UnitId> = spots.iter().map(|(x, y)| at(&state, *x, *y)).collect();
    let state = all_wounded(&state, &hurt, hp);
    (scenario, state, hurt)
}

fn harvesters(state: &State, player: u8) -> Vec<UnitId> {
    state
        .units()
        .iter()
        .filter(|unit| unit.player == PlayerId(player) && unit.kind == UnitKind::Harvester)
        .map(|unit| unit.id)
        .collect()
}

#[test]
fn a_worker_welds_the_most_wounded_unit_at_home_while_scrap_pays() {
    let welded = |scrap: u32| {
        let mut scenario = arena(scrap);
        scenario.units.extend([
            unit(0, UnitKind::Sentinel, 3, 10),
            unit(0, UnitKind::Sentinel, 4, 10),
        ]);
        let state = scenario.build().unwrap();
        let (grazed, mauled) = (at(&state, 3, 10), at(&state, 4, 10));
        let state = wounded(&wounded(&state, grazed, 40), mauled, 10);
        let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
        (harvesters(&state, 0), mauled, welds(&commands))
    };
    let (workers, mauled, paid) = welded(1_000);
    let [(welders, patient)] = &paid[..] else {
        panic!("one weld: {paid:?}");
    };
    assert_eq!(*patient, mauled);
    assert!(
        welders.len() == 1 && workers.contains(&welders[0]),
        "{paid:?}"
    );
    let (_, _, short) = welded(0);
    assert!(
        short.is_empty(),
        "a weld would stall for want of scrap: {short:?}"
    );
}

#[test]
fn an_excavator_welds_before_a_nearer_harvester() {
    let (mut scenario, _, _) = wounded_at(1_000, &[(8, 8)], 10);
    scenario.units.push(unit(0, UnitKind::Excavator, 1, 8));
    let state = scenario.build().unwrap();
    let (patient, excavator) = (at(&state, 8, 8), at(&state, 1, 8));
    let state = wounded(&state, patient, 10);
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    assert_eq!(welds(&commands), [(vec![excavator], patient)]);
}

#[test]
fn heavy_wounds_draw_more_welders_but_never_more_than_their_patients() {
    let crew = [(1, 7), (1, 8), (2, 7), (2, 8)];
    let welders = |kind: UnitKind, spots: &[(i32, i32)]| {
        let mut scenario = arena(5_000);
        for (x, y) in spots {
            scenario.units.push(unit(0, kind, *x, *y));
        }
        for (x, y) in crew {
            scenario.units.push(unit(0, UnitKind::Harvester, x, y));
        }
        let state = scenario.build().unwrap();
        let hurt: Vec<UnitId> = spots.iter().map(|(x, y)| at(&state, *x, *y)).collect();
        let state = all_wounded(&state, &hurt, 1);
        welds(&seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default())).len()
    };
    assert!(welders(UnitKind::Bombard, &SQUAD) > 1);
    assert_eq!(welders(UnitKind::Breaker, &[(4, 9)]), 1);
}

#[test]
fn workers_weld_no_unit_under_threat_or_away_from_home() {
    let (mut scenario, _, _) = wounded_at(1_000, &[(4, 10)], 10);
    // Anti-air only, so home defense leaves the patient free; armed all the
    // same, so the weld waits.
    scenario.units.push(unit(1, UnitKind::Flakhound, 9, 10));
    let state = scenario.build().unwrap();
    let state = wounded(&state, at(&state, 4, 10), 10);
    let near_enemy = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    assert!(welds(&near_enemy).is_empty(), "{near_enemy:?}");

    let (scenario, state, _) = wounded_at(1_000, &[(14, 9)], 10);
    let away = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    assert!(welds(&away).is_empty(), "{away:?}");
}

#[test]
fn a_damaged_building_is_welded_before_a_wounded_unit() {
    let (mut scenario, _, _) = wounded_at(1_000, &[(4, 10)], 10);
    scenario.units.retain(|unit| (unit.x, unit.y) != (6, 6));
    let state = scenario.build().unwrap();
    let foundry = state
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0))
        .unwrap()
        .id;
    let state = damaged(&wounded(&state, at(&state, 4, 10), 10), foundry, 100);
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    assert_eq!(repairs(&commands).len(), 1, "{commands:?}");
    assert!(welds(&commands).is_empty(), "{commands:?}");
}

#[test]
fn mirrored_seats_weld_mirrored() {
    let mut scenario = arena(1_000);
    let (width, height) = (24, 12);
    for (x, y) in [(3, 10), (4, 10)] {
        scenario.units.push(unit(0, UnitKind::Sentinel, x, y));
        scenario
            .units
            .push(unit(1, UnitKind::Sentinel, width - 1 - x, height - 1 - y));
    }
    let state = scenario.build().unwrap();
    let hurt: Vec<UnitId> = [(3, 10), (4, 10), (20, 1), (19, 1)]
        .iter()
        .map(|(x, y)| at(&state, *x, *y))
        .collect();
    let state = all_wounded(&state, &hurt, 10);
    let west = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let east = seat(&scenario, 1).act(&state, &mut OwnEvents::default());
    assert!(!welds(&west).is_empty(), "premise: {west:?}");
    assert_eq!(mirror(&state, west), east);
}

/// `state` at West's next decision after its `worker` was sent to weld
/// `patient`.
fn welding(state: &State, worker: UnitId, patient: UnitId) -> State {
    let mut state = state.clone();
    advance_to(
        &mut state,
        12,
        &[PlayerCommand {
            player: PlayerId(0),
            command: Command::RepairUnit {
                units: vec![worker],
                target: patient,
                queue: false,
            },
        }],
    );
    state
}

/// `state` with every worker's idle welding switched off, so a weld comes
/// only from the bot's own orders.
fn hands_off(state: &State) -> State {
    let mut value = serde_json::to_value(state).unwrap();
    for entry in value["units"].as_array_mut().unwrap() {
        if entry.get("worker").is_some() {
            entry["worker"]["idle_weld_off"] = true.into();
        }
    }
    serde_json::from_value(value).unwrap()
}

/// `state` with `worker` out welding `patient` on its own, on a trip from
/// where it stands.
fn tending(state: &State, worker: UnitId, patient: Target) -> State {
    let post = state.unit(worker).unwrap().tile();
    let mut value = serde_json::to_value(state).unwrap();
    let entry = value["units"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["id"] == worker.0)
        .unwrap();
    entry["order"] = serde_json::json!({
        "order": "tend",
        "goal": {"x": post.x, "y": post.y},
        "weld": patient,
    });
    serde_json::from_value(value).unwrap()
}

#[test]
fn a_tending_welders_patient_at_home_takes_no_second_welder() {
    let (scenario, state, hurt) = wounded_at(1_000, &[(3, 10)], 10);
    let tender = harvesters(&state, 0)[0];
    let state = tending(&state, tender, Target::Unit(hurt[0]));
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    assert!(
        welds(&commands)
            .iter()
            .all(|(_, patient)| *patient != hurt[0]),
        "{commands:?}"
    );
    assert!(
        !commands.iter().any(
            |command| matches!(&command.command, Command::Stop { units } if units.contains(&tender))
        ),
        "{commands:?}"
    );
}

#[test]
fn a_tending_welder_away_from_home_is_left_to_its_trip() {
    let (scenario, state, hurt) = wounded_at(1_000, &[(14, 8)], 10);
    let tender = harvesters(&state, 0)[0];
    let state = tending(&state, tender, Target::Unit(hurt[0]));
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    assert!(
        !commands.iter().any(
            |command| matches!(&command.command, Command::Stop { units } if units.contains(&tender))
        ),
        "{commands:?}"
    );
}

#[test]
fn a_tending_worker_away_from_home_runs_from_an_armed_enemy() {
    let mut scenario = arena(0);
    scenario.units.extend([
        harvester(0, 15, 9),
        unit(0, UnitKind::Sentinel, 14, 9),
        unit(1, UnitKind::Sentinel, 18, 10),
    ]);
    let state = scenario.build().unwrap();
    let (worker, patient) = (at(&state, 15, 9), at(&state, 14, 9));
    let state = tending(&wounded(&state, patient, 10), worker, Target::Unit(patient));
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    assert!(
        runs(&commands).iter().any(|(units, _)| *units == [worker]),
        "{commands:?}"
    );
}

#[test]
fn a_second_wound_gets_its_own_welder_while_the_first_weld_runs() {
    let mut scenario = arena(5_000);
    // Spare hands, since such wounds also call for a Repair Bay.
    scenario.units.extend([
        unit(0, UnitKind::Breaker, 2, 9),
        unit(0, UnitKind::Breaker, 5, 10),
        unit(0, UnitKind::Harvester, 1, 7),
        unit(0, UnitKind::Harvester, 1, 8),
    ]);
    // The spare hands would otherwise weld the second wound on their own
    // before the bot decides.
    let state = hands_off(&scenario.build().unwrap());
    let (first, second) = (at(&state, 2, 9), at(&state, 5, 10));
    let state = all_wounded(&state, &[first, second], 1);
    let welder = harvesters(&state, 0)[0];
    let state = welding(&state, welder, first);
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    assert!(
        welds(&commands)
            .iter()
            .any(|(units, patient)| *patient == second && !units.contains(&welder)),
        "{commands:?}"
    );
}

#[test]
fn a_welder_whose_patient_leaves_home_goes_back_to_work() {
    let (scenario, state, hurt) = wounded_at(1_000, &[(14, 8)], 10);
    let welder = harvesters(&state, 0)[0];
    let state = welding(&state, welder, hurt[0]);
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    assert!(
        commands.iter().any(|command| matches!(
            &command.command,
            Command::Stop { units } if *units == [welder]
        )),
        "{commands:?}"
    );
}

/// An attack regrouping at its rally with the arena's West squad, one of
/// them still walking in, and the squad's ids.
fn regrouping(scrap: u32) -> (State, Opponent, Vec<UnitId>) {
    let (scenario, ids) = squad(scrap);
    let mut state = scenario.build().unwrap();
    let squad = ids(&state);
    let walker = squad[7];
    state.tick(&[run(0, vec![walker], 5, 11)]);
    while state.current_tick() < 12 {
        state.tick(&[]);
    }
    let opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
        json["missions"] = serde_json::json!({
            "next": 1,
            "list": [{
                "id": 0,
                "task": {
                    "task": "attack",
                    "target": {"owner": 1, "building": "foundry", "anchor": {"x": 19, "y": 5}},
                    "phase": "recover",
                },
                "since": 12,
                "units": squad,
                "goal": {"x": 6, "y": 9},
            }],
        });
    });
    (state, opponent, squad)
}

#[test]
fn workers_weld_a_regrouping_attack_at_its_rally_while_scrap_pays() {
    let welded = |scrap: u32| {
        let (state, mut opponent, squad) = regrouping(scrap);
        let hurt = squad[0];
        let state = wounded(&state, hurt, 30);
        let commands = opponent.act(&state, &mut OwnEvents::default());
        (hurt, welds(&commands))
    };
    let (hurt, paid) = welded(1_000);
    assert!(
        matches!(&paid[..], [(welders, patient)] if *patient == hurt && welders.len() == 1),
        "{paid:?}"
    );
    let (_, short) = welded(0);
    assert!(short.is_empty(), "{short:?}");
}

/// Where West would put a Repair Bay in `state`, if anywhere.
fn bay(scenario: &Scenario, state: &State) -> Option<TilePos> {
    let observation = ObservationData::fog_honest(state, PlayerId(0));
    defenses::investments(
        &observation,
        &map(scenario),
        &Memory::default(),
        middling(),
        true,
        false,
        defenses::Stakes::default(),
    )
    .into_iter()
    .find_map(|(investment, _)| match investment {
        Investment::Defense {
            kind: BuildingKind::RepairBay,
            anchor,
        } => Some(anchor),
        _ => None,
    })
}

#[test]
fn a_repair_bay_goes_up_where_the_wounded_gather() {
    let (mut scenario, _) = squad(0);
    let hurt = |scenario: &Scenario, hp: u32| {
        let state = scenario.build().unwrap();
        let sentinels: Vec<UnitId> = state
            .units()
            .iter()
            .filter(|unit| unit.player == PlayerId(0) && unit.kind == UnitKind::Sentinel)
            .map(|unit| unit.id)
            .collect();
        all_wounded(&state, &sentinels, hp)
    };
    let healthy = hurt(&scenario, UnitKind::Sentinel.stats().max_hp);
    assert_eq!(bay(&scenario, &healthy), None, "nobody is hurt");
    let anchor = bay(&scenario, &hurt(&scenario, 1)).expect("a Repair Bay is wanted");
    let reached = SQUAD
        .iter()
        .filter(|(x, y)| gap(anchor, (2, 2), TilePos::new(*x, *y), (1, 1)) <= 3)
        .count();
    assert!(reached >= SQUAD.len() / 2, "{anchor:?} reaches {reached}");

    scenario
        .units
        .retain(|unit| gap(anchor, (2, 2), TilePos::new(unit.x, unit.y), (1, 1)) >= 0);
    scenario
        .buildings
        .push(building(0, BuildingKind::RepairBay, anchor.x, anchor.y));
    let second = bay(&scenario, &hurt(&scenario, 1));
    assert!(
        second.is_none_or(|second| gap(anchor, (2, 2), second, (2, 2)) > 3),
        "a second Bay only for wounded the first cannot reach: {second:?}"
    );
}

#[test]
fn wounded_a_full_repair_bay_cannot_treat_call_for_another() {
    let anchor = TilePos::new(9, 7);
    let around = [
        (8, 6),
        (9, 6),
        (10, 6),
        (11, 6),
        (8, 9),
        (9, 9),
        (10, 9),
        (11, 9),
    ];
    let wanted = |count: usize, moving: bool| {
        let mut scenario = arena(0);
        scenario
            .buildings
            .push(building(0, BuildingKind::RepairBay, anchor.x, anchor.y));
        for (x, y) in &around[..count] {
            scenario.units.push(unit(0, UnitKind::Sentinel, *x, *y));
        }
        let state = scenario.build().unwrap();
        let hurt: Vec<UnitId> = around[..count]
            .iter()
            .map(|(x, y)| at(&state, *x, *y))
            .collect();
        let mut state = all_wounded(&state, &hurt, 1);
        if moving {
            state.tick(&[run(0, hurt, 20, 9)]);
        }
        bay(&scenario, &state)
    };
    assert_eq!(wanted(4, false), None, "one Bay treats four");
    assert!(wanted(8, false).is_some(), "four more need a second Bay");
    assert_eq!(
        wanted(8, true),
        None,
        "wounded passing through call for none"
    );
}

#[test]
fn an_extractor_frame_waits_while_an_armed_enemy_stands_near() {
    let mut scenario = arena(0);
    scenario.map[1].replace_range(6..7, "E");
    let frame = TilePos::new(6, 1);
    let wanted = |scenario: &Scenario, memory: &mut Memory| {
        let state = scenario.build().unwrap();
        let observation = ObservationData::fog_honest(&state, PlayerId(0));
        memory.observe(&observation);
        expansion::candidates(&observation, &map(scenario), memory, 50, 0)
            .iter()
            .any(|(investment, _)| *investment == Investment::Extractor(frame))
    };
    let mut memory = Memory::default();
    assert!(wanted(&scenario, &mut memory), "premise");
    let mut raided = scenario.clone();
    raided.units.push(unit(1, UnitKind::Sentinel, 8, 2));
    assert!(!wanted(&raided, &mut memory));
    assert!(
        wanted(&scenario, &mut memory),
        "once the enemy is out of sight the frame is wanted again"
    );
}

#[test]
fn a_building_walled_in_on_every_side_is_not_welded() {
    let welded = |boxed: bool| {
        let mut scenario = arena(200);
        // A Turret in the arena's corner, rock on two sides and Barricades
        // on the rest.
        scenario
            .buildings
            .push(building(0, BuildingKind::Turret, 1, 1));
        let walls: &[(i32, i32)] = if boxed {
            &[(2, 1), (1, 2), (2, 2)]
        } else {
            &[(2, 1), (1, 2)]
        };
        for (x, y) in walls {
            scenario
                .buildings
                .push(building(0, BuildingKind::Barricade, *x, *y));
        }
        let state = scenario.build().unwrap();
        let turret = state
            .buildings()
            .iter()
            .find(|building| building.kind == BuildingKind::Turret)
            .unwrap()
            .id;
        let max = BuildingKind::Turret.base_stats().max_hp;
        let state = damaged(&state, turret, max / 2);
        let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
        repairs(&commands)
            .iter()
            .any(|(_, patient)| *patient == turret)
    };
    assert!(welded(false), "premise: one free tile is enough");
    assert!(!welded(true));
}
