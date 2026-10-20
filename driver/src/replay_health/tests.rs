use super::*;
use chassis::replay::Replay;
use oxide_sim::scenario::{BuildingSpec, PlayerSpec, ScenarioMode, UnitSpec};
use oxide_sim::stats::{RECLAIMER_PERIOD, REFINERY_PERIOD, UnitKind};
use oxide_sim::{AttackTarget, Command, PlayerCommand, PlayerId, Scenario, UnitId};

/// A 32-wide, 12-deep field: West's start at (2, 2) and East's at
/// (28, 2), each seat with `scrap`, plus the given units and buildings.
fn field(
    scrap: u32,
    units: &[(u8, UnitKind, i32, i32)],
    buildings: &[(u8, BuildingKind, i32, i32)],
) -> Scenario {
    wide_field(32, scrap, units, buildings)
}

/// [`field`] `width` tiles wide, East's start four tiles from its end.
fn wide_field(
    width: usize,
    scrap: u32,
    units: &[(u8, UnitKind, i32, i32)],
    buildings: &[(u8, BuildingKind, i32, i32)],
) -> Scenario {
    let ground = ".".repeat(width);
    let mut map = vec![ground.clone(); 12];
    let mut top: Vec<char> = ground.chars().collect();
    top[2] = '1';
    top[width - 4] = '2';
    map[2] = top.into_iter().collect();
    Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Match,
        name: "health".into(),
        map,
        players: (0..2)
            .map(|seat| PlayerSpec {
                name: format!("seat {seat}"),
                team: None,
                scrap,
                bot: false,
                bot_config: None,
            })
            .collect(),
        units: units
            .iter()
            .map(|&(player, kind, x, y)| UnitSpec { player, kind, x, y })
            .collect(),
        buildings: buildings
            .iter()
            .map(|&(player, kind, x, y)| BuildingSpec { player, kind, x, y })
            .collect(),
        meta: None,
    }
}

/// Plays `state`, built from `scenario`, for `ticks`, issuing each order on
/// its tick, and returns its health and ledgers.
fn play(
    scenario: &Scenario,
    mut state: State,
    ticks: u64,
    orders: &[(u64, PlayerCommand)],
) -> (MatchHealth, Vec<crate::ledger::SeatLedger>) {
    let mut ledger = ImpactLedger::new(&state);
    let mut pass = Pass::new(&state);
    for _ in 0..ticks {
        let now = state.current_tick();
        let commands: Vec<PlayerCommand> = orders
            .iter()
            .filter(|(tick, _)| *tick == now)
            .map(|(_, order)| order.clone())
            .collect();
        let report = state.tick(&commands);
        ledger.observe(&state, &report);
        pass.observe(&state, &report, &ledger);
    }
    let replay: GameReplay = Replay::new(SIM_VERSION, "test", scenario.clone());
    let ledgers = ledger.finish(&state);
    let health = pass.finish(&state, Path::new("staged.json"), &replay, &ledgers);
    (health, ledgers)
}

fn unit_at(state: &State, x: i32, y: i32) -> UnitId {
    state
        .units()
        .iter()
        .find(|unit| unit.tile() == TilePos::new(x, y))
        .unwrap()
        .id
}

fn attack(player: u8, units: Vec<UnitId>, target: AttackTarget) -> PlayerCommand {
    PlayerCommand {
        player: PlayerId(player),
        command: Command::Attack {
            units,
            target,
            queue: false,
        },
    }
}

