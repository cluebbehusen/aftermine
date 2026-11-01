use super::*;
use crate::runner;
use aftermine_sim::Scenario;
use aftermine_sim::scenario::ScenarioMode;

fn record_activity(ticks: u64) -> GameReplay {
    use aftermine_sim::scenario::{BuildingSpec, PlayerSpec, UnitSpec};
    use aftermine_sim::{BuildingKind, Command, Event, PlayerCommand, Target, UnitKind};
    use chassis::grid::TilePos;
    let mut map = vec![vec!['.'; 30]; 20];
    map[1][1] = '1';
    map[17][27] = '2';
    map[3][5] = 's';
    let scenario = Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Match,
        name: "statistics activity".into(),
        map: map
            .into_iter()
            .map(|row| row.into_iter().collect())
            .collect(),
        players: ["West", "East"]
            .into_iter()
            .map(|name| PlayerSpec {
                name: name.into(),
                team: None,
                scrap: 2000,
                bot: false,
                bot_config: None,
            })
            .collect(),
        units: vec![
            UnitSpec {
                player: 0,
                kind: UnitKind::Harvester,
                x: 4,
                y: 3,
            },
            UnitSpec {
                player: 0,
                kind: UnitKind::Harvester,
                x: 4,
                y: 7,
            },
            UnitSpec {
                player: 0,
                kind: UnitKind::Sentinel,
                x: 12,
                y: 12,
            },
            UnitSpec {
                player: 1,
                kind: UnitKind::Harvester,
                x: 14,
                y: 12,
            },
        ],
        buildings: Vec::<BuildingSpec>::new(),
        meta: None,
    };
    let mut state = scenario.build().unwrap();
    let units: Vec<_> = state.units().iter().map(|unit| unit.id).collect();
    let foundry = state
        .buildings()
        .iter()
        .find(|b| b.player == PlayerId(0))
        .unwrap()
        .id;
    let opening: Vec<_> = [
        Command::Harvest {
            units: vec![units[0]],
            node: TilePos::new(5, 3),
            queue: false,
        },
        Command::Build {
            units: vec![units[1]],
            kind: BuildingKind::Turret,
            anchor: TilePos::new(5, 7),
            queue: false,
            defer: false,
        },
        Command::Train {
            building: foundry,
            kind: UnitKind::Harvester,
        },
        Command::Attack {
            units: vec![units[2]],
            target: Target::Unit(units[3]).into(),
            queue: false,
        },
    ]
    .into_iter()
    .map(|command| PlayerCommand {
        player: PlayerId(0),
        command,
    })
    .collect();
    let mut replay = GameReplay::new(aftermine_sim::SIM_VERSION, "test", scenario);
    let mut events = Vec::new();
    for tick in 0..ticks {
        let commands = if tick == 0 { opening.as_slice() } else { &[] };
        for command in commands {
            replay.record(tick, command.clone());
        }
        let report = state.tick(commands);
        assert!(
            report
                .events
                .iter()
                .all(|event| !matches!(event, Event::CommandRejected { .. }))
        );
        events.extend(report.events);
    }
    if ticks >= 600 {
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::ScrapDeposited { amount, .. } if *amount > 0))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::UnitTrained { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::BuildingCompleted { .. }))
        );
        assert!(events.iter().any(|e| matches!(e, Event::UnitDied { .. })));
    }
    replay.meta.ticks = Some(ticks);
    replay
}

/// Feeds `events` to `stats` against an untouched match, and returns the
/// moments they raised.
fn accumulate(stats: &mut [PlayerStats], events: &[Event]) -> Vec<Moment> {
    let state = Scenario::skirmish().build().expect("skirmish builds");
    let mut moments = Vec::new();
    accumulate_events(&state, stats, &mut moments, events);
    moments
}

fn track_replay(replay: &GameReplay) -> MatchStats {
    let total = replay.meta.ticks.expect("recorded duration");
    let mut state = replay.setup.build().expect("scenario builds");
    let mut cursor = replay.cursor();
    let mut live = LiveMatchStats::new(&state);
    for tick in 0..total {
        let commands: Vec<_> = cursor
            .take_tick(tick)
            .iter()
            .map(|timed| timed.command.clone())
            .collect();
        let report = state.tick(&commands);
        live.observe(&state, &report.events);
    }
    live.snapshot(&state)
}

