use super::*;
use crate::investments::{self, Investment, Step};
use crate::memory::Memory;
use oxide_sim::observation::ObservationData;
use serde_json::json;

/// Two East Crucibles side by side: a warhead between them takes more than
/// its round's price.
const PAIR: [(i32, i32); 2] = [(30, 8), (32, 8)];

/// Four East Crucibles meeting at one corner.
const BLOCK: [(i32, i32); 4] = [(30, 8), (32, 8), (30, 10), (32, 10)];

/// The field with West Spires at `spires`, East Crucibles at `crucibles`,
/// East's other `east` works, and West Kestrels watching from `eyes`.
fn strikes(
    spires: &[(i32, i32)],
    crucibles: &[(i32, i32)],
    east: &[(BuildingKind, i32, i32)],
    eyes: &[(i32, i32)],
) -> Scenario {
    let mut scenario = field();
    garrison(&mut scenario);
    for (x, y) in spires {
        scenario
            .buildings
            .push(building(0, BuildingKind::Spire, *x, *y));
    }
    for (x, y) in crucibles {
        scenario
            .buildings
            .push(building(1, BuildingKind::Crucible, *x, *y));
    }
    for (kind, x, y) in east {
        scenario.buildings.push(building(1, *kind, *x, *y));
    }
    for (x, y) in eyes {
        scenario.units.push(unit(0, UnitKind::Kestrel, *x, *y));
    }
    scenario
}

/// `state` with `player`'s magazines holding `loaded` rounds, autofill off.
fn stocked(state: &State, player: u8, kind: BuildingKind, loaded: u8) -> State {
    let mut value = serde_json::to_value(state).unwrap();
    for (b, entry) in state
        .buildings()
        .iter()
        .zip(value["buildings"].as_array_mut().unwrap())
    {
        if b.player == PlayerId(player) && b.kind == kind {
            entry["magazine"] = json!({"loaded": loaded, "autofill": false});
        }
    }
    serde_json::from_value(value).unwrap()
}

/// `scenario` played to West's first decision with its Spires loaded and
/// East's Lattices holding `interceptors` each.
fn staged(scenario: &Scenario, interceptors: u8) -> State {
    let state = scenario.build().unwrap();
    let state = stocked(&state, 0, BuildingKind::Spire, 1);
    let mut state = stocked(&state, 1, BuildingKind::Lattice, interceptors);
    advance_to(&mut state, 12, &[]);
    state
}

/// West's launches, as (Spire, target).
fn launches(commands: &[PlayerCommand]) -> Vec<(BuildingId, TilePos)> {
    commands
        .iter()
        .filter_map(|command| match command.command {
            Command::Launch { building, target } => Some((building, target)),
            _ => None,
        })
        .collect()
}

fn decide(scenario: &Scenario, state: &State, config: BotConfig) -> Vec<PlayerCommand> {
    seat_with(scenario, 0, config).act(state, &mut OwnEvents::default())
}

/// Whether `tile` lies in the footprint of one of the Crucibles at `anchors`.
fn on(anchors: &[(i32, i32)], tile: TilePos) -> bool {
    anchors
        .iter()
        .any(|(x, y)| (*x..x + 2).contains(&tile.x) && (*y..y + 2).contains(&tile.y))
}

#[test]
fn a_loaded_spire_strikes_a_seen_block_of_enemy_works() {
    let scenario = strikes(&[(6, 14)], &PAIR, &[], &[(31, 4)]);
    let state = staged(&scenario, 0);
    let fired = launches(&decide(&scenario, &state, config()));
    let [(spire, target)] = fired[..] else {
        panic!("{fired:?}");
    };
    assert_eq!(state.building(spire).unwrap().kind, BuildingKind::Spire);
    assert!(on(&PAIR, target), "{target:?}");
}