#[test]
fn income_is_credited_to_its_source_minute_by_minute() {
    // A harvester keeps the seat off the recovery trickle, which the
    // ledger cannot credit.
    let scenario = field(
        0,
        &[(0, UnitKind::Harvester, 4, 8)],
        &[
            (0, BuildingKind::Reclaimer, 6, 6),
            (0, BuildingKind::Reclaimer, 8, 6),
        ],
    );
    let mut value = serde_json::to_value(scenario.build().unwrap()).unwrap();
    let reclaimer = value["buildings"]
        .as_array()
        .unwrap()
        .iter()
        .position(|building| building["kind"] == "reclaimer")
        .unwrap();
    value["buildings"][reclaimer]["tier"] = serde_json::json!(1);
    let state: State = serde_json::from_value(value).unwrap();
    let before = state.players()[0].scrap;
    let (health, ledgers) = play(&scenario, state, 2 * MINUTE + 600, &[]);
    let seat = &health.seats[0];
    for series in seat.income.values() {
        assert_eq!(series.len(), 3, "{:?}", seat.income);
    }
    let per_minute = |period: u64| MINUTE.div_ceil(period);
    assert_eq!(
        seat.income[&IncomeSource::Reclaimer][..2],
        [per_minute(RECLAIMER_PERIOD); 2]
    );
    assert_eq!(
        seat.income[&IncomeSource::Refinery][..2],
        [per_minute(REFINERY_PERIOD); 2]
    );
    let sum = |source| seat.income[&source].iter().sum::<u64>();
    let passive = |kind: &str| ledgers[0].buildings[kind].passive;
    assert_eq!(sum(IncomeSource::Reclaimer), passive("reclaimer"));
    assert_eq!(sum(IncomeSource::Refinery), passive("refinery"));
    assert_eq!(sum(IncomeSource::FoundryDrip), passive("foundry"));
    assert!(sum(IncomeSource::FoundryDrip) > 0, "the drip");
    let earned: u64 = seat.income.values().flatten().sum();
    assert_eq!(earned, u64::from(health_bank(&health, before)));
}

/// The seat-zero bank the run ended with, from its starting `before` plus
/// everything credited, for a run that spends nothing.
fn health_bank(health: &MatchHealth, before: u32) -> u32 {
    let earned: u64 = health.seats[0].income.values().flatten().sum();
    before + u32::try_from(earned).unwrap()
}

#[test]
fn deliveries_count_as_harvest_in_the_minute_they_land() {
    let scenario = Scenario::skirmish();
    let state = scenario.build().unwrap();
    let workers: Vec<UnitId> = state
        .units()
        .iter()
        .filter(|unit| unit.player == PlayerId(0) && unit.kind.stats().harvest.is_some())
        .map(|unit| unit.id)
        .collect();
    let from = state.unit(workers[0]).unwrap().tile();
    let node = state
        .map()
        .iter()
        .filter(|(_, tile)| tile.scrap > 0)
        .map(|(tile, _)| tile)
        .min_by_key(|tile| (tile.chebyshev(from), tile.y, tile.x))
        .unwrap();
    let harvest = PlayerCommand {
        player: PlayerId(0),
        command: Command::Harvest {
            units: workers,
            node,
            queue: false,
        },
    };
    let (health, ledgers) = play(&scenario, state, MINUTE * 3, &[(0, harvest)]);
    let harvested: u64 = ledgers[0]
        .units
        .values()
        .map(|totals| totals.harvested)
        .sum();
    assert!(harvested > 0, "premise: the starting workers deliver");
    let earned = |source| health.seats[0].income[&source].iter().sum::<u64>();
    assert_eq!(earned(IncomeSource::FieldHarvest), harvested);
    assert_eq!(earned(IncomeSource::WreckSalvage), 0);
}

#[test]
fn scrap_stripped_from_a_wreck_is_delivered_as_wreck_salvage() {
    let scenario = field(
        0,
        &[
            (0, UnitKind::Sentinel, 6, 6),
            (0, UnitKind::Harvester, 4, 7),
            (1, UnitKind::Harvester, 8, 6),
        ],
        &[],
    );
    let state = scenario.build().unwrap();
    let worker = unit_at(&state, 4, 7);
    let salvage = PlayerCommand {
        player: PlayerId(0),
        command: Command::Harvest {
            units: vec![worker],
            node: TilePos::new(8, 6),
            queue: false,
        },
    };
    let (health, ledgers) = play(&scenario, state, 2 * MINUTE, &[(150, salvage)]);
    assert_eq!(
        ledgers[1].units["harvester"].deaths, 1,
        "premise: the wreck"
    );
    let harvested = ledgers[0].units["harvester"].harvested;
    assert!(harvested > 0, "premise: the worker delivers");
    let earned = |source| health.seats[0].income[&source].iter().sum::<u64>();
    assert_eq!(earned(IncomeSource::WreckSalvage), harvested);
    assert_eq!(earned(IncomeSource::FieldHarvest), 0);
}

