use super::*;

fn island() -> Scenario {
    let mut scenario = field();
    let mut grid = vec![vec![b'~'; 64]; 34];
    for row in grid.iter_mut().take(12).skip(1) {
        for cell in row.iter_mut().take(63).skip(1) {
            *cell = b'.';
        }
    }
    for row in grid.iter_mut().take(33).skip(18) {
        for cell in row.iter_mut().take(44).skip(20) {
            *cell = b'.';
        }
    }
    grid[5][4] = b'1';
    grid[5][56] = b'2';
    for row in grid.iter_mut().take(27).skip(24) {
        for cell in row.iter_mut().take(40).skip(36) {
            *cell = b'S';
        }
    }
    grid[24][30] = b'E';
    scenario.map = grid
        .into_iter()
        .map(|row| String::from_utf8(row).unwrap())
        .collect();
    scenario.players[0].scrap = 10_000;
    scenario.units = (8..20).map(|x| harvester(0, x, 7)).collect();
    scenario.units.extend([
        unit(0, UnitKind::Skyhook, 14, 9),
        unit(0, UnitKind::Skyhook, 18, 9),
        unit(0, UnitKind::Skyhook, 22, 9),
        unit(0, UnitKind::Kestrel, 35, 22),
    ]);
    scenario
        .units
        .extend((8..20).map(|x| unit(0, UnitKind::Sentinel, x, 10)));
    scenario.buildings = vec![
        BuildingSpec {
            player: 0,
            kind: BuildingKind::Fabricator,
            x: 7,
            y: 2,
        },
        BuildingSpec {
            player: 0,
            kind: BuildingKind::Airworks,
            x: 14,
            y: 2,
        },
    ];
    scenario
}

fn own_island_foundry(state: &State) -> Option<&oxide_sim::Building> {
    state.buildings().iter().find(|b| {
        b.player == PlayerId(0) && b.kind == BuildingKind::Foundry && b.anchor.y >= 18 && b.built()
    })
}

#[test]
fn a_connected_enemy_does_not_prevent_a_resource_island_foundry() {
    let mut scenario = island();
    // The seat's army would otherwise win before one builder finishes.
    scenario.mode = ScenarioMode::Sandbox;
    let model = map(&scenario);
    assert_eq!(
        model.component(TilePos::new(4, 5)),
        model.component(TilePos::new(56, 5)),
        "the enemy is ground-connected"
    );
    let mut state = scenario.build().unwrap();
    let mut bot = seat_with(
        &scenario,
        0,
        BotConfig::new(BotDifficulty::Prime, BotStance::Aggressive, 38),
    );
    let mut events = OwnEvents::default();
    let mut loaded = Vec::new();
    let mut built_by: Vec<UnitId> = Vec::new();
    let mut settled = None;
    let mut harvested = false;
    while state.current_tick() < 4_000 {
        let commands = bot.act(&state, &mut events);
        settled = settled.or_else(|| {
            bot.missions().iter().find_map(|m| match m.kind {
                MissionKind::Settle { anchor, .. } => Some(anchor),
                _ => None,
            })
        });
        for command in &commands {
            match &command.command {
                Command::Load { units, .. } => loaded.extend(units.iter().copied().filter(|id| {
                    state
                        .units()
                        .iter()
                        .any(|u| u.id == *id && u.kind.stats().harvest.is_some())
                })),
                Command::Build {
                    units,
                    kind: BuildingKind::Foundry,
                    anchor,
                    ..
                } if anchor.y >= 18 => built_by.extend(units.iter().copied()),
                Command::Harvest { node, .. } if node.y >= 18 => harvested = true,
                _ => {}
            }
        }
        let report = state.tick(&commands);
        state.validate_invariants().unwrap();
        events.record(PlayerId(0), &report.events);
        if own_island_foundry(&state).is_some() && harvested {
            break;
        }
    }
    assert!(
        own_island_foundry(&state).is_some(),
        "tick {}, missions {:?}",
        state.current_tick(),
        bot.missions()
    );
    assert!(harvested);
    assert_eq!(loaded.len(), 1, "one builder is lifted");
    assert_eq!(built_by, loaded, "the landed builder raises the Foundry");
    assert_eq!(
        own_island_foundry(&state).map(|f| f.anchor),
        settled,
        "a quiet landing leaves its site unfailed"
    );
}

#[test]
fn a_landed_builder_can_build_an_island_frame() {
    let mut scenario = island();
    scenario.units.push(harvester(0, 28, 24));
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 24,
        y: 22,
    });
    let mut state = scenario.build().unwrap();
    let mut bot = seat(&scenario, 0);
    let mut events = OwnEvents::default();
    let frame = TilePos::new(30, 24);
    while state.current_tick() < 900 {
        let commands = bot.act(&state, &mut events);
        events.record(PlayerId(0), &state.tick(&commands).events);
        if state
            .buildings()
            .iter()
            .any(|b| b.kind == BuildingKind::Extractor && b.anchor == frame && b.built())
        {
            return;
        }
    }
    panic!("the island frame was not established: {:?}", bot.missions());
}