#[test]
fn a_spire_strikes_remembered_works_and_never_unknown_ones() {
    let scenario = strikes(&[(6, 14)], &PAIR, &[], &[]);
    let state = staged(&scenario, 0);
    assert!(
        launches(&decide(&scenario, &state, config())).is_empty(),
        "nothing it knows is worth a round"
    );

    let mut value = serde_json::to_value(&state).unwrap();
    value["vision"][0]["ghosts"] = json!(PAIR.map(
        |(x, y)| json!({"kind": "crucible", "owner": 1, "anchor": {"x": x, "y": y}, "hp": 900})
    ));
    let remembered: State = serde_json::from_value(value).unwrap();
    let fired = launches(&decide(&scenario, &remembered, config()));
    assert!(
        fired.len() == 1 && on(&PAIR, fired[0].1),
        "a remembered block is struck: {fired:?}"
    );
}

#[test]
fn a_strike_takes_the_uncovered_block_over_one_under_interceptors() {
    let covered = [(30, 3), (32, 3)];
    let open = [(30, 18), (32, 18)];
    let scenario = strikes(
        &[(6, 14)],
        &[covered, open].concat(),
        &[(BuildingKind::Lattice, 31, 1)],
        &[(31, 6), (31, 15)],
    );
    let state = staged(&scenario, 2);
    let fired = launches(&decide(&scenario, &state, config()));
    assert!(
        fired.len() == 1 && on(&open, fired[0].1),
        "the open block: {fired:?}"
    );
}

#[test]
fn a_strike_holds_until_it_can_get_past_the_known_interceptors() {
    let lattice = [(BuildingKind::Lattice, 36, 9)];
    let one = strikes(&[(6, 14)], &BLOCK, &lattice, &[(31, 5)]);
    let state = staged(&one, 1);
    assert!(
        launches(&decide(&one, &state, config())).is_empty(),
        "one warhead would be stopped"
    );

    let two = strikes(&[(6, 14), (6, 17)], &BLOCK, &lattice, &[(31, 5)]);
    let state = staged(&two, 1);
    let fired = launches(&decide(&two, &state, config()));
    assert_eq!(fired.len(), 2, "{fired:?}");
    assert!(fired.iter().all(|(_, target)| on(&BLOCK, *target)));
    assert_eq!(fired[0].1, fired[1].1, "one volley on one aim");
}

#[test]
fn a_volley_overwhelms_a_full_lattice_over_works_worth_the_interceptors_it_draws() {
    let lattice = [(BuildingKind::Lattice, 36, 9)];
    let spires = [(6, 14), (6, 17), (9, 14)];
    let two = strikes(&spires[..2], &BLOCK, &lattice, &[(31, 5)]);
    let state = staged(&two, 2);
    assert!(
        launches(&decide(&two, &state, config())).is_empty(),
        "two warheads would both be stopped"
    );

    let three = strikes(&spires, &BLOCK, &lattice, &[(31, 5)]);
    let mut state = staged(&three, 2);
    let fired = launches(&decide(&three, &state, config()));
    assert_eq!(fired.len(), 3, "{fired:?}");
    assert!(
        fired
            .iter()
            .all(|(_, target)| *target == fired[0].1 && on(&BLOCK, *target)),
        "one volley on one aim: {fired:?}"
    );

    // The whole volley is one order, so the narrowest attention fires it.
    let scrapheap = BotConfig::new(BotDifficulty::Scrapheap, BotStance::Balanced, 11);
    assert!(crate::decision::allowance(BotDifficulty::Scrapheap) < 3);
    advance_to(&mut state, 24, &[]);
    assert_eq!(launches(&decide(&three, &state, scrapheap)).len(), 3);
}

#[test]
fn a_spire_with_a_strike_pending_is_not_ordered_again() {
    let scenario = strikes(&[(6, 14)], &PAIR, &[], &[(31, 4)]);
    let state = staged(&scenario, 0);
    let mut value = serde_json::to_value(&state).unwrap();
    for entry in value["buildings"].as_array_mut().unwrap() {
        if entry["kind"] == json!("spire") {
            entry["launcher"]["target"] = json!({"x": 1, "y": 1});
        }
    }
    let pending: State = serde_json::from_value(value).unwrap();
    assert!(launches(&decide(&scenario, &pending, config())).is_empty());
}

