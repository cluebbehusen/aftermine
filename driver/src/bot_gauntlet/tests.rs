use super::*;

fn plan(families: Vec<Family>, seeds: std::ops::Range<u64>, tick_limit: u64) -> GauntletPlan {
    GauntletPlan {
        families,
        maps: vec![("skirmish".to_string(), Scenario::skirmish())],
        difficulties: vec![BotDifficulty::Prime],
        stances: vec![BotStance::Balanced],
        seeds,
        tick_limit,
        replay_dir: None,
    }
}

fn row(family: Family, difficulty: &str, winner: Winner) -> GameRow {
    GameRow {
        family,
        map: "skirmish".to_string(),
        difficulty: difficulty.to_string(),
        stance: "balanced".to_string(),
        seed: 0,
        challenger_seat: 0,
        ticks: 100,
        winner,
        challenger_worth: 1,
        bot_worth: 1,
        challenger_commands: 0,
        challenger_rejected: 0,
        bot_fights: crate::fights::SeatFights::default(),
    }
}

#[test]
fn family_lists_name_families_and_groups_once_each() {
    let names = |list: &[&str]| list.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(
        families(&names(&["ball-rush", "tuning", "forward-fabricator"])).unwrap(),
        [
            Family::BallRush,
            Family::ForwardFabricator,
            Family::ScuttlerRaid,
            Family::LancerHarass,
            Family::HeldArmy,
            Family::BuzzardHarass,
            Family::BombardSiege,
        ]
    );
    assert_eq!(
        families(&names(&["held-out"])).unwrap(),
        Family::HELD_OUT.to_vec()
    );
    assert!(families(&names(&["turtle"])).is_err());
}

#[test]
fn records_pool_games_by_family_and_rung() {
    let rows = [
        row(Family::BallRush, "prime", Winner::Challenger),
        row(Family::BallRush, "prime", Winner::Bot),
        row(Family::BallRush, "prime", Winner::Bot),
        row(Family::BallRush, "prime", Winner::None),
        row(Family::BallRush, "veteran", Winner::Challenger),
        row(Family::HeldArmy, "prime", Winner::None),
    ];
    let records = records(&rows);
    assert_eq!(records.len(), 3);
    let prime = &records[0];
    assert_eq!(
        (
            prime.games,
            prime.challenger_wins,
            prime.bot_wins,
            prime.undecided
        ),
        (4, 1, 2, 1)
    );
    assert_eq!(prime.share, Some(1.0 / 3.0));
    assert_eq!(prime.wilson, Some(crate::bot_ladder::wilson(1, 3)));
    assert_eq!(
        (records[1].difficulty.as_str(), records[1].games),
        ("veteran", 1)
    );
    assert_eq!((records[2].share, records[2].wilson), (None, None));
    let table = report(&records);
    let (wins, fights) = table
        .split_once("\nfights:")
        .expect("a fights table follows");
    assert!(wins.lines().count() == 4, "{table}");
    assert!(fights.lines().count() == 4, "{table}");
    assert!(table.contains("ball-rush") && table.contains("held-army"));
}

#[test]
fn the_challenger_changes_seats_with_the_seed_parity() {
    let rows = run(
        &plan(vec![Family::ForwardFabricator], 0..2, 300),
        NonZeroUsize::new(2).unwrap(),
    )
    .unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| (row.seed, row.challenger_seat))
            .collect::<Vec<_>>(),
        [(0, 0), (1, 1)]
    );
    for row in &rows {
        assert_eq!((row.ticks, row.winner), (300, Winner::None));
        assert!(row.challenger_worth > 0 && row.bot_worth > 0, "{row:?}");
        assert!(row.challenger_commands > 0);
        assert_eq!(row.challenger_rejected, 0);
    }
}

#[test]
fn a_gauntlet_refuses_an_empty_plan_or_a_crowded_map() {
    let jobs = NonZeroUsize::new(1).unwrap();
    assert!(run(&plan(Vec::new(), 0..1, 10), jobs).is_err());
    let mut crowded = plan(vec![Family::BallRush], 0..1, 10);
    let mut scenario = Scenario::skirmish();
    scenario.players.push(scenario.players[1].clone());
    crowded.maps = vec![("crowded".to_string(), scenario)];
    assert!(run(&crowded, jobs).is_err());
}
