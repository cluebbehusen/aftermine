use super::*;

#[test]
fn lancers_beat_sentinels_at_even_price() {
    let trade = fight(&even(UnitKind::Lancer, 440), &even(UnitKind::Sentinel, 440)).unwrap();
    assert!(trade.killed > 2 * trade.lost, "{trade:?}");
}

#[test]
fn a_fight_reads_the_same_from_either_army() {
    let lancers = even(UnitKind::Lancer, 440);
    let wardens = even(UnitKind::Warden, 440);
    let one = fight(&lancers, &wardens).unwrap();
    assert_eq!(fight(&wardens, &lancers).unwrap(), one.reversed());
    assert_eq!(fight(&lancers, &wardens).unwrap(), one, "deterministic");
}

#[test]
fn armies_that_cannot_hurt_each_other_stop_at_once() {
    // Talons only hit aircraft and Lancers only hit the ground.
    let trade = fight(&even(UnitKind::Talon, 300), &even(UnitKind::Lancer, 300)).unwrap();
    assert_eq!(trade, Trade::default());
}

#[test]
fn an_army_too_big_for_the_field_is_refused() {
    let error = fight(&[(UnitKind::Scuttler, 200)], &even(UnitKind::Lancer, 300)).unwrap_err();
    assert!(error.to_string().contains("does not fit"), "{error}");
}

#[test]
fn even_buys_what_the_budget_buys_and_at_least_one() {
    assert_eq!(even(UnitKind::Lancer, 1_400), vec![(UnitKind::Lancer, 13)]);
    assert_eq!(even(UnitKind::Breaker, 100), vec![(UnitKind::Breaker, 1)]);
}

#[test]
fn a_matrix_mirrors_each_pair() {
    let kinds = [UnitKind::Scuttler, UnitKind::Sentinel];
    let matrix = matrix(&kinds, &[200], NonZeroUsize::new(2).unwrap()).unwrap();
    let pair = matrix.trades[0][0][1];
    assert_ne!(pair, Trade::default(), "premise: they fought");
    assert_eq!(matrix.trades[0][1][0], pair.reversed());
    assert_eq!(matrix.trades[0][0][0], Trade::default());
}

#[test]
fn mixed_fights_report_from_the_first_army() {
    let preset = [(
        "Lancers v Sentinels",
        even(UnitKind::Lancer, 440),
        even(UnitKind::Sentinel, 440),
    )];
    let fights = mixed(&preset, NonZeroUsize::new(1).unwrap()).unwrap();
    assert_eq!(fights.len(), 1);
    assert_eq!(fights[0].name, "Lancers v Sentinels");
    assert_eq!(fights[0].trade, fight(&preset[0].1, &preset[0].2).unwrap());
}

#[test]
fn presets_fit_the_field_at_about_even_price() {
    let value = |army: &Army| -> u32 {
        army.iter()
            .map(|(kind, count)| kind.stats().cost * count)
            .sum()
    };
    for (name, first, second) in presets() {
        assert!(field(&first, &second).is_ok(), "{name}");
        let (first, second) = (value(&first), value(&second));
        assert!(
            first.abs_diff(second) * 100 <= first.max(second) * 25,
            "{name}: {first} v {second}"
        );
    }
}

#[test]
fn the_report_shows_each_budget_and_the_mixed_fights() {
    let lab = Lab {
        matrix: Matrix {
            kinds: vec![UnitKind::Lancer, UnitKind::Talon],
            budgets: vec![1_400],
            trades: vec![vec![
                vec![Trade::default(), Trade { killed: 0, lost: 0 }],
                vec![
                    Trade {
                        killed: 30,
                        lost: 0,
                    },
                    Trade::default(),
                ],
            ]],
        },
        mixed: vec![Mixed {
            name: "Lancers v Wardens".to_owned(),
            first: even(UnitKind::Lancer, 440),
            second: even(UnitKind::Warden, 440),
            trade: Trade {
                killed: 30,
                lost: 20,
            },
        }],
    };
    let report = report(&lab);
    assert!(report.contains("1400 scrap a side"), "{report}");
    assert!(report.contains(" Lance Talon"), "{report}");
    assert!(report.contains("inf"), "{report}");
    assert!(report.contains("Lancers v Wardens"), "{report}");
    assert!(report.contains("1.5"), "{report}");
}

#[test]
fn a_fight_s_ratio_is_held_within_an_eighth_and_eight_times() {
    let ratio_of = |killed, lost| ratio(Trade { killed, lost });
    assert_eq!(ratio_of(0, 0), 1_000, "neither touched the other");
    assert_eq!(ratio_of(30, 0), 8_000);
    assert_eq!(ratio_of(0, 30), 125);
    assert_eq!(ratio_of(300, 200), 1_500);
    assert_eq!(ratio_of(100, 1), 8_000);
}

#[test]
fn the_geometric_mean_is_the_root_of_the_product() {
    assert_eq!(geometric_mean(&[1_000, 4_000]), 2_000);
    assert_eq!(geometric_mean(&[125, 8_000]), 1_000);
    assert_eq!(geometric_mean(&[1_500]), 1_500);
}

#[test]
fn the_chart_source_carries_the_fingerprint_and_every_row() {
    let (_, chart) = aftermine_opponent::chart();
    let trades: Vec<Vec<u16>> = chart.iter().map(|row| row.to_vec()).collect();
    let source = chart_source(&trades);
    let stats = format!("{:016x}", aftermine_opponent::fingerprint());
    assert!(
        source.contains(&format!("0x{}_{}", &stats[..4], &stats[4..8])),
        "{source}"
    );
    let rows = source
        .lines()
        .filter(|line| line.starts_with("    ["))
        .count();
    assert_eq!(rows, aftermine_opponent::MEASURED.len(), "{source}");
}

#[test]
fn the_check_accepts_the_checked_in_chart_and_refuses_another() {
    let (_, chart) = aftermine_opponent::chart();
    let mut trades: Vec<Vec<u16>> = chart.iter().map(|row| row.to_vec()).collect();
    check(&trades).unwrap();
    trades[0][1] += 1;
    let error = check(&trades).unwrap_err();
    assert!(error.to_string().contains("Scuttler row"), "{error}");
}

#[test]
fn a_unit_already_attacking_keeps_its_attack() {
    let mut state = field(&even(UnitKind::Lancer, 440), &even(UnitKind::Warden, 440))
        .unwrap()
        .build()
        .unwrap();
    let attacking = |state: &State| -> Vec<aftermine_sim::UnitId> {
        state
            .units()
            .iter()
            .filter(|unit| matches!(unit.order, aftermine_sim::Order::Attack { .. }))
            .map(|unit| unit.id)
            .collect()
    };
    while attacking(&state).is_empty() {
        assert!(
            state.current_tick() < TICK_LIMIT,
            "premise: the armies engage"
        );
        let orders = orders(&state);
        state.tick(&orders);
    }
    let hunting: Vec<aftermine_sim::UnitId> = orders(&state)
        .into_iter()
        .flat_map(|order| match order.command {
            Command::Hunt { units, .. } => units,
            _ => Vec::new(),
        })
        .collect();
    assert!(
        attacking(&state).iter().all(|unit| !hunting.contains(unit)),
        "an attack under way goes on"
    );
    assert_eq!(
        hunting.len() + attacking(&state).len(),
        state.units().len(),
        "every other unit hunts"
    );
}
