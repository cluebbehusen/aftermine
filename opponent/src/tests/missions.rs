use super::*;

/// The arena with west units and east raiders placed as given.
fn raided(defenders: &[(UnitKind, i32, i32)], raiders: &[(UnitKind, i32, i32)]) -> Scenario {
    let mut scenario = arena(0);
    for (kind, x, y) in defenders {
        scenario.units.push(unit(0, *kind, *x, *y));
    }
    for (kind, x, y) in raiders {
        scenario.units.push(unit(1, *kind, *x, *y));
    }
    scenario
}

#[test]
fn a_ground_threat_draws_the_nearest_sufficient_defenders() {
    let scenario = raided(
        &[
            (UnitKind::Sentinel, 5, 8),
            (UnitKind::Sentinel, 6, 8),
            (UnitKind::Sentinel, 10, 1),
            (UnitKind::Sentinel, 11, 1),
        ],
        &[(UnitKind::Sentinel, 9, 5)],
    );
    let state = scenario.build().unwrap();
    let (commands, trace) = seat(&scenario, 0).act_traced(&state, &mut OwnEvents::default());
    let mut near = vec![at(&state, 5, 8), at(&state, 6, 8)];
    near.sort_unstable();
    assert_eq!(hunts(&commands), [(near, TilePos::new(9, 5))]);
    let missions = trace.unwrap().missions;
    assert!(
        matches!(
            missions.as_slice(),
            [MissionStatus {
                kind: MissionKind::Defend { .. },
                phase: Phase::Engage,
                units: 2,
                ..
            }]
        ),
        "{missions:?}"
    );
}

#[test]
fn a_defense_takes_an_attack_gathering_at_home_last() {
    let scenario = raided(
        &[
            (UnitKind::Sentinel, 5, 8),
            (UnitKind::Sentinel, 6, 8),
            (UnitKind::Sentinel, 10, 1),
            (UnitKind::Sentinel, 11, 1),
        ],
        &[(UnitKind::Sentinel, 9, 5)],
    );
    let state = scenario.build().unwrap();
    let mut gathering = vec![at(&state, 5, 8), at(&state, 6, 8)];
    gathering.sort_unstable();
    let mut opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
        json["missions"] = serde_json::json!({
            "next": 1,
            "list": [{
                "id": 0,
                "task": {
                    "task": "attack",
                    "target": {"owner": 1, "building": "foundry", "anchor": {"x": 19, "y": 5}},
                    "phase": "gather",
                },
                "since": 0,
                "units": gathering,
                "goal": {"x": 7, "y": 8},
            }],
        });
    });
    let (commands, _) = opponent.act_traced(&state, &mut OwnEvents::default());
    let mut far = vec![at(&state, 10, 1), at(&state, 11, 1)];
    far.sort_unstable();
    assert_eq!(hunts(&commands), [(far, TilePos::new(9, 5))]);
}

#[test]
fn a_sapper_walking_at_the_foundry_draws_defenders() {
    let scenario = raided(
        &[(UnitKind::Sentinel, 5, 8), (UnitKind::Sentinel, 10, 1)],
        &[(UnitKind::Sapper, 9, 5)],
    );
    let state = scenario.build().unwrap();
    let (commands, trace) = seat(&scenario, 0).act_traced(&state, &mut OwnEvents::default());
    let [(units, goal)] = &hunts(&commands)[..] else {
        panic!("{commands:?}");
    };
    assert_eq!(*goal, TilePos::new(9, 5));
    assert!(units.contains(&at(&state, 5, 8)), "{units:?}");
    let missions = trace.unwrap().missions;
    assert!(
        matches!(
            missions.as_slice(),
            [MissionStatus {
                kind: MissionKind::Defend { .. },
                ..
            }]
        ),
        "{missions:?}"
    );
}

#[test]
fn an_air_raid_draws_only_units_that_hit_air_to_guard_the_foundry() {
    let scenario = raided(
        &[
            (UnitKind::Lancer, 5, 8),
            (UnitKind::Lancer, 6, 8),
            (UnitKind::Sentinel, 10, 1),
        ],
        &[(UnitKind::Buzzard, 9, 5)],
    );
    let state = scenario.build().unwrap();
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    // They move there without a hunt, which would chase the flyer out of
    // the base's cover.
    assert!(hunts(&commands).is_empty(), "{commands:?}");
    let [(units, goal)] = &advances(&commands)[..] else {
        panic!("{commands:?}");
    };
    assert_eq!(units, &[at(&state, 10, 1)]);
    assert_eq!(
        crate::frame::gap(TilePos::new(3, 5), (2, 2), *goal, (1, 1)),
        0,
        "defenders wait beside the Foundry rather than under the flyer"
    );
}

#[test]
fn a_mixed_raid_draws_defenders_for_each_domain_it_comes_from() {
    let scenario = raided(
        &[
            (UnitKind::Lancer, 5, 8),
            (UnitKind::Lancer, 6, 8),
            (UnitKind::Lancer, 5, 9),
            (UnitKind::Sentinel, 10, 1),
        ],
        &[(UnitKind::Sentinel, 9, 5), (UnitKind::Buzzard, 9, 6)],
    );
    let state = scenario.build().unwrap();
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let [(units, _)] = &hunts(&commands)[..] else {
        panic!("{commands:?}");
    };
    assert!(
        units.contains(&at(&state, 10, 1)),
        "the only unit that hits air joins, however far"
    );
    let lancers = units
        .iter()
        .filter(|id| {
            state
                .units()
                .iter()
                .any(|unit| unit.id == **id && unit.kind == UnitKind::Lancer)
        })
        .count();
    assert_eq!(lancers, 2, "and only the Lancers the ground raid needs");
}

#[test]
fn a_recovering_defense_lends_its_units_to_another_foundry() {
    let mut scenario = raided(
        &[(UnitKind::Sentinel, 5, 8), (UnitKind::Sentinel, 6, 8)],
        &[(UnitKind::Scuttler, 13, 7)],
    );
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 11,
        y: 9,
    });
    let mut state = scenario.build().unwrap();
    advance_to(&mut state, 12, &[]);
    let home = foundries(&state, PlayerId(0))[0];
    let mut defenders = vec![at(&state, 5, 8), at(&state, 6, 8)];
    defenders.sort_unstable();
    let mut opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
        json["missions"] = serde_json::json!({
            "next": 1,
            "list": [{
                "id": 0,
                "task": {"task": "defend", "asset": home.0, "phase": "recover"},
                "since": 12,
                "units": defenders,
                "goal": {"x": 9, "y": 5},
            }],
        });
    });
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    // The lent defender, far from the other Foundry, runs to its post.
    let [(units, _)] = &runs(&commands)[..] else {
        panic!("{commands:?}");
    };
    assert_eq!(units.len(), 1);
    assert!(defenders.contains(&units[0]));
    let missions = trace.unwrap().missions;
    assert!(
        matches!(
            missions.as_slice(),
            [
                MissionStatus {
                    id: 0,
                    phase: Phase::Recover,
                    units: 1,
                    ..
                },
                MissionStatus {
                    id: 1,
                    phase: Phase::Engage,
                    units: 1,
                    ..
                },
            ]
        ),
        "{missions:?}"
    );
}

#[test]
fn defenders_are_released_after_the_threat_ends_and_return_with_it() {
    let scenario = raided(
        &[(UnitKind::Sentinel, 5, 8), (UnitKind::Sentinel, 6, 8)],
        &[(UnitKind::Scuttler, 9, 5)],
    );
    let mut state = scenario.build().unwrap();
    let raider = at(&state, 9, 5);
    let mut opponent = seat(&scenario, 0);
    let script = [
        (1, run(1, vec![raider], 21, 1)),
        (360, run(1, vec![raider], 9, 5)),
    ];
    let traces = play(&mut opponent, &mut state, 600, &script);

    let phases: Vec<(u64, Phase)> = traces
        .iter()
        .flat_map(|trace| {
            trace
                .missions
                .iter()
                .map(|mission| (mission.id, mission.phase))
        })
        .fold(Vec::new(), |mut seen, step| {
            if seen.last() != Some(&step) {
                seen.push(step);
            }
            seen
        });
    assert!(
        phases.starts_with(&[(0, Phase::Engage), (0, Phase::Recover), (1, Phase::Engage)]),
        "defense, recovery, release, then a new defense: {phases:?}"
    );
    let released = traces
        .iter()
        .position(|trace| trace.missions.is_empty() && trace.tick > 0)
        .expect("the first defense let its units go");
    assert!(
        traces[released..]
            .iter()
            .any(|trace| !trace.missions.is_empty())
    );
}

#[test]
fn a_threat_no_ground_reaches_is_left_to_production() {
    let mut scenario = raided(
        &[(UnitKind::Sentinel, 5, 8), (UnitKind::Sentinel, 6, 8)],
        &[(UnitKind::Sentinel, 10, 3)],
    );
    for y in 1..=5 {
        scenario.map[y].replace_range(8..13, if y == 3 { "##.##" } else { "#####" });
    }
    let state = scenario.build().unwrap();
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    assert!(
        observation
            .enemy_units
            .iter()
            .any(|enemy| enemy.tile == TilePos::new(10, 3)),
        "premise: the walled-in enemy is seen"
    );
    let (commands, trace) = seat(&scenario, 0).act_traced(&state, &mut OwnEvents::default());
    assert!(hunts(&commands).is_empty(), "{commands:?}");
    assert!(trace.unwrap().missions.is_empty());
}

#[test]
fn a_moving_threat_is_chased_again_only_once_it_moves_away() {
    let staged = |x: i32, y: i32, tick: u64| {
        let scenario = raided(
            &[(UnitKind::Sentinel, 5, 8), (UnitKind::Sentinel, 6, 8)],
            &[(UnitKind::Scuttler, x, y)],
        );
        let mut state = scenario.build().unwrap();
        if tick > 0 {
            advance_to(&mut state, tick, &[]);
        }
        (scenario, state)
    };
    let (scenario, state) = staged(9, 4, 0);
    let mut opponent = seat(&scenario, 0);
    let goals = |opponent: &mut Opponent, state: &State| {
        hunts(&opponent.act(state, &mut OwnEvents::default()))
            .into_iter()
            .map(|(_, goal)| goal)
            .collect::<Vec<_>>()
    };
    assert_eq!(goals(&mut opponent, &state), [TilePos::new(9, 4)]);
    let (_, nudged) = staged(10, 3, 12);
    assert!(goals(&mut opponent, &nudged).is_empty());
    let (_, moved) = staged(12, 9, 24);
    assert_eq!(goals(&mut opponent, &moved), [TilePos::new(12, 9)]);
}

#[test]
fn defense_outranks_workers_under_a_small_allowance() {
    let mut scenario = raided(
        &[(UnitKind::Sentinel, 5, 9), (UnitKind::Sentinel, 6, 9)],
        &[(UnitKind::Scuttler, 9, 6)],
    );
    for row in [2, 8] {
        scenario.map[row].replace_range(2..3, "s");
    }
    scenario.units.extend((4..=5).map(|x| harvester(0, x, 7)));
    let state = scenario.build().unwrap();
    let scrapheap = rung(BotDifficulty::Scrapheap);
    let (commands, trace) =
        seat_with(&scenario, 0, scrapheap).act_traced(&state, &mut OwnEvents::default());
    let trace = trace.unwrap();
    assert_eq!(hunts(&commands).len(), 1);
    assert_eq!(
        u32::try_from(harvests(&commands).len()).unwrap(),
        trace.allowance - 1,
        "workers take what is left"
    );
    assert_eq!(trace.unit_orders, trace.allowance);
}

#[test]
fn a_short_defense_nothing_pays_back_against_keeps_its_savings() {
    // Lancers outrange a Turret and outfight what the seat could train: no
    // answer pays back against them, so the scrap kept for the saving target
    // stays kept rather than feed the attackers. A seat that judges the threat
    // by its size answers with its cheapest unit, as before.
    let threatened = |lancers: bool| {
        let mut scenario = arena(100);
        scenario.units.extend(workforce(0));
        scenario.buildings.push(BuildingSpec {
            player: 0,
            kind: BuildingKind::Turret,
            x: 13,
            y: 9,
        });
        if lancers {
            scenario.units.extend([
                unit(1, UnitKind::Lancer, 9, 5),
                unit(1, UnitKind::Lancer, 9, 6),
            ]);
        }
        scenario
    };
    let calm = threatened(false);
    let (_, trace) = seat_with(&calm, 0, reading_thrifty())
        .act_traced(&calm.build().unwrap(), &mut OwnEvents::default());
    assert!(trace.unwrap().protected > 0, "premise: scrap is protected");

    let scenario = threatened(true);
    let state = scenario.build().unwrap();
    let (commands, trace) =
        seat_with(&scenario, 0, reading_thrifty()).act_traced(&state, &mut OwnEvents::default());
    // Armed enemies near the base lower the share protected, but what is
    // protected stays so: nothing is trained from it.
    assert!(trace.unwrap().protected > 0, "{commands:?}");
    assert!(trains(&commands).is_empty(), "{commands:?}");
    assert!(
        commands.iter().all(|command| !matches!(
            command.command,
            Command::Build {
                kind: BuildingKind::Turret,
                ..
            }
        )),
        "{commands:?}"
    );
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    assert!(!trains(&commands).is_empty(), "{commands:?}");
}

