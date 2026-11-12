use super::lift::strait;
use super::*;

/// West's two Condors at home on the strait.
const BOMBERS: [(i32, i32); 2] = [(12, 2), (12, 21)];

/// `count` Flakhound tiles around East's start.
fn around_the_start(count: i32) -> Vec<(i32, i32)> {
    (0..count)
        .map(|index| (31 + index % 3, 9 + index / 3))
        .collect()
}

/// West on `scenario`, remembering East Flakhounds seen now at `flak`.
fn bombing(scenario: &Scenario, flak: &[(i32, i32)]) -> (State, Opponent) {
    let state = scenario.build().unwrap();
    let opponent = restored_with(&seat(scenario, 0), scenario, &state, |json| {
        json["memory"]["units"] = remembered(
            (1_000..)
                .zip(flak)
                .map(|(id, &(x, y))| (id, UnitKind::Flakhound, TilePos::new(x, y), 0)),
        );
    });
    (state, opponent)
}

fn with_bombers(mut scenario: Scenario) -> Scenario {
    for (x, y) in BOMBERS {
        scenario.units.push(unit(0, UnitKind::Condor, x, y));
    }
    scenario
}

fn clear(missions: &[MissionStatus]) -> Option<MissionStatus> {
    missions
        .iter()
        .copied()
        .find(|mission| matches!(mission.kind, MissionKind::Clear { .. }))
}

#[test]
fn splash_bombers_go_together_at_the_anti_air_around_the_lift_target() {
    let scenario = with_bombers(strait());
    // Enough anti-air to call for both bombers.
    let flak = around_the_start(4);
    let (state, mut opponent) = bombing(&scenario, &flak);
    let condors: Vec<UnitId> = BOMBERS.iter().map(|(x, y)| at(&state, *x, *y)).collect();
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    let mission = clear(&trace.unwrap().missions).expect("a clearance forms");
    let MissionKind::Clear { aim } = mission.kind else {
        unreachable!();
    };
    assert!(
        flak.contains(&(aim.x, aim.y)),
        "at the remembered anti-air: {aim:?}"
    );
    let sent: Vec<(Vec<UnitId>, TilePos)> = runs(&commands)
        .into_iter()
        .filter(|(units, _)| units.iter().any(|id| condors.contains(id)))
        .collect();
    let [(units, _)] = &sent[..] else {
        panic!("{commands:?}");
    };
    let mut units = units.clone();
    units.sort_unstable();
    assert_eq!(units, condors, "both bombers, in one order");
}

#[test]
fn a_clearance_takes_only_the_bombers_the_anti_air_calls_for() {
    let mut scenario = with_bombers(strait());
    for (x, y) in [(12, 4), (12, 19), (11, 2), (11, 21)] {
        scenario.units.push(unit(0, UnitKind::Condor, x, y));
    }
    let sent = |flak: i32| {
        let (state, mut opponent) = bombing(&scenario, &around_the_start(flak));
        let condors: Vec<UnitId> = state
            .units()
            .iter()
            .filter(|unit| unit.kind == UnitKind::Condor)
            .map(|unit| unit.id)
            .collect();
        let commands = opponent.act(&state, &mut OwnEvents::default());
        let groups: Vec<Vec<UnitId>> = runs(&commands)
            .into_iter()
            .map(|(units, _)| units)
            .filter(|units| units.iter().any(|id| condors.contains(id)))
            .collect();
        let [units] = &groups[..] else {
            panic!("one clearance order: {commands:?}");
        };
        (units.len(), condors.len())
    };
    let (light, all) = sent(3);
    let (heavy, _) = sent(4);
    assert!(
        light < heavy && heavy < all,
        "{light} then {heavy} of {all}"
    );
}

