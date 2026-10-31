use super::*;
use crate::investments::Investment;
use aftermine_sim::observation::ObservationData;

/// Ten East Bastions around East's start.
const RING: [(i32, i32); 10] = [
    (38, 4),
    (41, 4),
    (44, 4),
    (38, 8),
    (38, 14),
    (38, 18),
    (41, 18),
    (44, 18),
    (40, 8),
    (40, 14),
];

/// The field with a West Crucible, a Deep Bore frame and West Excavators,
/// a West Kestrel over East's start, and East Bastions at `guns`.
fn fortress(guns: &[(i32, i32)]) -> Scenario {
    let mut scenario = field();
    scenario.map[4].replace_range(8..9, "E");
    scenario.players[0].scrap = 2 * UnitKind::Maw.stats().cost;
    scenario
        .units
        .extend((5..10).map(|x| unit(0, UnitKind::Excavator, x, 18)));
    scenario.units.push(unit(0, UnitKind::Kestrel, 41, 11));
    scenario.buildings.extend([
        building(0, BuildingKind::Crucible, 3, 16),
        building(0, BuildingKind::Extractor, 8, 4),
    ]);
    for (x, y) in guns {
        scenario
            .buildings
            .push(building(1, BuildingKind::Bastion, *x, *y));
    }
    scenario
}

/// `scenario` built with West's Extractors refitted into Deep Bores.
fn bored(scenario: &Scenario) -> State {
    let mut value = serde_json::to_value(scenario.build().unwrap()).unwrap();
    for entry in value["buildings"].as_array_mut().unwrap() {
        if entry["player"] == 0 && entry["kind"] == "extractor" {
            entry["tier"] = 1.into();
        }
    }
    serde_json::from_value(value).unwrap()
}

/// West's breach investments on `state`.
fn wanted(scenario: &Scenario, state: &State) -> Vec<Investment> {
    wanted_with(scenario, state, &crate::memory::Memory::default())
}

/// West's breach investments on `state` with `memory`.
fn wanted_with(
    scenario: &Scenario,
    state: &State,
    memory: &crate::memory::Memory,
) -> Vec<Investment> {
    let observation = ObservationData::fog_honest(state, PlayerId(0));
    let traits = middling();
    crate::breach::investments(
        &observation,
        &map(scenario),
        memory,
        traits,
        360,
        crate::missions::margin(aftermine_sim::scenario::BotDifficulty::Standard),
    )
    .into_iter()
    .map(|(investment, _)| investment)
    .collect()
}

#[test]
fn a_cradle_is_wanted_against_guns_the_army_cannot_outweigh() {
    let cradle = [Investment::Tech(BuildingKind::MawCradle)];
    let scenario = fortress(&RING);
    assert_eq!(wanted(&scenario, &bored(&scenario)), cradle);

    let mut bare = scenario.clone();
    bare.buildings
        .retain(|b| !(b.player == 0 && b.kind == BuildingKind::Crucible));
    assert!(wanted(&bare, &bored(&bare)).is_empty(), "no Crucible yet");

    let quiet = fortress(&[]);
    assert!(
        wanted(&quiet, &bored(&quiet)).is_empty(),
        "no known guns to breach"
    );

    let mut strong = scenario.clone();
    strong
        .units
        .extend((3..=12).map(|y| unit(0, UnitKind::Breaker, 12, y)));
    assert!(
        wanted(&strong, &bored(&strong)).is_empty(),
        "an army that outweighs the guns takes them itself"
    );

    // Eight Bastions are within one Maw's price by the margin; ten are not.
    let mut fielded = fortress(&RING[..8]);
    assert_eq!(wanted(&fielded, &bored(&fielded)), cradle, "premise");
    fielded.units.push(unit(0, UnitKind::Maw, 20, 11));
    assert!(
        wanted(&fielded, &bored(&fielded)).is_empty(),
        "a Maw that outweighs the guns takes them itself"
    );
    let mut raising = bored(&fortress(&RING[..8]));
    let founder = raising
        .units()
        .iter()
        .find(|u| u.player == PlayerId(0) && u.kind == UnitKind::Excavator)
        .unwrap()
        .id;
    raising.tick(&[PlayerCommand {
        player: PlayerId(0),
        command: Command::Build {
            units: vec![founder],
            kind: BuildingKind::MawCradle,
            anchor: TilePos::new(6, 13),
            queue: false,
            defer: false,
        },
    }]);
    assert!(
        wanted(&fortress(&RING[..8]), &raising).is_empty(),
        "a cradle under way counts as its Maw"
    );
    let mut deeper = scenario.clone();
    deeper.units.push(unit(0, UnitKind::Maw, 20, 11));
    assert_eq!(
        wanted(&deeper, &bored(&deeper)),
        cradle,
        "guns past what one Maw answers want another"
    );

    // Ground cut by a wall of rock: the guns stand beyond any drive.
    let mut severed = scenario.clone();
    for row in &mut severed.map[1..23] {
        row.replace_range(30..31, "#");
    }
    assert!(
        wanted(&severed, &bored(&severed)).is_empty(),
        "a Maw cannot reach guns across severed ground"
    );
}