#[test]
fn a_claimed_billion_ticks_is_an_error_not_a_hang() {
    let mut scenario = Scenario::skirmish();
    crate::bench::all_bots(&mut scenario);
    let outcome = runner::run_scenario(
        &scenario,
        60,
        true,
        Some(&crate::build::BuildIdentity::default()),
    )
    .unwrap();
    let mut replay = outcome.replay.unwrap();
    replay.meta.ticks = Some(1_000_000_000);
    assert!(compute(&replay, 100).is_err());
}

#[test]
fn the_final_state_is_always_sampled() {
    let mut scenario = Scenario::skirmish();
    crate::bench::all_bots(&mut scenario);
    // 100 ticks with stride 41: without the closing sample the last
    // column would sit at tick 82 and closing numbers would be stale.
    let outcome = runner::run_scenario(
        &scenario,
        100,
        true,
        Some(&crate::build::BuildIdentity::default()),
    )
    .unwrap();
    let stats = compute(&outcome.replay.unwrap(), 41).unwrap();
    assert_eq!(stats.sample_ticks.last(), Some(&100));
    assert_eq!(stats.final_tick, 100);
}

#[test]
fn stats_recompute_identically_from_the_record() {
    let replay = record_activity(600);
    let a = compute(&replay, 100).unwrap();
    let b = compute(&replay, 100).unwrap();
    assert_eq!(a.final_tick, 600);
    assert_eq!(a.sample_ticks, b.sample_ticks);
    assert_eq!(a.sample_ticks.first(), Some(&0));
    for (pa, pb) in a.players.iter().zip(&b.players) {
        assert_eq!(pa.scrap, pb.scrap, "the record computes one truth");
        assert_eq!(pa.army_value, pb.army_value);
        assert_eq!(pa.scrap_collected, pb.scrap_collected);
        assert_eq!(pa.units_trained, pb.units_trained);
        assert_eq!(pa.buildings_completed, pb.buildings_completed);
    }
    // The fixture must have exercised nonzero activity.
    assert!(
        a.players
            .iter()
            .any(|p| p.army_value.iter().any(|&v| v > 0)),
        "somebody fielded an army"
    );
    assert!(
        a.players.iter().any(|p| p.scrap_collected > 0),
        "somebody delivered salvage"
    );
    assert!(
        a.players.iter().any(|p| p.units_trained > 0),
        "somebody completed production"
    );
}

#[test]
fn live_tracking_matches_tick_by_tick_replay_statistics() {
    let replay = record_activity(40);
    assert_eq!(track_replay(&replay), compute(&replay, 1).unwrap());
}

