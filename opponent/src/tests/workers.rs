use super::*;
use crate::decision::Producer;
use crate::frame::HomeFrame;
use crate::profile::ResolvedProfile;
use chassis::grid::as_index;

/// The field with a scrap node beside West's Foundry and another at `far`.
fn fielded(far: (i32, i32)) -> Scenario {
    let mut scenario = field();
    for (x, y) in [(6, 11), far] {
        scenario.map[as_index(y)].replace_range(as_index(x)..=as_index(x), "s");
    }
    scenario
}

/// West's worked nodes with their crews in `state`, at `stance` and `greed`.
fn crews(
    scenario: &Scenario,
    state: &State,
    stance: BotStance,
    greed: u8,
) -> Vec<(TilePos, usize)> {
    let observation = ObservationData::fog_honest(state, PlayerId(0));
    let model = map(scenario);
    let frame = HomeFrame::of(&observation, &model).unwrap();
    let mut profile = ResolvedProfile::resolve(BotConfig::new(BotDifficulty::Standard, stance, 11));
    profile.traits.greed = greed;
    let foundries: Vec<Producer<'_>> = observation
        .my_buildings
        .iter()
        .filter(|building| building.kind == BuildingKind::Foundry && building.built)
        .map(|building| Producer {
            building,
            ready: true,
        })
        .collect();
    let mut memory = crate::memory::Memory::default();
    memory.observe(&observation);
    let scratch = crate::missions::Scratch::new(
        &observation,
        &model,
        frame,
        &memory,
        stance,
        &crate::missions::Missions::default(),
    );
    crate::workers::staffing(&observation, &model, frame, &profile, &foundries, &scratch)
        .crews()
        .to_vec()
}

/// The field split by a rock wall at x = 20, open only at (20, 3), with a
/// scrap node at (30, 3) beyond it, an East Turret beside the gap when
/// `guarded`, and a West Kestrel that has seen both.
fn walled(guarded: bool) -> Scenario {
    let mut scenario = field();
    for (y, row) in scenario.map.iter_mut().enumerate() {
        if (1..=22).contains(&y) && y != 3 {
            row.replace_range(20..21, "#");
        }
    }
    scenario.map[3].replace_range(30..31, "s");
    scenario.units.push(unit(0, UnitKind::Kestrel, 25, 4));
    if guarded {
        scenario.buildings.push(BuildingSpec {
            player: 1,
            kind: BuildingKind::Turret,
            x: 19,
            y: 6,
        });
    }
    scenario
}

#[test]
fn a_node_whose_route_runs_past_a_known_turret_is_not_worked() {
    let worked = |guarded| {
        let scenario = walled(guarded);
        let state = scenario.build().unwrap();
        crew(&crews(&scenario, &state, BotStance::Turtle, 50), (30, 3))
    };
    assert!(worked(false).is_some(), "premise: the open route is worked");
    assert_eq!(worked(true), None, "the only route passes the Turret");
}

#[test]
fn an_idle_harvester_is_not_sent_past_a_known_turret() {
    let sent = |guarded| {
        let mut scenario = walled(guarded);
        scenario.units.push(harvester(0, 5, 11));
        let state = scenario.build().unwrap();
        let mut opponent = seat_with(&scenario, 0, fortified());
        harvests(&opponent.act(&state, &mut OwnEvents::default()))
            .into_iter()
            .any(|(node, _)| node == TilePos::new(30, 3))
    };
    assert!(sent(false), "premise: an open route draws the Harvester");
    assert!(!sent(true));
}

