use super::*;
use crate::composition;
use crate::investments::{self, Investment, Situation};
use crate::memory::Memory;

/// The arena with a built west Fabricator and a large west bank, plus
/// whatever the enemy stages in sight of the west base.
fn armed(
    enemy_units: &[(UnitKind, i32, i32)],
    enemy_buildings: &[(BuildingKind, i32, i32)],
) -> Scenario {
    let mut scenario = arena(2_000);
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Fabricator,
        x: 8,
        y: 8,
    });
    scenario
        .units
        .extend(enemy_units.iter().map(|(kind, x, y)| UnitSpec {
            player: 1,
            kind: *kind,
            x: *x,
            y: *y,
        }));
    scenario
        .buildings
        .extend(enemy_buildings.iter().map(|(kind, x, y)| BuildingSpec {
            player: 1,
            kind: *kind,
            x: *x,
            y: *y,
        }));
    scenario
}

fn fabricator(state: &State) -> BuildingId {
    state
        .buildings()
        .iter()
        .find(|building| {
            building.player == PlayerId(0) && building.kind == BuildingKind::Fabricator
        })
        .unwrap()
        .id
}

/// What the west Fabricator queues on the first decision.
fn fabricator_trains(scenario: &Scenario) -> Vec<UnitKind> {
    let state = scenario.build().unwrap();
    let at = fabricator(&state);
    trains(&seat(scenario, 0).act(&state, &mut OwnEvents::default()))
        .into_iter()
        .filter(|(building, _)| *building == at)
        .map(|(_, kind)| kind)
        .collect()
}

fn anti_air(kinds: &[UnitKind]) -> bool {
    kinds
        .iter()
        .any(|kind| composition::role(*kind) == Some(composition::Role::AntiAir))
}

#[test]
fn seen_enemy_air_brings_anti_air_in_proportion() {
    let buzzards = [(UnitKind::Buzzard, 6, 8), (UnitKind::Buzzard, 7, 9)];
    assert!(anti_air(&fabricator_trains(&armed(&buzzards, &[]))));
    assert!(
        !anti_air(&fabricator_trains(&armed(&[], &[]))),
        "no air, no anti-air"
    );

    // Over the base, the Buzzards are a defense's to meet first, with half
    // again their worth.
    let mut covered = armed(&buzzards, &[]);
    covered.units.extend((9..13).map(|x| UnitSpec {
        player: 0,
        kind: UnitKind::Flakhound,
        x,
        y: 6,
    }));
    assert!(
        !anti_air(&fabricator_trains(&covered)),
        "four Flakhounds already answer two Buzzards"
    );
}

#[test]
fn a_seen_enemy_airworks_raises_anti_air_before_any_flyer() {
    let airworks = [(BuildingKind::Airworks, 8, 2)];
    assert!(anti_air(&fabricator_trains(&armed(&[], &airworks))));

    let target = |scenario: &Scenario| {
        let state = scenario.build().unwrap();
        seat(scenario, 0)
            .act_traced(&state, &mut OwnEvents::default())
            .1
            .unwrap()
            .target
            .map(|target| target.investment)
    };
    let mut bare = arena(200);
    // A Turret already guards home, so tech need not wait for one.
    bare.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Turret,
        x: 4,
        y: 9,
    });
    assert_eq!(target(&bare), None, "premise: nothing worth saving for yet");
    bare.buildings.push(BuildingSpec {
        player: 1,
        kind: BuildingKind::Airworks,
        x: 8,
        y: 2,
    });
    assert_eq!(
        target(&bare),
        Some(Investment::Tech(BuildingKind::Fabricator)),
        "the cheapest anti-air producer"
    );
}

#[test]
fn a_seen_enemy_fabricator_raises_the_line_need_before_any_unit() {
    // A Fabricator is how a seat fields a ground army: known, it counts as the
    // threat it paid for, and the seat wants a line before the first enemy
    // unit is seen, as a known Airworks raises anti-air.
    let army = |fabricator: bool| {
        let mut scenario = arena(200);
        if fabricator {
            scenario.buildings.push(BuildingSpec {
                player: 1,
                kind: BuildingKind::Fabricator,
                x: 8,
                y: 2,
            });
        }
        let state = scenario.build().unwrap();
        let army = seat(&scenario, 0)
            .act_traced(&state, &mut OwnEvents::default())
            .1
            .unwrap()
            .army;
        let line = army
            .roles
            .iter()
            .find(|need| need.role == composition::Role::Line)
            .map_or(0, |need| need.need);
        (army.enemy.ground, line)
    };
    assert_eq!(
        army(false),
        (0, 0),
        "premise: nothing known, nothing wanted"
    );
    let (ground, line) = army(true);
    assert_eq!(
        ground,
        u64::from(BuildingKind::Fabricator.base_stats().construction.cost)
    );
    assert!(line > 0, "{line}");
}

fn siege(kind: UnitKind) -> bool {
    kind == UnitKind::Sapper || composition::role(kind) == Some(composition::Role::Siege)
}

