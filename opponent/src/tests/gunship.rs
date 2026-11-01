use super::*;
use crate::frame::HomeFrame;
use crate::investments::Investment;
use crate::memory::Memory;
use crate::missions::{Missions, Scratch};
use crate::profile::ResolvedProfile;
use aftermine_sim::observation::ObservationData;
use aftermine_sim::scenario::{BotConfig, BotDifficulty};

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

/// West's gunship investments on `state`, with `memory`, at `wants`.
fn asked(
    scenario: &Scenario,
    state: &State,
    memory: &Memory,
    traits: crate::profile::PersonalityTraits,
    wants: crate::gunship::Wants,
) -> Vec<(Investment, u32)> {
    let observation = ObservationData::fog_honest(state, PlayerId(0));
    crate::gunship::investments(&observation, &map(scenario), memory, traits, 360, wants)
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
    let wants = crate::gunship::Wants {
        flak,
        margin: crate::missions::margin(BotDifficulty::Standard),
        air_share: 0,
        severed,
    };
    asked(scenario, state, memory, middling(), wants)
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

/// A field with West's Crucible, Deep Bore frame and a Kestrel over East's
/// start, where ten East Bastions stand: more guns than West's army can
/// attack.
fn guarded() -> Scenario {
    let mut scenario = field();
    scenario.map[4].replace_range(8..9, "E");
    scenario.units.push(unit(0, UnitKind::Kestrel, 41, 11));
    scenario.buildings.extend([
        building(0, BuildingKind::Crucible, 3, 16),
        building(0, BuildingKind::Extractor, 8, 4),
    ]);
    for (x, y) in [
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
    ] {
        scenario
            .buildings
            .push(building(1, BuildingKind::Bastion, x, y));
    }
    scenario
}

fn leaning(air: u8, siege: u8) -> crate::profile::PersonalityTraits {
    crate::profile::PersonalityTraits {
        air,
        siege,
        ..middling()
    }
}

fn connected(air_share: u64) -> crate::gunship::Wants {
    crate::gunship::Wants {
        flak: 0,
        margin: crate::missions::margin(BotDifficulty::Standard),
        air_share,
        severed: false,
    }
}

fn scored(list: &[(Investment, u32)], investment: Investment) -> u32 {
    list.iter()
        .find(|(listed, _)| *listed == investment)
        .map_or(0, |(_, score)| *score)
}

#[test]
fn known_guns_beyond_the_army_on_connected_ground_want_a_roc_by_the_air_trait() {
    let scenario = guarded();
    let state = bored(&scenario);
    let memory = Memory::default();
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let margin = crate::missions::margin(BotDifficulty::Standard);
    let maw = |traits| {
        scored(
            &crate::breach::investments(
                &observation,
                &map(&scenario),
                &memory,
                traits,
                360,
                margin,
            ),
            Investment::Tech(BuildingKind::MawCradle),
        )
    };
    let roc = |traits| {
        scored(
            &asked(&scenario, &state, &memory, traits, connected(0)),
            ROC[0],
        )
    };
    assert!(
        roc(leaning(80, 20)) > maw(leaning(80, 20)),
        "an air seat flies over them"
    );
    assert!(
        maw(leaning(20, 80)) > roc(leaning(20, 80)),
        "a siege seat cuts through them"
    );
    assert!(
        roc(leaning(0, 100)) > 0,
        "the trait weighs it and never bars it"
    );
}

#[test]
fn one_machine_answers_the_guns_for_both_cradles() {
    let memory = Memory::default();
    let margin = crate::missions::margin(BotDifficulty::Standard);
    let wants = |scenario: &Scenario| {
        let state = bored(scenario);
        let observation = ObservationData::fog_honest(&state, PlayerId(0));
        let maws = crate::breach::investments(
            &observation,
            &map(scenario),
            &memory,
            middling(),
            360,
            margin,
        );
        (
            !maws.is_empty(),
            !asked(scenario, &state, &memory, middling(), connected(0)).is_empty(),
        )
    };
    // Seven Bastions are within either machine's price by the margin.
    let mut scenario = guarded();
    scenario
        .buildings
        .retain(|b| !(b.kind == BuildingKind::Bastion && (b.y == 8 || (b.x, b.y) == (44, 18))));
    assert_eq!(wants(&scenario), (true, true), "premise: both are wanted");
    for machine in [UnitKind::Roc, UnitKind::Maw] {
        let mut fielded = scenario.clone();
        fielded.units.push(unit(0, machine, 20, 11));
        assert_eq!(wants(&fielded), (false, false), "{machine:?} takes them");
    }
}

#[test]
fn an_attack_given_up_wants_a_machine_only_while_neither_stands() {
    let scenario = guarded();
    let state = bored(&scenario);
    let mut memory = Memory::default();
    memory.abandon(
        BuildingKind::Foundry,
        TilePos::new(41, 11),
        state.current_tick(),
    );
    let quiet = crate::gunship::Wants {
        margin: 0,
        ..connected(0)
    };
    assert_eq!(
        asked(&scenario, &state, &memory, middling(), quiet)
            .into_iter()
            .map(|(investment, _)| investment)
            .collect::<Vec<_>>(),
        ROC,
        "no need by the numbers, but that attack went without one"
    );
    let mut maw = scenario.clone();
    maw.units.push(unit(0, UnitKind::Maw, 20, 11));
    assert!(
        asked(&maw, &bored(&maw), &memory, middling(), quiet).is_empty(),
        "a Maw stands to break in"
    );
}

#[test]
fn the_air_share_wants_a_roc_at_an_air_leaning_seat() {
    let scenario = tooled(&[]);
    let state = bored(&scenario);
    let memory = Memory::default();
    let bar = u64::from(UnitKind::Roc.stats().cost) / 4;
    let score = |air, share| {
        scored(
            &asked(
                &scenario,
                &state,
                &memory,
                leaning(air, 50),
                connected(share),
            ),
            ROC[0],
        )
    };
    assert_eq!(score(80, 0), 0, "no share, no Roc");
    assert!(
        score(50, bar) >= crate::investments::ADOPT,
        "air 50 starts saving"
    );
    assert!(
        score(40, bar) < crate::investments::ADOPT,
        "lower waits behind what it needs"
    );
    assert_eq!(
        score(80, 10 * bar),
        score(80, bar),
        "a personality limit: one bar at most"
    );

    let mut fielded = scenario.clone();
    fielded.units.push(unit(0, UnitKind::Roc, 6, 20));
    let held = bored(&fielded);
    assert!(
        asked(
            &fielded,
            &held,
            &memory,
            leaning(80, 50),
            connected(2 * bar)
        )
        .is_empty(),
        "the Roc fills the share"
    );
}

#[test]
fn a_seat_with_nothing_to_refit_into_a_deep_bore_wants_no_cradle() {
    let mut scenario = tooled(&[]);
    scenario
        .buildings
        .retain(|b| b.kind != BuildingKind::Extractor);
    let state = scenario.build().unwrap();
    let price = u64::from(UnitKind::Roc.stats().cost);
    assert!(
        wanted(&scenario, &state, &Memory::default(), price, true).is_empty(),
        "its saving would hold scrap for a step it cannot take"
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