#[test]
fn a_short_defense_defers_the_saving_target_and_spends_protected_scrap() {
    let threatened = |scrap: u32| {
        let mut scenario = arena(scrap);
        scenario.units.extend(workforce(0));
        // A Turret out on the field: the seat has one, but it guards nothing
        // near home.
        scenario.buildings.push(BuildingSpec {
            player: 0,
            kind: BuildingKind::Turret,
            x: 13,
            y: 9,
        });
        scenario.units.push(unit(1, UnitKind::Sentinel, 9, 5));
        scenario
    };
    let scenario = threatened(400);
    let state = scenario.build().unwrap();
    let commands =
        seat_with(&scenario, 0, reading_thrifty()).act(&state, &mut OwnEvents::default());
    assert!(
        commands.iter().all(|command| !matches!(
            command.command,
            Command::Build { kind, .. } if kind != BuildingKind::Turret
        )),
        "the saving target waits; only an emergency Turret goes up: {commands:?}"
    );

    let scenario = threatened(100);
    let state = scenario.build().unwrap();
    let mut calm = scenario.clone();
    calm.units.pop();
    let (quiet, trace) = seat_with(&calm, 0, reading_thrifty())
        .act_traced(&calm.build().unwrap(), &mut OwnEvents::default());
    assert!(trace.unwrap().protected > 0, "premise: scrap is protected");
    assert!(trains(&quiet).is_empty(), "premise");
    let turret = |commands: &[PlayerCommand]| {
        commands.iter().any(|command| {
            matches!(
                command.command,
                Command::Build {
                    kind: BuildingKind::Turret,
                    ..
                }
            )
        })
    };
    let commands =
        seat_with(&scenario, 0, reading_thrifty()).act(&state, &mut OwnEvents::default());
    assert!(turret(&commands), "an emergency Turret first: {commands:?}");

    let scenario = threatened(200);
    let state = scenario.build().unwrap();
    let commands =
        seat_with(&scenario, 0, reading_thrifty()).act(&state, &mut OwnEvents::default());
    let foundry = foundries(&state, PlayerId(0))[0];
    assert!(turret(&commands), "{commands:?}");
    assert_eq!(trains(&commands), [(foundry, UnitKind::Sentinel)]);
}

#[test]
fn a_short_seat_too_poor_for_a_line_unit_or_a_gun_trains_what_it_can_afford() {
    let mut scenario = arena(50);
    scenario.units.extend(workforce(0));
    scenario.units.push(unit(1, UnitKind::Sentinel, 9, 5));
    let state = scenario.build().unwrap();
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    let foundry = foundries(&state, PlayerId(0))[0];
    assert_eq!(
        trains(&commands),
        [(foundry, UnitKind::Scuttler)],
        "{commands:?}"
    );
}

#[test]
fn a_raided_expansion_on_other_ground_is_armed_from_its_own_foundry() {
    // A rock wall down column 24 cuts West's home off from its expansion,
    // which stands on East's ground with a raider beside it.
    let mut scenario = field();
    for row in scenario.map.iter_mut().skip(1).take(22) {
        row.replace_range(24..25, "#");
    }
    scenario.players[0].scrap = 50;
    scenario.units.extend(workforce(0));
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 30,
        y: 11,
    });
    scenario.units.push(unit(1, UnitKind::Sentinel, 33, 11));
    let state = scenario.build().unwrap();
    let expansion = state
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0) && building.anchor == TilePos::new(30, 11))
        .unwrap()
        .id;
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    assert_eq!(
        trains(&commands),
        [(expansion, UnitKind::Scuttler)],
        "a Scuttler from home could never reach it: {commands:?}"
    );
}

#[test]
fn mirrored_seats_defend_alike() {
    let mut scenario = arena(0);
    scenario.units.extend([
        unit(0, UnitKind::Sentinel, 5, 8),
        unit(0, UnitKind::Sentinel, 6, 8),
        unit(1, UnitKind::Sentinel, 18, 3),
        unit(1, UnitKind::Sentinel, 17, 3),
        unit(1, UnitKind::Scuttler, 9, 5),
        unit(0, UnitKind::Scuttler, 14, 6),
    ]);
    let state = scenario.build().unwrap();
    let west = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let east = seat(&scenario, 1).act(&state, &mut OwnEvents::default());
    assert_eq!(hunts(&west).len(), 1);
    assert_eq!(mirror(&state, west), east);
}

#[test]
fn a_mid_defense_checkpoint_resumes_identically() {
    let scenario = raided(
        &[(UnitKind::Sentinel, 5, 8), (UnitKind::Sentinel, 6, 8)],
        &[(UnitKind::Scuttler, 9, 5)],
    );
    let mut state = scenario.build().unwrap();
    let raider = at(&state, 9, 5);
    let mut opponent = seat(&scenario, 0);
    play(
        &mut opponent,
        &mut state,
        60,
        &[(1, run(1, vec![raider], 21, 1))],
    );
    assert!(!opponent.missions().is_empty(), "premise: mid-defense");

    let ticks = 300 - state.current_tick();
    resumes_identically(&mut opponent, &scenario, &mut state, ticks);
}

#[test]
fn checkpoints_reject_impossible_missions() {
    let scenario = raided(
        &[(UnitKind::Sentinel, 5, 8), (UnitKind::Sentinel, 6, 8)],
        &[(UnitKind::Sentinel, 9, 5)],
    );
    let state = scenario.build().unwrap();
    let mut opponent = seat(&scenario, 0);
    opponent.act(&state, &mut OwnEvents::default());
    let json = serde_json::to_value(opponent.checkpoint()).unwrap();
    assert_eq!(json["missions"]["list"].as_array().unwrap().len(), 1);
    let restore = |json: &serde_json::Value| {
        let checkpoint: Checkpoint = serde_json::from_value(json.clone()).unwrap();
        Opponent::restore(&checkpoint, &scenario, &state, map(&scenario))
    };
    assert!(restore(&json).is_ok());

    let mission = json["missions"]["list"][0].clone();
    let with = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut json = json.clone();
        edit(&mut json["missions"]);
        restore(&json).err().unwrap()
    };
    assert_eq!(
        with(&|missions| missions["list"][0]["since"] = 99.into()),
        "checkpoint mission could not have been recorded"
    );
    assert_eq!(
        with(&|missions| missions["list"][0]["goal"] = serde_json::json!({"x": 99, "y": 0})),
        "checkpoint mission could not have been recorded"
    );
    assert_eq!(
        with(&|missions| missions["next"] = 0.into()),
        "checkpoint mission ids are out of order"
    );
    assert_eq!(
        with(&|missions| missions["next"] = u64::MAX.into()),
        "checkpoint mission ids are out of order",
        "no id is left for the next mission"
    );
    assert_eq!(
        with(&|missions| {
            let units = missions["list"][0]["units"].as_array_mut().unwrap();
            units.reverse();
        }),
        "checkpoint mission units are malformed"
    );
    assert_eq!(
        with(&|missions| missions["list"][0]["units"] = serde_json::json!([])),
        "checkpoint mission units are malformed"
    );
    assert_eq!(
        with(&|missions| {
            let mut copy = mission.clone();
            copy["id"] = 1.into();
            missions["list"].as_array_mut().unwrap().push(copy);
            missions["next"] = 2.into();
        }),
        "checkpoint missions share a unit"
    );
    assert_eq!(
        with(&|missions| {
            let over = crate::missions::MISSION_CAP + 1;
            let list: Vec<serde_json::Value> = (0..over)
                .map(|id| {
                    let mut copy = mission.clone();
                    copy["id"] = id.into();
                    copy
                })
                .collect();
            missions["list"] = list.into();
            missions["next"] = over.into();
        }),
        "checkpoint holds too many missions"
    );
}

#[test]
fn a_mission_as_large_as_a_late_army_restores() {
    let units: Vec<u32> = (1..=600).collect();
    let missions: crate::missions::Missions = serde_json::from_value(serde_json::json!({
        "next": 1,
        "list": [{
            "id": 0,
            "task": {
                "task": "attack",
                "target": {"owner": 1, "building": "foundry", "anchor": {"x": 19, "y": 5}},
                "phase": "travel",
            },
            "since": 0,
            "units": units,
            "goal": {"x": 10, "y": 5},
        }],
        "waiting": null,
    }))
    .unwrap();
    assert_eq!(missions.validate(100, 24, 12, 0), Ok(()));
}

#[test]
fn an_exhausted_mission_counter_is_rejected_at_any_tick() {
    let missions: crate::missions::Missions = serde_json::from_value(serde_json::json!({
        "next": u64::MAX,
        "list": [],
        "waiting": null,
    }))
    .unwrap();
    assert_eq!(
        missions.validate(u64::MAX / 2, 48, 24, 1).err().unwrap(),
        "checkpoint mission ids are out of order"
    );
}

/// Where a seat's picket stands east of its Foundry, where the enemy Bombard
/// shelling it stands, beyond the seat's sight but in range, and where the
/// enemy Kestrel spotting for it hovers, clear of the seat's guns; West's
/// spots, turned half about the field for East.
const PICKET: (i32, i32) = (8, 12);
const GUN: (i32, i32) = (17, 12);
const SPOTTER: (i32, i32) = (12, 5);

fn turned(player: u8, (x, y): (i32, i32)) -> (i32, i32) {
    if player == 0 {
        (x, y)
    } else {
        (47 - x, 23 - y)
    }
}

/// Orders each enemy Bombard of `shelled` at that seat's picket.
fn open_fire(state: &mut State, shelled: &[u8]) {
    let orders: Vec<PlayerCommand> = shelled
        .iter()
        .map(|&player| {
            let at = |spot| {
                let (x, y) = turned(player, spot);
                at(state, x, y)
            };
            PlayerCommand {
                player: PlayerId(1 - player),
                command: Command::Attack {
                    units: vec![at(GUN)],
                    target: AttackTarget::Unit(at(PICKET)),
                    queue: false,
                },
            }
        })
        .collect();
    state.tick(&orders);
}

/// Plays `state` to a decision tick on which every seat of `shelled` sees a
/// shell coming.
fn until_shells_come(scenario: &Scenario, state: &mut State, shelled: &[u8]) {
    let seat = seat(scenario, 0);
    let coming = |state: &State, player: u8| {
        !ObservationData::fog_honest(state, PlayerId(player))
            .incoming_shells
            .is_empty()
    };
    while !seat.decision_due(state) || !shelled.iter().all(|player| coming(state, *player)) {
        assert!(state.current_tick() < 600, "premise: the Bombards fire");
        state.tick(&[]);
    }
}

/// The field with each of `shelled` holding its garrison and picket, and the
/// other seat a Bombard and a Kestrel for that picket.
fn shelling(shelled: &[u8]) -> Scenario {
    let mut scenario = field();
    for &player in shelled {
        for spot in GARRISON.into_iter().chain([PICKET]) {
            let (x, y) = turned(player, spot);
            scenario.units.push(unit(player, UnitKind::Sentinel, x, y));
        }
    }
    for &player in shelled {
        for (kind, spot) in [(UnitKind::Bombard, GUN), (UnitKind::Kestrel, SPOTTER)] {
            let (x, y) = turned(player, spot);
            scenario.units.push(unit(1 - player, kind, x, y));
        }
    }
    scenario
}

/// `shelling(shelled)` played until the shells come.
fn shelled(shelled: &[u8]) -> (Scenario, State) {
    let scenario = shelling(shelled);
    let mut state = scenario.build().unwrap();
    open_fire(&mut state, shelled);
    until_shells_come(&scenario, &mut state, shelled);
    (scenario, state)
}

#[test]
fn shells_from_a_gun_out_of_sight_advance_fighters_on_it() {
    let (scenario, state) = shelled(&[0]);
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    assert!(
        observation
            .enemy_units
            .iter()
            .all(|enemy| enemy.kind != UnitKind::Bombard),
        "premise: the gun is out of sight"
    );
    let (commands, trace) = seat(&scenario, 0).act_traced(&state, &mut OwnEvents::default());
    // Advancing, they pass the spotter overhead rather than chase it.
    let [(units, goal)] = &advances(&commands)[..] else {
        panic!("{commands:?}");
    };
    assert!(!units.is_empty());
    let (picket, gun) = (TilePos::new(PICKET.0, PICKET.1), TilePos::new(GUN.0, GUN.1));
    assert!(
        goal.x > picket.x && goal.chebyshev(picket) >= goal.chebyshev(gun),
        "toward the gun, not the shelled picket: {goal:?}"
    );
    let reach = UnitKind::ALL
        .iter()
        .flat_map(|kind| kind.stats().weapons.iter())
        .filter(|weapon| weapon.indirect)
        .map(|weapon| weapon.range.ceil().to_num::<i32>())
        .max()
        .unwrap();
    assert!(
        goal.chebyshev(picket) <= reach,
        "within artillery reach: {goal:?}"
    );
    assert!(
        trace
            .unwrap()
            .missions
            .iter()
            .any(|mission| matches!(mission.kind, MissionKind::Defend { .. })
                && mission.phase == Phase::Engage),
        "a defense answers it"
    );
}

/// The field with West's garrison and picket, an East Bastion shelling the
/// picket with a Kestrel spotting for it, East Turrets around the Bastion
/// when `guarded`, and a West Kestrel that has seen them all; played until
/// the shells come.
fn bastion(guarded: bool) -> (Scenario, State) {
    bastion_with(guarded, &GARRISON)
}

/// `bastion` with West's garrison on `garrison` instead.
fn bastion_with(guarded: bool, garrison: &[(i32, i32)]) -> (Scenario, State) {
    let mut scenario = field();
    for spot in garrison.iter().copied().chain([PICKET]) {
        scenario
            .units
            .push(unit(0, UnitKind::Sentinel, spot.0, spot.1));
    }
    scenario
        .units
        .push(unit(1, UnitKind::Kestrel, SPOTTER.0, SPOTTER.1));
    scenario.units.push(unit(0, UnitKind::Kestrel, 15, 17));
    scenario.buildings.push(BuildingSpec {
        player: 1,
        kind: BuildingKind::Bastion,
        x: 16,
        y: 11,
    });
    if guarded {
        for (x, y) in [(19, 10), (19, 13), (19, 11)] {
            scenario.buildings.push(BuildingSpec {
                player: 1,
                kind: BuildingKind::Turret,
                x,
                y,
            });
        }
    }
    let mut state = scenario.build().unwrap();
    until_shells_come(&scenario, &mut state, &[0]);
    (scenario, state)
}