#[test]
fn known_defenses_bring_siege_or_sappers_before_a_line() {
    let first = |scenario: &Scenario| {
        let state = scenario.build().unwrap();
        trains(&seat(scenario, 0).act(&state, &mut OwnEvents::default()))
            .first()
            .map(|(_, kind)| *kind)
    };
    assert!(first(&armed(&[], &[(BuildingKind::Turret, 12, 9)])).is_some_and(siege));
    assert!(
        first(&armed(&[], &[])).is_some_and(|kind| !siege(kind)),
        "with no guns known, a line comes first"
    );
}

#[test]
fn an_army_with_its_line_wants_siege_behind_it() {
    let mut scenario = armed(&[], &[]);
    scenario.units.extend((3..=8).map(|x| UnitSpec {
        player: 0,
        kind: UnitKind::Sentinel,
        x,
        y: 10,
    }));
    assert!(
        fabricator_trains(&scenario)
            .first()
            .is_some_and(|kind| siege(*kind))
    );
}

#[test]
fn piledrivers_count_toward_the_army_that_wants_siege_behind_it() {
    let mut scenario = armed(&[], &[]);
    scenario
        .units
        .extend((3..=5).map(|x| unit(0, UnitKind::Piledriver, x, 10)));
    let kinds = fabricator_trains(&scenario);
    assert!(kinds.first().is_some_and(|kind| siege(*kind)), "{kinds:?}");
}

#[test]
fn raiders_are_never_line_units() {
    for kind in UnitKind::ALL {
        if composition::role(kind) == Some(composition::Role::Line) {
            assert_ne!(kind, UnitKind::Scuttler);
        }
    }
    let kinds = fabricator_trains(&armed(&[], &[]));
    assert!(!kinds.contains(&UnitKind::Scuttler));
}

#[test]
fn working_producers_ask_for_another_while_unspent_income_and_need_last() {
    let scenario = armed(&[], &[]);
    let mut state = scenario.build().unwrap();
    let model = map(&scenario);
    let memory = Memory::default();
    let busy = PlayerCommand {
        player: PlayerId(0),
        command: Command::Train {
            building: fabricator(&state),
            kind: UnitKind::Warden,
        },
    };
    state.tick(&[busy]);
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let wants = |income: u32, wanted: Vec<composition::Role>| {
        investments::candidates(&Situation {
            income,
            wanted,
            ..calm(&observation, &model, &memory)
        })
        .into_iter()
        .map(|candidate| candidate.investment)
        .collect::<Vec<_>>()
    };
    // What the working Fabricator spends a minute on its Warden.
    let stats = UnitKind::Warden.stats();
    let spends = stats.cost * 20 * 60 / stats.train_ticks;
    let another = Investment::Capacity(BuildingKind::Fabricator);
    let line = || vec![composition::Role::Line];
    assert!(
        wants(2 * spends, line()).contains(&another),
        "income left over keeps a second Fabricator as busy"
    );
    assert!(
        !wants(2 * spends - 1, line()).contains(&another),
        "but not just short of it"
    );
    assert!(
        !wants(4 * spends, Vec::new()).contains(&another),
        "and not while nothing it trains is wanted"
    );
    assert!(
        !wants(4 * spends, line()).contains(&Investment::Capacity(BuildingKind::Foundry)),
        "an idle Foundry is no reason for another"
    );
}

#[test]
fn enemy_units_fade_and_vanish_when_their_spot_is_seen_empty() {
    let state = armed(&[(UnitKind::Sentinel, 6, 9)], &[]).build().unwrap();
    let seen = ObservationData::fog_honest(&state, PlayerId(0));
    let mut memory = Memory::default();
    memory.observe(&seen);
    assert_eq!(memory.units().len(), 1);
    assert_eq!(memory.units()[0].confidence(seen.tick), 1_000);

    let mut hidden = seen.clone();
    hidden.enemy_units.clear();
    hidden.tick = 300;
    let tile = memory.units()[0].tile;
    let index = usize::try_from(tile.y * hidden.map_width + tile.x).unwrap();
    hidden.visible[index] = false;
    let mut faded = memory.clone();
    faded.observe(&hidden);
    assert_eq!(faded.units()[0].confidence(300), 500);
    hidden.tick = 600;
    faded.observe(&hidden);
    assert!(faded.units().is_empty(), "forgotten after 600 ticks");

    let mut empty = seen;
    empty.enemy_units.clear();
    empty.tick = 12;
    memory.observe(&empty);
    assert!(memory.units().is_empty(), "its spot is in sight and empty");
}

#[test]
fn a_producer_trains_the_best_unit_it_can_afford() {
    let clustered = [
        (UnitKind::Harvester, 10, 9),
        (UnitKind::Harvester, 11, 9),
        (UnitKind::Harvester, 10, 10),
        (UnitKind::Harvester, 11, 10),
    ];
    let mut scenario = armed(&clustered, &[(BuildingKind::Turret, 12, 9)]);
    scenario.players[0].scrap = 150 + UnitKind::Sentinel.stats().cost;
    let mut state = scenario.build().unwrap();
    let busy = PlayerCommand {
        player: PlayerId(0),
        command: Command::Train {
            building: foundries(&state, PlayerId(0))[0],
            kind: UnitKind::Sentinel,
        },
    };
    advance_to(&mut state, 12, &[busy]);
    let at = fabricator(&state);
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let trained: Vec<UnitKind> = trains(&commands)
        .into_iter()
        .filter(|(building, _)| *building == at)
        .map(|(_, kind)| kind)
        .collect();
    assert!(
        UnitKind::Bombard.stats().cost > 150,
        "premise: the splash unit is out of reach"
    );
    assert_eq!(trained, [UnitKind::Lancer]);
}