#[test]
fn mirrored_seats_strike_mirrored_aims() {
    let mut scenario = strikes(&[(6, 14)], &PAIR, &[], &[(31, 4)]);
    // East's half-turned Spire, West's half-turned pair and East's Kestrel.
    scenario
        .buildings
        .push(building(1, BuildingKind::Spire, 40, 8));
    for (x, y) in PAIR {
        scenario
            .buildings
            .push(building(0, BuildingKind::Crucible, 46 - x, 22 - y));
    }
    scenario.units.push(unit(1, UnitKind::Kestrel, 16, 19));
    scenario
        .units
        .extend(GARRISON.map(|(x, y)| unit(1, UnitKind::Sentinel, 47 - x, 23 - y)));
    let state = scenario.build().unwrap();
    let state = stocked(&state, 0, BuildingKind::Spire, 1);
    let mut state = stocked(&state, 1, BuildingKind::Spire, 1);
    advance_to(&mut state, 12, &[]);
    let only = |commands: Vec<PlayerCommand>| -> Vec<PlayerCommand> {
        commands
            .into_iter()
            .filter(|command| matches!(command.command, Command::Launch { .. }))
            .collect()
    };
    let west = only(seat(&scenario, 0).act(&state, &mut OwnEvents::default()));
    let east = only(seat(&scenario, 1).act(&state, &mut OwnEvents::default()));
    assert_eq!(west.len(), 1);
    assert_eq!(mirror(&state, west), east);
}

/// Four East Excavators packed beside an East Crucible, where one warhead
/// on the Crucible fells them all.
const PARK: [(i32, i32); 4] = [(30, 8), (31, 8), (30, 9), (31, 9)];

/// The launches of West's second decision, with its one loaded Spire having
/// seen a lone East Crucible, too little for a round, and East's Excavators
/// at `first` one decision before and at `second` now.
fn parked(first: [(i32, i32); 4], second: [(i32, i32); 4]) -> Vec<(BuildingId, TilePos)> {
    let staging = |spots: [(i32, i32); 4]| {
        let mut scenario = strikes(&[(6, 14)], &[(32, 8)], &[], &[(31, 4)]);
        scenario
            .units
            .extend(spots.map(|(x, y)| unit(1, UnitKind::Excavator, x, y)));
        scenario
    };
    let (before, after) = (staging(first), staging(second));
    let mut opponent = seat(&before, 0);
    let state = staged(&before, 0);
    assert!(
        launches(&opponent.act(&state, &mut OwnEvents::default())).is_empty(),
        "units seen once may be on the move"
    );
    let mut state = staged(&after, 0);
    advance_to(&mut state, 24, &[]);
    launches(&opponent.act(&state, &mut OwnEvents::default()))
}

#[test]
fn units_seen_standing_still_draw_a_strike_and_units_on_the_move_do_not() {
    let fired = parked(PARK, PARK);
    assert!(
        fired.len() == 1 && on(&[(32, 8)], fired[0].1),
        "the Crucible, for what stands beside it: {fired:?}"
    );
    assert!(parked(PARK, PARK.map(|(x, y)| (x - 4, y))).is_empty());
}

#[test]
fn a_lattice_over_works_worth_a_strike_is_struck_and_a_lone_one_is_not() {
    let lattice = [(BuildingKind::Lattice, 36, 9)];
    let spires = [(6, 14), (6, 17), (9, 14)];
    let prime = BotConfig::new(BotDifficulty::Prime, BotStance::Balanced, 11);
    let guarding = strikes(&spires, &BLOCK, &lattice, &[(31, 5)]);
    let fired = launches(&decide(&guarding, &staged(&guarding, 2), prime));
    assert_eq!(fired.len(), 3, "{fired:?}");
    assert!(
        fired
            .iter()
            .all(|(_, target)| *target == fired[0].1 && on(&[(36, 9)], *target)),
        "one volley on the Lattice: {fired:?}"
    );

    let lone = strikes(&spires, &[], &lattice, &[(31, 5)]);
    assert!(launches(&decide(&lone, &staged(&lone, 2), prime)).is_empty());
}

#[test]
fn a_known_enemy_spire_is_struck_while_the_seat_lies_open_and_spared_once_it_is_guarded() {
    let open = strikes(&[(6, 14)], &[], &[(BuildingKind::Spire, 30, 8)], &[(31, 4)]);
    let fired = launches(&decide(&open, &staged(&open, 0), config()));
    assert!(
        fired.len() == 1 && on(&[(30, 8)], fired[0].1),
        "an empty Spire, for what it threatens: {fired:?}"
    );

    let mut guarded = open.clone();
    guarded
        .buildings
        .push(building(0, BuildingKind::Lattice, 5, 11));
    let state = stocked(&staged(&guarded, 0), 0, BuildingKind::Lattice, 2);
    assert!(launches(&decide(&guarded, &state, config())).is_empty());
}