/// Whether `commands` send units beside the staged Bastion.
fn sent_beside_the_bastion(commands: &[PlayerCommand]) -> bool {
    let bastion = TilePos::new(16, 11);
    hunts(commands)
        .iter()
        .any(|(_, goal)| crate::frame::gap(bastion, (2, 2), *goal, (1, 1)) <= 1)
}

#[test]
fn a_lone_bastion_shelling_the_base_is_answered_beside_it() {
    let (scenario, state) = bastion(false);
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    assert!(sent_beside_the_bastion(&commands), "{commands:?}");
}

#[test]
fn a_bastion_among_guns_the_garrison_cannot_beat_is_left_to_production() {
    let (scenario, state) = bastion(true);
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    assert!(!sent_beside_the_bastion(&commands), "{commands:?}");
}

#[test]
fn a_bastion_is_left_alone_while_the_army_that_could_beat_it_is_away_fighting() {
    let (scenario, state) = bastion(false);
    let garrison: Vec<UnitId> = GARRISON.iter().map(|(x, y)| at(&state, *x, *y)).collect();
    let mut opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
        json["missions"] = serde_json::json!({
            "next": 1,
            "list": [{
                "id": 0,
                "task": {
                    "task": "attack",
                    "target": {"owner": 1, "building": "foundry", "anchor": {"x": 43, "y": 11}},
                    "phase": {"engage": {"focus": null, "progressed": state.current_tick()}},
                },
                "since": state.current_tick(),
                "units": garrison,
                "goal": {"x": 11, "y": 11},
            }],
        });
    });
    let commands = opponent.act(&state, &mut OwnEvents::default());
    assert!(!sent_beside_the_bastion(&commands), "{commands:?}");
}

#[test]
fn a_recovering_defense_counts_its_units_once_against_a_bastion() {
    // Two Sentinels and the picket, 270 scrap, fall short of the lone
    // Bastion's 315 unless counted twice.
    let garrison = [(2, 11), (2, 12)];
    let (scenario, state) = bastion_with(false, &garrison);
    let home = foundries(&state, PlayerId(0))[0];
    let units: Vec<UnitId> = garrison
        .iter()
        .chain([&PICKET])
        .map(|(x, y)| at(&state, *x, *y))
        .collect();
    let mut opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
        json["missions"] = serde_json::json!({
            "next": 1,
            "list": [{
                "id": 0,
                "task": {"task": "defend", "asset": home.0, "phase": "recover"},
                "since": state.current_tick(),
                "units": units,
                "goal": {"x": 6, "y": 11},
            }],
        });
    });
    let commands = opponent.act(&state, &mut OwnEvents::default());
    assert!(!sent_beside_the_bastion(&commands), "{commands:?}");
}

#[test]
fn a_site_in_sight_is_no_danger_but_a_remembered_one_may_be() {
    let mut scenario = field();
    scenario.units.push(unit(0, UnitKind::Kestrel, 20, 11));
    scenario.buildings.push(BuildingSpec {
        player: 1,
        kind: BuildingKind::Turret,
        x: 22,
        y: 11,
    });
    let state = scenario.build().unwrap();
    let mut observation = ObservationData::fog_honest(&state, PlayerId(0));
    let memory = crate::memory::Memory::default();
    let model = map(&scenario);
    let near = TilePos::new(22, 13);
    let covered = |observation: &ObservationData| {
        crate::missions::hazards(
            observation,
            &model,
            &memory,
            aftermine_sim::stats::Domain::Ground,
        )
        .iter()
        .any(|hazard| hazard.covers(crate::frame::doubled(near)))
    };
    assert!(covered(&observation), "premise: a built Turret covers it");
    observation.enemy_buildings[0].built = false;
    assert!(!covered(&observation), "a site in sight cannot fire");
    observation.enemy_buildings[0].seen = false;
    assert!(
        covered(&observation),
        "a remembered site may have been finished"
    );
}

#[test]
fn an_upgraded_building_is_worth_every_tier_it_paid_for() {
    let mut scenario = field();
    scenario.units.push(unit(0, UnitKind::Kestrel, 20, 11));
    scenario.buildings.push(BuildingSpec {
        player: 1,
        kind: BuildingKind::Turret,
        x: 22,
        y: 11,
    });
    let state = scenario.build().unwrap();
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let mut turret = observation.enemy_buildings[0].clone();
    assert_eq!(crate::missions::building_value(&turret), 100);
    turret.tier = 2;
    turret.hp = 900;
    let paid = [100, 150, 300].iter().sum::<u64>();
    assert_eq!(crate::missions::building_value(&turret), paid);
    turret.hp = 450;
    assert_eq!(crate::missions::building_value(&turret), paid / 2);
}

#[test]
fn a_bulwark_out_of_sight_is_valued_and_feared_as_a_bulwark() {
    let mut scenario = field();
    scenario.units.push(unit(0, UnitKind::Kestrel, 20, 11));
    scenario.buildings.push(BuildingSpec {
        player: 1,
        kind: BuildingKind::Turret,
        x: 22,
        y: 11,
    });
    let state = scenario.build().unwrap();
    let mut value = serde_json::to_value(&state).unwrap();
    for building in value["buildings"].as_array_mut().unwrap() {
        if building["kind"] == "turret" {
            building["tier"] = 2.into();
            building["hp"] = BuildingKind::Turret.tier_stats(2).max_hp.into();
        }
    }
    let mut state: State = serde_json::from_value(value).unwrap();
    let scout = state.units()[0].id;
    state.tick(&[PlayerCommand {
        player: PlayerId(0),
        command: Command::Run {
            units: vec![scout],
            goal: TilePos::new(4, 4),
            queue: false,
        },
    }]);
    let turret = TilePos::new(22, 11);
    while state.can_see(PlayerId(0), turret) {
        state.tick(&[]);
    }
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let bulwark = &observation.enemy_buildings[0];
    assert!(!bulwark.seen, "premise: only memory remains");
    assert_eq!(
        crate::missions::building_value(bulwark),
        [100, 150, 300].iter().sum::<u64>(),
        "worth every tier it was seen to have"
    );
    // Nine tiles out: clear of a Turret's hazard, inside a Bulwark's.
    let beyond = crate::frame::doubled(TilePos::new(22, 20));
    let memory = crate::memory::Memory::default();
    let model = map(&scenario);
    assert!(
        crate::missions::hazards(
            &observation,
            &model,
            &memory,
            aftermine_sim::stats::Domain::Ground,
        )
        .iter()
        .any(|hazard| hazard.covers(beyond)),
        "it reaches as far as a Bulwark"
    );
}

#[test]
fn a_gun_in_sight_shelling_the_base_is_attacked() {
    for config in [config(), rung(BotDifficulty::Prime)] {
        let mut scenario = shelling(&[0]);
        // A West Scuttler beside the gun reveals it. The gun stands beyond the
        // reach that makes a unit a threat to buildings, but its shells land in
        // the base.
        scenario
            .units
            .push(unit(0, UnitKind::Scuttler, GUN.0 + 4, GUN.1));
        let mut state = scenario.build().unwrap();
        open_fire(&mut state, &[0]);
        until_shells_come(&scenario, &mut state, &[0]);
        let gun = at(&state, GUN.0, GUN.1);
        let observation = ObservationData::fog_honest(&state, PlayerId(0));
        assert!(
            observation.enemy_units.iter().any(|enemy| enemy.id == gun),
            "premise: the gun is in sight"
        );
        let commands = seat_with(&scenario, 0, config).act(&state, &mut OwnEvents::default());
        let sent: Vec<UnitId> = attacks(&commands)
            .into_iter()
            .filter(|(_, target)| *target == AttackTarget::Unit(gun))
            .flat_map(|(units, _)| units)
            .collect();
        assert!(!sent.is_empty(), "{commands:?}");
        // Focus fire at Prime keeps them on it rather than hunting.
        assert!(
            hunts(&commands)
                .iter()
                .all(|(units, _)| units.iter().all(|unit| !sent.contains(unit))),
            "{commands:?}"
        );
    }
}

#[test]
fn defenders_short_of_artillery_shelling_the_base_still_go_at_it() {
    // Guns shell from where they stand, so defenders outweighed by them do
    // not wait at home for them as for attackers that walk in: the picket
    // alone, without its garrison, still goes at the gun.
    let mut scenario = shelling(&[0]);
    scenario
        .units
        .retain(|spec| spec.player != 0 || !GARRISON.contains(&turned(0, (spec.x, spec.y))));
    scenario
        .units
        .push(unit(0, UnitKind::Scuttler, GUN.0 + 4, GUN.1));
    let mut state = scenario.build().unwrap();
    open_fire(&mut state, &[0]);
    until_shells_come(&scenario, &mut state, &[0]);
    let gun = at(&state, GUN.0, GUN.1);
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    assert!(
        attacks(&commands)
            .iter()
            .any(|(_, target)| *target == AttackTarget::Unit(gun)),
        "{commands:?}"
    );
}

/// Lancers standing beside the gun shelling each of `shelled`, and a Surveyor
/// of that seat watching the gun and them from beyond their reach.
fn escort(scenario: &mut Scenario, shelled: &[u8]) {
    for &player in shelled {
        for dy in -1..=1 {
            let (x, y) = turned(player, (GUN.0 + 2, GUN.1 + dy));
            scenario
                .units
                .push(unit(1 - player, UnitKind::Lancer, x, y));
        }
    }
    for &player in shelled {
        let (x, y) = turned(player, (GUN.0, GUN.1 - 7));
        scenario.units.push(unit(player, UnitKind::Surveyor, x, y));
    }
}

#[test]
fn defenders_short_of_an_escorted_gun_gather_beside_the_foundry_first() {
    // Lancers stand beside the gun shelling the picket. Defenders going at
    // the gun one by one would die to them on the way, so a seat that
    // gathers waits beside the Foundry until it matches them and the gun,
    // then goes at the gun; Scrapheap, which defends piecemeal, goes at once.
    let staged = |sentinels: &[(i32, i32)]| {
        let mut scenario = shelling(&[0]);
        escort(&mut scenario, &[0]);
        for (x, y) in sentinels {
            scenario.units.push(unit(0, UnitKind::Sentinel, *x, *y));
        }
        let mut state = scenario.build().unwrap();
        open_fire(&mut state, &[0]);
        until_shells_come(&scenario, &mut state, &[0]);
        (scenario, state)
    };
    let (scenario, state) = staged(&[]);
    let gun = at(&state, GUN.0, GUN.1);
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let lancers = observation
        .enemy_units
        .iter()
        .filter(|enemy| enemy.kind == UnitKind::Lancer)
        .count();
    assert!(
        observation.enemy_units.iter().any(|enemy| enemy.id == gun) && lancers == 3,
        "premise: the gun and its escort are in sight"
    );
    let at_gun = |commands: &[PlayerCommand]| {
        attacks(commands)
            .iter()
            .any(|(_, target)| *target == AttackTarget::Unit(gun))
    };
    let beside = beside_foundry(&state, 0, None);
    for difficulty in [BotDifficulty::Standard, BotDifficulty::Prime] {
        let commands =
            seat_with(&scenario, 0, rung(difficulty)).act(&state, &mut OwnEvents::default());
        assert!(!at_gun(&commands), "{difficulty:?}: {commands:?}");
        let moves: Vec<(Vec<UnitId>, TilePos)> = hunts(&commands)
            .into_iter()
            .chain(runs(&commands))
            .collect();
        assert!(
            !moves.is_empty() && moves.iter().all(|(_, goal)| beside.contains(goal)),
            "{difficulty:?}: {commands:?}"
        );
    }
    // Scrapheap decides less often: on to its next decision.
    let mut scrapheap = seat_with(&scenario, 0, rung(BotDifficulty::Scrapheap));
    let mut later = state.clone();
    while !scrapheap.decision_due(&later) {
        assert!(later.current_tick() < 1_200, "premise: Scrapheap decides");
        later.tick(&[]);
    }
    let commands = scrapheap.act(&later, &mut OwnEvents::default());
    assert!(at_gun(&commands), "{commands:?}");

    // Prime waits at home for a quarter more than it would meet, but beside
    // a gun waiting only gets the defenders shelled: once they match it and
    // its escort, they go.
    let (scenario, state) = staged(&[(2, 9), (3, 9)]);
    let gun = at(&state, GUN.0, GUN.1);
    let commands =
        seat_with(&scenario, 0, rung(BotDifficulty::Prime)).act(&state, &mut OwnEvents::default());
    assert!(
        attacks(&commands)
            .iter()
            .any(|(_, target)| *target == AttackTarget::Unit(gun)),
        "{commands:?}"
    );

    // Enough beside the Foundry to match the gun and its escort: they go.
    let (scenario, state) = staged(&[(2, 9), (3, 9), (4, 9), (5, 9), (2, 10), (5, 10)]);
    let gun = at(&state, GUN.0, GUN.1);
    for difficulty in [BotDifficulty::Standard, BotDifficulty::Prime] {
        let commands =
            seat_with(&scenario, 0, rung(difficulty)).act(&state, &mut OwnEvents::default());
        assert!(
            attacks(&commands)
                .iter()
                .any(|(_, target)| *target == AttackTarget::Unit(gun)),
            "{difficulty:?}: {commands:?}"
        );
    }
}

