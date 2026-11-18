use super::*;

#[test]
fn a_settled_interval_names_its_side() {
    assert_eq!(standing(&[0.9; 12], 0.03).verdict, Verdict::Better);
    assert_eq!(
        standing(&[0.1, 0.2, 0.15, 0.1, 0.2, 0.1], 0.03).verdict,
        Verdict::Worse
    );
    let even: Vec<f64> = (0..400)
        .map(|i| if i % 2 == 0 { 0.49 } else { 0.51 })
        .collect();
    assert_eq!(standing(&even, 0.03).verdict, Verdict::NoDifference);
    assert_eq!(standing(&[0.0, 1.0, 0.0, 1.0], 0.03).verdict, Verdict::Open);
    assert_eq!(standing(&[1.0], 0.03).verdict, Verdict::Open);
}

#[test]
fn a_pair_plays_both_seats_at_one_seed() {
    let plan = AbPlan {
        maps: vec![("skirmish".to_string(), Scenario::skirmish())],
        difficulties: vec![BotDifficulty::Prime],
        stances: vec![BotStance::Balanced],
        seed_base: 9000,
        tick_limit: 240,
        max_pairs: 1,
        margin: 0.03,
    };
    let mut seen = Vec::new();
    let standing = run(&plan, NonZeroUsize::new(2).unwrap(), |legs, _| {
        seen.extend(legs.iter().cloned());
        Ok(())
    })
    .unwrap();
    assert_eq!(standing.pairs, 1);
    assert_eq!(
        seen.iter()
            .map(|leg| leg.candidate_seat)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    for leg in &seen {
        assert_eq!((leg.seed, leg.ticks, leg.winner), (9000, 240, None));
        assert!(
            leg.candidate_worth > 0 && leg.reference_worth > 0,
            "{leg:?}"
        );
        assert!((0.0..=1.0).contains(&leg.score));
    }
    // The reference is a copy of this bot, so mirrored legs mirror.
    assert_eq!(seen[0].candidate_worth, seen[1].reference_worth);
    assert_eq!(seen[0].reference_worth, seen[1].candidate_worth);
}