#[test]
fn bombers_go_only_once_they_outweigh_all_the_anti_air_around_the_target() {
    // Two groups north and south of East's start, each of which the bombers
    // outweigh alone.
    let groups: Vec<(i32, i32)> = [1, 20]
        .into_iter()
        .flat_map(|y| [(29, y), (30, y), (29, y + 1), (30, y + 1)])
        .collect();
    let scenario = with_bombers(strait());
    let (state, mut opponent) = bombing(&scenario, &groups[..4]);
    let (_, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert!(
        clear(&trace.unwrap().missions).is_some(),
        "premise: one group"
    );
    let (state, mut opponent) = bombing(&scenario, &groups);
    let (_, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert!(clear(&trace.unwrap().missions).is_none());
}

#[test]
fn a_seat_whose_ground_reaches_the_enemy_sends_no_clearance() {
    let mut scenario = with_bombers(field());
    garrison(&mut scenario);
    let (state, mut opponent) = bombing(&scenario, &around_the_start(3));
    let (_, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert!(clear(&trace.unwrap().missions).is_none());
}

#[test]
fn a_seat_that_can_only_lift_keeps_its_splash_bombers_out_of_strikes() {
    let mut scenario = with_bombers(strait());
    // A Fabricator far from the Flakhounds guarding East's start, and a
    // Kestrel that sees both.
    scenario.buildings.push(BuildingSpec {
        player: 1,
        kind: BuildingKind::Fabricator,
        x: 29,
        y: 20,
    });
    scenario.units.push(unit(0, UnitKind::Kestrel, 32, 15));
    let (state, mut opponent) = bombing(&scenario, &around_the_start(12));
    let condors: Vec<u32> = BOMBERS.iter().map(|(x, y)| at(&state, *x, *y).0).collect();
    opponent.act(&state, &mut OwnEvents::default());
    let json = serde_json::to_value(opponent.checkpoint()).unwrap();
    for mission in json["missions"]["list"].as_array().unwrap() {
        let units: Vec<u32> = serde_json::from_value(mission["units"].clone()).unwrap();
        assert!(
            units.iter().all(|id| !condors.contains(id)),
            "{}",
            mission["task"]
        );
    }
}

#[test]
fn a_mid_clearance_checkpoint_resumes_identically() {
    let scenario = with_bombers(strait());
    let (mut state, mut opponent) = bombing(&scenario, &around_the_start(3));
    loop {
        assert!(state.current_tick() < 1_200, "no clearance set out");
        let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
        state.tick(&commands);
        if trace.is_some_and(|trace| {
            clear(&trace.missions).is_some_and(|mission| mission.phase == Phase::Travel)
        }) {
            break;
        }
    }
    while !state.current_tick().is_multiple_of(12) {
        state.tick(&[]);
    }
    resumes_identically(&mut opponent, &scenario, &mut state, 300);
}

#[test]
fn checkpoints_reject_a_clearance_aimed_off_the_map() {
    let scenario = with_bombers(strait());
    let (state, mut opponent) = bombing(&scenario, &around_the_start(3));
    opponent.act(&state, &mut OwnEvents::default());
    let json = serde_json::to_value(opponent.checkpoint()).unwrap();
    let list = json["missions"]["list"].as_array().unwrap();
    let index = list
        .iter()
        .position(|mission| mission["task"]["task"] == "clear")
        .expect("premise: a clearance");
    let restore = |json: serde_json::Value| {
        let checkpoint: Checkpoint = serde_json::from_value(json).unwrap();
        Opponent::restore(&checkpoint, &scenario, &state, map(&scenario)).map(|_| ())
    };
    assert!(restore(json.clone()).is_ok());
    let mut off = json.clone();
    off["missions"]["list"][index]["task"]["aim"] = serde_json::json!({"x": 99, "y": 0});
    assert_eq!(
        restore(off).err().unwrap(),
        "checkpoint mission could not have been recorded"
    );
}

#[test]
fn mirrored_seats_clear_alike() {
    let (width, height) = (40, 24);
    let flip = |(x, y): (i32, i32)| (width - 1 - x, height - 1 - y);
    let mut scenario = with_bombers(strait());
    for (kind, x, y) in super::lift::TECH {
        let (w, h) = kind.size();
        scenario.buildings.push(BuildingSpec {
            player: 1,
            kind,
            x: width - x - w,
            y: height - y - h,
        });
    }
    let east = super::lift::LIFT
        .iter()
        .map(|(kind, x, y)| (*kind, (*x, *y)))
        .chain(BOMBERS.iter().map(|tile| (UnitKind::Condor, *tile)));
    for (kind, tile) in east {
        let (x, y) = flip(tile);
        scenario.units.push(unit(1, kind, x, y));
    }
    let state = scenario.build().unwrap();
    let flak = around_the_start(3);
    let remembering = |player: u8, flak: Vec<(i32, i32)>| {
        restored_with(&seat(&scenario, player), &scenario, &state, |json| {
            json["memory"]["units"] = remembered(
                (1_000..)
                    .zip(flak)
                    .map(|(id, (x, y))| (id, UnitKind::Flakhound, TilePos::new(x, y), 0)),
            );
        })
    };
    let mut seats = [
        remembering(0, flak.clone()),
        remembering(1, flak.into_iter().map(flip).collect()),
    ];
    // The simulation heads a spawned Condor by its column, which a half
    // turn does not mirror, so flights stop mirroring once the bombers turn:
    // compare until the clearance sets out.
    let mut state = state;
    loop {
        assert!(state.current_tick() < 300, "premise: a clearance forms");
        let (west, trace) = seats[0].act_traced(&state, &mut OwnEvents::default());
        let east = seats[1].act(&state, &mut OwnEvents::default());
        assert_eq!(
            mirror(&state, west.clone()),
            east,
            "tick {}",
            state.current_tick()
        );
        if trace.is_some_and(|trace| clear(&trace.missions).is_some()) {
            break;
        }
        let commands: Vec<PlayerCommand> = west.into_iter().chain(east).collect();
        state.tick(&commands);
    }
}

/// West's Roc at home.
const ROC_HOME: (i32, i32) = (11, 11);

fn with_roc(mut scenario: Scenario) -> Scenario {
    scenario
        .units
        .push(unit(0, UnitKind::Roc, ROC_HOME.0, ROC_HOME.1));
    scenario
}

/// The strait with West's Roc and neither carriers nor riders.
fn lone_roc() -> Scenario {
    let mut scenario = with_roc(strait());
    scenario.units.retain(|unit| {
        !(unit.player == 0 && matches!(unit.kind, UnitKind::Skyhook | UnitKind::Sentinel))
    });
    scenario
}

fn boards(commands: &[PlayerCommand], carrier: UnitId) -> bool {
    commands.iter().any(|command| {
        matches!(&command.command, Command::Load { transport, .. } if *transport == carrier)
    })
}

#[test]
fn an_empty_roc_goes_at_the_anti_air_while_skyhooks_carry_the_lift() {
    let scenario = with_roc(strait());
    let (state, mut opponent) = bombing(&scenario, &around_the_start(3));
    let roc = at(&state, ROC_HOME.0, ROC_HOME.1);
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert!(
        clear(&trace.unwrap().missions).is_some(),
        "a clearance forms: {commands:?}"
    );
    assert!(
        runs(&commands).iter().any(|(units, _)| units == &[roc]),
        "the Roc gathers on its own: {commands:?}"
    );
    assert!(
        !boards(&commands, roc),
        "no rider waits on it: {commands:?}"
    );
}

#[test]
fn an_empty_roc_with_no_anti_air_known_is_left_to_lead_the_lift() {
    let scenario = with_roc(strait());
    let state = scenario.build().unwrap();
    let roc = at(&state, ROC_HOME.0, ROC_HOME.1);
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    assert!(
        boards(&commands, roc),
        "riders board it first: {commands:?}"
    );
}

#[test]
fn a_roc_outweighed_by_the_anti_air_around_the_target_stays_for_the_lift() {
    let scenario = with_roc(strait());
    let (state, mut opponent) = bombing(&scenario, &around_the_start(30));
    let roc = at(&state, ROC_HOME.0, ROC_HOME.1);
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert!(
        clear(&trace.unwrap().missions).is_none(),
        "it would not go: {commands:?}"
    );
    assert!(
        !runs(&commands)
            .iter()
            .any(|(units, _)| units.contains(&roc)),
        "it stays free: {commands:?}"
    );
}

#[test]
fn a_roc_the_anti_air_does_not_call_for_carries_the_lift() {
    let mut scenario = with_roc(strait());
    scenario.units.push(unit(0, UnitKind::Roc, 11, 14));
    // More riders than the Skyhooks hold.
    for x in 2..10 {
        scenario.units.push(unit(0, UnitKind::Sentinel, x, 16));
    }
    let (mut state, mut opponent) = bombing(&scenario, &around_the_start(3));
    let rocs = [at(&state, ROC_HOME.0, ROC_HOME.1), at(&state, 11, 14)];
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert!(clear(&trace.unwrap().missions).is_some(), "premise");
    let cleared: Vec<UnitId> = runs(&commands)
        .into_iter()
        .flat_map(|(units, _)| units)
        .filter(|id| rocs.contains(id))
        .collect();
    let [cleared] = cleared[..] else {
        panic!("one Roc outweighs the anti-air: {commands:?}");
    };
    let other = rocs.into_iter().find(|roc| *roc != cleared).unwrap();
    let mut carried = false;
    state.tick(&commands);
    while state.current_tick() < 120 && !carried {
        let commands = opponent.act(&state, &mut OwnEvents::default());
        carried = boards(&commands, other);
        state.tick(&commands);
    }
    assert!(carried, "the other Roc carries riders");
}

#[test]
fn a_roc_going_to_clear_leaves_the_seat_training_carriers_for_its_riders() {
    let mut scenario = with_roc(strait());
    scenario.units.retain(|unit| unit.kind != UnitKind::Skyhook);
    scenario.players[0].scrap = 2_000;
    let skyhook = |commands: &[PlayerCommand]| {
        trains(commands)
            .iter()
            .any(|(_, kind)| *kind == UnitKind::Skyhook)
    };
    let (state, mut opponent) = bombing(&scenario, &around_the_start(3));
    let commands = opponent.act(&state, &mut OwnEvents::default());
    assert!(skyhook(&commands), "riders need a carrier: {commands:?}");

    let state = scenario.build().unwrap();
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    assert!(
        !skyhook(&commands),
        "the free Roc holds them all: {commands:?}"
    );
}

#[test]
fn a_lone_roc_leaves_home_whatever_the_reserve_keeps() {
    // An enemy carrier in sight means ground units could reach home by air.
    let mut scenario = lone_roc();
    scenario.units.push(unit(1, UnitKind::Skyhook, 14, 11));
    let (state, mut opponent) = bombing(&scenario, &around_the_start(3));
    let observation = aftermine_sim::observation::ObservationData::fog_honest(&state, PlayerId(0));
    let model = map(&scenario);
    let mut memory = crate::memory::Memory::default();
    memory.observe(&observation);
    let frame = crate::frame::HomeFrame::of(&observation, &model).unwrap();
    let scratch = crate::missions::Scratch::new(
        &observation,
        &model,
        frame,
        &memory,
        aftermine_sim::scenario::BotStance::Balanced,
        middling(),
        &crate::missions::Missions::default(),
    );
    assert!(
        scratch.reserve[0] > u64::from(UnitKind::Roc.stats().cost),
        "premise: the reserve wants more than it"
    );
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert!(
        clear(&trace.unwrap().missions).is_some(),
        "it goes anyway: {commands:?}"
    );
}

#[test]
fn a_roc_clears_the_flak_and_then_takes_the_target_it_guards() {
    let mut scenario = lone_roc();
    for (x, y) in [(32, 8), (32, 14), (38, 8), (38, 14)] {
        scenario
            .buildings
            .push(building(1, BuildingKind::FlakTurret, x, y));
    }
    let east = TilePos::new(35, 11);
    let mut state = scenario.build().unwrap();
    let roc = at(&state, ROC_HOME.0, ROC_HOME.1);
    let mut opponent = seat(&scenario, 0);
    let mut hunted = false;
    let foundry = |state: &State| {
        state
            .buildings()
            .iter()
            .any(|b| b.player == PlayerId(1) && b.kind == BuildingKind::Foundry)
    };
    let flak = |state: &State| {
        state
            .buildings()
            .iter()
            .filter(|b| b.kind == BuildingKind::FlakTurret)
            .count()
    };
    assert_eq!(flak(&state), 4, "premise");
    while state.current_tick() < 12_000 && foundry(&state) {
        let commands = opponent.act(&state, &mut OwnEvents::default());
        hunted |= hunts(&commands)
            .iter()
            .any(|(units, _)| units.contains(&roc));
        state.tick(&commands);
    }
    assert!(hunted, "it goes on attack-move");
    assert!(!foundry(&state), "East's Foundry at {east:?} falls");
    assert!(state.unit(roc).is_some(), "a Roc takes that fire");
}

#[test]
fn a_wounded_roc_turns_home_from_a_clearance() {
    let scenario = lone_roc();
    let (state, mut opponent) = bombing(&scenario, &around_the_start(3));
    let roc = at(&state, ROC_HOME.0, ROC_HOME.1);
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert!(clear(&trace.unwrap().missions).is_some(), "premise");
    let mut state = state;
    state.tick(&commands);
    while !state.current_tick().is_multiple_of(12) {
        state.tick(&[]);
    }
    let hurt = wounded(&state, roc, UnitKind::Roc.stats().max_hp / 4);
    let (commands, trace) = opponent.act_traced(&hurt, &mut OwnEvents::default());
    assert!(
        runs(&commands).iter().any(|(units, _)| units == &[roc]),
        "it goes home: {commands:?}"
    );
    assert!(
        clear(&trace.unwrap().missions).is_none(),
        "and out of the clearance"
    );
}

#[test]
fn a_mid_gunship_clearance_checkpoint_resumes_identically() {
    let scenario = lone_roc();
    let (mut state, mut opponent) = bombing(&scenario, &around_the_start(3));
    loop {
        assert!(state.current_tick() < 1_200, "no clearance set out");
        let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
        state.tick(&commands);
        if trace.is_some_and(|trace| {
            clear(&trace.missions).is_some_and(|mission| mission.phase == Phase::Travel)
        }) {
            break;
        }
    }
    while !state.current_tick().is_multiple_of(12) {
        state.tick(&[]);
    }
    resumes_identically(&mut opponent, &scenario, &mut state, 300);
}

#[test]
fn mirrored_seats_send_their_rocs_alike() {
    let (width, height) = (40, 24);
    let flip = |(x, y): (i32, i32)| (width - 1 - x, height - 1 - y);
    let mut scenario = lone_roc();
    for (kind, x, y) in super::lift::TECH {
        let (w, h) = kind.size();
        scenario.buildings.push(BuildingSpec {
            player: 1,
            kind,
            x: width - x - w,
            y: height - y - h,
        });
    }
    let (x, y) = flip(ROC_HOME);
    scenario.units.push(unit(1, UnitKind::Roc, x, y));
    let state = scenario.build().unwrap();
    let flak = around_the_start(3);
    let remembering = |player: u8, flak: Vec<(i32, i32)>| {
        restored_with(&seat(&scenario, player), &scenario, &state, |json| {
            json["memory"]["units"] = remembered(
                (1_000..)
                    .zip(flak)
                    .map(|(id, (x, y))| (id, UnitKind::Flakhound, TilePos::new(x, y), 0)),
            );
        })
    };
    let mut seats = [
        remembering(0, flak.clone()),
        remembering(1, flak.into_iter().map(flip).collect()),
    ];
    let (west, trace) = seats[0].act_traced(&state, &mut OwnEvents::default());
    let east = seats[1].act(&state, &mut OwnEvents::default());
    assert!(
        clear(&trace.unwrap().missions).is_some(),
        "premise: a clearance forms"
    );
    assert_eq!(mirror(&state, west), east);
}

#[test]
fn a_clearance_goes_at_a_known_flak_turret_around_the_target() {
    let mut scenario = with_bombers(strait());
    scenario.buildings.push(BuildingSpec {
        player: 1,
        kind: BuildingKind::FlakTurret,
        x: 30,
        y: 4,
    });
    scenario.units.push(unit(0, UnitKind::Kestrel, 31, 6));
    let (state, mut opponent) = bombing(&scenario, &[]);
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    assert!(
        observation
            .enemy_buildings
            .iter()
            .any(|building| building.kind == BuildingKind::FlakTurret),
        "premise: West has seen it"
    );
    let (_, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    let mission = clear(&trace.unwrap().missions).expect("a clearance forms");
    assert_eq!(
        mission.kind,
        MissionKind::Clear {
            aim: TilePos::new(30, 4)
        }
    );
}

/// West's Condors at tick 1,200, still busy, in a clearance at the East Flak
/// Turret beside the lift's target, fighting since tick 0, whose fight last
/// knew of East's buildings `tally` more health than West knows now.
fn clearing_on(tally: Option<i64>) -> (Scenario, State, Opponent, Vec<UnitId>) {
    let mut scenario = with_bombers(strait());
    scenario
        .buildings
        .push(building(1, BuildingKind::FlakTurret, 30, 4));
    scenario.units.push(unit(0, UnitKind::Kestrel, 31, 6));
    let mut state = scenario.build().unwrap();
    let condors: Vec<UnitId> = BOMBERS.iter().map(|(x, y)| at(&state, *x, *y)).collect();
    advance_to(&mut state, 1_188, &[]);
    advance_to(&mut state, 1_200, &[run(0, condors.clone(), 6, 12)]);
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let buildings: i64 = observation
        .enemy_buildings
        .iter()
        .map(|building| i64::from(building.hp))
        .sum();
    assert!(buildings > 0, "premise: the Flak Turret is known");
    let tally = tally.map_or(
        serde_json::Value::Null,
        |more| serde_json::json!({"buildings": buildings + more, "units": 0}),
    );
    let missions = serde_json::json!({
        "next": 1,
        "list": [{
            "id": 0,
            "task": {
                "task": "clear",
                "aim": {"x": 30, "y": 4},
                "phase": {"engage": {"focus": null, "tally": tally, "progressed": 0}},
            },
            "since": 0,
            "units": condors,
            "goal": {"x": 30, "y": 6},
        }],
    });
    let opponent = super::attack::restaged(&scenario, &state, missions);
    (scenario, state, opponent, condors)
}

#[test]
fn a_clearance_fights_on_past_its_time_only_while_it_makes_progress() {
    for (tally, goes_on, why) in [
        (Some(1), true, "a building lost health"),
        (Some(0), false, "nothing lost"),
        (None, false, "the fight's first reading"),
    ] {
        let (_, state, mut opponent, condors) = clearing_on(tally);
        let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
        let phase = clear(&trace.unwrap().missions).map(|mission| mission.phase);
        let sent = runs(&commands)
            .into_iter()
            .any(|(units, _)| units.iter().any(|id| condors.contains(id)));
        if goes_on {
            assert_eq!(phase, Some(Phase::Engage), "{why}");
            assert!(!sent, "{why}: {commands:?}");
        } else {
            assert_ne!(phase, Some(Phase::Engage), "{why}: the fight ends");
            assert!(sent, "{why}: the Condors move on: {commands:?}");
        }
    }
}

#[test]
fn a_clearance_fight_keeps_what_it_knew_and_resumes_identically() {
    let (scenario, mut state, mut opponent, _) = clearing_on(Some(1));
    let commands = opponent.act(&state, &mut OwnEvents::default());
    let json = serde_json::to_value(opponent.checkpoint()).unwrap();
    let fight = &json["missions"]["list"][0]["task"]["phase"]["engage"];
    assert!(fight["tally"].is_object(), "{json}");
    assert_eq!(fight["progressed"], state.current_tick(), "{json}");
    state.tick(&commands);
    resumes_identically(&mut opponent, &scenario, &mut state, 600);
}

/// Three islands in a row, West's start on the first, Mid's on the second and
/// East's on the third, with West's Condors at home, an East Flak Turret
/// beside East's start, West Kestrels over both enemy starts, and a Mid
/// Fabricator when `stocked`.
fn three_islands(stocked: bool) -> Scenario {
    let map: Vec<String> = (0..11)
        .map(|row| {
            let mut tiles: Vec<char> = ".".repeat(40).chars().collect();
            for x in [13, 14, 26, 27] {
                tiles[x] = '~';
            }
            if row == 4 {
                tiles[2] = '1';
                tiles[19] = '2';
                tiles[34] = '3';
            }
            tiles.into_iter().collect()
        })
        .collect();
    let mut scenario = match_on(
        map,
        &["West", "Mid", "East"],
        &[
            (0, UnitKind::Condor, 5, 2),
            (0, UnitKind::Condor, 5, 8),
            (0, UnitKind::Kestrel, 20, 6),
            (0, UnitKind::Kestrel, 33, 6),
        ],
    );
    scenario.players[0].bot = true;
    scenario.players[0].bot_config = Some(config());
    scenario
        .buildings
        .push(building(2, BuildingKind::FlakTurret, 34, 7));
    if stocked {
        scenario
            .buildings
            .push(building(1, BuildingKind::Fabricator, 21, 6));
    }
    scenario
}

/// West's Condors at tick 1,200, still busy, in a clearance at East's Flak
/// Turret fighting since tick 0, whose fight last knew of East's buildings
/// `more` health than West knows now, with a Mid Fabricator when `stocked`.
fn clearing_east(stocked: bool, more: i64) -> (State, Opponent, Vec<UnitId>) {
    let scenario = three_islands(stocked);
    let mut state = scenario.build().unwrap();
    let condors: Vec<UnitId> = [(5, 2), (5, 8)]
        .iter()
        .map(|(x, y)| at(&state, *x, *y))
        .collect();
    advance_to(&mut state, 1_188, &[]);
    advance_to(&mut state, 1_200, &[run(0, condors.clone(), 4, 6)]);
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let known = |owner: u8| -> i64 {
        observation
            .enemy_buildings
            .iter()
            .filter(|building| building.player == PlayerId(owner))
            .map(|building| i64::from(building.hp))
            .sum()
    };
    let (mid, east) = (known(1), known(2));
    assert!(
        mid > 0 && east > 0 && (mid < east) != stocked,
        "premise: both enemies known, Mid holding {} than East",
        if stocked { "more" } else { "less" }
    );
    let missions = serde_json::json!({
        "next": 1,
        "list": [{
            "id": 0,
            "task": {
                "task": "clear",
                "aim": {"x": 34, "y": 7},
                "phase": {"engage": {
                    "focus": null,
                    "tally": {"buildings": east + more, "units": 0},
                    "progressed": 0,
                }},
            },
            "since": 0,
            "units": condors,
            "goal": {"x": 34, "y": 9},
        }],
    });
    let opponent = super::attack::restaged(&scenario, &state, missions);
    (state, opponent, condors)
}

#[test]
fn a_clearance_weighs_the_anti_air_it_sweeps_whoever_the_lift_goes_after() {
    // The lift goes after Mid, nearer home, while the clearance sweeps East's
    // flak. Mid holding less than East would read as progress, and Mid
    // holding more would hide East's losses.
    for (stocked, more, goes_on, why) in [
        (false, 0, false, "nothing of East's lost"),
        (true, 1, true, "East's buildings lost health"),
    ] {
        let (state, mut opponent, condors) = clearing_east(stocked, more);
        let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
        let phase = clear(&trace.unwrap().missions).map(|mission| mission.phase);
        let sent = runs(&commands)
            .into_iter()
            .chain(hunts(&commands))
            .any(|(units, _)| units.iter().any(|id| condors.contains(id)));
        assert_eq!(phase == Some(Phase::Engage), goes_on, "{why}: {phase:?}");
        assert_eq!(sent, !goes_on, "{why}: {commands:?}");
    }
}

/// East's Foundry, the building a lift over the strait goes after.
const EAST_FOUNDRY: TilePos = TilePos::new(35, 11);

/// West's lone Roc at home at tick 1,200, in a clearance aimed at `aim` whose
/// fight there began at tick 0 and has made no progress since, with the
/// decision's commands and the clearance it leaves.
fn roc_done_fighting_at(aim: TilePos) -> (UnitId, Vec<PlayerCommand>, Option<MissionStatus>) {
    let scenario = lone_roc();
    let mut state = scenario.build().unwrap();
    let roc = at(&state, ROC_HOME.0, ROC_HOME.1);
    advance_to(&mut state, 1_200, &[]);
    let missions = serde_json::json!({
        "next": 1,
        "list": [{
            "id": 0,
            "task": {
                "task": "clear",
                "aim": aim,
                "phase": {"engage": {"focus": null, "tally": null, "progressed": 0}},
            },
            "since": 0,
            "units": [roc],
            "goal": {"x": aim.x, "y": aim.y + 2},
        }],
    });
    let mut opponent = super::attack::restaged(&scenario, &state, missions);
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    (roc, commands, clear(&trace.unwrap().missions))
}

#[test]
fn a_free_roc_sweeps_the_lift_target_until_it_falls_then_the_next() {
    // Still standing, the target is swept again rather than left behind.
    // Gone, the Roc goes at the next building the lift would take.
    for (aim, why) in [
        (EAST_FOUNDRY, "the target stands"),
        (TilePos::new(30, 20), "the target fell"),
    ] {
        let (roc, commands, mission) = roc_done_fighting_at(aim);
        assert_eq!(
            mission.map(|mission| (mission.kind, mission.phase)),
            Some((MissionKind::Clear { aim: EAST_FOUNDRY }, Phase::Travel)),
            "{why}: {commands:?}"
        );
        assert!(
            hunts(&commands)
                .iter()
                .chain(&runs(&commands))
                .any(|(units, _)| units == &[roc]),
            "{why}: the Roc sets out: {commands:?}"
        );
    }
}
