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
    // 900 apiece: 8.2 Lancers round to 8, 10 Sentinels exactly.
    assert_eq!(army, vec![(Lancer, 8), (Sentinel, 10)]);
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