#[test]
fn wreck_is_sampled_each_minute_at_the_home_it_lies_in() {
    let scenario = field(
        0,
        &[
            (0, UnitKind::Sentinel, 6, 6),
            (1, UnitKind::Harvester, 8, 6),
        ],
        &[],
    );
    let state = scenario.build().unwrap();
    let (health, _) = play(&scenario, state, 2 * MINUTE, &[]);
    let minutes: Vec<u64> = health.wrecks.iter().map(|sample| sample.minute).collect();
    assert_eq!(minutes, [1, 2]);
    let [first, second] = &health.wrecks[..] else {
        unreachable!()
    };
    assert!(first.total > 0, "{first:?}");
    assert_eq!(first.home, [first.total, 0], "it fell in West's base");
    assert!(second.total < first.total, "wreck decays: {second:?}");
}

#[test]
fn entropy_reads_how_evenly_value_spreads_over_kinds() {
    let army = |values: &[(&'static str, u64)]| {
        composition(&values.iter().copied().collect::<BTreeMap<_, _>>())
    };
    let even = army(&[("lancer", 300), ("bombard", 300)]);
    assert!((even.entropy_bits - 1.0).abs() < 1e-12);
    assert!((even.effective_kinds - 2.0).abs() < 1e-12);
    let mono = army(&[("breaker", 900)]);
    assert!(mono.entropy_bits.is_sign_positive() && mono.entropy_bits == 0.0);
    assert_eq!(mono.effective_kinds, 1.0);
    let none = army(&[]);
    assert_eq!((none.entropy_bits, none.effective_kinds), (0.0, 0.0));
    let skewed = army(&[("condor", 900), ("talon", 100)]);
    assert!(skewed.entropy_bits > 0.0 && skewed.entropy_bits < 1.0);
}

#[test]
fn late_armies_are_sampled_and_training_counted_from_minute_fifteen() {
    let scenario = field(
        1_000,
        &[
            (0, UnitKind::Lancer, 4, 9),
            (0, UnitKind::Lancer, 4, 10),
            (1, UnitKind::Bombard, 27, 9),
        ],
        &[],
    );
    let state = scenario.build().unwrap();
    let foundry = state
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0))
        .unwrap()
        .id;
    let train = |kind| PlayerCommand {
        player: PlayerId(0),
        command: Command::Train {
            building: foundry,
            kind,
        },
    };
    let orders = [
        (100, train(UnitKind::Sentinel)),
        (LATE_TICK, train(UnitKind::Sentinel)),
        (LATE_TICK, train(UnitKind::Harvester)),
    ];
    let (early, _) = play(&scenario, state.clone(), LATE_TICK - 1, &orders);
    assert!(early.seats.iter().all(|seat| {
        seat.late_minutes == 0 && seat.late_army.is_none() && seat.late_trained.is_none()
    }));
    let (health, _) = play(&scenario, state, LATE_TICK + MINUTE, &orders);
    let seat = &health.seats[0];
    assert_eq!(seat.late_minutes, 2);
    let (lancer, sentinel) = (
        u64::from(UnitKind::Lancer.stats().cost),
        u64::from(UnitKind::Sentinel.stats().cost),
    );
    let value = |pairs: &[(&str, u64)]| -> BTreeMap<String, u64> {
        pairs
            .iter()
            .map(|(kind, value)| ((*kind).to_owned(), *value))
            .collect()
    };
    assert_eq!(
        seat.late_army.as_ref().unwrap().value,
        value(&[("lancer", 4 * lancer), ("sentinel", 3 * sentinel)]),
        "two lancers and the early sentinel at both minutes, the late one at the second"
    );
    let trained = seat.late_trained.as_ref().unwrap();
    assert_eq!(trained.value, value(&[("sentinel", sentinel)]));
    assert_eq!(trained.effective_kinds, 1.0);
    let idle = &health.seats[1];
    assert_eq!(
        idle.late_army.as_ref().unwrap().value,
        value(&[("bombard", 2 * u64::from(UnitKind::Bombard.stats().cost))])
    );
    assert_eq!(idle.late_trained.as_ref().unwrap().effective_kinds, 0.0);
}