/// East Crucibles spread so that no warhead reaches two.
const SPREAD: [(i32, i32); 6] = [(20, 2), (28, 2), (36, 2), (20, 20), (28, 20), (36, 20)];

/// West's launches from loaded `spires`, remembering East's spread
/// Crucibles, and a second East Foundry when `second`, but never having
/// seen East's start.
fn last_stand(spires: &[(i32, i32)], second: bool) -> Vec<(BuildingId, TilePos)> {
    let mut scenario = strikes(spires, &SPREAD, &[], &[]);
    if second {
        scenario
            .buildings
            .push(building(1, BuildingKind::Foundry, 43, 19));
    }
    let state = staged(&scenario, 0);
    let mut east: Vec<&BuildingSpec> = scenario
        .buildings
        .iter()
        .filter(|b| b.player == 1)
        .collect();
    east.sort_by_key(|b| (b.y, b.x));
    let mut value = serde_json::to_value(&state).unwrap();
    value["vision"][0]["ghosts"] = east
        .into_iter()
        .map(|b| {
            let hp = b.kind.base_stats().max_hp;
            json!({"kind": b.kind, "owner": 1, "anchor": {"x": b.x, "y": b.y}, "hp": hp})
        })
        .collect();
    let remembered: State = serde_json::from_value(value).unwrap();
    launches(&decide(&scenario, &remembered, config()))
}

#[test]
fn a_seat_s_last_known_foundry_draws_the_volley_that_fells_it() {
    let spires = [(6, 14), (6, 17), (9, 14), (9, 17)];
    let fired = last_stand(&spires, false);
    assert_eq!(fired.len(), 4, "{fired:?}");
    let start = map(&strikes(&[], &[], &[], &[]))
        .start(PlayerId(1))
        .unwrap();
    assert!(
        fired
            .iter()
            .all(|(_, target)| *target == fired[0].1 && on(&[(start.x, start.y)], *target)),
        "one volley on the Foundry: {fired:?}"
    );
    assert!(
        last_stand(&spires[..3], false).is_empty(),
        "three warheads only damage it"
    );
    assert!(
        last_stand(&spires, true).is_empty(),
        "a second Foundry keeps the seat in the match"
    );
}

/// West's warhead investments on `state`.
fn wanted(scenario: &Scenario, state: &State) -> Vec<Investment> {
    let observation = ObservationData::fog_honest(state, PlayerId(0));
    crate::warheads::investments(&observation, &map(scenario), middling(), 360)
        .into_iter()
        .map(|(investment, _)| investment)
        .collect()
}

#[test]
fn a_spire_is_wanted_once_a_crucible_stands_and_a_known_strike_repays_its_round_and_the_spire() {
    let mut scenario = strikes(&[], &BLOCK, &[], &[(31, 5)]);
    let state = staged(&scenario, 0);
    assert!(wanted(&scenario, &state).is_empty(), "no Crucible yet");
    scenario
        .buildings
        .push(building(0, BuildingKind::Crucible, 6, 17));
    let state = staged(&scenario, 0);
    assert_eq!(
        wanted(&scenario, &state),
        [Investment::Tech(BuildingKind::Spire)]
    );
    let mut pair = scenario.clone();
    pair.buildings
        .retain(|b| !(b.player == 1 && b.kind == BuildingKind::Crucible && b.y == 10));
    let state = staged(&pair, 0);
    assert!(
        wanted(&pair, &state).is_empty(),
        "a pair of Crucibles repays a round but not the Spire as well"
    );
}