#[test]
fn live_tracking_stays_bounded_and_keeps_the_exact_final_tick() {
    let mut state = Scenario::skirmish().build().unwrap();
    let mut live = LiveMatchStats::new(&state);
    for _ in 0..5_000 {
        let report = state.tick(&[]);
        live.observe(&state, &report.events);
    }
    let report = live.snapshot(&state);
    assert!(report.sample_ticks.len() <= MAX_LIVE_SAMPLES + 1);
    assert_eq!(report.sample_ticks.first(), Some(&0));
    assert_eq!(report.sample_ticks.last(), Some(&5_000));
    assert_eq!(report.final_tick, 5_000);
    assert!(report.sample_ticks.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn thinned_live_totals_match_recomputed_event_totals() {
    let replay = record_activity(2_000);
    let live = track_replay(&replay);
    let recomputed = compute(&replay, 200).unwrap();

    assert_eq!(live.final_tick, recomputed.final_tick);
    assert!(
        live.sample_ticks
            .windows(2)
            .any(|ticks| ticks[1] - ticks[0] > 1),
        "the fixture must cross adaptive thinning"
    );
    assert!(live.sample_ticks.len() <= MAX_LIVE_SAMPLES + 1);
    for (actual, expected) in live.players.iter().zip(&recomputed.players) {
        assert_eq!(actual.scrap_collected, expected.scrap_collected);
        assert_eq!(actual.units_trained, expected.units_trained);
        assert_eq!(actual.buildings_completed, expected.buildings_completed);
        assert_eq!(actual.units_lost, expected.units_lost);
        assert_eq!(actual.buildings_lost, expected.buildings_lost);
        assert_eq!(actual.buildings_salvaged, expected.buildings_salvaged);
        assert_eq!(actual.units_retired, expected.units_retired);
        assert_eq!(actual.scrap.last(), expected.scrap.last());
        assert_eq!(actual.army_value.last(), expected.army_value.last());
        assert_eq!(actual.wealth.last(), expected.wealth.last());
        assert_eq!(actual.scrap_earned.last(), expected.scrap_earned.last());
        assert_eq!(actual.value_lost.last(), expected.value_lost.last());
        assert_eq!(actual.units_built, expected.units_built);
        assert_eq!(actual.structures_built, expected.structures_built);
    }
    assert_eq!(live.moments, recomputed.moments);
    assert!(
        live.players.iter().any(|player| {
            player.scrap_collected > 0 && player.units_trained > 0 && player.buildings_completed > 0
        }),
        "the thinned fixture must include real economy and construction events"
    );
}

#[test]
fn deliberate_salvage_is_not_counted_as_a_building_loss() {
    use aftermine_sim::BuildingId;
    use chassis::fx::Vec2Fx;

    let mut players = blank_players(2);
    accumulate(
        &mut players,
        &[
            Event::BuildingDestroyed {
                building: BuildingId(7),
                kind: BuildingKind::Fabricator,
                player: PlayerId(0),
                tier: 0,
                pos: Vec2Fx::ZERO,
            },
            Event::BuildingSalvaged {
                building: BuildingId(8),
                player: PlayerId(0),
                pos: Vec2Fx::ZERO,
                refund: 40,
            },
        ],
    );

    assert_eq!(players[0].buildings_lost, 1);
    assert_eq!(players[0].buildings_salvaged, 1);
    assert_eq!(players[1].buildings_lost, 0);
    assert_eq!(players[1].buildings_salvaged, 0);
}

#[test]
fn a_retirement_is_not_counted_as_a_unit_loss() {
    use aftermine_sim::{BuildingId, UnitId, UnitKind};
    use chassis::fx::Vec2Fx;

    let mut players = blank_players(2);
    accumulate(
        &mut players,
        &[Event::UnitRetired {
            unit: UnitId(3),
            kind: UnitKind::Harvester,
            player: PlayerId(1),
            pos: Vec2Fx::ZERO,
            crucible: BuildingId(2),
            hp: 30,
            refund: 25,
            cargo: 4,
        }],
    );

    assert_eq!(players[1].units_lost, 0);
    assert_eq!(players[1].units_retired, 1);
    assert_eq!(
        players[1].scrap_collected, 4,
        "the load counts as collected"
    );
    assert_eq!(players[0].units_retired, 0);
}

#[test]
fn event_counters_saturate_where_a_restored_record_left_them() {
    let state = aftermine_sim::Scenario::skirmish().build().unwrap();
    let mut live = LiveMatchStats::new(&state);
    live.stats.players[0].units_trained = u32::MAX;
    live.stats.players[0].buildings_completed = u32::MAX;
    let player = aftermine_sim::PlayerId(0);
    accumulate(
        &mut live.stats.players,
        &[
            Event::UnitTrained {
                building: aftermine_sim::BuildingId(0),
                unit: aftermine_sim::UnitId(0),
                kind: aftermine_sim::UnitKind::Harvester,
                player,
            },
            Event::BuildingCompleted {
                building: aftermine_sim::BuildingId(0),
                player,
                kind: aftermine_sim::BuildingKind::Foundry,
            },
        ],
    );
    assert_eq!(live.stats.players[0].units_trained, u32::MAX);
    assert_eq!(live.stats.players[0].buildings_completed, u32::MAX);
}

#[test]
fn drones_count_as_their_hosts_equipment_not_units() {
    let mut stats = blank_players(1);
    let player = aftermine_sim::PlayerId(0);
    accumulate(
        &mut stats,
        &[
            aftermine_sim::Event::DroneAssembled {
                unit: aftermine_sim::UnitId(1),
                player,
                docked: 1,
            },
            aftermine_sim::Event::UnitDied {
                unit: aftermine_sim::UnitId(2),
                kind: aftermine_sim::UnitKind::Drone,
                player,
                pos: chassis::grid::TilePos::new(3, 3).center(),
                grounded: false,
            },
            aftermine_sim::Event::UnitDied {
                unit: aftermine_sim::UnitId(3),
                kind: aftermine_sim::UnitKind::Talon,
                player,
                pos: chassis::grid::TilePos::new(3, 3).center(),
                grounded: false,
            },
        ],
    );
    assert_eq!((stats[0].units_trained, stats[0].units_lost), (0, 1));
}

fn died(player: PlayerId, kind: UnitKind) -> Event {
    Event::UnitDied {
        unit: aftermine_sim::UnitId(1),
        kind,
        player,
        pos: chassis::grid::TilePos::new(3, 3).center(),
        grounded: false,
    }
}

fn destroyed(player: PlayerId, kind: BuildingKind, tier: u8) -> Event {
    Event::BuildingDestroyed {
        building: aftermine_sim::BuildingId(7),
        kind,
        player,
        tier,
        pos: chassis::grid::TilePos::new(3, 3).center(),
    }
}

fn completed(player: PlayerId, kind: BuildingKind) -> Event {
    Event::BuildingCompleted {
        building: aftermine_sim::BuildingId(9_999),
        player,
        kind,
    }
}

fn trained(player: PlayerId, kind: UnitKind) -> Event {
    Event::UnitTrained {
        building: aftermine_sim::BuildingId(0),
        unit: aftermine_sim::UnitId(0),
        kind,
        player,
    }
}

#[test]
fn an_army_is_what_fights_and_wealth_is_everything_owned() {
    let state = Scenario::skirmish().build().expect("skirmish builds");
    let mut stats = blank_players(state.players().len());
    let mut ticks = Vec::new();
    sample(&state, &mut stats, &mut ticks);
    let own = |player: PlayerId| -> u32 {
        let units: u32 = state
            .units()
            .iter()
            .filter(|unit| unit.player == player)
            .map(|unit| unit.kind.stats().cost)
            .sum();
        let structures: u32 = state
            .buildings()
            .iter()
            .filter(|building| building.player == player)
            .map(|building| building.kind.invested_cost(building.tier))
            .sum();
        state.player(player).scrap + units + structures
    };
    for (seat, entry) in stats.iter().enumerate() {
        assert_eq!(entry.wealth[0], own(PlayerId::from_index(seat)));
    }

    let army: u32 = state
        .units()
        .iter()
        .filter(|unit| unit.player == PlayerId(0))
        .filter(|unit| unit.kind.stats().can_fight() || unit.kind.stats().point_defense.is_some())
        .map(|unit| unit.kind.stats().cost)
        .sum();
    assert_eq!(stats[0].army_value[0], army);
    assert!(
        state
            .units()
            .iter()
            .any(|unit| unit.player == PlayerId(0) && unit.kind == UnitKind::Harvester),
        "the opening has a worker to leave out"
    );
    assert!(
        stats[0].army_value[0] < stats[0].wealth[0],
        "workers and structures are wealth, not army"
    );
}

#[test]
fn scrap_earned_counts_every_income_and_no_refund() {
    let player = PlayerId(0);
    let mut stats = blank_players(2);
    accumulate(
        &mut stats,
        &[
            Event::ScrapDeposited {
                unit: aftermine_sim::UnitId(1),
                foundry: aftermine_sim::BuildingId(0),
                player,
                amount: 10,
            },
            Event::ScrapCredited { player, amount: 3 },
            Event::UnitRetired {
                unit: aftermine_sim::UnitId(2),
                kind: UnitKind::Harvester,
                player,
                pos: chassis::grid::TilePos::new(3, 3).center(),
                crucible: aftermine_sim::BuildingId(2),
                hp: 30,
                refund: 25,
                cargo: 5,
            },
            Event::BuildingSalvaged {
                building: aftermine_sim::BuildingId(8),
                player,
                pos: chassis::grid::TilePos::new(3, 3).center(),
                refund: 400,
            },
        ],
    );
    assert_eq!(stats[0].scrap_earned_total, 18);
    assert_eq!(stats[0].scrap_collected, 15, "deliveries alone stay apart");
    assert_eq!(stats[1].scrap_earned_total, 0);
}

#[test]
fn a_bank_with_no_spending_equals_what_it_earned() {
    // The idle Harvester keeps the seat off the recovery grant, which is not
    // income.
    let scenario: Scenario = serde_json::from_str(
        r#"{
            "name": "Income",
            "mode": "sandbox",
            "map": [
                "........................",
                "........................",
                "........................",
                "........................",
                "........................",
                "........................",
                "........................",
                "........................"
            ],
            "players": [{ "name": "Local", "scrap": 0, "bot": false }],
            "units": [{ "player": 0, "kind": "harvester", "x": 14, "y": 4 }],
            "buildings": [
                { "player": 0, "kind": "foundry", "x": 2, "y": 2 },
                { "player": 0, "kind": "reclaimer", "x": 8, "y": 2 }
            ]
        }"#,
    )
    .expect("the scene parses");
    let mut state = scenario.build().expect("the scene builds");
    let mut live = LiveMatchStats::new(&state);
    for _ in 0..u64::from(aftermine_sim::TICKS_PER_SECOND) * 120 {
        let report = state.tick(&[]);
        live.observe(&state, &report.events);
    }
    let earned = live.snapshot(&state).players[0].scrap_earned_total;
    assert!(earned > 0, "a Reclaimer and a Foundry pay in");
    assert_eq!(earned, state.player(PlayerId(0)).scrap);
}

#[test]
fn value_lost_prices_tiers_and_skips_deliberate_removals() {
    let player = PlayerId(0);
    let mut stats = blank_players(2);
    accumulate(
        &mut stats,
        &[
            died(player, UnitKind::Talon),
            died(player, UnitKind::Drone),
            destroyed(player, BuildingKind::Turret, 1),
            Event::UnitRetired {
                unit: aftermine_sim::UnitId(2),
                kind: UnitKind::Harvester,
                player,
                pos: chassis::grid::TilePos::new(3, 3).center(),
                crucible: aftermine_sim::BuildingId(2),
                hp: 30,
                refund: 25,
                cargo: 0,
            },
            Event::BuildingSalvaged {
                building: aftermine_sim::BuildingId(8),
                player,
                pos: chassis::grid::TilePos::new(3, 3).center(),
                refund: 40,
            },
        ],
    );
    assert_eq!(
        stats[0].value_lost_total,
        UnitKind::Talon.stats().cost + BuildingKind::Turret.invested_cost(1)
    );
}

#[test]
fn turning_points_mark_what_changed_the_match() {
    let west = PlayerId(0);
    let east = PlayerId(1);
    let mut stats = blank_players(2);
    let moments = accumulate(
        &mut stats,
        &[
            died(east, UnitKind::Harvester),
            died(west, UnitKind::Harvester),
            completed(west, BuildingKind::Foundry),
            completed(west, BuildingKind::Airworks),
            completed(west, BuildingKind::Airworks),
            completed(west, BuildingKind::Turret),
            trained(east, UnitKind::Skyhook),
            trained(east, UnitKind::Skyhook),
            trained(east, UnitKind::Sentinel),
            destroyed(east, BuildingKind::Foundry, 0),
        ],
    );
    let kinds: Vec<(u8, MomentKind)> = moments.iter().map(|m| (m.seat, m.kind)).collect();
    assert_eq!(
        kinds,
        [
            (1, MomentKind::FirstLoss),
            (0, MomentKind::Expansion),
            (0, MomentKind::TechStructure(BuildingKind::Airworks)),
            (1, MomentKind::TechUnit(UnitKind::Skyhook)),
            (1, MomentKind::FoundryLost),
        ]
    );
    assert_eq!(stats[0].structures_built[&BuildingKind::Airworks], 2);
    assert_eq!(stats[1].units_built[&UnitKind::Skyhook], 2);
}

#[test]
fn turning_points_stop_at_the_cap() {
    let player = PlayerId(0);
    let mut stats = blank_players(2);
    let events = vec![completed(player, BuildingKind::Foundry); MAX_MOMENTS + 10];
    assert_eq!(accumulate(&mut stats, &events).len(), MAX_MOMENTS);
    assert_eq!(
        stats[0].structures_built[&BuildingKind::Foundry],
        u32::try_from(MAX_MOMENTS + 10).unwrap(),
        "the totals keep counting"
    );
}

/// Corrupts a report for a validation test.
type Break = fn(&mut MatchStats, Moment);

#[test]
fn checkpoints_refuse_malformed_series_and_moments() {
    let state = Scenario::skirmish().build().expect("skirmish builds");
    let good = LiveMatchStats::new(&state).snapshot(&state);
    good.validate_checkpoint(&state)
        .expect("a fresh report is valid");
    let moment = Moment {
        tick: 0,
        seat: 0,
        kind: MomentKind::FirstLoss,
    };
    let broken: [(&str, Break); 6] = [
        ("wealth", |stats, _| stats.players[0].wealth.clear()),
        ("earned", |stats, _| stats.players[0].scrap_earned.clear()),
        ("lost", |stats, _| stats.players[0].value_lost.clear()),
        ("seat", |stats, moment| {
            stats.moments.push(Moment {
                seat: 200,
                ..moment
            });
        }),
        ("future", |stats, moment| {
            stats.moments.push(Moment {
                tick: stats.final_tick + 1,
                ..moment
            });
        }),
        ("cap", |stats, moment| {
            stats.moments = vec![moment; MAX_MOMENTS + 1];
        }),
    ];
    for (name, breaks) in broken {
        let mut stats = good.clone();
        breaks(&mut stats, moment);
        assert!(stats.validate_checkpoint(&state).is_err(), "{name}");
    }
    let mut unordered = good.clone();
    unordered.final_tick = 10;
    unordered.moments = vec![Moment { tick: 5, ..moment }, Moment { tick: 2, ..moment }];
    assert!(state.current_tick() < unordered.final_tick);
    assert!(unordered.validate_checkpoint(&state).is_err());
}