#[test]
fn frame_only_sites_are_valued_and_stop_drawing_redundant_foundries() {
    let mut scenario = island();
    for row in &mut scenario.map {
        *row = row.replace('S', ".");
    }
    for (x, y) in [(24, 20), (30, 20), (36, 20), (24, 27), (30, 27), (36, 27)] {
        scenario.map[y].replace_range(x..=x, "E");
    }
    let model = map(&scenario);
    let index = model
        .sites()
        .iter()
        .position(|site| site.nodes.is_empty() && site.frames.len() >= 3)
        .expect("frames alone make a site");
    let site = &model.sites()[index];
    let anchor = site.anchors[0];
    let state = scenario.build().unwrap();
    let observation = oxide_sim::observation::ObservationData::omniscient(&state, PlayerId(0));
    assert!(
        crate::expansion::value(
            &observation,
            &model,
            &crate::memory::Memory::default(),
            site,
            100
        )
        .is_some()
    );
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: anchor.x,
        y: anchor.y,
    });
    let observation = oxide_sim::observation::ObservationData::omniscient(
        &scenario.build().unwrap(),
        PlayerId(0),
    );
    assert_eq!(
        crate::expansion::value(
            &observation,
            &model,
            &crate::memory::Memory::default(),
            site,
            100
        ),
        None
    );
}

#[test]
fn settlement_checkpoints_keep_exact_ownership_during_delivery() {
    let scenario = island();
    let mut state = scenario.build().unwrap();
    let mut bot = seat(&scenario, 0);
    let mut events = OwnEvents::default();
    let mut phases = Vec::new();
    while state.current_tick() < 3_000 {
        let commands = bot.act(&state, &mut events);
        if let Some(mission) = bot.missions().iter().find(|m| {
            matches!(
                m.kind,
                MissionKind::Settle {
                    building: BuildingKind::Foundry,
                    ..
                }
            )
        }) && !phases.contains(&mission.phase)
        {
            phases.push(mission.phase);
            assert!(
                bot.protected_scrap() > 0,
                "the saving keeps its scrap while the builder travels"
            );
            let checkpoint =
                serde_json::from_str(&serde_json::to_string(&bot.checkpoint()).unwrap()).unwrap();
            let mut malformed = serde_json::to_value(&checkpoint).unwrap();
            let settlement = malformed["missions"]["list"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|m| !m["task"]["target"]["foundry"].is_null())
                .unwrap();
            settlement["task"]["target"]["foundry"]["anchor"]["x"] = (-1).into();
            assert!(
                Opponent::restore(
                    &serde_json::from_value(malformed).unwrap(),
                    &scenario,
                    &state,
                    map(&scenario)
                )
                .is_err()
            );
            let mut resumed =
                Opponent::restore(&checkpoint, &scenario, &state, map(&scenario)).unwrap();
            let mut original = bot.clone();
            let mut original_events = events.clone();
            let mut resumed_events = events.clone();
            let mut world = state.clone();
            for _ in 0..24 {
                let commands = original.act(&world, &mut original_events);
                assert_eq!(commands, resumed.act(&world, &mut resumed_events));
                let report = world.tick(&commands);
                original_events.record(PlayerId(0), &report.events);
                resumed_events.record(PlayerId(0), &report.events);
            }
        }
        events.record(PlayerId(0), &state.tick(&commands).events);
        if own_island_foundry(&state).is_some() {
            break;
        }
    }
    assert!(
        phases.contains(&Phase::Load) && phases.contains(&Phase::Fly),
        "{phases:?}"
    );
}

#[test]
fn a_flight_that_runs_late_turns_its_builder_home() {
    for covered in [false, true] {
        a_late_flight_turns_home(covered);
    }
}

/// With `covered`, known anti-air reaches every tile beside the home
/// Foundry, so the carriers have no safe pad to come down on.
fn a_late_flight_turns_home(covered: bool) {
    let mut scenario = island();
    scenario.mode = ScenarioMode::Sandbox;
    if covered {
        scenario
            .buildings
            .extend([(8, 5), (4, 9), (1, 3)].map(|(x, y)| BuildingSpec {
                player: 1,
                kind: BuildingKind::FlakTurret,
                x,
                y,
            }));
    }
    let mut state = scenario.build().unwrap();
    let id = |kind| state.units().iter().find(|u| u.kind == kind).unwrap().id;
    let (worker, carrier) = (id(UnitKind::Harvester), id(UnitKind::Skyhook));
    let order = |command| PlayerCommand {
        player: PlayerId(0),
        command,
    };
    state.tick(&[order(Command::Load {
        units: vec![worker],
        transport: carrier,
        queue: false,
    })]);
    while state.current_tick() < 2_400 {
        state.tick(&[]);
    }
    state.tick(&[order(Command::Run {
        units: vec![carrier],
        goal: TilePos::new(34, 25),
        queue: false,
    })]);
    for _ in 0..20 {
        state.tick(&[]);
    }
    let mut bot = seat(&scenario, 0);
    let mut checkpoint = serde_json::to_value(bot.checkpoint()).unwrap();
    let mut members = [worker.0, carrier.0];
    members.sort_unstable();
    checkpoint["missions"]["next"] = 1.into();
    checkpoint["missions"]["list"] = serde_json::json!([
        {"id":0,"since":0,"units":members,"goal":{"x":34,"y":25},"task":{"task":"settle","target":{"foundry":{"site":0,"anchor":{"x":37,"y":28}}},"phase":"fly"}}
    ]);
    bot = Opponent::restore(
        &serde_json::from_value(checkpoint).unwrap(),
        &scenario,
        &state,
        map(&scenario),
    )
    .unwrap();
    let mut events = OwnEvents::default();
    for _ in 0..60 {
        let commands = bot.act(&state, &mut events);
        if commands.iter().any(|c| match &c.command {
            Command::Run { units, goal, .. } => goal.y < 12 && units.contains(&carrier),
            _ => false,
        }) {
            return;
        }
        events.record(PlayerId(0), &state.tick(&commands).events);
    }
    panic!(
        "the late flight kept going (covered {covered}): {:?}",
        bot.missions()
    );
}