#[test]
fn the_most_wanted_role_takes_the_scrap_before_a_nearer_producer() {
    // Two Foundries nearer home than the Fabricator, their Harvesters enough,
    // and a bank that leaves scrap for one unit beside the saving target.
    let mut scenario = armed(&[], &[]);
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 3,
        y: 8,
    });
    scenario.units.extend(workforce(0));
    for player in &mut scenario.players {
        player.scrap = 200;
    }
    let state = scenario.build().unwrap();
    // Two Buzzards seen over the East base: anti-air is wanted, and nothing
    // is in sight to defend against.
    let mut opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
        json["memory"]["units"] = remembered((0..2).map(|index: u32| {
            let tile = TilePos::new(20, 2 + i32::try_from(index).unwrap());
            (1_000 + index, UnitKind::Buzzard, tile, state.current_tick())
        }));
    });
    let trained = trains(&opponent.act(&state, &mut OwnEvents::default()));
    let [(at, kind)] = trained[..] else {
        panic!("{trained:?}");
    };
    assert_eq!(
        at,
        fabricator(&state),
        "the Foundries nearer home do not spend the scrap on line units first"
    );
    assert!(anti_air(&[kind]), "{kind:?}");
}

#[test]
fn a_working_foundry_or_crucible_asks_for_another_at_home() {
    let mut scenario = armed(&[], &[]);
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Crucible,
        x: 12,
        y: 8,
    });
    let mut state = scenario.build().unwrap();
    let model = map(&scenario);
    let memory = Memory::default();
    let at = |kind: BuildingKind| {
        state
            .buildings()
            .iter()
            .find(|building| building.player == PlayerId(0) && building.kind == kind)
            .unwrap()
            .id
    };
    let train = |building, kind| PlayerCommand {
        player: PlayerId(0),
        command: Command::Train { building, kind },
    };
    let orders = [
        train(at(BuildingKind::Foundry), UnitKind::Sentinel),
        train(at(BuildingKind::Crucible), UnitKind::Breaker),
    ];
    state.tick(&orders);
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let wants = |wanted: Vec<composition::Role>| {
        investments::candidates(&Situation {
            income: 100_000,
            wanted,
            ..calm(&observation, &model, &memory)
        })
        .into_iter()
        .map(|candidate| candidate.investment)
        .collect::<Vec<_>>()
    };
    let offered = wants(vec![composition::Role::Line]);
    for kind in [BuildingKind::Foundry, BuildingKind::Crucible] {
        assert!(
            offered.contains(&Investment::Capacity(kind)),
            "{kind:?}: {offered:?}"
        );
    }
    let offered = wants(Vec::new());
    assert!(
        offered
            .iter()
            .all(|investment| !matches!(investment, Investment::Capacity(_))),
        "with nothing wanted, income alone buys no producer: {offered:?}"
    );
}

#[test]
fn an_idle_foundry_leaves_its_scrap_for_a_busy_producer_of_a_wanted_role() {
    // An army of line units, so siege is wanted and the line is not.
    let line = |scenario: &mut Scenario| {
        scenario.units.extend(workforce(0));
        scenario
            .units
            .extend((3..=10).map(|x| unit(0, UnitKind::Sentinel, x, 10)));
    };
    let foundry_trains = |scenario: &Scenario, busy: bool| {
        let mut state = scenario.build().unwrap();
        if busy {
            let fabricator = fabricator(&state);
            state.tick(&[PlayerCommand {
                player: PlayerId(0),
                command: Command::Train {
                    building: fabricator,
                    kind: UnitKind::Lancer,
                },
            }]);
            advance_to(
                &mut state,
                crate::decision_interval(BotDifficulty::Standard),
                &[],
            );
        }
        let foundry = foundries(&state, PlayerId(0))[0];
        trains(&seat(scenario, 0).act(&state, &mut OwnEvents::default()))
            .into_iter()
            .filter(|(building, _)| *building == foundry)
            .map(|(_, kind)| kind)
            .collect::<Vec<_>>()
    };
    let mut lone = arena(2_000);
    line(&mut lone);
    assert!(
        foundry_trains(&lone, false)
            .iter()
            .any(|kind| composition::role(*kind) == Some(composition::Role::Line)),
        "premise: with nothing else to train siege, the Foundry trains its line"
    );
    let mut fabricated = armed(&[], &[]);
    line(&mut fabricated);
    assert_eq!(
        foundry_trains(&fabricated, true),
        [],
        "the scrap waits for the Fabricator"
    );
}

