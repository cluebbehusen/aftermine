use super::*;

/// A seat's ledger from tick zero to `end` whose net worth at each sample
/// is `worth(tick)`, ending at `worth(end)`.
fn ledger(end: u64, worth: impl Fn(u64) -> u64) -> SeatLedger {
    SeatLedger {
        worth: (1..=end / WORTH_PERIOD)
            .map(|period| period * WORTH_PERIOD)
            .map(&worth)
            .collect(),
        end,
        final_worth: worth(end),
        ..SeatLedger::default()
    }
}

fn duel(a: &SeatLedger, b: &SeatLedger, result: Option<GameResult>) -> Pacing {
    pacing(&[a, b], &[0, 1], result).expect("two sampled sides")
}

fn won_by(team: u8) -> GameResult {
    GameResult::Victory { team }
}

#[test]
fn a_side_that_pulls_ahead_and_wins_settles_and_holds_its_lead() {
    // Even until minute 7, then team 0 pulls away and wins at minute 20.
    let end = 20 * MINUTE;
    let a = ledger(end, |tick| if tick < 7 * MINUTE { 1_000 } else { 2_000 });
    let b = ledger(end, |_| 1_000);
    let game = duel(&a, &b, Some(won_by(0)));
    assert_eq!(
        game.leads,
        [
            LeadAt {
                minute: 5,
                team: None
            },
            LeadAt {
                minute: 10,
                team: Some(0)
            },
            LeadAt {
                minute: 15,
                team: Some(0)
            },
        ]
    );
    assert_eq!(game.lead_changes, 0);
    assert_eq!(game.winner_low, Some(500));
    assert!(!game.comeback());
    // The first sample at or after minute 7 is tick 9,000.
    assert_eq!(game.settled_at, Some(9_000));
    assert_eq!(game.played_out(), Some(625));
}

#[test]
fn a_winner_that_fell_well_behind_after_the_opening_made_a_comeback() {
    // Team 1 leads from minute 4 to 9, then team 0 takes over and wins.
    let end = 16 * MINUTE;
    let a = ledger(end, |tick| match tick {
        tick if tick < 4 * MINUTE => 1_000,
        tick if tick < 9 * MINUTE => 650,
        _ => 1_500,
    });
    let b = ledger(end, |_| 1_000);
    let game = duel(&a, &b, Some(won_by(0)));
    assert_eq!(game.lead_changes, 1);
    assert_eq!(game.winner_low, Some(393), "650 of 1,650");
    assert!(game.comeback());
    assert_eq!(game.leads[0].team, Some(1), "team 1 was ahead at minute 5");
    assert_eq!(game.leads[1].team, Some(0));
}

#[test]
fn swings_in_the_opening_count_for_nothing() {
    let end = 12 * MINUTE;
    let a = ledger(end, |tick| if tick < 3 * MINUTE { 200 } else { 1_000 });
    let b = ledger(end, |tick| if tick < 3 * MINUTE { 900 } else { 800 });
    let game = duel(&a, &b, Some(won_by(0)));
    assert_eq!(game.lead_changes, 0, "team 1's opening lead is ignored");
    assert_eq!(game.winner_low, Some(555));
    assert!(!game.comeback());
    assert_eq!(
        game.settled_at,
        Some(4_000),
        "the first sample after minute 3"
    );
}

#[test]
fn a_side_falling_back_to_even_keeps_the_lead_until_another_is_ahead() {
    let end = 14 * MINUTE;
    let a = ledger(end, |tick| match tick {
        tick if tick < 5 * MINUTE => 1_300,
        tick if tick < 8 * MINUTE => 1_000,
        _ => 1_300,
    });
    let b = ledger(end, |_| 1_000);
    let game = duel(&a, &b, Some(won_by(0)));
    assert_eq!(game.lead_changes, 0);
    assert_eq!(
        game.settled_at,
        Some(10_000),
        "the first sample after minute 8"
    );
}

#[test]
fn shares_are_judged_against_an_equal_split_and_teams_sum_their_seats() {
    // Three teams, the first of two seats: 1.1 times an equal split is
    // 36.7% of all worth.
    let end = 11 * MINUTE;
    let half = ledger(end, |_| 170);
    let other = ledger(end, |_| 300);
    let even = pacing(
        &[&half, &half, &other, &other],
        &[0, 0, 1, 2],
        Some(won_by(0)),
    )
    .unwrap();
    assert_eq!(even.sides, 3);
    assert_eq!(even.leads[1].team, None, "340 of 940 is under 36.7%");
    let strong = ledger(end, |_| 250);
    let ahead = pacing(
        &[&strong, &strong, &other, &other],
        &[0, 0, 1, 2],
        Some(won_by(0)),
    )
    .unwrap();
    assert_eq!(ahead.leads[1].team, Some(0), "500 of 1,100 is 45%");
    let behind = ledger(end, |_| 100);
    let low = pacing(
        &[&behind, &behind, &other, &other],
        &[0, 0, 1, 2],
        Some(won_by(0)),
    )
    .unwrap();
    assert_eq!(low.winner_low, Some(250));
    assert!(low.comeback(), "a quarter is at most 0.8 of a third");
}