#[test]
fn a_quiet_expansion_does_not_park_units_for_carriers_alone() {
    let mut scenario = island();
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 24,
        y: 22,
    });
    scenario.units.push(unit(1, UnitKind::Skyhook, 30, 21));
    scenario
        .units
        .extend((23..33).map(|x| unit(0, UnitKind::Sentinel, x, 30)));
    let state = scenario.build().unwrap();
    let mut bot = seat_with(
        &scenario,
        0,
        BotConfig::new(BotDifficulty::Prime, BotStance::Turtle, 11),
    );
    bot.act(&state, &mut OwnEvents::default());
    assert!(bot.guarded_units().is_empty());
}

#[test]
fn an_air_escort_brings_ground_attack_and_anti_air_without_loading_them() {
    let mut scenario = island();
    scenario.units.extend((25..35).flat_map(|x| {
        [
            unit(0, UnitKind::Buzzard, x, 8),
            unit(0, UnitKind::Talon, x, 9),
        ]
    }));
    scenario.units.extend([
        unit(1, UnitKind::Sentinel, 43, 30),
        unit(1, UnitKind::Buzzard, 43, 31),
    ]);
    let mut bot = seat_with(&scenario, 0, thrifty());
    let mut state = scenario.build().unwrap();
    bot = restored_with(&bot, &scenario, &state, |json| {
        json["memory"]["units"] = remembered(
            state
                .units()
                .iter()
                .filter(|u| u.player == PlayerId(1))
                .map(|u| (u.id.0, u.kind, u.tile(), 0)),
        );
    });
    let (commands, trace) = bot.act_traced(&state, &mut OwnEvents::default());
    let trace = trace.unwrap();
    let settlement = trace
        .missions
        .iter()
        .find(|m| matches!(m.kind, MissionKind::Settle { .. }))
        .expect("a settlement starts");
    let checkpoint = serde_json::to_value(bot.checkpoint()).unwrap();
    let mission = checkpoint["missions"]["list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == settlement.id)
        .unwrap();
    let owned = |kind| {
        state.units().iter().any(|u| {
            u.kind == kind
                && mission["units"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!(u.id.0))
        })
    };
    assert!(owned(UnitKind::Buzzard), "ground fire accompanies builders");
    assert!(owned(UnitKind::Talon), "fighters cover enemy aircraft");
    assert!(commands.iter().all(|c| match &c.command {
        Command::Load { units, .. } => units.iter().all(|id| {
            state
                .units()
                .iter()
                .any(|u| u.id == *id && u.kind.stats().domain == oxide_sim::stats::Domain::Ground)
        }),
        _ => true,
    }));
    let mut events = OwnEvents::default();
    events.record(PlayerId(0), &state.tick(&commands).events);
    while state.current_tick() < 1_000 {
        let commands = bot.act(&state, &mut events);
        if commands.iter().any(|c| match &c.command {
            Command::Hunt { units, .. } => {
                units.iter().any(|id| {
                    state
                        .units()
                        .iter()
                        .any(|u| u.id == *id && !u.cargo.is_empty())
                }) && units.iter().any(|id| {
                    state
                        .units()
                        .iter()
                        .any(|u| u.id == *id && u.kind.stats().can_fight())
                })
            }
            _ => false,
        }) {
            return;
        }
        events.record(PlayerId(0), &state.tick(&commands).events);
    }
    panic!("escorts never protected the loaded flight");
}

#[test]
fn low_attention_can_deliver_a_crew_across_several_decisions() {
    let scenario = island();
    let mut state = scenario.build().unwrap();
    let mut bot = seat_with(
        &scenario,
        0,
        BotConfig::new(BotDifficulty::Scrapheap, BotStance::Aggressive, 38),
    );
    let mut events = OwnEvents::default();
    while state.current_tick() < 6_000 {
        let commands = bot.act(&state, &mut events);
        events.record(PlayerId(0), &state.tick(&commands).events);
        if own_island_foundry(&state).is_some() {
            return;
        }
    }
    panic!("low attention abandoned the crew: {:?}", bot.missions());
}

#[test]
fn escorts_join_the_local_guard_once_the_site_is_placed() {
    for shared in [false, true] {
        let mut scenario = island();
        scenario.buildings.push(BuildingSpec {
            player: 0,
            kind: BuildingKind::Foundry,
            x: 24,
            y: 22,
        });
        if shared {
            scenario.buildings.push(BuildingSpec {
                player: 0,
                kind: BuildingKind::Foundry,
                x: 30,
                y: 22,
            });
        }
        scenario.units.push(unit(1, UnitKind::Sentinel, 33, 32));
        scenario
            .units
            .extend((23..43).map(|x| unit(0, UnitKind::Sentinel, x, 30)));
        let state = scenario.build().unwrap();
        let mut bot = seat_with(
            &scenario,
            0,
            BotConfig::new(BotDifficulty::Prime, BotStance::Turtle, 11),
        );
        let foundries: Vec<_> = state
            .buildings()
            .iter()
            .filter(|b| b.kind == BuildingKind::Foundry && b.anchor.y >= 18)
            .collect();
        let placed = foundries.last().unwrap();
        let escorts: Vec<_> = state
            .units()
            .iter()
            .filter(|u| u.player == PlayerId(0) && u.kind == UnitKind::Sentinel && u.tile().y >= 18)
            .map(|u| u.id.0)
            .collect();
        let mut checkpoint = serde_json::to_value(bot.checkpoint()).unwrap();
        checkpoint["missions"]["next"] = 2.into();
        checkpoint["missions"]["list"] = serde_json::json!([
            {"id":0,"since":0,"units":[escorts[0]],"goal":{"x":25,"y":26},"task":{"task":"defend","asset":foundries[0].id.0,"phase":"guard"}},
            {"id":1,"since":0,"units":escorts[1..],"goal":{"x":25,"y":26},"task":{"task":"settle","target":{"foundry":{"site":0,"anchor":placed.anchor}},"phase":"found"}}
        ]);
        bot = Opponent::restore(
            &serde_json::from_value(checkpoint).unwrap(),
            &scenario,
            &state,
            map(&scenario),
        )
        .unwrap();
        bot.act(&state, &mut OwnEvents::default());
        let missions = bot.missions();
        assert!(
            missions.iter().all(|m| m.id != 1),
            "the held landing ends once its site is placed"
        );
        assert_eq!(
            missions
                .iter()
                .filter(|m| match m.kind {
                    MissionKind::Defend { asset } => foundries.iter().any(|f| f.id == asset),
                    _ => false,
                })
                .count(),
            1,
            "one guard per base"
        );
        assert!(
            bot.guarded_units()
                .iter()
                .any(|id| escorts[1..].contains(&id.0)),
            "a released escort guards the site"
        );
        Opponent::restore(&bot.checkpoint(), &scenario, &state, map(&scenario)).unwrap();
    }
}

#[test]
fn a_recovering_defense_outlasts_a_quiet_guard() {
    let mut scenario = island();
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 24,
        y: 22,
    });
    scenario.units.push(unit(0, UnitKind::Sentinel, 26, 26));
    let state = scenario.build().unwrap();
    let foundry = own_island_foundry(&state).unwrap().id;
    let defender = state
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Sentinel && u.tile().y >= 18)
        .unwrap()
        .id;
    let mut bot = seat(&scenario, 0);
    let mut checkpoint = serde_json::to_value(bot.checkpoint()).unwrap();
    checkpoint["missions"]["next"] = 1.into();
    checkpoint["missions"]["list"] = serde_json::json!([
        {"id":0,"since":0,"units":[defender.0],"goal":{"x":25,"y":26},"task":{"task":"defend","asset":foundry.0,"phase":"recover"}}
    ]);
    bot = Opponent::restore(
        &serde_json::from_value(checkpoint).unwrap(),
        &scenario,
        &state,
        map(&scenario),
    )
    .unwrap();
    bot.act(&state, &mut OwnEvents::default());
    assert!(
        bot.missions().iter().any(
            |m| matches!(m.kind, MissionKind::Defend { asset } if asset == foundry)
                && m.phase == Phase::Recover
        ),
        "{:?}",
        bot.missions()
    );
}