/// The arena with a working west base, a built Crucible and an army whose
/// line and siege shares are already met, with `remembered` enemy units seen
/// over the east base and `owned` extra west units.
fn crucible_seat(remembered: &[UnitKind], owned: &[UnitKind]) -> (Scenario, State, Opponent) {
    crucible_seat_banking(2_000, remembered, owned)
}

fn crucible_seat_banking(
    scrap: u32,
    remembered: &[UnitKind],
    owned: &[UnitKind],
) -> (Scenario, State, Opponent) {
    let mut scenario = arena(scrap);
    scenario.units.extend(workforce(0));
    scenario
        .units
        .extend((3..8).map(|x| unit(0, UnitKind::Avalanche, x, 10)));
    scenario
        .units
        .extend((3..7).map(|x| unit(0, UnitKind::Breaker, x, 7)));
    scenario
        .units
        .extend(owned.iter().zip(3..).map(|(kind, x)| unit(0, *kind, x, 4)));
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Crucible,
        x: 8,
        y: 8,
    });
    let state = scenario.build().unwrap();
    let now = state.current_tick();
    let opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
        json["memory"]["units"] = super::remembered(remembered.iter().zip(0..).map(
            |(&kind, index): (&UnitKind, u32)| {
                let tile = TilePos::new(20, 2 + i32::try_from(index).unwrap());
                (1_000 + index, kind, tile, now)
            },
        ));
    });
    (scenario, state, opponent)
}

fn trains_mantle(remembered: &[UnitKind], owned: &[UnitKind]) -> bool {
    let (_, state, mut opponent) = crucible_seat(remembered, owned);
    trains(&opponent.act(&state, &mut OwnEvents::default()))
        .iter()
        .any(|(_, kind)| *kind == UnitKind::Mantle)
}

#[test]
fn seen_heavy_shellfire_brings_point_defense_in_proportion() {
    let avalanches = [UnitKind::Avalanche, UnitKind::Avalanche];
    assert!(
        trains_mantle(&avalanches, &[]),
        "two Avalanches, one Mantle"
    );
    assert!(
        !trains_mantle(&avalanches, &[UnitKind::Mantle]),
        "an owned Mantle already answers them"
    );
    assert!(
        !trains_mantle(&[UnitKind::Lancer, UnitKind::Lancer], &[]),
        "gunfire passes through point defense"
    );
    assert!(
        !trains_mantle(&[UnitKind::Bombard, UnitKind::Bombard], &[]),
        "a few light shells are no reason for one"
    );
}

#[test]
fn a_short_bank_saves_for_point_defense_against_heavy_shellfire() {
    let saving = |remembered: &[UnitKind]| {
        let (_, state, mut opponent) = crucible_seat_banking(100, remembered, &[]);
        opponent
            .act_traced(&state, &mut OwnEvents::default())
            .1
            .unwrap()
            .target
            .map(|target| target.investment)
    };
    assert_eq!(
        saving(&[UnitKind::Avalanche, UnitKind::Avalanche]),
        Some(Investment::Unit(UnitKind::Mantle)),
        "a Mantle has no cheaper stand-in, so the seat saves for one"
    );
    assert_ne!(
        saving(&[UnitKind::Lancer, UnitKind::Lancer]),
        Some(Investment::Unit(UnitKind::Mantle))
    );
}

/// `player`'s seat at its first decision, remembering `seen` enemy units on
/// the given tiles.
fn recalling(
    scenario: &Scenario,
    state: &State,
    player: u8,
    seen: &[(UnitKind, TilePos)],
) -> Opponent {
    let now = state.current_tick();
    restored_with(&seat(scenario, player), scenario, state, |json| {
        json["memory"]["units"] = remembered(
            seen.iter()
                .zip(0_u32..)
                .map(|(&(kind, tile), index)| (1_000 + index, kind, tile, now)),
        );
    })
}

/// The West seat at its first decision, remembering `seen` East units over
/// the East base, which is out of its sight.
fn remembering(scenario: &Scenario, state: &State, seen: &[UnitKind]) -> Opponent {
    let spots: Vec<(UnitKind, TilePos)> = seen
        .iter()
        .zip(0_i32..)
        .map(|(kind, index)| (*kind, TilePos::new(20, 1 + index % 10)))
        .collect();
    recalling(scenario, state, 0, &spots)
}

/// The arena with a built West Crucible beside the Fabricator.
fn foundry_of_heavies() -> Scenario {
    let mut scenario = armed(&[], &[]);
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Crucible,
        x: 12,
        y: 8,
    });
    scenario
}

/// What the West Crucible queues on its first decision, remembering `seen`.
fn crucible_trains(scenario: &Scenario, seen: &[UnitKind]) -> Vec<UnitKind> {
    let state = scenario.build().unwrap();
    let at = state
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0) && building.kind == BuildingKind::Crucible)
        .unwrap()
        .id;
    trains(&remembering(scenario, &state, seen).act(&state, &mut OwnEvents::default()))
        .into_iter()
        .filter(|(building, _)| *building == at)
        .map(|(_, kind)| kind)
        .collect()
}