#[test]
fn an_empty_spire_wants_a_round_and_known_interceptors_want_another_spire() {
    let mut scenario = strikes(&[(6, 14)], &PAIR, &[], &[(31, 4)]);
    scenario
        .buildings
        .push(building(0, BuildingKind::Crucible, 6, 17));
    let mut state = scenario.build().unwrap();
    advance_to(&mut state, 12, &[]);
    let spire = state
        .buildings()
        .iter()
        .find(|b| b.kind == BuildingKind::Spire)
        .unwrap()
        .id;
    assert_eq!(wanted(&scenario, &state), [Investment::Round(spire)]);

    // A loaded Spire against a block under one interceptor wants company.
    let mut guarded = scenario.clone();
    guarded
        .buildings
        .push(building(1, BuildingKind::Lattice, 36, 9));
    for (x, y) in &BLOCK[2..] {
        guarded
            .buildings
            .push(building(1, BuildingKind::Crucible, *x, *y));
    }
    let state = staged(&guarded, 1);
    assert_eq!(
        wanted(&guarded, &state),
        [Investment::Capacity(BuildingKind::Spire)]
    );
}

#[test]
fn spires_short_of_a_full_lattice_over_works_worth_its_interceptors_want_another() {
    let lattice = [(BuildingKind::Lattice, 36, 9)];
    let mut scenario = strikes(&[(6, 14), (6, 17)], &BLOCK, &lattice, &[(31, 5)]);
    scenario
        .buildings
        .push(building(0, BuildingKind::Crucible, 9, 17));
    let state = staged(&scenario, 2);
    assert_eq!(
        wanted(&scenario, &state),
        [Investment::Capacity(BuildingKind::Spire)]
    );
}

#[test]
fn every_empty_spire_loads_toward_a_volley_past_a_full_lattice() {
    let lattice = [(BuildingKind::Lattice, 36, 9)];
    let mut scenario = strikes(&[(6, 14), (6, 17)], &BLOCK, &lattice, &[(31, 5)]);
    scenario
        .buildings
        .push(building(0, BuildingKind::Crucible, 9, 17));
    let state = scenario.build().unwrap();
    let mut state = stocked(&state, 1, BuildingKind::Lattice, 2);
    advance_to(&mut state, 12, &[]);
    let rounds: Vec<Investment> = state
        .buildings()
        .iter()
        .filter(|b| b.player == PlayerId(0) && b.kind == BuildingKind::Spire)
        .map(|b| Investment::Round(b.id))
        .collect();
    assert_eq!(rounds.len(), 2);
    assert_eq!(wanted(&scenario, &state), rounds);
}

#[test]
fn a_spire_steps_through_deepening_the_seats_own_extractor() {
    let mut scenario = arena(1_000);
    scenario.map[5].replace_range(12..=12, "E");
    scenario.buildings.extend([
        building(0, BuildingKind::Crucible, 1, 8),
        building(0, BuildingKind::Extractor, 12, 5),
    ]);
    let model = map(&scenario);
    let state = scenario.build().unwrap();
    let extractor = state
        .buildings()
        .iter()
        .find(|b| b.kind == BuildingKind::Extractor)
        .unwrap()
        .id;
    let spire = Investment::Tech(BuildingKind::Spire);
    let step = |state: &State| {
        investments::step(
            &ObservationData::fog_honest(state, PlayerId(0)),
            &model,
            spire,
        )
        .map(|(step, _)| step)
    };
    assert_eq!(step(&state), Some(Step::Upgrade(extractor)));
    let mut refitting = state.clone();
    refitting.tick(&[PlayerCommand {
        player: PlayerId(0),
        command: Command::UpgradeBuilding {
            building: extractor,
        },
    }]);
    assert_eq!(
        step(&refitting),
        None,
        "a refitting Deep Bore is waited for"
    );
    let mut value = serde_json::to_value(&state).unwrap();
    for entry in value["buildings"].as_array_mut().unwrap() {
        if entry["kind"] == json!("extractor") {
            entry["tier"] = json!(1);
        }
    }
    let deep: State = serde_json::from_value(value).unwrap();
    assert_eq!(step(&deep), Some(Step::Build(BuildingKind::Spire)));
}

/// West's checkpoint with its saving target set to `investment`.
fn saving_for(
    scenario: &Scenario,
    state: &State,
    investment: Investment,
) -> (Opponent, Checkpoint) {
    let opponent = restored_with(&seat(scenario, 0), scenario, state, |json| {
        json["saving"] = json!({
            "protected": 0,
            "target": {"investment": investment, "attempt": null},
        });
        json["income"] = json!({"previous": null, "per_minute": 2_000});
    });
    let checkpoint = opponent.checkpoint();
    (opponent, checkpoint)
}