/// The field split by a staggered rock wall, at x = 20 down to y = 10 and at
/// x = 19 below, so (19, 10) and (20, 11) touch only across a corner; the
/// wall opens at (19, 20). A scrap node at (21, 9) lies beyond it, an East
/// Turret covers the opening when `guarded`, and a West Kestrel has seen
/// both.
fn staggered(guarded: bool) -> Scenario {
    let mut scenario = field();
    for (y, row) in scenario.map.iter_mut().enumerate() {
        match y {
            1..=10 => row.replace_range(20..21, "#"),
            11..=22 if y != 20 => row.replace_range(19..20, "#"),
            _ => {}
        }
    }
    scenario.map[9].replace_range(21..22, "s");
    scenario.units.push(unit(0, UnitKind::Kestrel, 25, 14));
    if guarded {
        scenario.buildings.push(BuildingSpec {
            player: 1,
            kind: BuildingKind::Turret,
            x: 21,
            y: 19,
        });
    }
    scenario
}

#[test]
fn a_route_never_slips_past_danger_across_a_blocked_corner() {
    let worked = |guarded| {
        let scenario = staggered(guarded);
        let state = scenario.build().unwrap();
        crew(&crews(&scenario, &state, BotStance::Turtle, 50), (21, 9))
    };
    assert!(worked(false).is_some(), "premise: the open route is worked");
    assert_eq!(
        worked(true),
        None,
        "the only legal route runs past the Turret"
    );
}

#[test]
fn a_harvester_whose_route_turns_dangerous_is_called_home() {
    let mut scenario = walled(true);
    scenario.units.push(harvester(0, 5, 11));
    let mut state = scenario.build().unwrap();
    let worker = at(&state, 5, 11);
    state.tick(&[PlayerCommand {
        player: PlayerId(0),
        command: Command::Harvest {
            units: vec![worker],
            node: TilePos::new(30, 3),
            queue: false,
        },
    }]);
    let mut opponent = seat_with(&scenario, 0, fortified());
    while !opponent.decision_due(&state) {
        state.tick(&[]);
    }
    let commands = opponent.act(&state, &mut OwnEvents::default());
    assert!(
        runs(&commands)
            .iter()
            .any(|(units, _)| units.contains(&worker)),
        "{commands:?}"
    );
}

/// `state` with `unit` carrying `scrap`.
fn loaded(state: &State, unit: UnitId, scrap: u32) -> State {
    let mut value = serde_json::to_value(state).unwrap();
    let units = value["units"].as_array_mut().unwrap();
    let entry = units
        .iter_mut()
        .find(|entry| entry["id"] == unit.0)
        .unwrap();
    entry["worker"]["carrying"] = scrap.into();
    serde_json::from_value(value).unwrap()
}