#[test]
fn unfinished_defenses_do_not_replace_a_local_guard_but_completed_guns_do() {
    let mut scenario = island();
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 24,
        y: 22,
    });
    scenario.buildings.extend((25..31).map(|x| BuildingSpec {
        player: 0,
        kind: BuildingKind::Turret,
        x,
        y: 21,
    }));
    scenario.units.push(unit(1, UnitKind::Sentinel, 25, 32));
    scenario
        .units
        .extend((23..43).map(|x| unit(0, UnitKind::Sentinel, x, 30)));
    let completed = scenario.build().unwrap();
    let mut staged = serde_json::to_value(&completed).unwrap();
    for building in staged["buildings"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|b| b["kind"] == "turret")
    {
        building["phase"] = serde_json::json!({"phase":"site","progress":0});
        building["hp"] = 1.into();
    }
    let unfinished: State = serde_json::from_value(staged).unwrap();
    let config = BotConfig::new(BotDifficulty::Prime, BotStance::Turtle, 11);
    let mut before = seat_with(&scenario, 0, config);
    let mut after = seat_with(&scenario, 0, config);
    before.act(&unfinished, &mut OwnEvents::default());
    after.act(&completed, &mut OwnEvents::default());
    assert!(!before.guarded_units().is_empty());
    assert!(
        after.guarded_units().len() < before.guarded_units().len(),
        "completed coverage releases surplus"
    );
}

#[test]
fn settlement_and_military_lift_can_own_distinct_crews_at_the_same_time() {
    let mut scenario = island();
    scenario.mode = ScenarioMode::Sandbox;
    for row in scenario.map.iter_mut().take(12).skip(1) {
        row.replace_range(40..47, "~~~~~~~");
    }
    scenario
        .units
        .extend((26..38).map(|x| unit(0, UnitKind::Skyhook, x, 9)));
    scenario.units.push(unit(0, UnitKind::Kestrel, 51, 8));
    let mut state = scenario.build().unwrap();
    let mut bot = seat_with(
        &scenario,
        0,
        BotConfig::new(BotDifficulty::Prime, BotStance::Balanced, 38),
    );
    let mut events = OwnEvents::default();
    while state.current_tick() < 2_000 {
        let commands = bot.act(&state, &mut events);
        let missions = bot.missions();
        if missions
            .iter()
            .any(|m| matches!(m.kind, MissionKind::Settle { .. }))
            && missions
                .iter()
                .any(|m| matches!(m.kind, MissionKind::Lift { .. }))
        {
            Opponent::restore(&bot.checkpoint(), &scenario, &state, map(&scenario)).unwrap();
            return;
        }
        events.record(PlayerId(0), &state.tick(&commands).events);
    }
    panic!("delivery missions never overlapped: {:?}", bot.missions());
}