#[test]
fn a_won_push_that_razes_production_and_workers_converts() {
    let scenario = field(
        0,
        &[
            (0, UnitKind::Warden, 19, 6),
            (0, UnitKind::Warden, 19, 7),
            (0, UnitKind::Warden, 19, 8),
            (1, UnitKind::Sentinel, 21, 6),
            (1, UnitKind::Sentinel, 21, 7),
            (1, UnitKind::Sentinel, 21, 8),
            (1, UnitKind::Harvester, 22, 9),
        ],
        &[(1, BuildingKind::Fabricator, 23, 6)],
    );
    let state = scenario.build().unwrap();
    let (health, _) = play(&scenario, state, 3 * MINUTE, &[]);
    let [battle] = &health.battles[..] else {
        panic!("{:#?}", health.battles)
    };
    assert_eq!(battle.winner, Some(0));
    let follow = battle.follow_up.as_ref().unwrap();
    assert_eq!(follow.place, Place::Away);
    assert_eq!(follow.losers, [1]);
    assert_eq!(follow.force, 3);
    assert_eq!(follow.until, battle.to_tick + FOLLOW_UP_TICKS);
    assert_eq!(
        follow.production,
        u64::from(BuildingKind::Fabricator.invested_cost(0))
    );
    assert_eq!(follow.income, u64::from(UnitKind::Harvester.stats().cost));
    assert!(follow.held, "{follow:?}");
    assert!(follow.converted());
}

#[test]
fn only_the_winning_force_converts_and_leaving_gives_up_the_ground() {
    // The battle is 21 tiles from either Foundry, and a Reclaimer, which
    // trains nothing, does not make the ground nearby East's home. The
    // Fabricator and the worker each stand more than the battle radius
    // away from it.
    let scenario = wide_field(
        48,
        0,
        &[
            (0, UnitKind::Warden, 22, 6),
            (0, UnitKind::Warden, 22, 7),
            (0, UnitKind::Warden, 22, 8),
            (1, UnitKind::Sentinel, 24, 6),
            (1, UnitKind::Sentinel, 24, 7),
            (1, UnitKind::Sentinel, 24, 8),
            (0, UnitKind::Sentinel, 38, 11),
            (1, UnitKind::Harvester, 46, 11),
        ],
        &[
            (1, BuildingKind::Fabricator, 4, 9),
            (1, BuildingKind::Reclaimer, 31, 1),
        ],
    );
    let state = scenario.build().unwrap();
    let wardens: Vec<UnitId> = (6..=8).map(|y| unit_at(&state, 22, y)).collect();
    let fabricator = state
        .buildings()
        .iter()
        .find(|building| building.kind == BuildingKind::Fabricator)
        .unwrap()
        .id;
    let raid = PlayerCommand {
        player: PlayerId(0),
        command: Command::Hunt {
            units: vec![unit_at(&state, 38, 11)],
            goal: TilePos::new(46, 11),
            queue: false,
        },
    };
    let push = attack(0, wardens, AttackTarget::Building(fabricator));
    // The recording ends inside the minute after the battle, which clips
    // the follow-up to its end.
    let (health, ledgers) = play(&scenario, state, MINUTE, &[(300, push), (300, raid)]);
    assert_eq!(
        ledgers[1].units["harvester"].deaths, 1,
        "premise: the raid lands"
    );
    let [battle] = &health.battles[..] else {
        panic!("{:#?}", health.battles)
    };
    let follow = battle.follow_up.as_ref().unwrap();
    assert_eq!(follow.place, Place::Field);
    assert_eq!(follow.until, MINUTE);
    assert_eq!(
        follow.production,
        u64::from(BuildingKind::Fabricator.invested_cost(0)),
        "the force razed the Fabricator after the battle"
    );
    assert_eq!(
        follow.income, 0,
        "a raider outside the force took the worker"
    );
    assert_eq!(follow.stayed, 0);
    assert!(!follow.held && follow.converted());
}