#[test]
fn seen_enemy_heavies_bring_piledrivers_in_proportion() {
    let scenario = foundry_of_heavies();
    let breakers = [UnitKind::Breaker; 2];
    assert!(crucible_trains(&scenario, &breakers).contains(&UnitKind::Piledriver));
    // At least as much value in light machines wants no anti-heavy shot.
    let count = (2 * UnitKind::Breaker.stats().cost).div_ceil(UnitKind::Sentinel.stats().cost);
    let sentinels = vec![UnitKind::Sentinel; usize::try_from(count).unwrap()];
    assert!(!crucible_trains(&scenario, &sentinels).contains(&UnitKind::Piledriver));
    assert!(!crucible_trains(&scenario, &[]).contains(&UnitKind::Piledriver));

    // Enough Piledrivers already answer three quarters of two Breakers.
    let answer = 3 * 2 * UnitKind::Breaker.stats().cost / 4;
    let enough = answer.div_ceil(UnitKind::Piledriver.stats().cost);
    let mut covered = foundry_of_heavies();
    covered.units.extend(
        (0..i32::try_from(enough).unwrap())
            .map(|index| unit(0, UnitKind::Piledriver, 2 + index, 10)),
    );
    assert!(!crucible_trains(&covered, &breakers).contains(&UnitKind::Piledriver));
}

#[test]
fn an_enemy_heavy_out_of_sight_and_memory_brings_no_piledriver() {
    let mut scenario = foundry_of_heavies();
    scenario.units.push(unit(1, UnitKind::Breaker, 20, 2));
    let state = scenario.build().unwrap();
    let observation = aftermine_sim::observation::ObservationData::fog_honest(&state, PlayerId(0));
    assert!(
        observation
            .enemy_units
            .iter()
            .all(|unit| unit.kind != UnitKind::Breaker),
        "premise: the Breaker is out of sight"
    );
    assert!(!crucible_trains(&scenario, &[]).contains(&UnitKind::Piledriver));
}

#[test]
fn an_enemy_machine_rolled_out_of_sight_brings_piledrivers() {
    let mut scenario = foundry_of_heavies();
    scenario.units.push(unit(1, UnitKind::Maw, 20, 3));
    let state = scenario.build().unwrap();
    let maw = state
        .units()
        .iter()
        .find(|unit| unit.kind == UnitKind::Maw)
        .unwrap();
    let observation = aftermine_sim::observation::ObservationData::fog_honest(&state, PlayerId(0));
    assert!(
        observation.enemy_units.iter().all(|unit| unit.id != maw.id),
        "premise: the Maw is out of sight"
    );
    assert!(!crucible_trains(&scenario, &[]).contains(&UnitKind::Piledriver));

    // Every seat learns where a public cradle's machine stood up.
    let crucible = state
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0) && building.kind == BuildingKind::Crucible)
        .unwrap()
        .id;
    let mut events = OwnEvents::default();
    events.record(
        PlayerId(0),
        &[aftermine_sim::Event::UnitAssembled {
            cradle: aftermine_sim::BuildingId(99),
            unit: maw.id,
            kind: UnitKind::Maw,
            player: PlayerId(1),
            pos: maw.pos,
        }],
    );
    let trained: Vec<UnitKind> = trains(&seat(&scenario, 0).act(&state, &mut events))
        .into_iter()
        .filter(|(building, _)| *building == crucible)
        .map(|(_, kind)| kind)
        .collect();
    assert!(trained.contains(&UnitKind::Piledriver), "{trained:?}");
}

#[test]
fn mirrored_seats_answer_heavies_alike() {
    let mut scenario = arena(2_000);
    let (width, height) = (24, 12);
    for (kind, x, y) in [
        (BuildingKind::Fabricator, 8, 8),
        (BuildingKind::Crucible, 12, 8),
    ] {
        scenario.buildings.push(BuildingSpec {
            player: 0,
            kind,
            x,
            y,
        });
        scenario.buildings.push(BuildingSpec {
            player: 1,
            kind,
            x: width - 2 - x,
            y: height - 2 - y,
        });
    }
    let state = scenario.build().unwrap();
    let west_saw = [
        (UnitKind::Breaker, TilePos::new(20, 2)),
        (UnitKind::Breaker, TilePos::new(20, 3)),
    ];
    let east_saw: Vec<(UnitKind, TilePos)> = west_saw
        .iter()
        .map(|(kind, tile)| (*kind, TilePos::new(width - 1 - tile.x, height - 1 - tile.y)))
        .collect();
    let west = recalling(&scenario, &state, 0, &west_saw).act(&state, &mut OwnEvents::default());
    let east = recalling(&scenario, &state, 1, &east_saw).act(&state, &mut OwnEvents::default());
    assert!(
        trains(&west)
            .iter()
            .any(|(_, kind)| *kind == UnitKind::Piledriver),
        "premise: {west:?}"
    );
    assert_eq!(mirror(&state, west), east);
}