#[test]
fn a_later_island_expansion_reuses_carriers_from_another_component() {
    let mut scenario = island();
    scenario.mode = ScenarioMode::Sandbox;
    for row in &mut scenario.map {
        *row = row.replace('S', ".");
    }
    for row in scenario.map.iter_mut().take(33).skip(18) {
        row.replace_range(48..63, "...............");
    }
    scenario.map[24].replace_range(55..56, "E");
    scenario.map[29].replace_range(55..58, "SSS");
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 24,
        y: 22,
    });
    scenario.units.extend((22..34).map(|x| harvester(0, x, 30)));
    scenario.units.push(unit(0, UnitKind::Kestrel, 56, 22));
    let mut state = scenario.build().unwrap();
    let mut bot = seat_with(
        &scenario,
        0,
        BotConfig::new(BotDifficulty::Prime, BotStance::Aggressive, 38),
    );
    let mut events = OwnEvents::default();
    let mut reused = false;
    while state.current_tick() < 4_000 {
        let commands = bot.act(&state, &mut events);
        reused |= commands.iter().any(|c| matches!(&c.command, Command::Run { units, goal, .. } if goal.y >= 18 && goal.x < 44 && units.iter().any(|id| state.units().iter().any(|u| u.id == *id && u.kind == UnitKind::Skyhook && u.tile().y < 12))));
        events.record(PlayerId(0), &state.tick(&commands).events);
        if state.buildings().iter().any(|b| {
            b.player == PlayerId(0)
                && b.kind == BuildingKind::Foundry
                && b.anchor.x >= 48
                && b.built()
        }) {
            assert!(reused);
            return;
        }
    }
    panic!(
        "remote carriers did not establish the next island: {:?}",
        bot.missions()
    );
}

fn connected_resource_site() -> Scenario {
    let mut scenario = island();
    for (x, y) in [(24, 20), (28, 20), (32, 20), (40, 29)] {
        scenario.map[y].replace_range(x..=x, "E");
    }
    for row in scenario.map.iter_mut().take(19).skip(12) {
        row.replace_range(20..24, "....");
    }
    scenario
}

#[test]
fn mainland_construction_uses_the_ordinary_builder_path() {
    let scenario = connected_resource_site();
    let mut state = scenario.build().unwrap();
    let mut bot = seat_with(
        &scenario,
        0,
        BotConfig::new(BotDifficulty::Prime, BotStance::Balanced, 38),
    );
    let commands = bot.act(&state, &mut OwnEvents::default());
    let anchor = commands
        .iter()
        .find_map(|c| match &c.command {
            Command::Build {
                kind: BuildingKind::Foundry,
                anchor,
                units,
                ..
            } if anchor.y >= 18 => {
                assert!(
                    units
                        .iter()
                        .all(|id| state.units().iter().any(|u| u.id == *id && u.tile().y < 18)),
                    "a home worker walks to its site"
                );
                Some(*anchor)
            }
            _ => None,
        })
        .expect("a reachable site is an ordinary build");
    assert!(
        bot.missions()
            .iter()
            .all(|m| !matches!(m.kind, MissionKind::Settle { .. }))
    );
    let report = state.tick(&commands);
    assert!(
        report
            .events
            .iter()
            .all(|e| !matches!(e, Event::CommandRejected { .. }))
    );
    assert!(
        state
            .buildings()
            .iter()
            .any(|b| b.kind == BuildingKind::Foundry && b.anchor == anchor)
    );
}

#[test]
fn a_builder_already_on_an_island_needs_no_carrier_to_start_its_foundry() {
    let mut scenario = island();
    scenario.mode = ScenarioMode::Sandbox;
    scenario.units.retain(|u| u.kind != UnitKind::Skyhook);
    scenario.units.push(harvester(0, 34, 22));
    let mut state = scenario.build().unwrap();
    let mut bot = seat_with(
        &scenario,
        0,
        BotConfig::new(BotDifficulty::Prime, BotStance::Aggressive, 38),
    );
    let mut events = OwnEvents::default();
    while state.current_tick() < 1_200 {
        let commands = bot.act(&state, &mut events);
        if !state
            .buildings()
            .iter()
            .any(|b| b.player == PlayerId(0) && b.kind == BuildingKind::Foundry && b.anchor.y >= 18)
        {
            assert!(commands.iter().all(|c| !matches!(
                c.command,
                Command::Train {
                    kind: UnitKind::Skyhook,
                    ..
                } | Command::Load { .. }
            )));
        }
        events.record(PlayerId(0), &state.tick(&commands).events);
        if own_island_foundry(&state).is_some() {
            return;
        }
    }
    panic!(
        "the local builder did not found its island: {:?}",
        bot.missions()
    );
}