#[test]
fn mirrored_seats_short_of_escorted_guns_answer_alike() {
    let mut scenario = shelling(&[0, 1]);
    escort(&mut scenario, &[0, 1]);
    let mut state = scenario.build().unwrap();
    open_fire(&mut state, &[0, 1]);
    until_shells_come(&scenario, &mut state, &[0, 1]);
    let west = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let east = seat(&scenario, 1).act(&state, &mut OwnEvents::default());
    assert!(
        !hunts(&west).is_empty() || !runs(&west).is_empty(),
        "premise: {west:?}"
    );
    assert_eq!(mirror(&state, west), east);
}

#[test]
fn mirrored_seats_under_mirrored_shelling_answer_alike() {
    let (scenario, state) = shelled(&[0, 1]);
    let west = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let east = seat(&scenario, 1).act(&state, &mut OwnEvents::default());
    assert!(!advances(&west).is_empty(), "premise: {west:?}");
    assert_eq!(mirror(&state, west), east);
}

#[test]
fn a_checkpoint_while_answering_unseen_shelling_resumes_identically() {
    let (scenario, mut state) = shelled(&[0]);
    let mut opponent = seat(&scenario, 0);
    let commands = opponent.act(&state, &mut OwnEvents::default());
    assert!(!advances(&commands).is_empty(), "premise: {commands:?}");
    state.tick(&commands);
    resumes_identically(&mut opponent, &scenario, &mut state, 360);
}

#[test]
fn a_defense_re_sends_only_members_that_found_a_route() {
    let staged = |x: i32, y: i32, tick: u64| {
        let scenario = raided(
            &[(UnitKind::Sentinel, 5, 8), (UnitKind::Sentinel, 6, 8)],
            &[(UnitKind::Sentinel, x, y)],
        );
        let mut state = scenario.build().unwrap();
        if tick > 0 {
            advance_to(&mut state, tick, &[]);
        }
        (scenario, state)
    };
    let (scenario, state) = staged(9, 4, 0);
    let (stuck, free) = (at(&state, 5, 8), at(&state, 6, 8));
    let mut opponent = seat(&scenario, 0);
    let first = hunts(&opponent.act(&state, &mut OwnEvents::default()));
    assert_eq!(first, [(vec![stuck, free], TilePos::new(9, 4))], "premise");
    let (_, moved) = staged(12, 9, 24);
    let mut events = OwnEvents::default();
    events.record(
        PlayerId(0),
        &[aftermine_sim::Event::OrderStalled {
            unit: stuck,
            player: PlayerId(0),
            pos: moved.unit(stuck).unwrap().pos,
            reason: aftermine_sim::StallReason::NoRoute,
        }],
    );
    assert_eq!(
        hunts(&opponent.act(&moved, &mut events)),
        [(vec![free], TilePos::new(12, 9))]
    );
}

#[test]
fn a_gun_out_of_sight_waits_while_raiders_in_sight_hold_the_foundry() {
    let mut scenario = shelling(&[0]);
    scenario.units.push(unit(1, UnitKind::Buzzard, 4, 5));
    let mut state = scenario.build().unwrap();
    open_fire(&mut state, &[0]);
    until_shells_come(&scenario, &mut state, &[0]);
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    assert!(
        observation
            .enemy_units
            .iter()
            .any(|enemy| enemy.kind == UnitKind::Buzzard)
            && observation
                .enemy_units
                .iter()
                .all(|enemy| enemy.kind != UnitKind::Bombard),
        "premise: the raider is in sight and the gun is not"
    );
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let [(_, goal)] = &advances(&commands)[..] else {
        panic!("{commands:?}");
    };
    let foundry = TilePos::new(3, 11);
    assert!(
        crate::frame::gap(foundry, (3, 3), *goal, (1, 1)) <= 1,
        "defenders wait beside the Foundry for the raid: {goal:?}"
    );
}

/// The field with each of `shelled` holding its garrison and picket, the other
/// seat a Bastion in range of that picket and a Kestrel spotting for it, and,
/// when `watched`, a Kestrel of the shelled seat over that Bastion.
fn bastions(shelled: &[u8], watched: bool) -> Scenario {
    let mut scenario = field();
    for &player in shelled {
        for spot in GARRISON.into_iter().chain([PICKET]) {
            let (x, y) = turned(player, spot);
            scenario.units.push(unit(player, UnitKind::Sentinel, x, y));
        }
    }
    for &player in shelled {
        let (x, y) = turned(player, SPOTTER);
        scenario
            .units
            .push(unit(1 - player, UnitKind::Kestrel, x, y));
    }
    for &player in shelled.iter().filter(|_| watched) {
        let (x, y) = turned(player, (16, 9));
        scenario.units.push(unit(player, UnitKind::Kestrel, x, y));
    }
    for &player in shelled {
        // A 2x2 footprint turns about its far corner.
        let (x, y) = turned(player, (16, 11));
        let (x, y) = if player == 0 { (x, y) } else { (x - 1, y - 1) };
        scenario.buildings.push(BuildingSpec {
            player: 1 - player,
            kind: BuildingKind::Bastion,
            x,
            y,
        });
    }
    scenario
}

/// Plays `state` to a decision tick on which West sees a shell coming and,
/// when `away`, no longer sees the Bastion, after Running West's Kestrel home
/// on the first tick.
fn until_bastion_fires(scenario: &Scenario, state: &mut State, away: bool) {
    if away {
        let kestrels: Vec<UnitId> = state
            .units()
            .iter()
            .filter(|unit| unit.player == PlayerId(0) && unit.kind == UnitKind::Kestrel)
            .map(|unit| unit.id)
            .collect();
        state.tick(&[PlayerCommand {
            player: PlayerId(0),
            command: Command::Run {
                units: kestrels,
                goal: TilePos::new(3, 3),
                queue: false,
            },
        }]);
    }
    let seat = seat(scenario, 0);
    let ready = |state: &State| {
        let observation = ObservationData::fog_honest(state, PlayerId(0));
        !observation.incoming_shells.is_empty()
            && observation
                .enemy_buildings
                .iter()
                .any(|building| building.kind == BuildingKind::Bastion && building.seen != away)
    };
    while !seat.decision_due(state) || !ready(state) {
        assert!(state.current_tick() < 1_200, "premise: the Bastion fires");
        state.tick(&[]);
    }
}

/// Asserts West's one decision on `state` answers the Bastion's shells with a
/// defense hunting beside it.
fn defended_beside_the_bastion(scenario: &Scenario, state: &State, seen: bool) {
    let observation = ObservationData::fog_honest(state, PlayerId(0));
    let known = observation
        .enemy_buildings
        .iter()
        .find(|building| building.kind == BuildingKind::Bastion)
        .expect("premise: the Bastion is known");
    assert_eq!(known.seen, seen, "premise: in sight or remembered");
    assert!(
        observation
            .enemy_units
            .iter()
            .all(|enemy| enemy.kind.stats().weapons.is_empty()),
        "premise: no armed enemy unit in sight"
    );
    let (commands, trace) = seat(scenario, 0).act_traced(state, &mut OwnEvents::default());
    let [(units, goal)] = &hunts(&commands)[..] else {
        panic!("{commands:?}");
    };
    assert!(!units.is_empty());
    assert_eq!(
        crate::frame::gap(known.anchor, (2, 2), *goal, (1, 1)),
        0,
        "beside the Bastion, not toward a guess: {goal:?}"
    );
    assert!(
        trace
            .unwrap()
            .missions
            .iter()
            .any(|mission| matches!(mission.kind, MissionKind::Defend { .. })
                && mission.phase == Phase::Engage),
        "a defense answers it"
    );
}

#[test]
fn shells_from_a_bastion_in_sight_send_fighters_beside_it() {
    let scenario = bastions(&[0], true);
    let mut state = scenario.build().unwrap();
    until_bastion_fires(&scenario, &mut state, false);
    defended_beside_the_bastion(&scenario, &state, true);
}

#[test]
fn shells_from_a_remembered_bastion_send_fighters_beside_it() {
    let mut scenario = bastions(&[0], true);
    // A Barricade takes the picket's place: it neither sees the Bastion nor
    // walks out after it, so the Bastion stays remembered once the Kestrel
    // that saw it leaves.
    scenario
        .units
        .retain(|unit| !(unit.player == 0 && (unit.x, unit.y) == PICKET));
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Barricade,
        x: PICKET.0,
        y: PICKET.1,
    });
    let mut state = scenario.build().unwrap();
    until_bastion_fires(&scenario, &mut state, true);
    defended_beside_the_bastion(&scenario, &state, false);
}

#[test]
fn mirrored_seats_shelled_by_mirrored_bastions_answer_alike() {
    let scenario = bastions(&[0, 1], true);
    let mut state = scenario.build().unwrap();
    let both = |state: &State| {
        [0, 1].iter().all(|player| {
            !ObservationData::fog_honest(state, PlayerId(*player))
                .incoming_shells
                .is_empty()
        })
    };
    let seat0 = seat(&scenario, 0);
    while !seat0.decision_due(&state) || !both(&state) {
        assert!(state.current_tick() < 600, "premise: both Bastions fire");
        state.tick(&[]);
    }
    let west = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let east = seat(&scenario, 1).act(&state, &mut OwnEvents::default());
    assert!(!hunts(&west).is_empty(), "premise: {west:?}");
    assert_eq!(mirror(&state, west), east);
}

#[test]
fn defenders_short_of_a_ground_threat_wait_beside_the_foundry_until_they_match_it() {
    // One Sentinel cannot match three: it waits beside the Foundry, where
    // the raiders that come on meet it, instead of walking into them.
    let raiders = [
        (UnitKind::Sentinel, 9, 4),
        (UnitKind::Sentinel, 9, 5),
        (UnitKind::Sentinel, 9, 6),
    ];
    let scenario = raided(&[(UnitKind::Sentinel, 5, 8)], &raiders);
    let state = scenario.build().unwrap();
    let foundry = state
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0) && building.kind == BuildingKind::Foundry)
        .unwrap();
    let beside: Vec<TilePos> =
        crate::frame::ring(foundry.anchor, BuildingKind::Foundry.size()).collect();
    let waiting = hunts(&seat(&scenario, 0).act(&state, &mut OwnEvents::default()));
    assert!(
        matches!(&waiting[..], [(_, goal)] if beside.contains(goal)),
        "{waiting:?}"
    );

    // Four match them and go at the nearest.
    let scenario = raided(
        &[
            (UnitKind::Sentinel, 5, 8),
            (UnitKind::Sentinel, 6, 8),
            (UnitKind::Sentinel, 5, 9),
            (UnitKind::Sentinel, 6, 9),
        ],
        &raiders,
    );
    let state = scenario.build().unwrap();
    let going = hunts(&seat(&scenario, 0).act(&state, &mut OwnEvents::default()));
    assert!(
        matches!(&going[..], [(units, goal)] if units.len() == 4 && goal.x == 9),
        "{going:?}"
    );

    // The lowest rung sends what it has piecemeal.
    let scenario = raided(&[(UnitKind::Sentinel, 5, 8)], &raiders);
    let state = scenario.build().unwrap();
    let piecemeal = hunts(
        &seat_with(&scenario, 0, rung(BotDifficulty::Scrapheap))
            .act(&state, &mut OwnEvents::default()),
    );
    assert!(
        matches!(&piecemeal[..], [(_, goal)] if goal.x == 9),
        "{piecemeal:?}"
    );
}

#[test]
fn an_unarmed_enemy_among_the_seat_s_buildings_draws_a_defender_without_a_shortfall() {
    // A Surveyor parked by the Foundry on the seat's side of the map.
    let scenario = raided(
        &[(UnitKind::Sentinel, 5, 8), (UnitKind::Sentinel, 6, 8)],
        &[(UnitKind::Surveyor, 8, 3)],
    );
    let state = scenario.build().unwrap();
    let surveyor = at(&state, 8, 3);
    let (commands, trace) = seat(&scenario, 0).act_traced(&state, &mut OwnEvents::default());
    // Attacked, not hunted: a hunt would chase whatever stands nearest.
    assert!(hunts(&commands).is_empty(), "{commands:?}");
    assert!(
        matches!(&attacks(&commands)[..], [(units, AttackTarget::Unit(target))] if !units.is_empty() && *target == surveyor),
        "{commands:?}"
    );
    assert!(
        builds(&commands)
            .iter()
            .all(|(kind, _)| *kind != BuildingKind::Turret),
        "a scout is no emergency: {commands:?}"
    );
    assert!(
        trace
            .unwrap()
            .missions
            .iter()
            .any(|mission| matches!(mission.kind, MissionKind::Defend { .. }))
    );

    // One on the enemy's side of the map is left alone.
    let scenario = raided(
        &[(UnitKind::Sentinel, 5, 8), (UnitKind::Sentinel, 6, 8)],
        &[(UnitKind::Surveyor, 14, 3)],
    );
    let state = scenario.build().unwrap();
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    assert!(
        hunts(&commands).is_empty() && attacks(&commands).is_empty(),
        "{commands:?}"
    );
}

#[test]
fn an_enemy_building_raised_among_the_seat_s_buildings_is_attacked() {
    // A Flak Turret's gun cannot touch the ground units sent at it.
    for kind in [BuildingKind::Fabricator, BuildingKind::FlakTurret] {
        let mut scenario = raided(
            &[(UnitKind::Sentinel, 5, 8), (UnitKind::Sentinel, 6, 8)],
            &[],
        );
        scenario.buildings.push(BuildingSpec {
            player: 1,
            kind,
            x: 8,
            y: 1,
        });
        let state = scenario.build().unwrap();
        let raised = state
            .buildings()
            .iter()
            .find(|building| building.kind == kind)
            .unwrap()
            .id;
        let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
        assert!(
            matches!(&attacks(&commands)[..], [(units, AttackTarget::Building(target))] if !units.is_empty() && *target == raised),
            "{kind:?}: {commands:?}"
        );
    }
}