#[test]
fn a_cradle_crew_tops_up_with_the_fastest_free_workers() {
    let mut scenario = fortress(&RING);
    scenario.units.extend((5..10).map(|x| harvester(0, x, 20)));
    let mut state = bored(&scenario);
    let founder = state
        .units()
        .iter()
        .find(|u| u.player == PlayerId(0) && u.kind == UnitKind::Harvester)
        .unwrap()
        .id;
    let anchor = TilePos::new(6, 13);
    state.tick(&[PlayerCommand {
        player: PlayerId(0),
        command: Command::Build {
            units: vec![founder],
            kind: BuildingKind::MawCradle,
            anchor,
            queue: false,
            defer: false,
        },
    }]);
    assert!(
        state
            .buildings()
            .iter()
            .any(|b| b.kind == BuildingKind::MawCradle),
        "the cradle was placed"
    );
    let mut opponent = seat_with(&scenario, 0, thrifty());
    while !opponent.decision_due(&state) {
        state.tick(&[]);
    }
    let commands = opponent.act(&state, &mut OwnEvents::default());
    let crew: Vec<UnitId> = commands
        .iter()
        .find_map(|command| match &command.command {
            Command::Build {
                units,
                kind: BuildingKind::MawCradle,
                anchor: at,
                ..
            } if *at == anchor => Some(units.clone()),
            _ => None,
        })
        .expect("the crew is topped up");
    assert_eq!(crew.len(), aftermine_sim::stats::ASSEMBLY_CREW_CAP - 1);
    assert!(
        crew.iter()
            .all(|id| state.unit(*id).unwrap().kind == UnitKind::Excavator),
        "Excavators build fastest: {crew:?}"
    );
}

#[test]
fn a_cradle_stands_only_where_its_machine_can_drive_out() {
    // A pocket of rock around the spot, open by a gap one tile wide or
    // three, inside a base that reaches past it.
    let pocket = |gap: i32| {
        let mut scenario = fortress(&RING);
        scenario
            .buildings
            .push(building(0, BuildingKind::Turret, 26, 11));
        for y in 8..=16 {
            for x in 14..=22 {
                let edge = y == 8 || y == 16 || x == 14 || x == 22;
                let opening = x == 14 && (12 - gap / 2..=12 + gap / 2).contains(&y);
                if edge && !opening {
                    scenario.map[usize::try_from(y).unwrap()].replace_range(
                        usize::try_from(x).unwrap()..=usize::try_from(x).unwrap(),
                        "#",
                    );
                }
            }
        }
        scenario
    };
    let exits = |scenario: &Scenario| {
        let state = bored(scenario);
        let observation = ObservationData::fog_honest(&state, PlayerId(0));
        crate::breach::exits(
            &map(scenario),
            &observation,
            BuildingKind::MawCradle,
            TilePos::new(17, 11),
        )
    };
    assert!(
        !exits(&pocket(1)),
        "a hull three tiles wide never squeezes out"
    );
    assert!(exits(&pocket(3)));
    let across = |kind: BuildingKind| {
        let mut scenario = pocket(3);
        scenario
            .buildings
            .extend((11..=13).map(|y| building(0, kind, 14, y)));
        scenario
    };
    assert!(
        exits(&across(BuildingKind::ScuttleCharge)),
        "a buried charge blocks nothing"
    );
    assert!(!exits(&across(BuildingKind::Barricade)));
}

#[test]
fn a_free_maw_goes_with_an_attack() {
    let mut scenario = super::attack::armed(8, &[]);
    scenario.units.push(unit(0, UnitKind::Maw, 14, 11));
    let state = scenario.build().unwrap();
    let maw = state
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Maw)
        .unwrap()
        .id;
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let sent = hunts(&commands);
    assert_eq!(sent.len(), 1, "an attack forms");
    assert!(sent[0].0.contains(&maw), "the Maw goes along: {sent:?}");
}