#[test]
fn a_saved_for_round_is_ordered_and_a_resumed_seat_orders_it_alike() {
    let mut scenario = strikes(&[(6, 14)], &PAIR, &[], &[(31, 4)]);
    // Every tech building stands, so the round competes with no tech.
    scenario.buildings.extend([
        building(0, BuildingKind::Crucible, 6, 17),
        building(0, BuildingKind::Fabricator, 9, 17),
        building(0, BuildingKind::Airworks, 9, 14),
    ]);
    scenario.players[0].scrap = 1_000;
    let mut state = scenario.build().unwrap();
    advance_to(&mut state, 12, &[]);
    let spire = state
        .buildings()
        .iter()
        .find(|b| b.kind == BuildingKind::Spire)
        .unwrap()
        .id;
    let (mut opponent, checkpoint) = saving_for(&scenario, &state, Investment::Round(spire));
    let resumed =
        serde_json::from_str::<Checkpoint>(&serde_json::to_string(&checkpoint).unwrap()).unwrap();
    let mut twin = Opponent::restore(&resumed, &scenario, &state, map(&scenario)).unwrap();
    let commands = opponent.act(&state, &mut OwnEvents::default());
    assert!(
        commands
            .iter()
            .any(|command| command.command == Command::OrderRound { building: spire }),
        "{commands:?}"
    );
    assert_eq!(twin.act(&state, &mut OwnEvents::default()), commands);
    assert_eq!(opponent.checkpoint(), twin.checkpoint());
}

/// West's defense investments with what it sees remembered.
fn guards(scenario: &Scenario) -> Vec<(Investment, u32)> {
    guards_of(scenario, 0)
}

/// A seat's defense investments with what it sees remembered.
fn guards_of(scenario: &Scenario, player: u8) -> Vec<(Investment, u32)> {
    let state = scenario.build().unwrap();
    let observation = ObservationData::fog_honest(&state, PlayerId(player));
    let mut memory = Memory::default();
    memory.observe(&observation);
    crate::defenses::investments(
        &observation,
        &map(scenario),
        &memory,
        middling(),
        true,
        false,
        crate::defenses::Stakes::default(),
    )
}

fn lattice_sites(offers: &[(Investment, u32)]) -> Vec<TilePos> {
    offers
        .iter()
        .filter_map(|(investment, _)| match investment {
            Investment::Defense {
                kind: BuildingKind::Lattice,
                anchor,
            } => Some(*anchor),
            _ => None,
        })
        .collect()
}

/// West's works on the field: its Foundry with a Fabricator and a Crucible
/// beside it, a Harvester to build, and East's Spires at `spires`, seen.
fn guarded(spires: &[(i32, i32)], lattices: &[(i32, i32)]) -> Scenario {
    let mut scenario = field();
    garrison(&mut scenario);
    scenario.units.push(harvester(0, 7, 9));
    scenario.buildings.extend([
        building(0, BuildingKind::Fabricator, 3, 8),
        building(0, BuildingKind::Crucible, 5, 11),
    ]);
    for (x, y) in lattices {
        scenario
            .buildings
            .push(building(0, BuildingKind::Lattice, *x, *y));
    }
    for (x, y) in spires {
        scenario
            .buildings
            .push(building(1, BuildingKind::Spire, *x, *y));
        scenario.units.push(unit(0, UnitKind::Kestrel, *x, y + 3));
    }
    scenario
}

#[test]
fn a_known_enemy_spire_brings_a_lattice_over_the_base_and_none_without_one() {
    assert!(lattice_sites(&guards(&guarded(&[], &[]))).is_empty());
    let sites = lattice_sites(&guards(&guarded(&[(30, 4)], &[])));
    let [site] = sites[..] else {
        panic!("{sites:?}");
    };
    // Its ring holds the Foundry's centre.
    let ring = BuildingKind::Lattice
        .battery()
        .unwrap()
        .ring
        .to_num::<i64>();
    let (lattice, foundry) = (
        crate::frame::footprint_centre(BuildingKind::Lattice, site),
        crate::frame::footprint_centre(BuildingKind::Foundry, TilePos::new(3, 11)),
    );
    let (dx, dy) = (lattice.0 - foundry.0, lattice.1 - foundry.1);
    assert!(dx * dx + dy * dy <= 4 * ring * ring, "{site:?}");
}