fn returns(commands: &[PlayerCommand]) -> Vec<UnitId> {
    commands
        .iter()
        .filter_map(|command| match &command.command {
            Command::ReturnCargo { units, .. } => Some(units.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// West's commands for an idle Harvester at (5, 11) carrying `scrap` when
/// its only node lies past a known Turret.
fn nowhere_to_harvest(scrap: u32) -> (UnitId, Vec<PlayerCommand>) {
    let mut scenario = walled(true);
    scenario.units.push(harvester(0, 5, 11));
    let mut state = scenario.build().unwrap();
    let worker = at(&state, 5, 11);
    let mut opponent = seat_with(&scenario, 0, fortified());
    while !opponent.decision_due(&state) {
        state.tick(&[]);
    }
    let state = loaded(&state, worker, scrap);
    (worker, opponent.act(&state, &mut OwnEvents::default()))
}

#[test]
fn an_idle_harvester_with_no_safe_node_delivers_its_cargo() {
    let (worker, commands) = nowhere_to_harvest(7);
    assert_eq!(returns(&commands), [worker], "{commands:?}");
}

#[test]
fn an_empty_idle_harvester_with_no_safe_node_is_not_sent_to_deliver() {
    let (worker, commands) = nowhere_to_harvest(0);
    assert!(!returns(&commands).contains(&worker), "{commands:?}");
}

#[test]
fn a_loaded_harvester_whose_route_turns_dangerous_delivers_instead_of_running() {
    let mut scenario = walled(true);
    scenario.units.push(harvester(0, 5, 11));
    let mut state = scenario.build().unwrap();
    let worker = at(&state, 5, 11);
    state.tick(&[PlayerCommand {
        player: PlayerId(0),
        command: Command::Harvest {
            units: vec![worker],
            node: TilePos::new(30, 3),
            queue: false,
        },
    }]);
    let mut opponent = seat_with(&scenario, 0, fortified());
    while !opponent.decision_due(&state) {
        state.tick(&[]);
    }
    let state = loaded(&state, worker, 7);
    let commands = opponent.act(&state, &mut OwnEvents::default());
    assert_eq!(returns(&commands), [worker], "{commands:?}");
    assert!(
        !runs(&commands)
            .iter()
            .any(|(units, _)| units.contains(&worker)),
        "{commands:?}"
    );
}

#[test]
fn contested_scrap_with_a_clear_route_is_worked() {
    // Nearer East's Foundry than West's, but nothing known guards it.
    let mut scenario = fielded((30, 11));
    scenario.units.push(unit(0, UnitKind::Kestrel, 29, 10));
    let state = scenario.build().unwrap();
    let worked = crews(&scenario, &state, BotStance::Turtle, 50);
    assert!(crew(&worked, (30, 11)).is_some(), "{worked:?}");
}

fn crew(crews: &[(TilePos, usize)], node: (i32, i32)) -> Option<usize> {
    crews
        .iter()
        .find(|(worked, _)| *worked == TilePos::new(node.0, node.1))
        .map(|(_, crew)| *crew)
}

#[test]
fn a_node_beside_the_foundry_takes_more_than_two_and_a_far_one_waits_for_a_long_horizon() {
    let mut scenario = fielded((22, 3));
    // A Kestrel has seen the far node.
    scenario.units.push(unit(0, UnitKind::Kestrel, 21, 4));
    let state = scenario.build().unwrap();
    let balanced = crews(&scenario, &state, BotStance::Balanced, 50);
    assert!(
        crew(&balanced, (6, 11)).is_some_and(|crew| crew > 2),
        "{balanced:?}"
    );
    let hasty = crews(&scenario, &state, BotStance::Aggressive, 0);
    assert!(crew(&hasty, (6, 11)).is_some(), "{hasty:?}");
    assert_eq!(
        crew(&hasty, (22, 3)),
        None,
        "too far to repay a Harvester within an aggressive horizon"
    );
    assert!(
        crew(&balanced, (22, 3)).is_some(),
        "but within a balanced one: {balanced:?}"
    );
}

#[test]
fn an_economic_seat_fills_its_nodes_fuller_than_an_aggressive_one() {
    let scenario = fielded((10, 11));
    let state = scenario.build().unwrap();
    let wanted = |stance, greed| -> usize {
        crews(&scenario, &state, stance, greed)
            .iter()
            .map(|(_, crew)| crew)
            .sum()
    };
    assert!(wanted(BotStance::Turtle, 50) > wanted(BotStance::Aggressive, 50));
    assert!(wanted(BotStance::Balanced, 100) > wanted(BotStance::Balanced, 0));
}

#[test]
fn a_node_running_out_wants_no_more_workers_than_its_scrap_repays() {
    let scenario = fielded((10, 11));
    let mut state = scenario.build().unwrap();
    let node = TilePos::new(6, 11);
    let price = UnitKind::Harvester.stats().cost;
    let left = |state: &State| {
        ObservationData::fog_honest(state, PlayerId(0))
            .known_scrap
            .iter()
            .find(|(tile, _)| *tile == node)
            .map_or(0, |(_, amount)| *amount)
    };
    let full = crew(&crews(&scenario, &state, BotStance::Turtle, 100), (6, 11)).unwrap();
    // Mine the node down with a staged crew until only a few Harvesters'
    // worth of scrap is left.
    let mut spec = scenario.clone();
    spec.units.extend((5..=7).map(|y| harvester(0, 5, y + 5)));
    let mut state_mined = spec.build().unwrap();
    let miners: Vec<UnitId> = seat_units(&state_mined, PlayerId(0));
    state_mined.tick(&[PlayerCommand {
        player: PlayerId(0),
        command: Command::Harvest {
            units: miners,
            node,
            queue: false,
        },
    }]);
    while left(&state_mined) > 3 * price {
        assert!(
            state_mined.current_tick() < 20_000,
            "premise: the node is mined"
        );
        state_mined.tick(&[]);
    }
    state = state_mined;
    let repaid = (left(&state) / price) as usize;
    let crews = crews(&spec, &state, BotStance::Turtle, 100);
    let mined = crew(&crews, (6, 11)).unwrap_or(0);
    assert!(
        mined <= repaid.max(1) && mined < full,
        "{mined} of {full}, {repaid} repaid"
    );
}

#[test]
fn a_node_goes_to_the_nearest_foundry() {
    let mut scenario = fielded((22, 11));
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 19,
        y: 11,
    });
    let state = scenario.build().unwrap();
    // Too far from the home Foundry for an aggressive horizon, but beside the
    // second one.
    let worked = crews(&scenario, &state, BotStance::Aggressive, 0);
    assert!(crew(&worked, (22, 11)).is_some(), "{worked:?}");
}

#[test]
fn memory_keeps_every_enemy_unit_in_sight_beyond_a_hundred_and_twenty_eight() {
    let mut scenario = field();
    for x in 8..=22 {
        for y in 4..=18 {
            scenario.units.push(unit(1, UnitKind::Sentinel, x, y));
        }
    }
    for (x, y) in [(10, 6), (10, 16), (20, 6), (20, 16)] {
        scenario.units.push(unit(0, UnitKind::Kestrel, x, y));
    }
    let state = scenario.build().unwrap();
    let seen = ObservationData::fog_honest(&state, PlayerId(0))
        .enemy_units
        .len();
    assert!(seen > 128, "premise: {seen} in sight");
    let mut opponent = seat(&scenario, 0);
    opponent.act(&state, &mut OwnEvents::default());
    let json = serde_json::to_value(opponent.checkpoint()).unwrap();
    assert_eq!(json["memory"]["units"].as_array().unwrap().len(), seen);
    let checkpoint: Checkpoint = serde_json::from_value(json).unwrap();
    assert!(Opponent::restore(&checkpoint, &scenario, &state, map(&scenario)).is_ok());
}

/// `player`'s own events holding one `no_route` stall of `unit` where it
/// stands in `state`.
fn no_route(state: &State, player: u8, unit: UnitId) -> OwnEvents {
    let mut events = OwnEvents::default();
    events.record(
        PlayerId(player),
        &[oxide_sim::Event::OrderStalled {
            unit,
            player: PlayerId(player),
            pos: state.unit(unit).unwrap().pos,
            reason: oxide_sim::StallReason::NoRoute,
        }],
    );
    events
}

/// Units `commands` send to harvest.
fn harvesting(commands: &[PlayerCommand]) -> Vec<UnitId> {
    harvests(commands)
        .into_iter()
        .flat_map(|(_, units)| units)
        .collect()
}

#[test]
fn a_harvester_that_found_no_route_sits_out_until_a_while_passes() {
    let scenario = arena(0);
    let mut state = scenario.build().unwrap();
    let (stuck, free) = (at(&state, 7, 6), at(&state, 6, 6));
    let mut opponent = seat_with(&scenario, 0, thrifty());
    let first = harvesting(&opponent.act(&state, &mut no_route(&state, 0, stuck)));
    assert!(
        first.contains(&free) && !first.contains(&stuck),
        "{first:?}"
    );
    let checkpoint = opponent.checkpoint();
    let json = serde_json::to_value(&checkpoint).unwrap();
    assert_eq!(json["memory"]["stuck"][0]["unit"], stuck.0, "{json}");
    let mut opponent = Opponent::restore(&checkpoint, &scenario, &state, map(&scenario)).unwrap();
    advance_to(&mut state, 120, &[]);
    let later = harvesting(&opponent.act(&state, &mut OwnEvents::default()));
    assert!(!later.contains(&stuck), "still where it stalled: {later:?}");
    advance_to(&mut state, 1_200, &[]);
    let retried = harvesting(&opponent.act(&state, &mut OwnEvents::default()));
    assert!(retried.contains(&stuck), "{retried:?}");
}

#[test]
fn a_stuck_harvester_that_moves_is_ordered_again() {
    let scenario = arena(0);
    let mut state = scenario.build().unwrap();
    let stuck = at(&state, 7, 6);
    let mut opponent = seat_with(&scenario, 0, thrifty());
    opponent.act(&state, &mut no_route(&state, 0, stuck));
    advance_to(&mut state, 120, &[run(0, vec![stuck], 10, 9)]);
    assert_ne!(
        state.unit(stuck).unwrap().tile(),
        TilePos::new(7, 6),
        "premise"
    );
    let moved = harvesting(&opponent.act(&state, &mut OwnEvents::default()));
    assert!(moved.contains(&stuck), "{moved:?}");
}

#[test]
fn checkpoints_reject_malformed_stuck_units() {
    let validated = |stuck: serde_json::Value| {
        let memory: crate::memory::Memory = serde_json::from_value(serde_json::json!({
            "units": [],
            "failures": [],
            "abandoned": [],
            "scouted": [],
            "stuck": stuck,
        }))
        .unwrap();
        memory.validate(100, 24, 12, 0)
    };
    let entry = |unit: u32, x: i32, at: u64| serde_json::json!({"unit": unit, "tile": {"x": x, "y": 3}, "at": at});
    assert_eq!(
        validated(serde_json::json!([entry(3, 1, 90), entry(5, 2, 100)])),
        Ok(())
    );
    for stuck in [
        serde_json::json!([entry(5, 1, 90), entry(3, 2, 90)]),
        serde_json::json!([entry(3, 1, 90), entry(3, 2, 90)]),
        serde_json::json!([entry(3, 1, 101)]),
        serde_json::json!([entry(3, 99, 90)]),
    ] {
        assert!(validated(stuck.clone()).is_err(), "{stuck}");
    }
}

#[test]
fn an_allied_building_beside_a_node_takes_its_places_as_an_own_one_does() {
    let crews_beside = |owner: u8| {
        // A third seat far to the east, allied with West.
        let mut scenario = fielded((22, 3));
        scenario.map[3].replace_range(40..41, "3");
        let ally = scenario.players[1].clone();
        scenario.players.push(ally);
        for (player, team) in scenario.players.iter_mut().zip([0, 1, 0]) {
            player.team = Some(team);
        }
        for y in 10..=12 {
            scenario.buildings.push(BuildingSpec {
                player: owner,
                kind: BuildingKind::Barricade,
                x: 7,
                y,
            });
        }
        let state = scenario.build().unwrap();
        crew(&crews(&scenario, &state, BotStance::Turtle, 100), (6, 11))
    };
    let own = crews_beside(0);
    assert!(own.is_some(), "premise: the node is worked");
    assert_eq!(crews_beside(2), own);
}

/// `state` with each `(x, y, salvage)` of `wreck` lying on the map.
fn wrecked(state: &State, wreck: &[(i32, i32, u32)]) -> State {
    let mut value = serde_json::to_value(state).unwrap();
    let grid = &mut value["map"]["grid"];
    let width = grid["width"].as_i64().unwrap();
    let cells = grid["cells"].as_array_mut().unwrap();
    for (x, y, salvage) in wreck {
        let index = usize::try_from(i64::from(*y) * width + i64::from(*x)).unwrap();
        cells[index]["wreck"] = (*salvage).into();
    }
    serde_json::from_value(value).unwrap()
}

/// West's commands for a field with no scrap left, an idle Harvester beside
/// the Foundry and `wreck` on the map.
fn salvaging(wreck: &[(i32, i32, u32)]) -> (UnitId, Vec<PlayerCommand>) {
    let mut scenario = field();
    scenario.units.push(harvester(0, 5, 11));
    let state = scenario.build().unwrap();
    let worker = at(&state, 5, 11);
    let state = wrecked(&state, wreck);
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    (worker, commands)
}

#[test]
fn an_idle_harvester_with_no_node_left_salvages_known_wreck() {
    let (_, bare) = salvaging(&[]);
    assert!(harvests(&bare).is_empty(), "premise: {bare:?}");
    let (worker, commands) = salvaging(&[(9, 12, 30)]);
    assert_eq!(
        harvests(&commands),
        [(TilePos::new(9, 12), vec![worker])],
        "{commands:?}"
    );
}

#[test]
fn unseen_wreck_is_never_salvaged() {
    let mut scenario = field();
    scenario.units.push(harvester(0, 5, 11));
    let state = wrecked(&scenario.build().unwrap(), &[(30, 11, 400)]);
    let fogged = ObservationData::fog_honest(&state, PlayerId(0));
    let omniscient = ObservationData::omniscient(&state, PlayerId(0));
    assert!(fogged.known_wrecks.is_empty());
    assert_eq!(omniscient.known_wrecks.len(), 1, "premise: the wreck lies");
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    assert!(harvests(&commands).is_empty(), "{commands:?}");
}

#[test]
fn wreck_past_a_known_turret_is_not_salvaged() {
    let sent = |guarded| {
        let mut scenario = walled(guarded);
        scenario.map[3].replace_range(30..31, ".");
        scenario.units.push(harvester(0, 5, 11));
        let state = wrecked(&scenario.build().unwrap(), &[(30, 3, 400)]);
        let mut opponent = seat_with(&scenario, 0, fortified());
        harvests(&opponent.act(&state, &mut OwnEvents::default()))
            .into_iter()
            .any(|(source, _)| source == TilePos::new(30, 3))
    };
    assert!(sent(false), "premise: an open route draws the Harvester");
    assert!(!sent(true));
}

#[test]
fn a_harvester_working_known_wreck_is_left_to_it() {
    // A worked node beside the Foundry would draw a stale worker.
    let mut scenario = fielded((10, 11));
    scenario.units.push(harvester(0, 8, 14));
    let state = scenario.build().unwrap();
    let worker = at(&state, 8, 14);
    let mut state = wrecked(&state, &[(9, 14, 300)]);
    state.tick(&[PlayerCommand {
        player: PlayerId(0),
        command: Command::Harvest {
            units: vec![worker],
            node: TilePos::new(9, 14),
            queue: false,
        },
    }]);
    let observed = ObservationData::fog_honest(&state, PlayerId(0));
    let unit = observed
        .my_units
        .iter()
        .find(|unit| unit.id == worker)
        .unwrap();
    assert_eq!(unit.harvesting, Some(TilePos::new(9, 14)), "premise");
    let mut opponent = seat_with(&scenario, 0, thrifty());
    while !opponent.decision_due(&state) {
        state.tick(&[]);
    }
    let commands = opponent.act(&state, &mut OwnEvents::default());
    assert!(!harvesting(&commands).contains(&worker), "{commands:?}");
    assert!(
        !runs(&commands)
            .iter()
            .any(|(units, _)| units.contains(&worker)),
        "{commands:?}"
    );
}

#[test]
fn wreck_worth_a_harvester_is_worked_and_less_is_not() {
    let scenario = field();
    let price = UnitKind::Harvester.stats().cost;
    let state = wrecked(
        &scenario.build().unwrap(),
        &[(9, 9, 8 * price), (9, 14, price / 2)],
    );
    let worked = crews(&scenario, &state, BotStance::Turtle, 50);
    assert!(crew(&worked, (9, 9)).is_some(), "{worked:?}");
    assert_eq!(crew(&worked, (9, 14)), None, "{worked:?}");
}

#[test]
fn a_seat_with_its_fields_dry_trains_harvesters_for_wreck_that_repays_them() {
    let trained = |wreck: &[(i32, i32, u32)]| {
        let mut scenario = field();
        scenario.players[0].scrap = 1_000;
        garrison(&mut scenario);
        scenario.units.push(harvester(0, 5, 11));
        let state = wrecked(&scenario.build().unwrap(), wreck);
        trains(&seat(&scenario, 0).act(&state, &mut OwnEvents::default()))
            .into_iter()
            .any(|(_, kind)| kind == UnitKind::Harvester)
    };
    assert!(
        !trained(&[]),
        "premise: one Harvester is enough without wreck"
    );
    let price = UnitKind::Harvester.stats().cost;
    assert!(trained(&[(9, 9, 8 * price)]));
}

#[test]
fn routes_traced_once_per_decision_judge_every_source_as_tracing_it_alone_does() {
    for scenario in [walled(true), staggered(true)] {
        let state = scenario.build().unwrap();
        let observation = ObservationData::fog_honest(&state, PlayerId(0));
        let model = map(&scenario);
        let frame = HomeFrame::of(&observation, &model).unwrap();
        let memory = {
            let mut memory = crate::memory::Memory::default();
            memory.observe(&observation);
            memory
        };
        let scratch = crate::missions::Scratch::new(
            &observation,
            &model,
            frame,
            &memory,
            BotStance::Turtle,
            &crate::missions::Missions::default(),
        );
        let homes: Vec<&oxide_sim::observation::BuildingObs> = observation
            .my_buildings
            .iter()
            .filter(|building| building.kind == BuildingKind::Foundry)
            .collect();
        let (width, height) = model.size();
        let tiles: Vec<TilePos> = (0..height)
            .flat_map(|y| (0..width).map(move |x| TilePos::new(x, y)))
            .collect();
        let alone: Vec<bool> = tiles
            .iter()
            .map(|tile| {
                crate::workers::Routes::new(&observation, &model, frame, &scratch, &homes)
                    .clear(*tile)
            })
            .collect();
        assert!(alone.contains(&false), "premise: some routes cross danger");
        for order in [tiles.clone(), tiles.iter().rev().copied().collect()] {
            let mut shared =
                crate::workers::Routes::new(&observation, &model, frame, &scratch, &homes);
            let mut results = std::collections::BTreeMap::new();
            for tile in &order {
                results.insert((tile.y, tile.x), shared.clear(*tile));
            }
            let shared: Vec<bool> = tiles
                .iter()
                .map(|tile| results[&(tile.y, tile.x)])
                .collect();
            assert_eq!(shared, alone);
        }
    }
}

#[test]
fn workers_beyond_every_open_place_salvage_wreck_before_crowding_a_node() {
    let salvaged = |wreck: &[(i32, i32, u32)]| {
        let mut scenario = arena(0);
        scenario.units.extend(workforce(0));
        let state = wrecked(&scenario.build().unwrap(), wreck);
        let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
        harvests(&commands)
            .into_iter()
            .find(|(source, _)| *source == TilePos::new(4, 8))
            .map_or(0, |(_, units)| units.len())
    };
    assert_eq!(salvaged(&[]), 0, "premise");
    // Less than a Harvester's price, so no worked source: only spare
    // workers take it, as many as its loads.
    let price = UnitKind::Harvester.stats().cost;
    let capacity = UnitKind::Harvester.stats().harvest.unwrap().capacity;
    assert_eq!(
        salvaged(&[(4, 8, price - 1)]),
        (price - 1).div_ceil(capacity) as usize
    );
}