#[test]
fn a_maw_leaves_with_an_army_that_turns_home() {
    // Eighteen East Sentinels in contact with the attack outweigh it at
    // once; with none, a fight at Standard ends on time.
    let east: Vec<(UnitKind, i32, i32)> = (27..30)
        .flat_map(|x| (7..13).map(move |y| (UnitKind::Sentinel, x + i32::from(y % 2 == 0), y)))
        .collect();
    for (east, ends, phase) in [
        (&east[..], 12, Phase::Withdraw),
        (&[][..], 3_600, Phase::Recover),
    ] {
        let mut scenario = super::attack::advanced(BotDifficulty::Standard, east);
        scenario.units.push(unit(0, UnitKind::Maw, 21, 15));
        let mut state = scenario.build().unwrap();
        let maw = at(&state, 21, 15);
        advance_to(&mut state, ends - 12, &[]);
        let fighting = serde_json::json!({"engage": {"focus": null}});
        let (mut members, mut missions) = super::attack::under_way(&state, &fighting, 0);
        members.push(maw);
        members.sort_unstable();
        missions["list"][0]["units"] = serde_json::json!(members);
        advance_to(&mut state, ends, &[run(0, members.clone(), 30, 11)]);
        let mut opponent = super::attack::restaged(&scenario, &state, missions);
        let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
        let mission = super::attack::attack(&trace.unwrap().missions).unwrap();
        assert_eq!(mission.phase, phase);
        assert!(
            runs(&commands)
                .iter()
                .any(|(units, _)| units.contains(&maw)),
            "{phase:?}: the Maw goes too: {commands:?}"
        );
    }
}

#[test]
fn a_maw_cutting_guns_keeps_a_winning_fight_going() {
    // An East Turret in contact with the attack, outweighed by the Maw that
    // answers it: still an armed enemy, so Prime presses on past its time.
    let mut scenario = super::attack::advanced(BotDifficulty::Prime, &[]);
    scenario.buildings.push(BuildingSpec {
        player: 1,
        kind: BuildingKind::Turret,
        x: 30,
        y: 11,
    });
    scenario.units.push(unit(0, UnitKind::Maw, 21, 15));
    let mut state = scenario.build().unwrap();
    let maw = at(&state, 21, 15);
    advance_to(&mut state, 3_588, &[]);
    let fighting = serde_json::json!({"engage": {"focus": null}});
    let (mut members, mut missions) = super::attack::under_way(&state, &fighting, 0);
    members.push(maw);
    members.sort_unstable();
    missions["list"][0]["units"] = serde_json::json!(members);
    advance_to(&mut state, 3_600, &[run(0, members.clone(), 20, 16)]);
    let mut opponent = super::attack::restaged(&scenario, &state, missions);
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    let mission = super::attack::attack(&trace.unwrap().missions).unwrap();
    assert_eq!(mission.phase, Phase::Engage, "{commands:?}");
    assert!(
        !runs(&commands)
            .iter()
            .any(|(units, _)| units.iter().any(|id| members.contains(id))),
        "{commands:?}"
    );
}

#[test]
fn a_given_up_attack_wants_a_cradle_even_against_no_known_guns() {
    let scenario = fortress(&[]);
    let state = bored(&scenario);
    assert!(wanted(&scenario, &state).is_empty(), "no evidence yet");
    let mut memory = crate::memory::Memory::default();
    memory.abandon(
        BuildingKind::Foundry,
        TilePos::new(43, 11),
        state.current_tick(),
    );
    assert_eq!(
        wanted_with(&scenario, &state, &memory),
        [Investment::Tech(BuildingKind::MawCradle)]
    );
    let mut fielded = scenario.clone();
    fielded.units.push(unit(0, UnitKind::Maw, 20, 11));
    assert!(
        wanted_with(&fielded, &bored(&fielded), &memory).is_empty(),
        "an attack given up says nothing of one with a Maw along"
    );

    // Ground cut by rock: a target given up there proves nothing a Maw could
    // drive to.
    let mut severed = scenario.clone();
    for row in &mut severed.map[1..23] {
        row.replace_range(30..31, "#");
    }
    let state = bored(&severed);
    assert!(wanted_with(&severed, &state, &memory).is_empty());
}

/// The field with West's garrison and an army fit to attack, a wounded West
/// Maw at `maw`, West harvesters at home, and wreck at `wreck`, which West
/// sees.
fn feeding(maw: (i32, i32), wreck: TilePos) -> (Scenario, State, UnitId) {
    feeding_on(super::attack::armed(8, &[]), maw, wreck)
}

/// [`feeding`] on `scenario`'s map.
fn feeding_on(
    mut scenario: Scenario,
    maw: (i32, i32),
    wreck: TilePos,
) -> (Scenario, State, UnitId) {
    scenario.units.push(unit(0, UnitKind::Maw, maw.0, maw.1));
    scenario
        .units
        .extend([harvester(0, 5, 8), harvester(0, 6, 8)]);
    let state = scenario.build().unwrap();
    let id = state
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Maw)
        .unwrap()
        .id;
    let state = wounded(&state, id, UnitKind::Maw.stats().max_hp / 2);
    let mut value = serde_json::to_value(&state).unwrap();
    let width = usize::try_from(state.map().width()).unwrap();
    let index = usize::try_from(wreck.y).unwrap() * width + usize::try_from(wreck.x).unwrap();
    value["map"]["grid"]["cells"][index]["wreck"] = 200.into();
    let state: State = serde_json::from_value(value).unwrap();
    assert!(
        ObservationData::fog_honest(&state, PlayerId(0))
            .known_wrecks
            .contains(&(wreck, 200)),
        "West sees the wreck"
    );
    (scenario, state, id)
}