/// The arena with a built west Fabricator, Harvesters on every west node and
/// a west army of `sentinels` Sentinels, `wardens` Wardens and `lancers`
/// Lancers, the line short of two fifths of it.
fn lined(sentinels: i32, wardens: i32, lancers: i32) -> Scenario {
    let mut scenario = armed(&[], &[]);
    scenario.units.extend(workforce(0));
    let line = std::iter::repeat_n(UnitKind::Sentinel, sentinels.unsigned_abs() as usize).chain(
        std::iter::repeat_n(UnitKind::Warden, wardens.unsigned_abs() as usize),
    );
    scenario
        .units
        .extend(line.zip(3..).map(|(kind, x)| unit(0, kind, x, 10)));
    scenario
        .units
        .extend((3..3 + lancers).map(|x| unit(0, UnitKind::Lancer, x, 1)));
    scenario
}

/// What the west Foundry queues on the first decision after the Fabricator
/// starts a Lancer.
fn foundry_trains_beside_a_busy_fabricator(scenario: &Scenario) -> Vec<UnitKind> {
    beside_a_busy_fabricator(scenario, |_| seat(scenario, 0))
}

/// What the west Foundry queues on the first decision after the Fabricator
/// starts a Lancer, as the seat `opponent` builds for that state decides.
fn beside_a_busy_fabricator(
    scenario: &Scenario,
    opponent: impl FnOnce(&State) -> Opponent,
) -> Vec<UnitKind> {
    let mut state = scenario.build().unwrap();
    let at = fabricator(&state);
    state.tick(&[PlayerCommand {
        player: PlayerId(0),
        command: Command::Train {
            building: at,
            kind: UnitKind::Lancer,
        },
    }]);
    advance_to(
        &mut state,
        crate::decision_interval(BotDifficulty::Standard),
        &[],
    );
    let foundry = foundries(&state, PlayerId(0))[0];
    trains(&opponent(&state).act(&state, &mut OwnEvents::default()))
        .into_iter()
        .filter(|(building, _)| *building == foundry)
        .map(|(_, kind)| kind)
        .collect()
}

#[test]
fn the_line_goes_to_the_producer_of_its_best_unit_before_a_nearer_one() {
    // Scrap for a Warden beside the Airworks the seat saves for, and not for
    // a Sentinel as well.
    let mut scenario = lined(3, 0, 8);
    scenario.players[0].scrap = 500;
    let state = scenario.build().unwrap();
    assert_eq!(
        trains(&seat(&scenario, 0).act(&state, &mut OwnEvents::default())),
        [(fabricator(&state), UnitKind::Warden)],
        "the Foundry nearer home does not spend the scrap on a Sentinel first"
    );
}

#[test]
fn an_idle_foundry_waits_for_a_busy_producer_of_a_much_better_line_unit() {
    let mut lone = lined(3, 0, 8);
    lone.buildings
        .retain(|building| building.kind != BuildingKind::Fabricator);
    let state = lone.build().unwrap();
    let foundry = foundries(&state, PlayerId(0))[0];
    assert!(
        trains(&seat(&lone, 0).act(&state, &mut OwnEvents::default()))
            .contains(&(foundry, UnitKind::Sentinel)),
        "premise: with nothing better to train, the Foundry trains its line"
    );
    assert_eq!(
        foundry_trains_beside_a_busy_fabricator(&lined(3, 0, 8)),
        [],
        "a line of Sentinels waits for a Warden"
    );
    assert_eq!(
        foundry_trains_beside_a_busy_fabricator(&lined(1, 2, 12)),
        [UnitKind::Sentinel],
        "a line mostly of Wardens, where a Warden is only a little better, does not"
    );
}

#[test]
fn a_traced_decision_explains_each_army_unit_without_changing_it() {
    let scenario = foundry_of_heavies();
    let state = scenario.build().unwrap();
    let seen = [UnitKind::Kestrel, UnitKind::Sentinel, UnitKind::Sentinel];
    let (commands, trace) =
        remembering(&scenario, &state, &seen).act_traced(&state, &mut OwnEvents::default());
    assert_eq!(
        commands,
        remembering(&scenario, &state, &seen).act(&state, &mut OwnEvents::default()),
        "tracing changes no command"
    );
    let army = trace.unwrap().army;
    assert!(
        army.enemy.air > 0 && army.enemy.ground > 0,
        "{:?}",
        army.enemy
    );
    let army_trains: Vec<(BuildingId, UnitKind)> = trains(&commands)
        .into_iter()
        .filter(|(_, kind)| composition::role(*kind).is_some())
        .collect();
    assert!(!army_trains.is_empty(), "premise: the seat trains its army");
    assert_eq!(
        army.picks
            .iter()
            .map(|pick| (pick.building, pick.unit))
            .collect::<Vec<_>>(),
        army_trains,
        "every army unit is explained, in order"
    );
    for pick in &army.picks {
        let PickPath::Wanted { role, bar } = pick.path else {
            continue;
        };
        let chosen = pick
            .candidates
            .iter()
            .find(|candidate| candidate.unit == pick.unit)
            .unwrap();
        assert!(chosen.score >= bar, "{pick:?}");
        assert!(
            pick.candidates
                .iter()
                .all(|candidate| composition::role(candidate.unit) == Some(role)),
            "{pick:?}"
        );
        assert!(
            pick.candidates
                .windows(2)
                .all(|pair| pair[0].score >= pair[1].score),
            "best first: {pick:?}"
        );
    }
}