#[test]
fn scuttlers_can_provide_an_expansions_ground_guard() {
    let mut scenario = island();
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 24,
        y: 22,
    });
    scenario.units.push(unit(1, UnitKind::Sentinel, 38, 22));
    scenario
        .units
        .extend((23..43).map(|x| unit(0, UnitKind::Scuttler, x, 30)));
    let state = scenario.build().unwrap();
    let mut bot = seat_with(
        &scenario,
        0,
        BotConfig::new(BotDifficulty::Prime, BotStance::Turtle, 11),
    );
    let commands = bot.act(&state, &mut OwnEvents::default());
    let scuttlers: Vec<_> = state
        .units()
        .iter()
        .filter(|u| u.kind == UnitKind::Scuttler)
        .map(|u| u.id)
        .collect();
    assert!(commands.iter().any(|c| matches!(&c.command, Command::Hunt { units, goal, .. } if goal.y >= 18 && units.iter().any(|id| scuttlers.contains(id)))));
    assert!(bot.guarded_units().iter().any(|id| scuttlers.contains(id)));
}

#[test]
fn distant_armies_do_not_absorb_every_expansion_guard() {
    let mut scenario = connected_resource_site();
    scenario.buildings.extend([
        BuildingSpec {
            player: 0,
            kind: BuildingKind::Foundry,
            x: 24,
            y: 22,
        },
        BuildingSpec {
            player: 0,
            kind: BuildingKind::Foundry,
            x: 39,
            y: 22,
        },
    ]);
    scenario.units.push(unit(0, UnitKind::Kestrel, 41, 5));
    scenario
        .units
        .extend((0..45).map(|i| unit(1, UnitKind::Sentinel, 34 + i % 15, 2 + 2 * (i / 15))));
    scenario
        .units
        .extend((0..80).map(|i| unit(0, UnitKind::Sentinel, 23 + i % 20, 28 + i / 20)));
    let state = scenario.build().unwrap();
    let observation = oxide_sim::observation::ObservationData::fog_honest(&state, PlayerId(0));
    assert_eq!(observation.enemy_units.len(), 45);
    let mut bot = seat_with(
        &scenario,
        0,
        BotConfig::new(BotDifficulty::Prime, BotStance::Turtle, 11),
    );
    bot.act(&state, &mut OwnEvents::default());
    let guards = bot.guarded_units();
    assert!(guards.is_empty());
    let free_value: u64 = state
        .units()
        .iter()
        .filter(|u| {
            u.player == PlayerId(0) && u.kind.stats().can_fight() && !guards.contains(&u.id)
        })
        .map(|u| u64::from(u.kind.stats().cost))
        .sum();
    let enemy_value: u64 = observation
        .enemy_units
        .iter()
        .map(|u| u64::from(u.kind.stats().cost))
        .sum();
    assert!(
        free_value >= enemy_value,
        "local guards absorbed the force available to answer a distant army"
    );
}

#[test]
fn quiet_bases_and_lone_extractors_do_not_multiply_a_reserve_floor() {
    let mut scenario = island();
    scenario.buildings.extend([
        BuildingSpec {
            player: 0,
            kind: BuildingKind::Foundry,
            x: 24,
            y: 22,
        },
        BuildingSpec {
            player: 0,
            kind: BuildingKind::Foundry,
            x: 39,
            y: 22,
        },
        BuildingSpec {
            player: 1,
            kind: BuildingKind::Airworks,
            x: 51,
            y: 2,
        },
    ]);
    for x in [25, 30, 35, 40] {
        scenario.map[9].replace_range(x..=x, "E");
        scenario.buildings.push(BuildingSpec {
            player: 0,
            kind: BuildingKind::Extractor,
            x: i32::try_from(x).unwrap(),
            y: 9,
        });
    }
    scenario.units.push(unit(0, UnitKind::Kestrel, 53, 2));
    scenario
        .units
        .extend((23..43).map(|x| unit(0, UnitKind::Lancer, x, 30)));
    let state = scenario.build().unwrap();
    for stance in [
        BotStance::Turtle,
        BotStance::Balanced,
        BotStance::Aggressive,
    ] {
        let mut bot = seat_with(
            &scenario,
            0,
            BotConfig::new(BotDifficulty::Prime, stance, 11),
        );
        bot.act(&state, &mut OwnEvents::default());
        assert!(
            bot.guarded_units().is_empty(),
            "quiet guards for {stance:?}"
        );
    }
}

#[test]
fn an_incoming_guard_counts_before_more_defenders_are_trained() {
    let mut scenario = island();
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 24,
        y: 22,
    });
    scenario.units.push(unit(1, UnitKind::Buzzard, 38, 22));
    scenario
        .units
        .extend((8..26).map(|x| unit(0, UnitKind::Talon, x, 4)));
    let state = scenario.build().unwrap();
    let model = map(&scenario);
    let mut bot = seat_with(
        &scenario,
        0,
        BotConfig::new(BotDifficulty::Prime, BotStance::Turtle, 11),
    );
    bot.act(&state, &mut OwnEvents::default());
    let guards = bot.guarded_units();
    assert!(!guards.is_empty());
    assert!(
        guards
            .iter()
            .all(|id| state.units().iter().any(|u| u.id == *id && u.tile().y < 18))
    );
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let frame = crate::frame::HomeFrame::of(&observation, &model).unwrap();
    let scratch = crate::missions::Scratch::new(
        &observation,
        &model,
        frame,
        &bot.persistent.memory,
        bot.profile.stance,
        &bot.persistent.missions,
    );
    let mut ledger = crate::decision::Ledger::new(PlayerId(0), observation.scrap, u32::MAX);
    assert!(
        bot.persistent
            .missions
            .guard(
                &observation,
                &model,
                &scratch,
                &bot.persistent.memory,
                bot.profile.stance,
                &mut ledger,
            )
            .is_empty()
    );
    let left: u64 = observation
        .my_units
        .iter()
        .filter(|u| u.kind == UnitKind::Talon && !guards.contains(&u.id))
        .map(|u| u64::from(u.kind.stats().cost))
        .sum();
    assert!(left >= crate::missions::minimum(BotStance::Turtle));
}