#[test]
fn armed_raiders_draw_the_defense_before_a_nearer_unarmed_intruder() {
    // An enemy Harvester stands nearer the Foundry than the raider, but
    // chasing it would leave the raider to the base.
    let mut scenario = raided(
        &[
            (UnitKind::Sentinel, 5, 8),
            (UnitKind::Sentinel, 6, 8),
            (UnitKind::Sentinel, 5, 9),
        ],
        &[(UnitKind::Sentinel, 9, 5)],
    );
    scenario.units.push(unit(1, UnitKind::Harvester, 6, 3));
    let state = scenario.build().unwrap();
    let goals: Vec<TilePos> = hunts(&seat(&scenario, 0).act(&state, &mut OwnEvents::default()))
        .into_iter()
        .map(|(_, goal)| goal)
        .collect();
    assert_eq!(goals, [TilePos::new(9, 5)]);
}

#[test]
fn an_escort_beside_an_enemy_building_counts_once() {
    // Three Sentinels match two escorting a forward Fabricator and go at
    // them; counted twice, the escort would hold them beside the Foundry.
    let escorted = |defenders: &[(UnitKind, i32, i32)]| {
        let mut scenario = raided(
            defenders,
            &[(UnitKind::Sentinel, 11, 2), (UnitKind::Sentinel, 11, 3)],
        );
        scenario.buildings.push(BuildingSpec {
            player: 1,
            kind: BuildingKind::Fabricator,
            x: 8,
            y: 1,
        });
        scenario
    };
    let scenario = escorted(&[
        (UnitKind::Sentinel, 5, 8),
        (UnitKind::Sentinel, 6, 8),
        (UnitKind::Sentinel, 5, 9),
    ]);
    let state = scenario.build().unwrap();
    let foundry = state
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0) && building.kind == BuildingKind::Foundry)
        .unwrap();
    let post: Vec<TilePos> =
        crate::frame::ring(foundry.anchor, BuildingKind::Foundry.size()).collect();
    let going = hunts(&seat(&scenario, 0).act(&state, &mut OwnEvents::default()));
    assert!(
        matches!(&going[..], [(units, goal)] if units.len() == 3 && !post.contains(goal)),
        "{going:?}"
    );

    // One does not match them and waits, running to the post.
    let scenario = escorted(&[(UnitKind::Sentinel, 5, 8)]);
    let state = scenario.build().unwrap();
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let waiting: Vec<_> = hunts(&commands)
        .into_iter()
        .chain(runs(&commands))
        .collect();
    assert!(
        matches!(&waiting[..], [(_, goal)] if post.contains(goal)),
        "{waiting:?}"
    );
}

#[test]
fn an_escort_remembered_beside_an_enemy_building_counts_once() {
    // As above, with the escort just lost from sight beyond the Fabricator:
    // remembered near the base, it presses the defense and counts once.
    let goals = |defenders: &[(UnitKind, i32, i32)]| {
        let mut scenario = raided(defenders, &[]);
        scenario.buildings.push(BuildingSpec {
            player: 1,
            kind: BuildingKind::Fabricator,
            x: 8,
            y: 1,
        });
        let mut state = scenario.build().unwrap();
        for _ in 0..12 {
            state.tick(&[]);
        }
        let escort = [TilePos::new(12, 1), TilePos::new(12, 2)];
        let observation = ObservationData::fog_honest(&state, PlayerId(0));
        assert!(
            escort.iter().all(|tile| !observation.visible(*tile)),
            "premise: the escort is out of sight"
        );
        let mut opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
            json["memory"]["units"] = remembered(
                (900..)
                    .zip(escort)
                    .map(|(id, tile)| (id, UnitKind::Sentinel, tile, 11)),
            );
        });
        let foundry = state
            .buildings()
            .iter()
            .find(|building| {
                building.player == PlayerId(0) && building.kind == BuildingKind::Foundry
            })
            .unwrap();
        let post: Vec<TilePos> =
            crate::frame::ring(foundry.anchor, BuildingKind::Foundry.size()).collect();
        hunts(&opponent.act(&state, &mut OwnEvents::default()))
            .into_iter()
            .map(|(units, goal)| (units.len(), post.contains(&goal)))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        goals(&[
            (UnitKind::Sentinel, 5, 8),
            (UnitKind::Sentinel, 6, 8),
            (UnitKind::Sentinel, 5, 9),
        ]),
        [(3, false)],
        "three Sentinels match the escort and go"
    );
    assert!(
        goals(&[(UnitKind::Sentinel, 5, 8)])
            .iter()
            .all(|(_, home)| *home),
        "premise: one does not match the escort and stays home"
    );
}

#[test]
fn an_air_raider_already_answered_does_not_stop_training_for_an_enemy_builder() {
    let trains = |buzzard: bool| {
        let mut scenario = arena(40);
        scenario.units.extend([
            unit(0, UnitKind::Flakhound, 5, 7),
            unit(0, UnitKind::Flakhound, 5, 8),
            unit(1, UnitKind::Harvester, 8, 3),
        ]);
        if buzzard {
            scenario.units.push(unit(1, UnitKind::Buzzard, 9, 5));
        }
        let state = scenario.build().unwrap();
        // A rung that reads the raiders' kinds.
        seat_with(&scenario, 0, rung(BotDifficulty::Prime))
            .act(&state, &mut OwnEvents::default())
            .iter()
            .any(|command| {
                matches!(
                    command.command,
                    Command::Train {
                        kind: UnitKind::Scuttler,
                        ..
                    }
                )
            })
    };
    assert!(trains(false), "premise: an enemy builder draws a Scuttler");
    assert!(
        trains(true),
        "Flakhounds that cover the Buzzard leave the ground shortfall to answer"
    );
}

#[test]
fn a_defense_out_short_of_its_attackers_holds_its_recruit_at_the_post() {
    // Sentinels already out at six raiders hold their ground. Four and a
    // fifth would still be short, so the recruit waits beside the Foundry
    // instead of walking into the fight alone; five and a sixth match them,
    // so it goes on.
    let raiders: Vec<(UnitKind, i32, i32)> = (4..=6)
        .flat_map(|y| [(UnitKind::Sentinel, 9, y), (UnitKind::Sentinel, 10, y)])
        .collect();
    let recruit_waits = |front: &[(i32, i32)]| {
        let mut defenders: Vec<(UnitKind, i32, i32)> = front
            .iter()
            .map(|(x, y)| (UnitKind::Sentinel, *x, *y))
            .collect();
        defenders.push((UnitKind::Sentinel, 2, 8));
        let scenario = raided(&defenders, &raiders);
        let state = scenario.build().unwrap();
        let home = state
            .buildings()
            .iter()
            .find(|b| b.player == PlayerId(0) && b.kind == BuildingKind::Foundry)
            .unwrap();
        let mut members: Vec<UnitId> = front.iter().map(|(x, y)| at(&state, *x, *y)).collect();
        members.sort_unstable();
        let recruit = at(&state, 2, 8);
        let missions = serde_json::json!({
            "next": 1,
            "list": [{
                "id": 0,
                "task": {"task": "defend", "asset": home.id.0, "phase": {"engage": {"focus": null}}},
                "since": 0,
                "units": members,
                "goal": {"x": 9, "y": 5},
            }],
        });
        let mut json = serde_json::to_value(seat(&scenario, 0).checkpoint()).unwrap();
        json["missions"] = missions;
        let checkpoint: crate::checkpoint::Checkpoint = serde_json::from_value(json).unwrap();
        let mut opponent =
            Opponent::restore(&checkpoint, &scenario, &state, map(&scenario)).unwrap();
        let commands = opponent.act(&state, &mut OwnEvents::default());
        let beside: Vec<TilePos> =
            crate::frame::ring(home.anchor, BuildingKind::Foundry.size()).collect();
        hunts(&commands)
            .into_iter()
            .find(|(units, _)| units.contains(&recruit))
            .map(|(_, goal)| beside.contains(&goal))
    };
    assert_eq!(
        recruit_waits(&[(8, 7), (8, 8), (9, 8), (10, 8)]),
        Some(true),
        "short together: the recruit waits at the post"
    );
    assert_eq!(
        recruit_waits(&[(8, 7), (8, 8), (9, 8), (10, 8), (11, 8)]),
        Some(false),
        "level together: it goes on"
    );
}

#[test]
fn a_defense_s_reserve_is_not_taken_back_by_a_regrouping_attack() {
    // The recruit gathers with an attack forming at home. The defense out at
    // six raiders, still short with it, borrows it to wait at the post; the
    // attack lets it go, and nothing else orders it this decision.
    let raiders: Vec<(UnitKind, i32, i32)> = (4..=6)
        .flat_map(|y| [(UnitKind::Sentinel, 9, y), (UnitKind::Sentinel, 10, y)])
        .collect();
    let front = [(10, 7), (11, 7), (10, 8), (11, 8), (12, 8)];
    let mut defenders: Vec<(UnitKind, i32, i32)> = front
        .iter()
        .map(|(x, y)| (UnitKind::Sentinel, *x, *y))
        .collect();
    defenders.push((UnitKind::Sentinel, 2, 8));
    let mut scenario = raided(&defenders, &raiders);
    scenario.players[0].bot_config = Some(rung(BotDifficulty::Prime));
    let state = scenario.build().unwrap();
    let home = state
        .buildings()
        .iter()
        .find(|b| b.player == PlayerId(0) && b.kind == BuildingKind::Foundry)
        .unwrap();
    let mut members: Vec<UnitId> = front.iter().map(|(x, y)| at(&state, *x, *y)).collect();
    members.sort_unstable();
    let recruit = at(&state, 2, 8);
    let mut json =
        serde_json::to_value(seat_with(&scenario, 0, rung(BotDifficulty::Prime)).checkpoint())
            .unwrap();
    json["missions"] = serde_json::json!({
        "next": 2,
        "list": [
            {
                "id": 0,
                "task": {"task": "defend", "asset": home.id.0, "phase": {"engage": {"focus": null}}},
                "since": 0,
                "units": members,
                "goal": {"x": 9, "y": 5},
            },
            {
                "id": 1,
                "task": {
                    "task": "attack",
                    "target": {"owner": 1, "building": "foundry", "anchor": {"x": 19, "y": 5}},
                    "phase": "gather",
                },
                "since": 0,
                "units": [recruit],
                "goal": {"x": 2, "y": 2},
            },
        ],
    });
    let checkpoint: crate::checkpoint::Checkpoint = serde_json::from_value(json).unwrap();
    let mut opponent = Opponent::restore(&checkpoint, &scenario, &state, map(&scenario)).unwrap();
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    let beside: Vec<TilePos> =
        crate::frame::ring(home.anchor, BuildingKind::Foundry.size()).collect();
    let orders: Vec<&Command> = commands
        .iter()
        .map(|command| &command.command)
        .filter(|command| {
            serde_json::to_value(command).unwrap()["units"]
                .as_array()
                .is_some_and(|units| units.contains(&recruit.0.into()))
        })
        .collect();
    assert!(
        orders
            .iter()
            .all(|order| matches!(order, Command::Hunt { goal, .. } if beside.contains(goal))),
        "{orders:?}"
    );
    assert!(
        trace
            .unwrap()
            .missions
            .iter()
            .all(
                |mission| !matches!(mission.kind, MissionKind::Attack { .. }) || mission.units == 0
            ),
        "the attack lets the recruit go"
    );
}

#[test]
fn a_defense_s_reserve_waits_at_its_post_through_the_decision() {
    // The defense out at six raiders, still short with a wounded recruit,
    // holds it at the post. A calm Repair Bay takes a wounded Flakhound no
    // mission holds, but leaves the recruit there this decision.
    const WEST: i32 = 14;
    let raiders: Vec<(UnitKind, i32, i32)> = (4..=6)
        .flat_map(|y| [(UnitKind::Sentinel, 9, y), (UnitKind::Sentinel, 10, y)])
        .collect();
    let front = [(10, 7), (11, 7), (10, 8), (11, 8), (12, 8)];
    let mut defenders: Vec<(UnitKind, i32, i32)> = front
        .iter()
        .map(|(x, y)| (UnitKind::Sentinel, *x, *y))
        .collect();
    defenders.extend([(UnitKind::Sentinel, 2, 8), (UnitKind::Flakhound, 2, 9)]);
    let mut scenario = raided(&defenders, &raiders);
    // The arena drawn out west, so a Bay at its far end stands clear of the
    // raiders.
    scenario.map = ARENA
        .iter()
        .map(|row| {
            let fill = if row.starts_with("##") { "#" } else { "." };
            format!("{}{}{}", &row[..1], fill.repeat(WEST as usize), &row[1..])
        })
        .collect();
    for unit in &mut scenario.units {
        unit.x += WEST;
    }
    scenario.players[0].bot_config = Some(rung(BotDifficulty::Prime));
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::RepairBay,
        x: 1,
        y: 1,
    });
    let state = scenario.build().unwrap();
    let (recruit, flakhound) = (at(&state, 2 + WEST, 8), at(&state, 2 + WEST, 9));
    let state = wounded(&wounded(&state, recruit, 40), flakhound, 40);
    let home = state
        .buildings()
        .iter()
        .find(|b| b.player == PlayerId(0) && b.kind == BuildingKind::Foundry)
        .unwrap();
    let mut members: Vec<UnitId> = front
        .iter()
        .map(|(x, y)| at(&state, x + WEST, *y))
        .collect();
    members.sort_unstable();
    let mut opponent = restored_with(
        &seat_with(&scenario, 0, rung(BotDifficulty::Prime)),
        &scenario,
        &state,
        |json| {
            json["missions"] = serde_json::json!({
                "next": 1,
                "list": [{
                    "id": 0,
                    "task": {"task": "defend", "asset": home.id.0, "phase": {"engage": {"focus": null}}},
                    "since": 0,
                    "units": members,
                    "goal": {"x": 9 + WEST, "y": 5},
                }],
            });
        },
    );
    let commands = opponent.act(&state, &mut OwnEvents::default());
    let ordered = |unit: UnitId| -> Vec<&Command> {
        commands
            .iter()
            .map(|command| &command.command)
            .filter(|command| {
                serde_json::to_value(command).unwrap()["units"]
                    .as_array()
                    .is_some_and(|units| units.contains(&unit.0.into()))
            })
            .collect()
    };
    assert!(
        matches!(ordered(flakhound)[..], [Command::Run { goal, .. }] if goal.x < 4),
        "premise: the Bay takes the wounded no mission holds: {:?}",
        ordered(flakhound)
    );
    let beside: Vec<TilePos> =
        crate::frame::ring(home.anchor, BuildingKind::Foundry.size()).collect();
    assert!(
        ordered(recruit)
            .iter()
            .all(|order| matches!(order, Command::Hunt { goal, .. } if beside.contains(goal))),
        "{:?}",
        ordered(recruit)
    );
}