#[test]
fn known_spires_beyond_the_interceptors_over_a_base_bring_another_lattice() {
    let lattice = [(8, 14)];
    assert!(
        lattice_sites(&guards(&guarded(&[(30, 4), (30, 18)], &lattice))).is_empty(),
        "two interceptors answer two Spires"
    );
    assert_eq!(
        lattice_sites(&guards(&guarded(&[(30, 4), (30, 18), (36, 10)], &lattice))).len(),
        1
    );
}

/// The score of West's best Array offer.
fn array_score(offers: &[(Investment, u32)]) -> u32 {
    offers
        .iter()
        .filter(|(investment, _)| {
            matches!(
                investment,
                Investment::Defense {
                    kind: BuildingKind::Array,
                    ..
                }
            )
        })
        .map(|(_, score)| *score)
        .max()
        .unwrap_or(0)
}

#[test]
fn radar_is_worth_more_where_a_known_spire_s_line_enters_a_guarded_ring() {
    let lattice = [(8, 14)];
    let mut known = guarded(&[(40, 4)], &lattice);
    // The Kestrel that saw it is gone; it is remembered.
    known.units.retain(|unit| unit.kind != UnitKind::Kestrel);
    let state = known.build().unwrap();
    let mut value = serde_json::to_value(&state).unwrap();
    value["vision"][0]["ghosts"] =
        json!([{"kind": "spire", "owner": 1, "anchor": {"x": 40, "y": 4}, "hp": 800}]);
    let remembered: State = serde_json::from_value(value).unwrap();
    let offers = |state: &State| {
        let observation = ObservationData::fog_honest(state, PlayerId(0));
        let mut memory = Memory::default();
        memory.observe(&observation);
        crate::defenses::investments(
            &observation,
            &map(&known),
            &memory,
            middling(),
            true,
            false,
            crate::defenses::Stakes::default(),
        )
    };
    assert!(array_score(&offers(&remembered)) > array_score(&offers(&state)));
}

#[test]
fn mirrored_seats_guard_mirrored_lattice_sites() {
    let mut scenario = guarded(&[(30, 4)], &[]);
    // East's half-turned works, a West Spire where East's Kestrel sees it.
    scenario.units.push(harvester(1, 47 - 7, 23 - 9));
    scenario.buildings.extend([
        building(1, BuildingKind::Fabricator, 46 - 3, 22 - 8),
        building(1, BuildingKind::Crucible, 46 - 5, 22 - 11),
        building(0, BuildingKind::Spire, 46 - 30, 22 - 4),
    ]);
    scenario
        .units
        .push(unit(1, UnitKind::Kestrel, 47 - 30, 23 - 7));
    scenario
        .units
        .extend(GARRISON.map(|(x, y)| unit(1, UnitKind::Sentinel, 47 - x, 23 - y)));
    let west = lattice_sites(&guards_of(&scenario, 0));
    let east = lattice_sites(&guards_of(&scenario, 1));
    assert_eq!(west.len(), 1);
    assert_eq!(
        east,
        west.iter()
            .map(|site| TilePos::new(46 - site.x, 22 - site.y))
            .collect::<Vec<_>>()
    );
}