#[test]
fn only_an_armed_enemy_aircraft_gives_a_sentinel_air_to_cover() {
    // Long-range enemy guns outrange every line unit alike, so reach decides
    // nothing between the Sentinel and the Warden.
    let scenario = lined(2, 1, 8);
    let with = |aircraft: UnitKind| {
        let seen = [UnitKind::Bombard, UnitKind::Bombard, aircraft];
        beside_a_busy_fabricator(&scenario, |state| remembering(&scenario, state, &seen))
    };
    assert_eq!(
        with(UnitKind::Buzzard),
        [UnitKind::Sentinel],
        "a gun for both domains is worth a Sentinel now"
    );
    assert_eq!(
        with(UnitKind::Kestrel),
        [],
        "an unarmed scout leaves the line waiting for a Warden"
    );
}

#[test]
fn a_defense_short_of_wardens_saves_for_an_answer_rather_than_feed_scuttlers() {
    let wardens = [(UnitKind::Warden, 7, 4), (UnitKind::Warden, 7, 6)];
    let trained_at = |difficulty, scrap: u32| {
        let mut scenario = armed(&wardens, &[]);
        scenario.players[0].scrap = scrap;
        let state = scenario.build().unwrap();
        trains(&seat_with(&scenario, 0, rung(difficulty)).act(&state, &mut OwnEvents::default()))
            .into_iter()
            .map(|(_, kind)| kind)
            .collect::<Vec<_>>()
    };
    for difficulty in BotDifficulty::ALL {
        let poor = trained_at(difficulty, 60);
        assert!(
            !poor.contains(&UnitKind::Scuttler),
            "{difficulty:?}: a Scuttler only feeds the Wardens: {poor:?}"
        );
        let rich = trained_at(difficulty, 400);
        assert!(
            rich.iter()
                .any(|kind| matches!(kind, UnitKind::Lancer | UnitKind::Warden)),
            "{difficulty:?}: an answer that outlasts or outranges them: {rich:?}"
        );
    }
}

#[test]
fn a_short_defense_trains_its_answer_before_general_production_takes_the_producer() {
    // A rich bank: general production would queue an army unit at the only
    // Fabricator first and leave the defense no producer for its answer.
    let scenario = armed(&[(UnitKind::Warden, 7, 4), (UnitKind::Warden, 7, 6)], &[]);
    let state = scenario.build().unwrap();
    let (_, trace) = seat_with(&scenario, 0, rung(BotDifficulty::Prime))
        .act_traced(&state, &mut OwnEvents::default());
    let fabricator = fabricator(&state);
    let picks = trace.unwrap().army.picks;
    assert!(
        matches!(
            picks.iter().find(|pick| pick.building == fabricator),
            Some(pick) if matches!(pick.path, PickPath::Answer { .. })
        ),
        "{picks:?}"
    );
}

#[test]
fn a_flak_turret_the_emergency_passes_over_does_not_hold_back_the_answer() {
    // Buzzards raid the base past a standing Flak Turret: the emergency puts
    // no second one up on an approach already covered, so production trains
    // anti-air rather than wait for a structure that never comes.
    let mut scenario = armed(&[(UnitKind::Buzzard, 9, 5), (UnitKind::Buzzard, 9, 6)], &[]);
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::FlakTurret,
        x: 5,
        y: 2,
    });
    let state = scenario.build().unwrap();
    let (commands, trace) = seat_with(&scenario, 0, rung(BotDifficulty::Prime))
        .act_traced(&state, &mut OwnEvents::default());
    assert!(
        !commands.iter().any(|command| matches!(
            command.command,
            Command::Build {
                kind: BuildingKind::FlakTurret,
                ..
            }
        )),
        "premise: no second Flak Turret goes up: {commands:?}"
    );
    let picks = trace.unwrap().army.picks;
    assert!(
        picks
            .iter()
            .any(|pick| matches!(pick.path, PickPath::Answer { .. })
                && pick.unit == UnitKind::Flakhound),
        "{picks:?}"
    );
}

#[test]
fn an_enemy_builder_alone_short_is_met_with_the_cheapest_unit() {
    // Flakhounds already cover the Buzzard, so only the enemy Harvester among
    // the buildings leaves the defense short: a rich bank still sends the
    // cheapest unit, the first one there soonest.
    let mut scenario = armed(
        &[(UnitKind::Harvester, 8, 3), (UnitKind::Buzzard, 9, 5)],
        &[],
    );
    scenario.units.extend([
        unit(0, UnitKind::Flakhound, 5, 7),
        unit(0, UnitKind::Flakhound, 5, 8),
    ]);
    let state = scenario.build().unwrap();
    let (_, trace) = seat_with(&scenario, 0, rung(BotDifficulty::Prime))
        .act_traced(&state, &mut OwnEvents::default());
    let picks: Vec<UnitKind> = trace
        .unwrap()
        .army
        .picks
        .iter()
        .filter(|pick| pick.path == PickPath::Shortfall)
        .map(|pick| pick.unit)
        .collect();
    assert_eq!(picks, [UnitKind::Scuttler]);
}