#[test]
fn the_upper_rungs_go_out_at_raiders_only_with_an_edge() {
    // Three Sentinels against three raiders: even odds. Prime waits beside
    // the Foundry for an edge; Standard goes out at even odds.
    let raiders = [
        (UnitKind::Sentinel, 9, 4),
        (UnitKind::Sentinel, 9, 5),
        (UnitKind::Sentinel, 9, 6),
    ];
    let defenders = [
        (UnitKind::Sentinel, 5, 8),
        (UnitKind::Sentinel, 6, 8),
        (UnitKind::Sentinel, 5, 9),
    ];
    let goes_out = |difficulty: BotDifficulty| {
        let scenario = raided(&defenders, &raiders);
        let state = scenario.build().unwrap();
        hunts(&seat_with(&scenario, 0, rung(difficulty)).act(&state, &mut OwnEvents::default()))
            .iter()
            .any(|(_, goal)| goal.x == 9)
    };
    assert!(!goes_out(BotDifficulty::Prime), "Prime waits for an edge");
    assert!(
        goes_out(BotDifficulty::Standard),
        "Standard goes at even odds"
    );
}

/// `scenario` with `columns` more columns of open ground inserted before map
/// column `at`, and every unit and building from there on moved over.
pub(super) fn widened(mut scenario: Scenario, at: i32, columns: i32) -> Scenario {
    let index = usize::try_from(at).unwrap();
    let width = usize::try_from(columns).unwrap();
    scenario.map = scenario
        .map
        .iter()
        .map(|row| {
            let fill = if row.starts_with("##") { "#" } else { "." };
            format!("{}{}{}", &row[..index], fill.repeat(width), &row[index..])
        })
        .collect();
    for unit in scenario.units.iter_mut().filter(|unit| unit.x >= at) {
        unit.x += columns;
    }
    for building in scenario.buildings.iter_mut().filter(|b| b.x >= at) {
        building.x += columns;
    }
    scenario
}

/// The tiles beside `player`'s Foundry at `anchor`.
pub(super) fn beside_foundry(state: &State, player: u8, anchor: Option<TilePos>) -> Vec<TilePos> {
    let foundry = state
        .buildings()
        .iter()
        .find(|b| {
            b.player == PlayerId(player)
                && b.kind == BuildingKind::Foundry
                && anchor.is_none_or(|anchor| b.anchor == anchor)
        })
        .unwrap();
    crate::frame::ring(foundry.anchor, BuildingKind::Foundry.size()).collect()
}

#[test]
fn defenders_still_walking_in_wait_at_the_post_until_they_arrive() {
    // Five Sentinels would beat three raiders by Prime's edge, but three are
    // still far off: all five gather at the post beside the Foundry rather
    // than the two already there meeting the raiders alone. Once all five
    // stand beside the Foundry, they go together.
    const WEST: i32 = 14;
    let raiders = [
        (UnitKind::Sentinel, 9, 4),
        (UnitKind::Sentinel, 9, 5),
        (UnitKind::Sentinel, 9, 6),
    ];
    let sent = |others: [(i32, i32); 3]| {
        let near = [(UnitKind::Sentinel, 5, 8), (UnitKind::Sentinel, 6, 8)];
        let mut scenario = widened(raided(&near, &raiders), 1, WEST);
        scenario.units.extend(
            others
                .iter()
                .map(|(x, y)| unit(0, UnitKind::Sentinel, *x, *y)),
        );
        let state = scenario.build().unwrap();
        let beside = beside_foundry(&state, 0, None);
        let commands = seat_with(&scenario, 0, rung(BotDifficulty::Prime))
            .act(&state, &mut OwnEvents::default());
        // Workers running from the raiders are not the defense.
        let sentinels = |units: &[UnitId]| {
            units.iter().all(|id| {
                state
                    .unit(*id)
                    .is_some_and(|unit| unit.kind == UnitKind::Sentinel)
            })
        };
        hunts(&commands)
            .into_iter()
            .chain(runs(&commands))
            .filter(|(units, _)| sentinels(units))
            .map(|(units, goal)| (units.len(), beside.contains(&goal)))
            .collect::<Vec<_>>()
    };
    // Those far off run there, so they do not stop to fight on the way.
    assert_eq!(
        sent([(2, 2), (2, 3), (2, 4)]),
        [(2, true), (3, true)],
        "three still far off: all gather at the post"
    );
    assert_eq!(
        sent([(5 + WEST, 9), (6 + WEST, 9), (7 + WEST, 8)]),
        [(5, false)],
        "all beside the Foundry: they go"
    );
}

#[test]
fn defenders_count_the_line_behind_the_raider_they_would_meet() {
    // One raider beside the base and four more holding a line in sight,
    // beyond reach of its buildings but near the fight: Prime's three
    // Sentinels would meet all five, so they wait beside the Foundry rather
    // than walk into the line. Without the line they go at the raider.
    let defenders = [
        (UnitKind::Sentinel, 5, 8),
        (UnitKind::Sentinel, 6, 8),
        (UnitKind::Sentinel, 5, 9),
    ];
    let line = [(14, 4), (14, 5), (15, 4), (15, 5)];
    let sent = |with_line: bool| {
        let mut raiders = vec![(UnitKind::Sentinel, 9, 5)];
        if with_line {
            raiders.extend(line.iter().map(|(x, y)| (UnitKind::Sentinel, *x, *y)));
        }
        let mut scenario = raided(&defenders, &raiders);
        // An unarmed scout gives West sight of the line.
        scenario.units.push(unit(0, UnitKind::Surveyor, 12, 7));
        let state = scenario.build().unwrap();
        assert_eq!(
            ObservationData::fog_honest(&state, PlayerId(0))
                .enemy_units
                .iter()
                .filter(|enemy| enemy.kind == UnitKind::Sentinel)
                .count(),
            raiders.len(),
            "premise: West sees every raider"
        );
        let beside = beside_foundry(&state, 0, None);
        hunts(
            &seat_with(&scenario, 0, rung(BotDifficulty::Prime))
                .act(&state, &mut OwnEvents::default()),
        )
        .into_iter()
        .map(|(_, goal)| beside.contains(&goal))
        .collect::<Vec<_>>()
    };
    assert_eq!(sent(false), [false], "the raider alone: they go at it");
    assert_eq!(sent(true), [true], "with the line: they wait at the post");
}

/// The arena drawn out in the middle, with a West expansion Foundry out on
/// it, six East Sentinels beside the expansion, and West Sentinels at home
/// on `home`.
fn expansion_raided(home: &[(i32, i32)]) -> Scenario {
    let mut scenario = widened(arena(0), 12, 20);
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 24,
        y: 5,
    });
    for (x, y) in home {
        scenario.units.push(unit(0, UnitKind::Sentinel, *x, *y));
    }
    for x in 28..=29 {
        for y in 4..=6 {
            scenario.units.push(unit(1, UnitKind::Sentinel, x, y));
        }
    }
    scenario
}

#[test]
fn a_place_away_from_home_the_army_cannot_match_is_given_up() {
    let expansion = Some(TilePos::new(24, 5));
    // Three Sentinels at home against six at the expansion: no defense or
    // guard goes out to it.
    let few = [(5, 8), (6, 8), (5, 9)];
    let scenario = expansion_raided(&few);
    let state = scenario.build().unwrap();
    let commands =
        seat_with(&scenario, 0, rung(BotDifficulty::Prime)).act(&state, &mut OwnEvents::default());
    assert!(
        hunts(&commands)
            .iter()
            .chain(&runs(&commands))
            .all(|(_, goal)| goal.x < 20),
        "{commands:?}"
    );

    // An Aggressive seat keeps less home, so its local guard could take
    // some of six; they would not match the raiders either, so none go.
    let six = [(5, 8), (6, 8), (7, 8), (5, 9), (6, 9), (8, 9)];
    let scenario = expansion_raided(&six);
    let state = scenario.build().unwrap();
    let aggressive = BotConfig::new(BotDifficulty::Prime, BotStance::Aggressive, 11);
    let commands = seat_with(&scenario, 0, aggressive).act(&state, &mut OwnEvents::default());
    assert!(
        hunts(&commands)
            .iter()
            .all(|(_, goal)| goal.chebyshev(TilePos::new(25, 6)) > 8),
        "{commands:?}"
    );

    // Eight would match them by Prime's edge: they gather beside the
    // expansion.
    let many = [
        (5, 8),
        (6, 8),
        (7, 8),
        (8, 8),
        (9, 8),
        (5, 9),
        (6, 9),
        (8, 9),
    ];
    let scenario = expansion_raided(&many);
    let state = scenario.build().unwrap();
    let beside = beside_foundry(&state, 0, expansion);
    let commands =
        seat_with(&scenario, 0, rung(BotDifficulty::Prime)).act(&state, &mut OwnEvents::default());
    assert!(
        hunts(&commands)
            .iter()
            .chain(&runs(&commands))
            .any(|(_, goal)| beside.contains(goal)),
        "premise: a match is defended: {commands:?}"
    );

    // A defense already engaged there is not given up: turned out under
    // fire, its members would only die on the way home and be sent back at
    // the next look. It stays a defense, and nobody is turned home.
    let mut scenario = expansion_raided(&[(5, 8)]);
    scenario.players[0].bot_config = Some(rung(BotDifficulty::Prime));
    scenario.units.extend([
        unit(0, UnitKind::Sentinel, 24, 9),
        unit(0, UnitKind::Sentinel, 25, 9),
    ]);
    let state = scenario.build().unwrap();
    let outpost = state
        .buildings()
        .iter()
        .find(|b| b.player == PlayerId(0) && Some(b.anchor) == expansion)
        .unwrap()
        .id;
    let mut members = vec![at(&state, 24, 9), at(&state, 25, 9)];
    members.sort_unstable();
    let mut opponent = restored_with(
        &seat_with(&scenario, 0, rung(BotDifficulty::Prime)),
        &scenario,
        &state,
        |json| {
            json["missions"] = serde_json::json!({
                "next": 1,
                "list": [{
                    "id": 0,
                    "task": {"task": "defend", "asset": outpost.0, "phase": {"engage": {"focus": null}}},
                    "since": 0,
                    "units": members,
                    "goal": {"x": 28, "y": 5},
                }],
            });
        },
    );
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    let home = beside_foundry(&state, 0, Some(TilePos::new(3, 5)));
    assert!(
        hunts(&commands)
            .iter()
            .chain(&runs(&commands))
            .all(|(units, goal)| !(units.contains(&members[0]) && home.contains(goal))),
        "{commands:?}"
    );
    assert!(
        trace
            .unwrap()
            .missions
            .iter()
            .any(|mission| matches!(mission.kind, MissionKind::Defend { .. })),
        "the defense goes on"
    );
}