/// West's guarded base (see [`guarded`]) with a lone Extractor out at
/// (22, 3) and any own Lattices at `lattices`; East's Crucible at (30, 4),
/// seen while `crucible`, and East's Extractor at (40, 19), a Deep Bore
/// every seat knows of while `bore`.
fn spire_tech(crucible: bool, bore: bool, lattices: &[(i32, i32)]) -> (Scenario, State) {
    let mut scenario = guarded(&[], lattices);
    let mut rows: Vec<Vec<char>> = scenario
        .map
        .iter()
        .map(|row| row.chars().collect())
        .collect();
    rows[3][22] = 'E';
    rows[19][40] = 'E';
    scenario.map = rows
        .into_iter()
        .map(|row| row.into_iter().collect())
        .collect();
    scenario
        .buildings
        .push(building(0, BuildingKind::Extractor, 22, 3));
    scenario
        .buildings
        .push(building(1, BuildingKind::Extractor, 40, 19));
    if crucible {
        scenario
            .buildings
            .push(building(1, BuildingKind::Crucible, 30, 4));
        scenario.units.push(unit(0, UnitKind::Kestrel, 30, 7));
    }
    let state = scenario.build().unwrap();
    let mut value = serde_json::to_value(&state).unwrap();
    if bore {
        for b in value["buildings"].as_array_mut().unwrap() {
            if b["kind"] == json!("extractor") && b["player"] == json!(1) {
                b["tier"] = json!(1);
                b["hp"] = json!(BuildingKind::Extractor.tier_stats(1).max_hp);
            }
        }
    }
    (scenario, serde_json::from_value(value).unwrap())
}

/// West's defense offers in `state`.
fn guards_in(scenario: &Scenario, state: &State) -> Vec<(Investment, u32)> {
    let observation = ObservationData::fog_honest(state, PlayerId(0));
    let mut memory = Memory::default();
    memory.observe(&observation);
    crate::defenses::investments(
        &observation,
        &map(scenario),
        &memory,
        middling(),
        true,
        false,
        crate::defenses::Stakes::default(),
    )
}

fn lattice_score(offers: &[(Investment, u32)]) -> u32 {
    offers
        .iter()
        .filter(|(investment, _)| {
            matches!(
                investment,
                Investment::Defense {
                    kind: BuildingKind::Lattice,
                    ..
                }
            )
        })
        .map(|(_, score)| *score)
        .max()
        .unwrap_or(0)
}

#[test]
fn spire_tech_on_the_enemy_side_brings_a_lattice_before_any_spire_is_seen() {
    let offers = |crucible, bore, lattices: &[(i32, i32)]| {
        let (scenario, state) = spire_tech(crucible, bore, lattices);
        guards_in(&scenario, &state)
    };
    assert!(lattice_sites(&offers(false, false, &[])).is_empty());
    // A Deep Bore every seat sees needs no scouting; a seen Crucible is the
    // other half of a Spire's tech.
    for (crucible, bore) in [(true, false), (false, true)] {
        let sites = lattice_sites(&offers(crucible, bore, &[]));
        assert_eq!(sites.len(), 1, "crucible {crucible} bore {bore}");
    }
    assert!(
        lattice_score(&offers(true, true, &[])) > lattice_score(&offers(true, false, &[])),
        "both halves of the tech press harder than one"
    );
    // One half guards only the most valuable base; both guard every one.
    let home = lattice_sites(&offers(true, false, &[]))[0];
    let covered = [(home.x, home.y)];
    assert!(lattice_sites(&offers(true, false, &covered)).is_empty());
    let [expansion] = lattice_sites(&offers(true, true, &covered))[..] else {
        panic!("both halves guard the lone Extractor too");
    };
    assert!(
        expansion.x > 14,
        "{expansion:?} guards the Extractor at (22, 3)"
    );
}

#[test]
fn a_lattice_rings_the_costliest_point_of_its_base_from_the_home_side() {
    let sites = lattice_sites(&guards(&guarded(&[(30, 4)], &[])));
    let [site] = sites[..] else {
        panic!("{sites:?}");
    };
    let ring = BuildingKind::Lattice
        .battery()
        .unwrap()
        .ring
        .to_num::<i64>();
    let lattice = crate::frame::footprint_centre(BuildingKind::Lattice, site);
    // Every West work stands in its ring, wherever a strike lands on them.
    for (kind, anchor) in [
        (BuildingKind::Foundry, TilePos::new(3, 11)),
        (BuildingKind::Fabricator, TilePos::new(3, 8)),
        (BuildingKind::Crucible, TilePos::new(5, 11)),
    ] {
        let centre = crate::frame::footprint_centre(kind, anchor);
        let (dx, dy) = (lattice.0 - centre.0, lattice.1 - centre.1);
        assert!(
            dx * dx + dy * dy <= 4 * ring * ring,
            "{kind:?} outside {site:?}"
        );
    }
    // Behind the works rather than toward the Spire to the east.
    assert!(site.x <= 7, "{site:?}");
}
