use super::*;

#[test]
fn mixes_split_the_budget_and_leave_out_kinds_a_share_cannot_buy() {
    use UnitKind::{Breaker, Lancer, Sentinel};
    let kinds = [Sentinel, Lancer, Breaker];
    let names = |mixes: Vec<Mix>| mixes.iter().map(Mix::name).collect::<Vec<_>>();
    // A third of 1,800 buys no Breaker, so no three-kind mix holds one.
    assert_eq!(
        names(Mix::all(&kinds, 3, 1_800)),
        [
            "sentinel",
            "lancer",
            "breaker",
            "sentinel + lancer",
            "sentinel + breaker",
            "lancer + breaker",
        ]
    );
    // A third of 600 buys no Breaker, and neither does all of it.
    assert_eq!(
        names(Mix::all(&kinds, 3, 600)),
        ["sentinel", "lancer", "sentinel + lancer"]
    );
    let Force::Army(army) = Mix(vec![Lancer, Sentinel]).force(1_800) else {
        panic!("a mix fields an army");
    };
    // Bodies fill until another would overshoot more than it closes, with
    // the kinds' scrap kept within one body of each other.
    let spends: Vec<u32> = army.iter().map(|&(k, n)| k.stats().cost * n).collect();
    let total: u32 = spends.iter().sum();
    assert!(total.abs_diff(1_800) <= 55, "{army:?}");
    assert!(spends[0].abs_diff(spends[1]) <= 110, "{army:?}");
}

#[test]
fn a_pairing_fields_equal_spend_even_for_coarse_mixes() {
    use UnitKind::{Avalanche, Condor};
    // Rounding each kind alone fields three Condors (2,100) against one
    // Condor and one Avalanche (1,400).
    let (a, b) = Mix::matched(&Mix(vec![Condor]), &Mix(vec![Condor, Avalanche]), 1_800);
    assert_eq!(a.spent(), b.spent(), "{a:?} vs {b:?}");
}

#[test]
fn the_solver_finds_a_cycle_and_a_dominant_strategy() {
    // Rock, paper, scissors: an even blend.
    let rps = vec![
        vec![0.0, -1.0, 1.0],
        vec![1.0, 0.0, -1.0],
        vec![-1.0, 1.0, 0.0],
    ];
    for w in solve(&rps) {
        assert!((w - 1.0 / 3.0).abs() < 0.02, "{w}");
    }
    // The second strategy beats both others.
    let dominant = vec![
        vec![0.0, -0.5, 0.2],
        vec![0.5, 0.0, 0.4],
        vec![-0.2, -0.4, 0.0],
    ];
    let w = solve(&dominant);
    assert!(w[1] > 0.99, "{w:?}");
}

/// Payoffs from a fixed table, so the search runs without any duels.
fn from_table(table: &[Vec<f64>]) -> impl FnMut(&[(usize, usize)]) -> Result<Vec<f64>> + '_ {
    |pairs| Ok(pairs.iter().map(|&(i, j)| table[i][j]).collect())
}

#[test]
fn the_search_grows_its_pool_until_a_dominant_mix_settles_it() {
    // Candidate 3 beats every other; the pool starts without it.
    let table = vec![
        vec![0.0, 0.3, -0.1, -0.6],
        vec![-0.3, 0.0, 0.2, -0.7],
        vec![0.1, -0.2, 0.0, -0.5],
        vec![0.6, 0.7, 0.5, 0.0],
    ];
    let settled = search(4, &[0, 1, 2], from_table(&table)).expect("the search runs");
    assert_eq!(settled.pool, vec![0, 1, 2, 3]);
    let three = settled.pool.iter().position(|&m| m == 3).expect("3 joined");
    assert!(settled.weights[three] > 0.99, "{:?}", settled.weights);
    assert!(settled.challenger.1 <= SETTLED);
    assert_eq!(settled.rounds, 2);
}

#[test]
fn the_search_settles_on_a_cycle_when_mixes_counter_each_other() {
    // 0, 1 and 2 form a cycle; 3 loses to all of them.
    let table = vec![
        vec![0.0, -0.4, 0.4, 0.3],
        vec![0.4, 0.0, -0.4, 0.3],
        vec![-0.4, 0.4, 0.0, 0.3],
        vec![-0.3, -0.3, -0.3, 0.0],
    ];
    let settled = search(4, &[3, 0], from_table(&table)).expect("the search runs");
    assert!(!settled.pool.contains(&3) || settled.weights[0] < 0.01);
    let supported = settled.weights.iter().filter(|&&w| w > 0.1).count();
    assert_eq!(supported, 3, "{:?} {:?}", settled.pool, settled.weights);
    assert!(settled.challenger.1 <= SETTLED);
}

#[test]
fn a_small_search_reports_its_blend() {
    let jobs = NonZeroUsize::new(4).expect("four workers");
    let outcome = run(
        &[UnitKind::Sentinel, UnitKind::Lancer, UnitKind::Bombard],
        2,
        600,
        4_000,
        jobs,
    )
    .expect("the search runs");
    assert_eq!(outcome.mixes, 6);
    assert!(!outcome.blend.is_empty());
    let weights: f64 = outcome.blend.iter().map(|b| b.weight).sum();
    assert!(weights > 0.95 && weights <= 1.0 + 1e-9, "{weights}");
    let table = render(std::slice::from_ref(&outcome));
    assert!(
        table.contains("composition search at 600 scrap: 6 mixes"),
        "{table}"
    );
    serde_json::to_string(&outcome).expect("the outcome serializes");
}

#[test]
fn a_heavy_mix_propped_up_by_small_partners_is_not_dominant() {
    use UnitKind::{Breaker, Lancer, Talon};
    let member = |kinds: Vec<UnitKind>, weight, counter_score| Blended {
        mix: Mix(kinds),
        weight,
        counter: Mix(vec![Lancer]),
        counter_score,
    };
    // Lancers beat the Breaker alone; only the 1% Talon partner covers it.
    let propped = [
        member(vec![Breaker], 0.99, 0.3),
        member(vec![Talon], 0.01, 0.5),
    ];
    assert!(!dominant(&propped));
    assert!(dominant(&[member(vec![Breaker], 1.0, 0.0)]));
}