#[test]
fn the_last_foundry_keeps_its_guard() {
    // West's start falls and its expansion stands alone: six raiders in
    // reach of it, though not yet at it, outweigh its two Sentinels, yet it
    // keeps them as its guard.
    let mut scenario = widened(arena(0), 12, 20);
    scenario
        .buildings
        .push(building(0, BuildingKind::Foundry, 24, 5));
    scenario.units.retain(|unit| unit.player == 1);
    scenario.units.extend([
        unit(0, UnitKind::Sentinel, 21, 8),
        unit(0, UnitKind::Sentinel, 22, 8),
        unit(0, UnitKind::Sentinel, 21, 9),
        unit(0, UnitKind::Sentinel, 22, 9),
        unit(0, UnitKind::Surveyor, 29, 9),
        unit(1, UnitKind::Sentinel, 7, 4),
        unit(1, UnitKind::Sentinel, 7, 5),
    ]);
    for y in [1, 2, 3, 4, 7, 8, 9, 10] {
        scenario.units.push(unit(1, UnitKind::Sentinel, 35, y));
    }
    let state = scenario.build().unwrap();
    let start = state
        .buildings()
        .iter()
        .find(|b| b.player == PlayerId(0) && b.anchor == TilePos::new(3, 5))
        .unwrap()
        .id;
    let mut state = damaged(&state, start, 1);
    let raze = PlayerCommand {
        player: PlayerId(1),
        command: Command::Attack {
            units: vec![at(&state, 7, 4), at(&state, 7, 5)],
            target: AttackTarget::Building(start),
            queue: false,
        },
    };
    advance_to(&mut state, 24, &[raze]);
    assert_eq!(
        foundries(&state, PlayerId(0)).len(),
        1,
        "premise: West's start fell"
    );
    let expansion = state
        .buildings()
        .iter()
        .find(|b| b.player == PlayerId(0) && b.anchor == TilePos::new(24, 5))
        .unwrap()
        .id;
    let mut members = vec![
        at(&state, 21, 8),
        at(&state, 22, 8),
        at(&state, 21, 9),
        at(&state, 22, 9),
    ];
    members.sort_unstable();
    let mut opponent = restored_with(
        &seat_with(&scenario, 0, rung(BotDifficulty::Prime)),
        &scenario,
        &state,
        |json| {
            json["missions"] = serde_json::json!({
                "next": 1,
                "list": [{
                    "id": 0,
                    "task": {"task": "defend", "asset": expansion.0, "phase": "guard"},
                    "since": 0,
                    "units": members,
                    "goal": {"x": 23, "y": 6},
                }],
            });
        },
    );
    let (_, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert!(
        trace
            .unwrap()
            .missions
            .iter()
            .any(|mission| matches!(mission.kind, MissionKind::Defend { .. })),
        "the last Foundry keeps its guard"
    );
}

#[test]
fn home_is_never_given_up() {
    // Six raiders at home against three Sentinels and an expansion: the
    // defenders wait beside the home Foundry rather than leave it.
    let mut scenario = expansion_raided(&[(5, 8), (6, 8), (5, 9)]);
    scenario
        .units
        .retain(|unit| unit.player == 0 || unit.x < 28);
    for x in 9..=10 {
        for y in 4..=6 {
            scenario.units.push(unit(1, UnitKind::Sentinel, x, y));
        }
    }
    let state = scenario.build().unwrap();
    let home = beside_foundry(&state, 0, Some(TilePos::new(3, 5)));
    let commands =
        seat_with(&scenario, 0, rung(BotDifficulty::Prime)).act(&state, &mut OwnEvents::default());
    assert!(
        hunts(&commands).iter().any(|(_, goal)| home.contains(goal)),
        "{commands:?}"
    );
}

#[test]
fn a_defense_standing_down_brings_its_members_back_beside_the_foundry() {
    // Defenders out where the raiders were, the raiders gone: they come
    // back beside the Foundry rather than be let go out there, unless an
    // enemy can still hit them, aircraft too, and does not outweigh them.
    let stand_down_with = |defenders: &[(i32, i32)],
                           defender: UnitKind,
                           threat: Option<(UnitKind, i32)>| {
        let placed: Vec<(UnitKind, i32, i32)> =
            defenders.iter().map(|(x, y)| (defender, *x, *y)).collect();
        let mut scenario = raided(&placed, &[]);
        if let Some((threat, x)) = threat {
            scenario.units.push(unit(1, threat, x, 8));
        }
        scenario.players[0].bot_config = Some(rung(BotDifficulty::Prime));
        let state = scenario.build().unwrap();
        let home = state
            .buildings()
            .iter()
            .find(|b| b.player == PlayerId(0) && b.kind == BuildingKind::Foundry)
            .unwrap();
        let mut members: Vec<UnitId> = defenders.iter().map(|(x, y)| at(&state, *x, *y)).collect();
        members.sort_unstable();
        let mut opponent = restored_with(
            &seat_with(&scenario, 0, rung(BotDifficulty::Prime)),
            &scenario,
            &state,
            |json| {
                json["missions"] = serde_json::json!({
                    "next": 1,
                    "list": [{
                        "id": 0,
                        "task": {"task": "defend", "asset": home.id.0, "phase": {"engage": {"focus": null}}},
                        "since": 0,
                        "units": members,
                        "goal": {"x": 15, "y": 6},
                    }],
                });
            },
        );
        let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
        let beside = beside_foundry(&state, 0, None);
        let home = runs(&commands)
            .iter()
            .any(|(units, goal)| units == &members && beside.contains(goal));
        let recovers = trace
            .unwrap()
            .missions
            .iter()
            .any(|mission| mission.phase == Phase::Recover);
        (home, recovers)
    };
    let stand_down = |defender: UnitKind, threat: Option<(UnitKind, i32)>| {
        stand_down_with(&[(13, 8), (14, 8)], defender, threat)
    };
    assert_eq!(
        stand_down(UnitKind::Sentinel, None),
        (true, true),
        "the raiders gone: home"
    );
    // A Lancer beyond the base's threat gap but still in reach of them: two
    // do not turn their backs to it, but one, outweighed out there, runs
    // home rather than die where it stands.
    assert_eq!(
        stand_down(UnitKind::Sentinel, Some((UnitKind::Lancer, 19))),
        (false, false),
        "still under fire and not outweighed: they stay"
    );
    assert_eq!(
        stand_down_with(&[(13, 8)], UnitKind::Sentinel, Some((UnitKind::Lancer, 19))),
        (true, true),
        "outweighed out there: home"
    );
    // Nor do two Talons with an enemy Talon on them, though it cannot hit
    // the ground.
    assert_eq!(
        stand_down(UnitKind::Talon, None),
        (true, true),
        "premise: Talons come home"
    );
    assert_eq!(
        stand_down(UnitKind::Talon, Some((UnitKind::Talon, 17))),
        (false, false),
        "Talons under fire stay"
    );
    // Judged in the air: one Talon facing an equal one at Prime's margin is
    // outweighed and comes home.
    assert_eq!(
        stand_down_with(&[(13, 8)], UnitKind::Talon, Some((UnitKind::Talon, 17))),
        (true, true),
        "a lone Talon outweighed in the air comes home"
    );
}

#[test]
fn a_landed_defender_under_ground_fire_stays() {
    // A Condor defending out where raiders were has set down, and a Lancer,
    // which cannot hit aircraft in flight, walks up within reach of it:
    // landed, it is under fire, so it does not stand down.
    let mut scenario = widened(arena(0), 12, 20);
    scenario.units.extend([
        unit(0, UnitKind::Condor, 13, 8),
        unit(1, UnitKind::Lancer, 40, 1),
    ]);
    scenario.players[0].bot_config = Some(rung(BotDifficulty::Prime));
    let mut state = scenario.build().unwrap();
    let condor = at(&state, 13, 8);
    let lancer = at(&state, 40, 1);
    state.tick(&[PlayerCommand {
        player: PlayerId(0),
        command: Command::Run {
            units: vec![condor],
            goal: TilePos::new(13, 8),
            queue: false,
        },
    }]);
    let landed = |state: &State| {
        state
            .unit(condor)
            .is_some_and(aftermine_sim::state::Unit::landed)
    };
    while !landed(&state) {
        assert!(state.current_tick() < 1_500, "premise: the Condor lands");
        state.tick(&[]);
    }
    let parked = state.unit(condor).unwrap().tile();
    state.tick(&[PlayerCommand {
        player: PlayerId(1),
        command: Command::Run {
            units: vec![lancer],
            goal: parked.offset(5, 0),
            queue: false,
        },
    }]);
    while state
        .unit(lancer)
        .is_some_and(|unit| unit.tile().chebyshev(parked) > 5)
    {
        assert!(state.current_tick() < 3_000, "premise: the Lancer walks up");
        state.tick(&[]);
    }
    let interval = crate::decision_interval(BotDifficulty::Prime);
    while !state.current_tick().is_multiple_of(interval) {
        state.tick(&[]);
    }
    assert!(
        landed(&state)
            && state
                .unit(lancer)
                .is_some_and(|unit| unit.tile().chebyshev(parked) <= 5),
        "premise: the Lancer stands in reach of the landed Condor"
    );
    let home = state
        .buildings()
        .iter()
        .find(|b| b.player == PlayerId(0) && b.kind == BuildingKind::Foundry)
        .unwrap()
        .id;
    let mut opponent = restored_with(
        &seat_with(&scenario, 0, rung(BotDifficulty::Prime)),
        &scenario,
        &state,
        |json| {
            json["missions"] = serde_json::json!({
                "next": 1,
                "list": [{
                    "id": 0,
                    "task": {"task": "defend", "asset": home.0, "phase": {"engage": {"focus": null}}},
                    "since": 0,
                    "units": [condor],
                    "goal": {"x": 15, "y": 6},
                }],
            });
        },
    );
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert!(
        runs(&commands).iter().all(|(units, _)| units != &[condor]),
        "{commands:?}"
    );
    assert!(
        trace
            .unwrap()
            .missions
            .iter()
            .all(|mission| mission.phase != Phase::Recover),
        "it stays engaged"
    );
}

#[test]
fn a_defense_whose_order_home_finds_no_room_stays_engaged() {
    // Two defenders out where raiders by the start were, the raiders gone,
    // while raiders press two other Foundries: at Scrapheap, answering those
    // takes every order this look, so the two stay engaged rather than
    // recover without their order home.
    let mut scenario = widened(arena(0), 12, 20);
    scenario.buildings.extend([
        building(0, BuildingKind::Foundry, 24, 2),
        building(0, BuildingKind::Foundry, 24, 8),
    ]);
    scenario.units.extend([
        unit(0, UnitKind::Sentinel, 9, 8),
        unit(0, UnitKind::Sentinel, 10, 8),
        unit(0, UnitKind::Sentinel, 22, 2),
        unit(0, UnitKind::Sentinel, 22, 3),
        unit(0, UnitKind::Sentinel, 22, 4),
        unit(0, UnitKind::Sentinel, 22, 8),
        unit(0, UnitKind::Sentinel, 22, 9),
        unit(0, UnitKind::Sentinel, 22, 10),
        unit(1, UnitKind::Sentinel, 28, 2),
        unit(1, UnitKind::Sentinel, 28, 9),
    ]);
    scenario.players[0].bot_config = Some(rung(BotDifficulty::Scrapheap));
    let state = scenario.build().unwrap();
    let start = state
        .buildings()
        .iter()
        .find(|b| b.player == PlayerId(0) && b.anchor == TilePos::new(3, 5))
        .unwrap()
        .id;
    let mut members = vec![at(&state, 9, 8), at(&state, 10, 8)];
    members.sort_unstable();
    let mut opponent = restored_with(
        &seat_with(&scenario, 0, rung(BotDifficulty::Scrapheap)),
        &scenario,
        &state,
        |json| {
            json["missions"] = serde_json::json!({
                "next": 1,
                "list": [{
                    "id": 0,
                    "task": {"task": "defend", "asset": start.0, "phase": {"engage": {"focus": null}}},
                    "since": 0,
                    "units": members,
                    "goal": {"x": 11, "y": 6},
                }],
            });
        },
    );
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert_eq!(
        hunts(&commands).len(),
        2,
        "premise: both raids are answered: {commands:?}"
    );
    assert!(
        runs(&commands).iter().all(|(units, _)| units != &members),
        "{commands:?}"
    );
    let missions = trace.unwrap().missions;
    assert!(
        missions
            .iter()
            .any(|mission| mission.id == 0 && mission.phase == Phase::Engage),
        "{missions:?}"
    );
}

#[test]
fn only_a_defense_out_at_its_fight_holds_on_and_only_where_it_can_fall_back() {
    // One Sentinel at home and two by the expansion against six raiders: under
    // even odds. A Prime defense out at the raiders holds on and falls back by
    // its own rule; one still waiting at the expansion's post, or sent at the
    // raiders but still on its way, is given up; at Scrapheap, which never
    // falls back, one out at them is given up too.
    let expansion = TilePos::new(24, 5);
    let defended = |config: BotConfig, goal: TilePos, x: i32| {
        let mut scenario = expansion_raided(&[(5, 8)]);
        scenario.players[0].bot_config = Some(config);
        scenario.units.extend([
            unit(0, UnitKind::Sentinel, x, 9),
            unit(0, UnitKind::Sentinel, x + 1, 9),
        ]);
        let state = scenario.build().unwrap();
        let outpost = state
            .buildings()
            .iter()
            .find(|b| b.player == PlayerId(0) && b.anchor == expansion)
            .unwrap()
            .id;
        let mut members = vec![at(&state, x, 9), at(&state, x + 1, 9)];
        members.sort_unstable();
        let mut opponent = restored_with(
            &seat_with(&scenario, 0, config),
            &scenario,
            &state,
            |json| {
                json["missions"] = serde_json::json!({
                    "next": 1,
                    "list": [{
                        "id": 0,
                        "task": {"task": "defend", "asset": outpost.0, "phase": {"engage": {"focus": null}}},
                        "since": 0,
                        "units": members,
                        "goal": {"x": goal.x, "y": goal.y},
                    }],
                });
            },
        );
        let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
        let defends = trace
            .unwrap()
            .missions
            .iter()
            .any(|mission| matches!(mission.kind, MissionKind::Defend { .. }));
        (defends, commands, members)
    };
    let out = TilePos::new(28, 5);
    // A tile beside the expansion, on the raiders' side.
    let post = crate::frame::ring(expansion, BuildingKind::Foundry.size())
        .max_by_key(|tile| (tile.x, tile.y))
        .unwrap();
    assert!(
        defended(rung(BotDifficulty::Prime), out, 24).0,
        "out at the fight"
    );
    assert!(
        !defended(rung(BotDifficulty::Prime), post, 24).0,
        "waiting at the post"
    );
    let (defends, commands, members) = defended(rung(BotDifficulty::Prime), out, 17);
    assert!(!defends, "on its way out");
    // Given up, they run home rather than fight their way there.
    assert!(
        runs(&commands)
            .iter()
            .any(|(units, goal)| *units == members && goal.x < 12),
        "{commands:?}"
    );
    assert!(
        hunts(&commands)
            .iter()
            .all(|(units, _)| members.iter().all(|id| !units.contains(id))),
        "{commands:?}"
    );
    assert!(
        !defended(rung(BotDifficulty::Scrapheap), out, 24).0,
        "never falls back"
    );
}

#[test]
fn a_guard_short_of_known_enemies_is_answered_with_what_beats_them() {
    // Lancers stand near a bare expansion, not near enough to attack it, in a
    // West scout's sight. Its guard is short; a seat that reads threats trains
    // what beats them, not Scuttlers, while Standard, which judges the threat
    // by its size, still answers with the cheapest unit.
    let trained = |difficulty: BotDifficulty| {
        let mut scenario = widened(arena(400), 12, 20);
        scenario.players[0].bot_config = Some(rung(difficulty));
        scenario.buildings.extend([
            BuildingSpec {
                player: 0,
                kind: BuildingKind::Foundry,
                x: 24,
                y: 8,
            },
            BuildingSpec {
                player: 0,
                kind: BuildingKind::Fabricator,
                x: 6,
                y: 1,
            },
        ]);
        scenario.units.push(unit(0, UnitKind::Surveyor, 33, 9));
        for y in [9, 10] {
            scenario.units.push(unit(1, UnitKind::Lancer, 35, y));
        }
        let state = scenario.build().unwrap();
        let commands =
            seat_with(&scenario, 0, rung(difficulty)).act(&state, &mut OwnEvents::default());
        trains(&commands)
            .into_iter()
            .map(|(_, kind)| kind)
            .collect::<Vec<_>>()
    };
    let prime = trained(BotDifficulty::Prime);
    assert!(!prime.is_empty(), "premise: the short guard trains");
    assert!(!prime.contains(&UnitKind::Scuttler), "{prime:?}");
    let standard = trained(BotDifficulty::Standard);
    assert!(standard.contains(&UnitKind::Scuttler), "{standard:?}");
}

#[test]
fn a_builder_raising_a_site_is_attacked_before_a_nearer_scout() {
    // An east Harvester raises a Fabricator among the west buildings while
    // a Surveyor stands nearer the Foundry. Killing the builder stops the
    // site soonest, so the defenders attack it and hold it; a hunt would
    // chase the scout.
    let mut scenario = raided(
        &[(UnitKind::Sentinel, 5, 8), (UnitKind::Sentinel, 6, 8)],
        &[(UnitKind::Harvester, 10, 3), (UnitKind::Surveyor, 7, 4)],
    );
    scenario.players[1].scrap = 400;
    let mut state = scenario.build().unwrap();
    let builder = at(&state, 10, 3);
    let surveyor = at(&state, 7, 4);
    state.tick(&[PlayerCommand {
        player: PlayerId(1),
        command: Command::Build {
            units: vec![builder],
            kind: BuildingKind::Fabricator,
            anchor: TilePos::new(9, 1),
            queue: false,
            defer: false,
        },
    }]);
    let raising = |state: &State| {
        state
            .buildings()
            .iter()
            .any(|building| building.kind == BuildingKind::Fabricator && !building.built())
    };
    while !raising(&state) && state.current_tick() < 240 {
        state.tick(&[]);
    }
    assert!(raising(&state), "premise: the site is going up");
    let next = state.current_tick().next_multiple_of(12);
    advance_to(&mut state, next, &[]);
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    assert!(
        matches!(&attacks(&commands)[..], [(units, AttackTarget::Unit(target))] if !units.is_empty() && *target == builder),
        "{commands:?}"
    );
    assert!(
        attacks(&commands)
            .iter()
            .all(|(_, target)| *target != AttackTarget::Unit(surveyor)),
        "{commands:?}"
    );
}

#[test]
fn a_defense_held_on_a_builder_turns_when_an_armed_enemy_comes() {
    // West Sentinels are attacking an east builder when a Lancer walks in
    // beside it: held on the builder, they would stand under its fire.
    let mut scenario = raided(
        &[(UnitKind::Sentinel, 5, 8), (UnitKind::Sentinel, 6, 8)],
        &[(UnitKind::Harvester, 9, 3), (UnitKind::Lancer, 10, 4)],
    );
    scenario.players[1].scrap = 400;
    let state = scenario.build().unwrap();
    let builder = at(&state, 9, 3);
    let lancer = at(&state, 10, 4);
    let home = state
        .buildings()
        .iter()
        .find(|b| b.player == PlayerId(0) && b.kind == BuildingKind::Foundry)
        .unwrap();
    let mut members = vec![at(&state, 5, 8), at(&state, 6, 8)];
    members.sort_unstable();
    let mut opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
        json["missions"] = serde_json::json!({
            "next": 1,
            "list": [{
                "id": 0,
                "task": {"task": "defend", "asset": home.id.0, "phase": {"engage": {"focus": builder}}},
                "since": 0,
                "units": members,
                "goal": {"x": 9, "y": 3},
            }],
        });
    });
    let commands = opponent.act(&state, &mut OwnEvents::default());
    assert!(
        ObservationData::fog_honest(&state, PlayerId(0))
            .enemy_units
            .iter()
            .any(|enemy| enemy.id == lancer),
        "premise: the Lancer is in sight"
    );
    let ordered: Vec<UnitId> = hunts(&commands)
        .into_iter()
        .map(|(units, _)| units)
        .chain(attacks(&commands).into_iter().map(|(units, _)| units))
        .flatten()
        .collect();
    assert!(
        members.iter().all(|member| ordered.contains(member)),
        "{commands:?}"
    );
    assert!(
        attacks(&commands)
            .iter()
            .all(|(_, target)| *target != AttackTarget::Unit(builder)),
        "{commands:?}"
    );
    // The builder no longer holds the defense, and once the Lancer has gone
    // the defenders go back to it.
    let checkpoint = opponent.checkpoint();
    let json = serde_json::to_value(&checkpoint).unwrap();
    assert_eq!(
        json["missions"]["list"][0]["task"]["phase"]["engage"]["focus"],
        serde_json::Value::Null,
        "{json}"
    );
    let mut gone = scenario.clone();
    gone.units.retain(|spec| spec.kind != UnitKind::Lancer);
    let state = gone.build().unwrap();
    let mut opponent = Opponent::restore(&checkpoint, &gone, &state, map(&gone)).unwrap();
    let next = (state.current_tick() + 1).next_multiple_of(12);
    let mut state = state;
    advance_to(&mut state, next, &[]);
    let commands = opponent.act(&state, &mut OwnEvents::default());
    assert!(
        attacks(&commands)
            .iter()
            .any(|(_, target)| *target == AttackTarget::Unit(builder)),
        "{commands:?}"
    );
}

