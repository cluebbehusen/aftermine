use super::*;
use crate::frame::HomeFrame;
use crate::investments::Investment;
use crate::memory::Memory;
use crate::missions::{Missions, Scratch};
use crate::profile::ResolvedProfile;
use oxide_sim::observation::ObservationData;
use oxide_sim::scenario::{BotConfig, BotDifficulty};

const ROC: [Investment; 1] = [Investment::Tech(BuildingKind::RocCradle)];

/// East's start, across the strait.
const EAST_START: TilePos = TilePos::new(35, 11);

/// East Flak Turrets around its start.
const FLAK: [(i32, i32); 4] = [(32, 8), (32, 14), (38, 8), (38, 14)];

/// The strait with West's lift force and tech, a Crucible and a Deep Bore
/// frame, and East Flak Turrets at `flak` around its start.
fn tooled(flak: &[(i32, i32)]) -> Scenario {
    let mut scenario = super::lift::strait();
    scenario.map[4].replace_range(3..4, "E");
    scenario.buildings.extend([
        building(0, BuildingKind::Crucible, 3, 16),
        building(0, BuildingKind::Extractor, 3, 4),
    ]);
    for (x, y) in flak {
        scenario
            .buildings
            .push(building(1, BuildingKind::FlakTurret, *x, *y));
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

/// West's gunship investments on `state` against `flak`, the known anti-air
/// by the margin, with `memory` and whether its ground reaches no enemy.
fn wanted(
    scenario: &Scenario,
    state: &State,
    memory: &Memory,
    flak: u64,
    severed: bool,
) -> Vec<Investment> {
    let observation = ObservationData::fog_honest(state, PlayerId(0));
    let traits = middling();
    crate::gunship::investments(
        &observation,
        &map(scenario),
        memory,
        traits,
        360,
        flak,
        severed,
    )
    .into_iter()
    .map(|(investment, _)| investment)
    .collect()
}

/// The known anti-air around West's next lift target on `state`, as a
/// decision works it out.
fn lift_flak(scenario: &Scenario, state: &State) -> u64 {
    let observation = ObservationData::fog_honest(state, PlayerId(0));
    let model = map(scenario);
    let mut memory = Memory::default();
    memory.observe(&observation);
    let frame = HomeFrame::of(&observation, &model).unwrap();
    let missions = Missions::default();
    let scratch = Scratch::new(
        &observation,
        &model,
        frame,
        &memory,
        BotStance::Balanced,
        middling(),
        &missions,
    );
    let profile = ResolvedProfile::resolve(BotConfig::new(
        BotDifficulty::Standard,
        BotStance::Balanced,
        1,
    ));
    missions.lift_flak(&observation, &model, &profile, &memory, &scratch)
}

/// Three islands in a row, West's start on the first, Mid's on the second
/// and East's on the third, with an East Flak Turret beside its start that a
/// West Kestrel overhead sees.
fn three_islands() -> Scenario {
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
    let mut scenario = crate::tests::match_on(
        map,
        &["West", "Mid", "East"],
        &[(0, UnitKind::Kestrel, 33, 6)],
    );
    scenario
        .buildings
        .push(building(2, BuildingKind::FlakTurret, 34, 7));
    scenario
}

#[test]
fn lift_support_prepares_for_the_rival_the_lift_goes_after() {
    let mut scenario = three_islands();
    let calm = scenario.build().unwrap();
    assert_eq!(
        lift_flak(&scenario, &calm),
        0,
        "premise: the lift goes after Mid's nearer start, which no anti-air guards"
    );
    // East's bombers pressing West's base make East the rival.
    scenario.units.extend([
        unit(2, UnitKind::Buzzard, 5, 7),
        unit(2, UnitKind::Buzzard, 6, 7),
    ]);
    let pressed = scenario.build().unwrap();
    assert!(
        lift_flak(&scenario, &pressed) > 0,
        "the anti-air guarding the rival's start"
    );
}

#[test]
fn known_anti_air_over_a_severed_target_wants_a_roc_cradle() {
    let scenario = tooled(&[]);
    let state = bored(&scenario);
    let memory = Memory::default();
    let price = u64::from(UnitKind::Roc.stats().cost);
    assert_eq!(wanted(&scenario, &state, &memory, price, true), ROC);
    assert!(
        wanted(&scenario, &state, &memory, 0, true).is_empty(),
        "no known anti-air: the Skyhooks go as they are"
    );
    assert!(
        wanted(&scenario, &state, &memory, price, false).is_empty(),
        "ground that reaches the enemy needs no lift"
    );

    let mut bare = scenario.clone();
    bare.buildings
        .retain(|b| !(b.player == 0 && b.kind == BuildingKind::Crucible));
    assert!(
        wanted(&bare, &bored(&bare), &memory, price, true).is_empty(),
        "no Crucible yet"
    );

    let mut fielded = scenario.clone();
    fielded.units.push(unit(0, UnitKind::Roc, 6, 20));
    assert!(
        wanted(&fielded, &bored(&fielded), &memory, price, true).is_empty(),
        "a Roc already takes that fire"
    );
    assert_eq!(
        wanted(&fielded, &bored(&fielded), &memory, 2 * price, true),
        ROC,
        "anti-air past what one Roc takes wants another"
    );

    let mut raising = scenario.clone();
    raising.players[0].scrap = UnitKind::Roc.stats().cost;
    raising.units.push(unit(0, UnitKind::Excavator, 9, 18));
    let mut state = bored(&raising);
    let founder = at(&state, 9, 18);
    state.tick(&[PlayerCommand {
        player: PlayerId(0),
        command: Command::Build {
            units: vec![founder],
            kind: BuildingKind::RocCradle,
            anchor: TilePos::new(9, 19),
            queue: false,
            defer: false,
        },
    }]);
    assert!(
        state
            .buildings()
            .iter()
            .any(|b| b.kind == BuildingKind::RocCradle),
        "premise: the cradle is placed"
    );
    assert!(
        wanted(&raising, &state, &memory, price, true).is_empty(),
        "a cradle under way counts as its Roc"
    );
}

#[test]
fn a_lift_lost_over_severed_ground_wants_a_roc_cradle_while_none_is_raised() {
    let scenario = tooled(&[]);
    let state = bored(&scenario);
    assert!(
        wanted(&scenario, &state, &Memory::default(), 0, true).is_empty(),
        "no evidence yet"
    );
    let mut memory = Memory::default();
    memory.abandon(BuildingKind::Foundry, EAST_START, state.current_tick());
    assert_eq!(wanted(&scenario, &state, &memory, 0, true), ROC);

    let mut fielded = scenario.clone();
    fielded.units.push(unit(0, UnitKind::Roc, 6, 20));
    assert!(
        wanted(&fielded, &bored(&fielded), &memory, 0, true).is_empty(),
        "a lift lost without a Roc says nothing of one with a Roc"
    );

    // A target given up on West's own ground says nothing about lifts.
    let mut home = Memory::default();
    home.abandon(
        BuildingKind::Foundry,
        TilePos::new(8, 11),
        state.current_tick(),
    );
    assert!(wanted(&scenario, &state, &home, 0, true).is_empty());
}

#[test]
fn only_anti_air_the_seat_knows_of_counts_against_its_lift() {
    let unseen = tooled(&FLAK);
    assert_eq!(
        lift_flak(&unseen, &bored(&unseen)),
        0,
        "flak across the strait no one has seen is unknown"
    );
    let mut seen = unseen.clone();
    seen.units.push(unit(0, UnitKind::Kestrel, 35, 4));
    let flak = lift_flak(&seen, &bored(&seen));
    assert!(flak > 0, "a Kestrel over East shows its flak");
    let margin = crate::missions::margin(BotDifficulty::Standard);
    assert_eq!(
        wanted(
            &seen,
            &bored(&seen),
            &Memory::default(),
            flak * margin / 1_000,
            true
        ),
        ROC,
        "that flak, by the margin, wants a Roc"
    );
}

#[test]
fn an_empty_roc_goes_with_an_attack_while_ground_reaches_the_enemy() {
    let mut scenario = super::attack::armed(8, &[]);
    scenario.units.push(unit(0, UnitKind::Roc, 14, 11));
    let state = scenario.build().unwrap();
    let roc = state
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Roc)
        .unwrap()
        .id;
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let sent = hunts(&commands);
    assert_eq!(sent.len(), 1, "an attack forms");
    assert!(sent[0].0.contains(&roc), "the Roc goes along: {sent:?}");
}