#[test]
fn a_wounded_maw_drives_to_safe_wreck_and_feeds_there_out_of_every_mission() {
    let wreck = TilePos::new(8, 15);
    let (scenario, state, maw) = feeding((8, 9), wreck);
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    // The nearest tile from which its intake reaches the wreck.
    assert!(
        runs(&commands).contains(&(vec![maw], TilePos::new(8, 13))),
        "the Maw heads for the wreck: {:?}",
        runs(&commands)
    );
    assert!(
        hunts(&commands)
            .iter()
            .any(|(units, _)| !units.contains(&maw)),
        "the attack forms without it"
    );

    // Standing on it, the Maw stays while the attack forms: no order moves
    // it, and no harvester is sent to salvage what it eats.
    let (scenario, state, maw) = feeding((8, 14), wreck);
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let ordered = |command: &PlayerCommand| {
        serde_json::to_value(&command.command).unwrap()["units"]
            .as_array()
            .is_some_and(|units| units.contains(&maw.0.into()))
    };
    assert!(!commands.iter().any(ordered), "the Maw keeps feeding");
    assert!(
        !harvests(&commands).iter().any(|(tile, _)| *tile == wreck),
        "harvesters leave the Maw its wreck"
    );
}

/// Whether a Maw's hull fits on `tile` in `state`: every tile under it is
/// open ground.
fn hull_fits(state: &State, tile: TilePos) -> bool {
    aftermine_sim::geometry::hull_open(tile, UnitKind::Maw.stats().radius, |cell| {
        state.passable(cell)
    })
}

#[test]
fn a_wounded_maw_feeds_only_from_ground_its_hull_fits() {
    // Wreck against the map's edge: the Maw's centre can never stand on it,
    // but its intake reaches it from open ground beside.
    let wreck = TilePos::new(1, 15);
    let (scenario, state, maw) = feeding((8, 9), wreck);
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let goal = runs(&commands)
        .into_iter()
        .find(|(units, _)| units == &[maw])
        .map(|(_, goal)| goal)
        .expect("the Maw heads for the wreck");
    assert!(
        !hull_fits(&state, wreck),
        "premise: no hull fits on the wreck"
    );
    assert!(
        hull_fits(&state, goal),
        "the hull fits where it goes: {goal:?}"
    );
    assert!(
        goal.chebyshev(wreck) <= 2,
        "its intake reaches the wreck: {goal:?}"
    );

    // Wreck in a pocket whose door is one tile wide: ordinary ground reaches
    // it, the Maw's hull never does, so it is not sent at all.
    let mut pocket = super::attack::armed(8, &[]);
    pocket.map[16].replace_range(1..6, "#####");
    for row in &mut pocket.map[17..23] {
        row.replace_range(5..6, "#");
    }
    pocket.map[19].replace_range(5..6, ".");
    let wreck = TilePos::new(2, 19);
    let (scenario, state, maw) = feeding_on(pocket, (8, 9), wreck);
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    assert!(
        runs(&commands)
            .iter()
            .all(|(units, _)| !units.contains(&maw)),
        "no Run the hull cannot drive: {:?}",
        runs(&commands)
    );
}

#[test]
fn harvesters_leave_wreck_to_the_maw_bound_for_it() {
    let wreck = TilePos::new(8, 15);
    let (scenario, mut state, maw) = feeding((8, 9), wreck);
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let sent: Vec<PlayerCommand> = commands
        .iter()
        .filter(|command| matches!(&command.command, Command::Run { units, .. } if units == &[maw]))
        .cloned()
        .collect();
    assert_eq!(sent.len(), 1, "the Maw heads for the wreck");
    assert!(
        !harvests(&commands).iter().any(|(tile, _)| *tile == wreck),
        "no harvester races it there in the same decision"
    );

    // On its way, a later decision still leaves the wreck to it.
    state.tick(&sent);
    state.tick(&[]);
    assert!(
        ObservationData::fog_honest(&state, PlayerId(0))
            .my_units
            .iter()
            .any(|unit| unit.id == maw && !unit.idle),
        "premise: the Maw is under way"
    );
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    assert!(
        !harvests(&commands).iter().any(|(tile, _)| *tile == wreck),
        "harvesters leave it the wreck it drives to: {:?}",
        harvests(&commands)
    );
}