#[test]
fn a_defender_chased_past_the_base_runs_back_while_one_at_the_fight_stays() {
    // A raider stands by the Foundry. One defender fights it there; another
    // chased an earlier raider out past the base, where nothing outweighs
    // it: hunting, it would chase on, so it runs back to the post.
    let scenario = raided(
        &[(UnitKind::Sentinel, 7, 6), (UnitKind::Sentinel, 21, 9)],
        &[(UnitKind::Sentinel, 8, 5)],
    );
    let state = scenario.build().unwrap();
    let home = state
        .buildings()
        .iter()
        .find(|b| b.player == PlayerId(0) && b.kind == BuildingKind::Foundry)
        .unwrap();
    let (near, chaser) = (at(&state, 7, 6), at(&state, 21, 9));
    let mut members = vec![near, chaser];
    members.sort_unstable();
    let mut opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
        json["missions"] = serde_json::json!({
            "next": 1,
            "list": [{
                "id": 0,
                "task": {"task": "defend", "asset": home.id.0, "phase": {"engage": {"focus": null}}},
                "since": 0,
                "units": members,
                "goal": {"x": 8, "y": 5},
            }],
        });
    });
    let commands = opponent.act(&state, &mut OwnEvents::default());
    let beside: Vec<TilePos> =
        crate::frame::ring(home.anchor, BuildingKind::Foundry.size()).collect();
    assert!(
        runs(&commands)
            .iter()
            .any(|(units, goal)| units.contains(&chaser) && beside.contains(goal)),
        "{commands:?}"
    );
    assert!(
        runs(&commands)
            .iter()
            .all(|(units, _)| !units.contains(&near)),
        "{commands:?}"
    );
}

#[test]
fn a_structure_hit_by_fire_nobody_sees_draws_a_search_and_no_welder() {
    // An East Lancer shoots a West Extractor from beyond what the Extractor
    // sees. Defenders worth it advance beside it to find the shooter,
    // without chasing anything on the way, and no worker is sent to weld it.
    let mut scenario = field();
    scenario.map[4].replace_range(12..13, "E");
    // Enough for the weld, not for a Turret, which would take the worker.
    scenario.players[0].scrap = 60;
    scenario.units.extend(
        GARRISON
            .iter()
            .map(|(x, y)| unit(0, UnitKind::Sentinel, *x, *y)),
    );
    scenario.units.push(harvester(0, 5, 9));
    scenario.units.push(unit(1, UnitKind::Lancer, 19, 5));
    scenario
        .buildings
        .push(building(0, BuildingKind::Extractor, 12, 4));
    let mut state = scenario.build().unwrap();
    let extractor = state
        .buildings()
        .iter()
        .find(|b| b.kind == BuildingKind::Extractor)
        .unwrap()
        .id;
    let lancer = at(&state, 19, 5);
    state.tick(&[PlayerCommand {
        player: PlayerId(1),
        command: Command::Attack {
            units: vec![lancer],
            target: AttackTarget::Building(extractor),
            queue: false,
        },
    }]);
    // Hurt enough that a welder would otherwise be sent.
    let hurt = |state: &State| {
        state.building(extractor).is_some_and(|building| {
            building.hp * 10 <= BuildingKind::Extractor.base_stats().max_hp * 7
        })
    };
    while !hurt(&state) || !state.current_tick().is_multiple_of(12) {
        assert!(state.current_tick() < 1_200, "premise: the Lancer hits it");
        state.tick(&[]);
    }
    assert!(
        ObservationData::fog_honest(&state, PlayerId(0))
            .enemy_units
            .is_empty(),
        "premise: the Lancer is out of sight"
    );
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let anchor = TilePos::new(12, 4);
    let beside: Vec<TilePos> = crate::frame::ring(anchor, BuildingKind::Extractor.size()).collect();
    assert!(
        advances(&commands)
            .iter()
            .any(|(units, goal)| !units.is_empty() && beside.contains(goal)),
        "{commands:?}"
    );
    assert!(
        commands.iter().all(|command| !matches!(
            command.command,
            Command::Repair { building, .. } if building == extractor
        )),
        "{commands:?}"
    );
}

#[test]
fn a_hit_the_seat_can_explain_draws_no_search() {
    // The same Extractor hit while a West Surveyor sees the Lancer: the hit
    // is the Lancer's, met like any threat in sight, and leaves nothing
    // unexplained. Out of sight, the same hit is unexplained.
    let unexplained = |spotted: bool| {
        let mut scenario = field();
        scenario.map[4].replace_range(12..13, "E");
        if spotted {
            scenario.units.push(unit(0, UnitKind::Surveyor, 17, 5));
        }
        scenario.units.push(unit(1, UnitKind::Lancer, 19, 5));
        scenario
            .buildings
            .push(building(0, BuildingKind::Extractor, 12, 4));
        let mut state = scenario.build().unwrap();
        let extractor = state
            .buildings()
            .iter()
            .find(|b| b.kind == BuildingKind::Extractor)
            .unwrap()
            .id;
        let lancer = at(&state, 19, 5);
        state.tick(&[PlayerCommand {
            player: PlayerId(1),
            command: Command::Attack {
                units: vec![lancer],
                target: AttackTarget::Building(extractor),
                queue: false,
            },
        }]);
        let full = BuildingKind::Extractor.base_stats().max_hp;
        while state.building(extractor).is_some_and(|b| b.hp == full) {
            assert!(state.current_tick() < 600, "premise: the Lancer hits it");
            state.tick(&[]);
        }
        let observation = ObservationData::fog_honest(&state, PlayerId(0));
        assert_eq!(
            observation.enemy_units.is_empty(),
            !spotted,
            "premise: the Lancer is in sight only when spotted"
        );
        crate::placement::unexplained(&observation, &map(&scenario)).len()
    };
    assert_eq!(unexplained(true), 0);
    assert_eq!(unexplained(false), 1);
}

#[test]
fn an_aircraft_defender_chased_past_the_base_runs_back_too() {
    // A Buzzard in the defense has followed an earlier raider out past the
    // base: hunting, it would chase on, so it is brought back like a ground
    // defender.
    let scenario = raided(
        &[(UnitKind::Sentinel, 7, 6), (UnitKind::Buzzard, 21, 9)],
        &[(UnitKind::Sentinel, 8, 5)],
    );
    let state = scenario.build().unwrap();
    let home = state
        .buildings()
        .iter()
        .find(|b| b.player == PlayerId(0) && b.kind == BuildingKind::Foundry)
        .unwrap();
    let (near, buzzard) = (at(&state, 7, 6), at(&state, 21, 9));
    let mut members = vec![near, buzzard];
    members.sort_unstable();
    let mut opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
        json["missions"] = serde_json::json!({
            "next": 1,
            "list": [{
                "id": 0,
                "task": {"task": "defend", "asset": home.id.0, "phase": {"engage": {"focus": null}}},
                "since": 0,
                "units": members,
                "goal": {"x": 8, "y": 5},
            }],
        });
    });
    let commands = opponent.act(&state, &mut OwnEvents::default());
    assert!(
        runs(&commands)
            .iter()
            .any(|(units, _)| units.contains(&buzzard)),
        "{commands:?}"
    );
}