#[test]
fn a_replay_directory_is_measured_to_each_decision_in_name_order() {
    let dir = std::env::temp_dir().join(format!("oxide-replay-health-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Wardens beside East's only Foundry raze it well before the recording
    // runs out.
    let scenario = field(
        0,
        &[
            (0, UnitKind::Warden, 26, 2),
            (0, UnitKind::Warden, 26, 3),
            (0, UnitKind::Warden, 27, 4),
            (0, UnitKind::Warden, 28, 4),
        ],
        &[],
    );
    let mut decisive: GameReplay = Replay::new(SIM_VERSION, "test", scenario);
    decisive.meta.ticks = Some(10 * MINUTE);
    oxide_kit::save_recording(
        dir.join("b-decisive.oxreplay"),
        &decisive,
        &oxide_kit::keyframes::Keyframes::default(),
    )
    .unwrap();
    crate::support_tests::replay_fixture()
        .save(dir.join("a-short.json"))
        .unwrap();
    std::fs::write(dir.join("legs.jsonl"), b"{}\n").unwrap();

    let matches = measure(std::slice::from_ref(&dir), NonZeroUsize::new(2).unwrap()).unwrap();
    let names: Vec<_> = matches
        .iter()
        .map(|game| game.path.file_name().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["a-short.json", "b-decisive.oxreplay"]);
    let [short, decisive] = &matches[..] else {
        unreachable!()
    };
    assert_eq!((short.end, short.result, short.long), (12, None, false));
    assert!(
        short.seats[0]
            .income
            .values()
            .all(|series| series.len() == 1)
    );
    assert_eq!(decisive.result, Some(GameResult::Victory { team: 0 }));
    assert!(decisive.end < 10 * MINUTE, "{}", decisive.end);
    assert_eq!(decisive.seats[1].eliminated_at, Some(decisive.end - 1));
    assert_eq!(decisive.seats[0].controller, SeatPlayer::Human);
    let pacing = decisive
        .pacing
        .as_ref()
        .expect("worth is sampled to the decision");
    assert_eq!(
        (pacing.sides, pacing.winner, pacing.end),
        (2, Some(0), decisive.end)
    );
    assert_eq!(
        short.pacing.as_ref().map(|pacing| pacing.winner),
        Some(None)
    );

    let summary = summarize(&matches);
    assert_eq!(
        (summary.matches, summary.decided, summary.cut_short),
        (2, 1, 1)
    );
    assert_eq!((summary.pacing.matches, summary.pacing.won), (2, 1));
    std::fs::remove_dir_all(&dir).unwrap();

    let empty =
        std::env::temp_dir().join(format!("oxide-replay-health-empty-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    let error = expand(std::slice::from_ref(&empty)).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("no .json or .oxreplay recordings"),
        "{error}"
    );
    std::fs::remove_dir_all(&empty).unwrap();
}

/// A seat that earned `income` per source in each minute and left the
/// match at `out`.
fn seat(income: &[(IncomeSource, Vec<u64>)], out: Option<u64>) -> SeatHealth {
    SeatHealth {
        seat: 0,
        name: "seat".into(),
        team: 0,
        controller: SeatPlayer::Human,
        eliminated_at: out,
        income: income.iter().cloned().collect(),
        late_minutes: 0,
        late_army: None,
        late_trained: None,
    }
}

fn game(end: u64, result: Option<GameResult>, seats: Vec<SeatHealth>) -> MatchHealth {
    MatchHealth {
        path: PathBuf::from("synthetic.json"),
        scenario: "synthetic".into(),
        start: 0,
        end,
        result,
        winner_seats: Vec::new(),
        long: end >= LONG_MINUTES * MINUTE,
        first_minute: 0,
        seats,
        wrecks: Vec::new(),
        battles: Vec::new(),
        pacing: None,
    }
}

#[test]
fn pooled_income_is_per_minute_a_seat_was_in_the_match() {
    // Six minutes of one seat earning 120 Refinery scrap a minute, and a
    // seat knocked out halfway through minute five that earned nothing.
    let refinery = vec![120; 6];
    let matches = [game(
        6 * MINUTE,
        Some(GameResult::Victory { team: 0 }),
        vec![
            seat(&[(IncomeSource::Refinery, refinery)], None),
            seat(&[], Some(5 * MINUTE + MINUTE / 2)),
        ],
    )];
    let summary = summarize(&matches);
    let [early, middle] = &summary.income[..] else {
        panic!("{:?}", summary.income)
    };
    assert_eq!((early.from, early.to), (0, Some(5)));
    assert!((early.seat_minutes - 10.0).abs() < 1e-9);
    assert!((early.per_minute[&IncomeSource::Refinery] - 60.0).abs() < 1e-9);
    assert!((middle.seat_minutes - 1.5).abs() < 1e-9, "{middle:?}");
    assert!((middle.per_minute[&IncomeSource::Refinery] - 80.0).abs() < 1e-9);
    assert_eq!(
        summary.late_income[&IncomeSource::Refinery],
        0.0,
        "no income past minute 15"
    );
}

#[test]
fn matches_reaching_thirty_minutes_count_as_long() {
    let matches = [
        game(LONG_MINUTES * MINUTE, Some(GameResult::Draw), Vec::new()),
        game(
            LONG_MINUTES * MINUTE - 1,
            Some(GameResult::Draw),
            Vec::new(),
        ),
        game(10 * MINUTE, None, Vec::new()),
        game(40 * MINUTE, None, Vec::new()),
    ];
    let summary = summarize(&matches);
    assert_eq!(
        (summary.long, summary.decided, summary.cut_short),
        (2, 2, 1)
    );
    assert_eq!(summary.minutes.first(), Some(&10.0));
}

#[test]
fn the_report_renders_every_section_and_a_line_per_match() {
    let mut late = game(
        LONG_MINUTES * MINUTE,
        Some(GameResult::Victory { team: 0 }),
        vec![seat(&[(IncomeSource::Extractor, vec![180; 30])], None)],
    );
    late.seats[0].late_army = Some(composition(
        &[("breaker", 600), ("condor", 600)].into_iter().collect(),
    ));
    late.seats[0].late_trained = Some(composition(&[("condor", 900)].into_iter().collect()));
    late.wrecks = vec![WreckSample {
        minute: 5,
        total: 400,
        home: vec![100],
    }];
    late.battles = vec![BattleHealth {
        from_tick: LATE_TICK,
        to_tick: LATE_TICK + 100,
        at: [10, 10],
        losses: Vec::new(),
        shootdown: false,
        winner: Some(0),
        follow_up: Some(FollowUp {
            place: Place::Away,
            own_ground: false,
            losers: vec![1],
            force: 4,
            until: LATE_TICK + 100 + FOLLOW_UP_TICKS,
            production: 300,
            income: 0,
            stayed: 0,
            opposed: 0,
            held: false,
        }),
    }];
    late.pacing = Some(Pacing {
        sides: 2,
        winner: Some(0),
        end: LONG_MINUTES * MINUTE,
        leads: vec![crate::pacing::LeadAt {
            minute: 10,
            team: Some(0),
        }],
        lead_changes: 1,
        winner_low: Some(380),
        settled_at: Some(20 * MINUTE),
    });
    let matches = [late];
    let report = render(&matches, &summarize(&matches));
    for expected in [
        "1 match, 1 decided; 1 reached 30 minutes (100%)",
        "share from minute 15: extractor 100%",
        "5m 400 (25%, n=1)",
        "standing 1 seat-games, entropy 1.00 bits (2.0 effective kinds): breaker 50%, condor 50%",
        "trained  1 seat-games, entropy 0.00 bits (1.0 effective kinds): condor 100%",
        "at the loser's home",
        "  ahead at minute 10: 1 won matches, the side ahead won 1 (100%)",
        "  comebacks: 1 of 1 winners fell to 0.8 times an equal split (40% in a duel)",
        "peak wreck 400; lead changes 1, winner's low 38%",
        "synthetic.json: synthetic, 30.0 min, team 0 won; 1 battles, 1 won, 1 converted; late kinds standing 2.0, trained 1.0",
    ] {
        assert!(report.contains(expected), "{expected:?} in\n{report}");
    }
}

#[test]
fn workers_sharing_a_tile_take_its_field_scrap_before_its_wreck() {
    // A lone node holding 3 field scrap under 12 wreck, worked by two
    // harvesters: the sim strips field scrap first.
    let scenario = field(
        0,
        &[
            (0, UnitKind::Harvester, 5, 7),
            (0, UnitKind::Harvester, 5, 8),
        ],
        &[],
    );
    let mut value = serde_json::to_value(scenario.build().unwrap()).unwrap();
    let width = usize::try_from(value["map"]["grid"]["width"].as_u64().unwrap()).unwrap();
    let cell = &mut value["map"]["grid"]["cells"][7 * width + 8];
    cell["scrap"] = serde_json::json!(3);
    cell["wreck"] = serde_json::json!(12);
    let state: State = serde_json::from_value(value).unwrap();
    let workers = vec![unit_at(&state, 5, 7), unit_at(&state, 5, 8)];
    let work = PlayerCommand {
        player: PlayerId(0),
        command: Command::Harvest {
            units: workers,
            node: TilePos::new(8, 7),
            queue: false,
        },
    };
    let (health, ledgers) = play(&scenario, state, 3 * MINUTE, &[(5, work)]);
    let harvested = ledgers[0].units["harvester"].harvested;
    let earned = |source| health.seats[0].income[&source].iter().sum::<u64>();
    assert_eq!(earned(IncomeSource::FieldHarvest), 3);
    assert!(earned(IncomeSource::WreckSalvage) > 0);
    assert_eq!(earned(IncomeSource::WreckSalvage), harvested - 3);
}

#[test]
fn pooled_kind_shares_weigh_each_seat_game_equally() {
    let army = |values: &[(&'static str, u64)]| {
        Some(composition(
            &values.iter().copied().collect::<BTreeMap<_, _>>(),
        ))
    };
    let mut short = seat(&[], None);
    short.late_army = army(&[("breaker", 100)]);
    let mut long = seat(&[], None);
    long.late_army = army(&[("condor", 9_000), ("talon", 9_000)]);
    let mut empty = seat(&[], None);
    empty.late_army = army(&[]);
    let matches = [game(MINUTE * 40, None, vec![short, long, empty])];
    let summary = summarize(&matches);
    let army = &summary.late_army;
    assert_eq!(army.seats, 2, "a seat with no units is left out");
    assert_eq!(
        army.shares,
        [
            ("breaker".to_owned(), 0.5),
            ("condor".to_owned(), 0.25),
            ("talon".to_owned(), 0.25)
        ]
    );
    assert!((army.effective_kinds - 1.5).abs() < 1e-12);
    assert_eq!(summary.late_trained.seats, 0);
}

#[test]
fn a_win_converts_on_production_real_income_or_ground_that_is_not_its_own() {
    let follow = |place, own_ground, production, income, held| FollowUp {
        place,
        own_ground,
        losers: vec![1],
        force: 1,
        until: 0,
        production,
        income,
        stayed: 0,
        opposed: 0,
        held,
    };
    let battle = |from_tick, follow_up: FollowUp| BattleHealth {
        from_tick,
        to_tick: from_tick,
        at: [0, 0],
        losses: Vec::new(),
        shootdown: false,
        winner: Some(0),
        follow_up: Some(follow_up),
    };
    let held_at_home = follow(Place::Home, true, 0, 0, true);
    let one_worker = follow(Place::Away, false, 0, INCOME_FLOOR - 1, false);
    let held_away = follow(Place::Field, false, 0, 0, true);
    // Away from its factories but among its Refineries and turrets.
    let held_outskirts = follow(Place::Field, true, 0, 0, true);
    let workers = follow(Place::Home, true, 0, INCOME_FLOOR, false);
    let razed = follow(Place::Away, false, 120, 0, false);
    assert!(!held_at_home.converted() && !one_worker.converted() && !held_outskirts.converted());
    assert!(held_away.converted() && workers.converted() && razed.converted());

    let mut matches = [game(MINUTE * 30, None, Vec::new())];
    matches[0].battles = vec![
        battle(0, held_at_home),
        battle(0, one_worker),
        battle(0, held_away),
        battle(0, held_outskirts),
        battle(LATE_TICK, workers),
        battle(LATE_TICK, razed),
    ];
    let rows = summarize(&matches).follow_up;
    let row = |label: &str| {
        let row = rows.iter().find(|row| row.label == label).unwrap();
        (row.won, row.converted, row.production, row.income, row.held)
    };
    assert_eq!(rows[0].label, "pushes");
    assert_eq!(row("pushes"), (4, 2, 1, 0, 1));
    assert_eq!(row("pushes before minute 15"), (3, 1, 0, 0, 1));
    assert_eq!(row("pushes from minute 15"), (1, 1, 1, 0, 0));
    assert_eq!(row("at the winner's home"), (2, 1, 0, 1, 0));
    assert_eq!(row("in the field"), (2, 1, 0, 0, 1));
    assert_eq!(row("all"), (6, 3, 1, 1, 1));
}

#[test]
fn only_buildings_that_train_units_mark_a_home() {
    for kind in [
        BuildingKind::Foundry,
        BuildingKind::Fabricator,
        BuildingKind::Airworks,
        BuildingKind::Crucible,
    ] {
        assert!(marks_home(kind), "{kind:?}");
    }
    for kind in [
        BuildingKind::Reclaimer,
        BuildingKind::Extractor,
        BuildingKind::Turret,
        BuildingKind::Bastion,
        BuildingKind::Array,
    ] {
        assert!(!marks_home(kind), "{kind:?}");
    }
}

#[test]
fn a_recording_restored_on_a_whole_minute_samples_that_minute() {
    let scenario = field(0, &[(0, UnitKind::Lancer, 4, 9)], &[]);
    let mut state = scenario.build().unwrap();
    for _ in 0..MINUTE {
        state.tick(&[]);
    }
    let restored = Pass::new(&state);
    let minutes: Vec<u64> = restored.wrecks.iter().map(|sample| sample.minute).collect();
    assert_eq!(minutes, [1]);
    state.tick(&[]);
    assert!(
        Pass::new(&state).wrecks.is_empty(),
        "mid-minute starts wait"
    );
    assert!(Pass::new(&scenario.build().unwrap()).wrecks.is_empty());
}

#[test]
fn a_factory_still_under_construction_marks_no_home() {
    // The fight is 21 tiles from either Foundry and 10 from East's
    // Fabricator, beyond the Wardens' sight: standing, or still a site its
    // worker is raising.
    let place = |site: bool| {
        let scenario = wide_field(
            48,
            1_000,
            &[
                (0, UnitKind::Warden, 22, 6),
                (0, UnitKind::Warden, 22, 7),
                (0, UnitKind::Warden, 22, 8),
                (1, UnitKind::Sentinel, 24, 6),
                (1, UnitKind::Sentinel, 24, 7),
                (1, UnitKind::Sentinel, 24, 8),
                (1, UnitKind::Harvester, 33, 4),
            ],
            if site {
                &[]
            } else {
                &[(1, BuildingKind::Fabricator, 33, 1)]
            },
        );
        let state = scenario.build().unwrap();
        let raise = PlayerCommand {
            player: PlayerId(1),
            command: Command::Build {
                units: vec![unit_at(&state, 33, 4)],
                kind: BuildingKind::Fabricator,
                anchor: TilePos::new(33, 1),
                queue: false,
                defer: false,
            },
        };
        let orders = if site { vec![(0, raise)] } else { Vec::new() };
        let (health, _) = play(&scenario, state, MINUTE, &orders);
        let [battle] = &health.battles[..] else {
            panic!("{:#?}", health.battles)
        };
        let follow = battle.follow_up.clone().unwrap();
        (follow.place, follow.own_ground)
    };
    assert_eq!(place(false), (Place::Away, false));
    assert_eq!(
        place(true),
        (Place::Field, false),
        "the site is still the loser's ground"
    );
}

#[test]
fn a_seat_earns_nothing_once_it_is_out_of_the_match() {
    // Middle loses its only Foundry while its ally East keeps the match
    // going; its Reclaimer keeps paying a bank nobody plays.
    let ground = ".".repeat(40);
    let mut map = vec![ground.clone(); 12];
    let mut top: Vec<char> = ground.chars().collect();
    top[2] = '1';
    top[20] = '2';
    top[36] = '3';
    map[2] = top.into_iter().collect();
    let scenario = Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Match,
        name: "teams".into(),
        map,
        players: [0, 1, 1]
            .into_iter()
            .enumerate()
            .map(|(seat, team)| PlayerSpec {
                name: format!("seat {seat}"),
                team: Some(team),
                scrap: 0,
                bot: false,
                bot_config: None,
            })
            .collect(),
        units: [(19, 4), (20, 4), (21, 4), (22, 4), (22, 3)]
            .into_iter()
            .map(|(x, y)| UnitSpec {
                player: 0,
                kind: UnitKind::Warden,
                x,
                y,
            })
            .collect(),
        buildings: vec![BuildingSpec {
            player: 1,
            kind: BuildingKind::Reclaimer,
            x: 30,
            y: 10,
        }],
        meta: None,
    };
    let state = scenario.build().unwrap();
    let (health, ledgers) = play(&scenario, state, 4 * MINUTE, &[]);
    let out = health.seats[1]
        .eliminated_at
        .expect("premise: Middle loses its Foundry");
    assert!(health.result.is_none(), "premise: East plays on");
    let paid_while_in = (0..out).filter(|tick| tick % RECLAIMER_PERIOD == 0).count();
    let earned: u64 = health.seats[1].income[&IncomeSource::Reclaimer]
        .iter()
        .sum();
    assert_eq!(earned, u64::try_from(paid_while_in).unwrap());
    assert!(
        ledgers[1].buildings["reclaimer"].passive > earned,
        "premise: the Reclaimer kept paying"
    );
}