#[test]
fn tied_leaders_are_not_ahead() {
    let end = 6 * MINUTE;
    let big = ledger(end, |_| 400);
    let small = ledger(end, |_| 200);
    let game = pacing(&[&big, &big, &small], &[0, 1, 2], None).unwrap();
    assert_eq!(game.leads[0].team, None);
}

#[test]
fn an_undecided_match_records_leads_but_no_winner() {
    let end = 50 * MINUTE;
    let a = ledger(end, |_| 1_400);
    let b = ledger(end, |_| 1_000);
    let game = duel(&a, &b, None);
    assert_eq!(game.leads.len(), 3);
    assert_eq!(game.winner, None);
    assert_eq!((game.winner_low, game.settled_at), (None, None));
    let draw = duel(&a, &b, Some(GameResult::Draw));
    assert_eq!(draw.winner, None);
}

#[test]
fn only_minutes_the_recording_covers_record_a_lead() {
    let short = 12 * MINUTE;
    let game = duel(
        &ledger(short, |_| 1),
        &ledger(short, |_| 1),
        Some(won_by(1)),
    );
    assert_eq!(
        game.leads
            .iter()
            .map(|lead| lead.minute)
            .collect::<Vec<_>>(),
        [5, 10]
    );
    // A recording restored at minute 8 has no minute-5 sample.
    let mut late = ledger(20 * MINUTE, |_| 1);
    late.start = 8 * MINUTE;
    late.worth.truncate(10);
    let game = duel(&late, &late.clone(), Some(won_by(1)));
    assert_eq!(
        game.leads
            .iter()
            .map(|lead| lead.minute)
            .collect::<Vec<_>>(),
        [10, 15]
    );
}

#[test]
fn a_ledger_without_samples_or_a_single_team_has_no_pacing() {
    let empty = SeatLedger::default();
    assert_eq!(pacing(&[&empty, &empty], &[0, 1], None), None);
    let seat = ledger(6 * MINUTE, |_| 10);
    assert_eq!(pacing(&[&seat, &seat], &[0, 0], Some(won_by(0))), None);
    assert_eq!(pacing(&[&seat], &[0, 1], None), None);
}

#[test]
fn the_summary_counts_held_leads_comebacks_changes_and_play_after_settling() {
    let end = 20 * MINUTE;
    let even = ledger(end, |_| 1_000);
    let early = ledger(end, |tick| if tick < 4 * MINUTE { 1_000 } else { 2_000 });
    let comeback = ledger(end, |tick| if tick < 12 * MINUTE { 600 } else { 2_000 });
    let games = [
        duel(&early, &even, Some(won_by(0))),
        duel(&comeback, &even, Some(won_by(0))),
        duel(&early, &even, Some(won_by(1))),
        duel(&early, &even, None),
    ];
    let summary = summarize(&games);
    assert_eq!((summary.matches, summary.won), (4, 3));
    assert_eq!(
        summary.leads,
        [
            LeadRow {
                minute: 5,
                ahead: 3,
                held: 1
            },
            LeadRow {
                minute: 10,
                ahead: 3,
                held: 1
            },
            LeadRow {
                minute: 15,
                ahead: 3,
                held: 2
            },
        ]
    );
    // The second winner fell to 37.5%, the third to a third.
    assert_eq!(summary.comebacks, 2);
    assert_eq!((summary.lead_changes, summary.changed), (1, 1));
    // Settled at 5,000 (791 per mille played out) and at 15,000 (375); the
    // upper median of two is the later share.
    assert_eq!(summary.settled, 2);
    assert_eq!(summary.played_out, Some(791));
    let mut text = String::new();
    render(&mut text, &summary, "  ");
    for expected in [
        "  3 of 4 matches won; ahead means 1.1 times an equal split of net worth (55% in a duel)",
        "  ahead at minute 15: 3 won matches, the side ahead won 2 (67%)",
        "  comebacks: 2 of 3 winners fell to 0.8 times an equal split (40% in a duel) after minute 3 (67%)",
        "  lead changes: 0.33 per won match; 1 of 3 had any (33%)",
        "  settled: 2 of 3 winners stayed ahead to the end; median 79% of the match played after that",
    ] {
        assert!(text.contains(expected), "{expected:?} in\n{text}");
    }
}
