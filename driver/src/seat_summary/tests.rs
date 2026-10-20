use super::*;

fn evidence(idle: u64, income: [(u64, u32, u32); 2]) -> serde_json::Value {
    serde_json::json!({
        "seat": 0,
        "failures": {
            "repeated_orders": {"incidents": 1, "examples": []},
            "abandoned_sites": {"incidents": 0, "examples": []},
            "starved_production": {"incidents": 0, "examples": []},
            "stuck_missions": {"incidents": 0, "examples": []},
            "idle_army": {"incidents": idle, "examples": []},
        },
        "income": income
            .iter()
            .map(|(tick, actual, saturation)| serde_json::json!({
                "tick": tick,
                "actual_per_minute": actual,
                "saturation_per_minute": saturation,
            }))
            .collect::<Vec<_>>(),
    })
}

fn row(teams: &[u8], difficulties: &[&str], idle: u64) -> String {
    serde_json::json!({
        "duration_ticks": 9_000,
        "seats": teams
            .iter()
            .zip(difficulties)
            .enumerate()
            .map(|(seat, (team, difficulty))| serde_json::json!({
                "seat": seat,
                "team": team,
                "config": {"difficulty": difficulty},
            }))
            .collect::<Vec<_>>(),
        "evidence": teams
            .iter()
            .map(|_| evidence(idle, [(6_000, 900, 1_000), (12_000, 600, 1_200)]))
            .collect::<Vec<_>>(),
    })
    .to_string()
}

#[test]
fn rows_pool_by_layout_and_difficulty_from_the_lowest_rung_up() {
    let dir = std::env::temp_dir().join(format!("oxide-seat-summary-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rows.jsonl");
    let rows = [
        row(&[0, 1], &["prime", "standard"], 2),
        row(&[0, 1], &["prime", "standard"], 0),
        row(&[0, 0, 1, 1], &["standard"; 4], 1),
        row(&[0, 0, 1], &["standard"; 3], 0),
        row(&[0, 0, 1, 1, 2, 2], &["standard"; 6], 0),
    ];
    std::fs::write(&path, rows.join("\n")).unwrap();
    let summary = summarize(&[path]).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(summary.legs, 5);
    let groups: Vec<(&str, u32, u64, u64)> = summary
        .groups
        .iter()
        .map(|(label, seats)| {
            (
                label.as_str(),
                seats.seat_legs,
                seats.repeated_orders,
                seats.idle_army,
            )
        })
        .collect();
    assert_eq!(
        groups,
        [
            ("2v1 standard", 3, 3, 0),
            ("2v2v2 standard", 6, 6, 0),
            ("duel standard", 2, 2, 2),
            ("duel prime", 2, 2, 2),
            ("teams standard", 4, 4, 4),
        ]
    );
    let income = &summary.groups[2].1.income;
    assert_eq!(income.len(), 2);
    assert_eq!(
        (income[0].samples, income[0].percent_of_saturation),
        (2, Some(90))
    );
    let text = summary.render();
    assert!(text.contains("failure incidents"), "{text}");
    assert!(text.contains("teams standard"), "{text}");
}

/// A seat's ledger over `minutes`: worth 1,000, rising to 2,000 from minute
/// `rise` when given.
fn ledger(minutes: u64, rise: Option<u64>) -> serde_json::Value {
    let end = minutes * MINUTE;
    let worth = |tick: u64| {
        if rise.is_some_and(|rise| tick >= rise * MINUTE) {
            2_000
        } else {
            1_000
        }
    };
    serde_json::to_value(SeatLedger {
        worth: (1..=end / crate::ledger::WORTH_PERIOD)
            .map(|period| period * crate::ledger::WORTH_PERIOD)
            .map(worth)
            .collect(),
        end,
        final_worth: worth(end),
        ..SeatLedger::default()
    })
    .unwrap()
}

/// A duel that lasted `minutes`, whose winner pulled ahead from minute 4.
fn duel(difficulties: [&str; 2], stances: [&str; 2], winner: Option<u8>, minutes: u64) -> String {
    serde_json::json!({
        "duration_ticks": minutes * MINUTE,
        "result": winner.map(|team| serde_json::json!({"outcome": "victory", "team": team})),
        "seats": (0..2)
            .map(|seat| serde_json::json!({
                "seat": seat,
                "team": seat,
                "config": {"difficulty": difficulties[seat], "stance": stances[seat]},
            }))
            .collect::<Vec<_>>(),
        "evidence": (0..2u8)
            .map(|seat| serde_json::json!({
                "ledger": ledger(minutes, (winner == Some(seat)).then_some(4)),
            }))
            .collect::<Vec<_>>(),
    })
    .to_string()
}

#[test]
fn outcomes_pool_by_layout_and_difficulties_with_seat_stance_and_pacing_records() {
    let dir = std::env::temp_dir().join(format!("oxide-seat-outcomes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rows.jsonl");
    let rows = [
        duel(["prime", "prime"], ["aggressive", "turtle"], Some(0), 20),
        duel(["prime", "prime"], ["turtle", "aggressive"], Some(1), 16),
        duel(["prime", "prime"], ["balanced", "balanced"], Some(0), 10),
        duel(["prime", "prime"], ["balanced", "balanced"], None, 50),
        duel(["standard", "prime"], ["balanced", "turtle"], Some(1), 12),
    ];
    std::fs::write(&path, rows.join("\n")).unwrap();
    let summary = summarize(&[path]).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    let labels: Vec<&str> = summary
        .outcomes
        .iter()
        .map(|(label, _)| label.as_str())
        .collect();
    assert_eq!(labels, ["duel standard v prime", "duel prime"]);
    let prime = &summary.outcomes[1].1;
    assert_eq!((prime.legs, prime.decided, prime.won), (4, 3, 3));
    assert_eq!(prime.median_decided_ticks, Some(16 * MINUTE));
    assert_eq!(prime.first_seat_won, 2);
    // The Balanced mirror counts for neither side.
    let records: Vec<(BotStance, u32, u32)> = prime
        .stances
        .iter()
        .map(|record| (record.stance, record.seat_legs, record.won))
        .collect();
    assert_eq!(
        records,
        [
            (BotStance::Turtle, 2, 0),
            (BotStance::Balanced, 0, 0),
            (BotStance::Aggressive, 2, 2),
        ]
    );
    assert_eq!((prime.pacing.matches, prime.pacing.won), (4, 3));
    assert_eq!(
        prime.pacing.leads[0].held, 3,
        "every winner led at minute 5"
    );
    assert_eq!(prime.pacing.comebacks, 0);
    let mixed = &summary.outcomes[0].1;
    assert_eq!((mixed.won, mixed.first_seat_won), (1, 0));
    assert_eq!(
        mixed.stances[0].won, 1,
        "the Prime seat played Turtle and won"
    );

    let text = summary.render();
    for expected in [
        "  duel prime                   4        3        16.0       2/3 (67%)",
        "stances against other stances, duel prime: turtle won 0/2 (0%), aggressive won 2/2 (100%)",
        "net worth pacing, duel prime:\n  3 of 4 matches won",
    ] {
        assert!(text.contains(expected), "{expected:?} in\n{text}");
    }
}