#[test]
fn a_funded_island_raises_escort_capability_without_owning_future_units() {
    let mut scenario = island();
    scenario.mode = ScenarioMode::Sandbox;
    for unit in scenario
        .units
        .iter_mut()
        .filter(|u| u.kind == UnitKind::Sentinel)
    {
        unit.kind = UnitKind::Warden;
    }
    for (x, y) in [(24, 20), (28, 20), (32, 20), (40, 29)] {
        scenario.map[y].replace_range(x..=x, "E");
    }
    scenario.units.extend([
        unit(0, UnitKind::Kestrel, 24, 28),
        unit(1, UnitKind::Buzzard, 24, 31),
    ]);
    let aa = crate::missions::minimum(BotStance::Turtle)
        .div_ceil(u64::from(UnitKind::Talon.stats().cost));
    scenario
        .units
        .extend((0..aa).map(|i| unit(0, UnitKind::Talon, 8 + i32::try_from(i).unwrap(), 4)));
    let other = 12 * u64::from(UnitKind::Warden.stats().cost)
        + aa * u64::from(UnitKind::Talon.stats().cost);
    let bombers = other.div_ceil(3 * u64::from(UnitKind::Buzzard.stats().cost));
    scenario
        .units
        .extend((0..bombers).map(|i| unit(0, UnitKind::Buzzard, 8 + i32::try_from(i).unwrap(), 3)));
    let state = scenario.build().unwrap();
    let mut bot = seat_with(
        &scenario,
        0,
        BotConfig::new(BotDifficulty::Prime, BotStance::Turtle, 38),
    );
    let (commands, trace) = bot.act_traced(&state, &mut OwnEvents::default());
    assert!(
        matches!(
            trace.as_ref().unwrap().target.as_ref().unwrap().investment,
            Investment::Expansion(_)
        ),
        "{trace:?}"
    );
    assert!(
        commands.iter().any(|c| matches!(
            c.command,
            Command::Train {
                kind: UnitKind::Talon,
                ..
            }
        )),
        "{commands:?}"
    );
    assert!(
        !bot.missions()
            .iter()
            .any(|mission| matches!(mission.kind, MissionKind::Settle { .. })),
        "no settlement starts before its escort exists: {:?}",
        bot.missions()
    );
    let checkpoint = serde_json::to_value(bot.checkpoint()).unwrap();
    for mission in checkpoint["missions"]["list"].as_array().unwrap() {
        for id in mission["units"].as_array().unwrap() {
            assert!(
                state
                    .units()
                    .iter()
                    .any(|u| u64::from(u.id.0) == id.as_u64().unwrap())
            );
        }
    }
}

#[test]
fn a_seat_without_a_worker_at_any_foundry_trains_one_before_settling() {
    let mut scenario = island();
    scenario.mode = ScenarioMode::Sandbox;
    for y in 28..32 {
        scenario.map[y].replace_range(50..56, "......");
    }
    scenario.units.retain(|u| u.kind != UnitKind::Harvester);
    scenario.units.push(harvester(0, 52, 29));
    let mut state = scenario.build().unwrap();
    let mut bot = seat(&scenario, 0);
    let mut events = OwnEvents::default();
    while state.current_tick() < 1_200 {
        let (commands, trace) = bot.act_traced(&state, &mut events);
        let wanted = trace.and_then(|trace| trace.target).is_some_and(|target| {
            matches!(target.investment, Investment::Expansion(_))
                && target
                    .next
                    .is_some_and(|next| next.step == Step::Train(UnitKind::Harvester))
        });
        if wanted
            && commands.iter().any(|c| {
                matches!(
                    c.command,
                    Command::Train {
                        kind: UnitKind::Harvester,
                        ..
                    }
                )
            })
        {
            return;
        }
        events.record(PlayerId(0), &state.tick(&commands).events);
    }
    panic!("no builder was trained: {:?}", bot.missions());
}

#[test]
fn a_guard_follows_its_base_to_a_nearer_foundry() {
    let mut scenario = island();
    scenario
        .buildings
        .extend([(24, 22), (30, 22)].map(|(x, y)| BuildingSpec {
            player: 0,
            kind: BuildingKind::Foundry,
            x,
            y,
        }));
    scenario.units.push(unit(1, UnitKind::Sentinel, 33, 32));
    scenario
        .units
        .extend((23..43).map(|x| unit(0, UnitKind::Sentinel, x, 30)));
    let state = scenario.build().unwrap();
    let foundry = |x| {
        state
            .buildings()
            .iter()
            .find(|b| b.kind == BuildingKind::Foundry && b.anchor == TilePos::new(x, 22))
            .unwrap()
            .id
    };
    let (near, far) = (foundry(24), foundry(30));
    let guards: Vec<_> = state
        .units()
        .iter()
        .filter(|u| u.player == PlayerId(0) && u.kind == UnitKind::Sentinel && u.tile().y >= 18)
        .take(3)
        .map(|u| u.id.0)
        .collect();
    let mut bot = seat_with(
        &scenario,
        0,
        BotConfig::new(BotDifficulty::Prime, BotStance::Turtle, 11),
    );
    let mut checkpoint = serde_json::to_value(bot.checkpoint()).unwrap();
    checkpoint["missions"]["next"] = 1.into();
    checkpoint["missions"]["list"] = serde_json::json!([
        {"id":0,"since":0,"units":guards,"goal":{"x":32,"y":24},"task":{"task":"defend","asset":far.0,"phase":"guard"}}
    ]);
    bot = Opponent::restore(
        &serde_json::from_value(checkpoint).unwrap(),
        &scenario,
        &state,
        map(&scenario),
    )
    .unwrap();
    bot.act(&state, &mut OwnEvents::default());
    let defends: Vec<_> = bot
        .missions()
        .iter()
        .filter_map(|m| match m.kind {
            MissionKind::Defend { asset } => Some(asset),
            _ => None,
        })
        .collect();
    assert_eq!(defends, [near], "one guard, for the Foundry nearer home");
}