#[test]
fn a_busy_producer_keeps_no_scrap_from_an_answer_trained_now() {
    // A Lancer raids the base and the Fabricator has Lancers queued past this
    // decision: the Lancer it would answer with comes too late to hold the
    // bank, so the Foundry trains what helps now.
    let mut scenario = armed(&[(UnitKind::Lancer, 8, 3)], &[]);
    scenario.players[0].scrap = 150;
    let state = scenario.build().unwrap();
    let fabricator = fabricator(&state);
    let mut value = serde_json::to_value(&state).unwrap();
    let entry = value["buildings"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["id"] == fabricator.0)
        .unwrap();
    entry["queue"] = serde_json::json!(["lancer", "lancer"]);
    let state: State = serde_json::from_value(value).unwrap();
    let commands =
        seat_with(&scenario, 0, rung(BotDifficulty::Prime)).act(&state, &mut OwnEvents::default());
    assert!(!trains(&commands).is_empty(), "{commands:?}");
}

#[test]
fn attackers_in_a_domain_already_covered_do_not_set_the_answer() {
    use crate::composition::Fighter;
    use crate::missions::Shortfall;
    let flakhound = Fighter::Unit(UnitKind::Flakhound);
    let both = |gap: [u64; 2]| Shortfall {
        ground: None,
        gap,
        attackers: vec![(UnitKind::Scuttler, 200), (UnitKind::Buzzard, 120)],
        read: true,
    };
    // With only the air short, the Scuttlers on the ground don't dilute what
    // a Flakhound is worth against the Buzzard.
    assert_eq!(
        crate::decision::answer(flakhound, &both([0, 120])),
        crate::counters::pair(UnitKind::Flakhound, UnitKind::Buzzard)
    );
    assert!(
        crate::decision::answer(flakhound, &both([200, 120]))
            < crate::decision::answer(flakhound, &both([0, 120]))
    );
}

#[test]
fn a_lone_enemy_builder_draws_the_cheapest_unit_before_general_production() {
    // An enemy Harvester beside West's Foundry and nothing armed: the defense
    // trains its cheapest unit at the Foundry before general production takes
    // it, and an armed raider is weighed for what it is instead.
    let trained_beside = |raider: bool, flak: bool| {
        let mut scenario = arena(130);
        scenario.units.push(harvester(1, 8, 3));
        if raider {
            scenario.units.push(unit(1, UnitKind::Scuttler, 8, 4));
        }
        if flak {
            scenario
                .buildings
                .push(building(1, BuildingKind::FlakTurret, 7, 2));
        }
        let state = scenario.build().unwrap();
        trains(
            &seat_with(&scenario, 0, rung(BotDifficulty::Prime))
                .act(&state, &mut OwnEvents::default()),
        )
        .into_iter()
        .map(|(_, kind)| kind)
        .collect::<Vec<_>>()
    };
    let trained = |raider: bool| trained_beside(raider, false);
    assert_eq!(trained(false).first(), Some(&UnitKind::Scuttler));
    assert!(
        !trained(true).contains(&UnitKind::Scuttler),
        "a Scuttler is no answer to a raider it only feeds"
    );
    assert_eq!(
        trained_beside(false, true).first(),
        Some(&UnitKind::Scuttler),
        "a Flak Turret cannot hurt what goes after the builder"
    );
}

#[test]
fn a_line_facing_lancers_waits_for_a_warden_rather_than_add_a_sentinel() {
    // Sentinels die to Lancers' reach where Wardens outlast them: once
    // Lancers are known, a line mostly of Wardens no longer tops up with a
    // Sentinel while the Fabricator is busy.
    let scenario = lined(1, 2, 12);
    assert_eq!(
        foundry_trains_beside_a_busy_fabricator(&scenario),
        [UnitKind::Sentinel],
        "premise: with no enemy known it does"
    );
    let lancers = (0..6_u32).map(|index| {
        let tile = TilePos::new(
            20 + i32::try_from(index % 2).unwrap(),
            1 + i32::try_from(index / 2).unwrap(),
        );
        (1_000 + index, UnitKind::Lancer, tile, 0)
    });
    let trained = beside_a_busy_fabricator(&scenario, |state| {
        restored_with(&seat(&scenario, 0), &scenario, state, |json| {
            json["memory"]["units"] = remembered(lancers);
        })
    });
    assert_eq!(trained, []);
}

#[test]
fn a_foundry_does_not_pad_a_full_army_with_sentinels_a_fabricator_outdoes() {
    // No role wants more, so idle producers spend on the line; the
    // Fabricator's Warden suits it far better than another Sentinel.
    let mut scenario = lined(10, 0, 10);
    scenario.players[0].scrap = 1_000;
    let state = scenario.build().unwrap();
    let trained = trains(&seat(&scenario, 0).act(&state, &mut OwnEvents::default()));
    let foundry = foundries(&state, PlayerId(0))[0];
    assert!(
        trained.contains(&(fabricator(&state), UnitKind::Warden)),
        "premise: the army is padded: {trained:?}"
    );
    assert!(
        !trained.contains(&(foundry, UnitKind::Sentinel)),
        "{trained:?}"
    );
}