#[test]
fn an_escort_leaves_a_landing_no_known_enemy_threatens() {
    let mut scenario = island();
    scenario.units.push(harvester(0, 34, 25));
    scenario
        .units
        .extend((30..34).map(|x| unit(0, UnitKind::Sentinel, x, 26)));
    let state = scenario.build().unwrap();
    let escorts: Vec<_> = state
        .units()
        .iter()
        .filter(|u| u.player == PlayerId(0) && u.kind == UnitKind::Sentinel && u.tile().y >= 18)
        .map(|u| u.id.0)
        .collect();
    let mut bot = seat(&scenario, 0);
    let mut checkpoint = serde_json::to_value(bot.checkpoint()).unwrap();
    checkpoint["missions"]["next"] = 1.into();
    checkpoint["missions"]["list"] = serde_json::json!([
        {"id":0,"since":0,"units":escorts,"goal":{"x":34,"y":25},"task":{"task":"settle","target":{"foundry":{"site":0,"anchor":{"x":37,"y":28}}},"phase":"found"}}
    ]);
    bot = Opponent::restore(
        &serde_json::from_value(checkpoint).unwrap(),
        &scenario,
        &state,
        map(&scenario),
    )
    .unwrap();
    bot.act(&state, &mut OwnEvents::default());
    assert!(
        bot.missions()
            .iter()
            .all(|m| !matches!(m.kind, MissionKind::Settle { .. })),
        "{:?}",
        bot.missions()
    );
}

#[test]
fn local_guards_train_while_a_lift_waits_for_carriers() {
    let mut scenario = island();
    scenario.mode = ScenarioMode::Sandbox;
    // Sever the starts so a lift is needed, with no carriers yet and too
    // little scrap for one.
    for row in scenario.map.iter_mut().take(12).skip(1) {
        row.replace_range(30..34, "~~~~");
    }
    scenario.units.retain(|u| u.kind != UnitKind::Skyhook);
    for row in &mut scenario.map {
        *row = row.replace(['S', 'E'], ".");
    }
    scenario.players[0].scrap = 200;
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 24,
        y: 22,
    });
    // Near enough for the expansion's guard, too far to call its defense.
    scenario.units.push(unit(1, UnitKind::Buzzard, 36, 23));
    let state = scenario.build().unwrap();
    let mut bot = seat_with(
        &scenario,
        0,
        BotConfig::new(BotDifficulty::Prime, BotStance::Aggressive, 11),
    );
    let commands = bot.act(&state, &mut OwnEvents::default());
    assert!(
        commands.iter().any(|c| matches!(
            c.command,
            Command::Train {
                kind: UnitKind::Talon,
                ..
            }
        )),
        "{commands:?}"
    );
}

#[test]
fn an_empty_carrier_already_home_is_let_go_where_it_stands() {
    let mut scenario = island();
    scenario.units.retain(|u| u.kind != UnitKind::Skyhook);
    scenario.units.push(unit(0, UnitKind::Skyhook, 6, 7));
    let state = scenario.build().unwrap();
    let carrier = state
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Skyhook)
        .unwrap()
        .id;
    let mut bot = seat(&scenario, 0);
    let mut checkpoint = serde_json::to_value(bot.checkpoint()).unwrap();
    checkpoint["missions"]["next"] = 1.into();
    checkpoint["missions"]["list"] = serde_json::json!([
        {"id":0,"since":0,"units":[carrier.0],"goal":{"x":34,"y":25},"task":{"task":"settle","target":{"foundry":{"site":0,"anchor":{"x":37,"y":28}}},"phase":"recover"}}
    ]);
    bot = Opponent::restore(
        &serde_json::from_value(checkpoint).unwrap(),
        &scenario,
        &state,
        map(&scenario),
    )
    .unwrap();
    let commands = bot.act(&state, &mut OwnEvents::default());
    assert!(
        commands.iter().all(|c| match &c.command {
            Command::Run { units, .. } | Command::Hunt { units, .. } => !units.contains(&carrier),
            _ => true,
        }),
        "{commands:?}"
    );
    assert!(
        bot.missions().iter().all(|m| m.id != 0),
        "the recovered settlement ends"
    );
}

#[test]
fn a_base_with_nothing_left_to_mine_keeps_a_builder() {
    let mut scenario = island();
    for row in &mut scenario.map {
        *row = row.replace('S', ".");
    }
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 24,
        y: 22,
    });
    let state = scenario.build().unwrap();
    let island = own_island_foundry(&state).unwrap().id;
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    assert!(
        commands.iter().any(|c| matches!(
            c.command,
            Command::Train {
                building,
                kind: UnitKind::Harvester,
            } if building == island
        )),
        "{commands:?}"
    );
}
