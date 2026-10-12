use super::*;

#[test]
fn a_mix_is_legal_when_one_body_of_each_kind_fits_the_budget() {
    use UnitKind::{Breaker, Lancer, Sentinel};
    let kinds = [Sentinel, Lancer, Breaker];
    let names = |mixes: Vec<Mix>| mixes.iter().map(Mix::name).collect::<Vec<_>>();
    // One of each costs 1,100, so a Breaker joins a three-kind mix at 1,800
    // even though a third of the budget cannot buy one.
    assert_eq!(
        names(Mix::all(&kinds, 3, 1_800, false)),
        [
            "sentinel",
            "lancer",
            "breaker",
            "sentinel + lancer",
            "sentinel + breaker",
            "lancer + breaker",
            "sentinel + lancer + breaker",
        ]
    );
    assert_eq!(
        names(Mix::all(&kinds, 3, 600, false)),
        ["sentinel", "lancer", "sentinel + lancer"]
    );
}

#[test]
fn an_army_lands_as_close_to_the_budget_as_whole_bodies_allow() {
    use UnitKind::{Bombard, Flakhound, Lancer, Sentinel, Skyhook};
    // Nine of each spends 1,800 exactly.
    assert_eq!(
        Mix::even(vec![Lancer, Sentinel]).force(1_800),
        Force::Army(vec![(Lancer, 9), (Sentinel, 9)])
    );
    // A fill that stopped when its next body would overshoot left Flakhound +
    // Skyhook at 520 against Bombard + Skyhook's 650.
    assert_eq!(Mix::even(vec![Bombard, Skyhook]).force(600).spent(), 650);
    assert_eq!(Mix::even(vec![Flakhound, Skyhook]).force(600).spent(), 610);
    // No legal army strays from the budget by more than half its costliest
    // body.
    for budget in [600, 1_800, 5_000] {
        for mix in Mix::all(&crate::duel_lab::armed_units(), 3, budget, true) {
            let army = mix.force(budget);
            let costliest = mix.kinds.iter().map(|k| k.stats().cost).max().unwrap();
            assert!(
                army.spent().abs_diff(budget) <= costliest / 2,
                "{} at {budget}: {army:?}",
                mix.name()
            );
        }
    }
}

#[test]
fn skewed_splits_give_each_kind_a_double_share_once() {
    use UnitKind::{Lancer, Sentinel};
    let mixes = Mix::all(&[Sentinel, Lancer], 2, 1_800, true);
    let names: Vec<String> = mixes.iter().map(Mix::name).collect();
    assert_eq!(
        names,
        [
            "sentinel",
            "lancer",
            "sentinel + lancer",
            "sentinel + lancer (2:1)",
            "sentinel + lancer (1:2)",
        ]
    );
    // A split that fields an army already listed is not a new candidate.
    let armies: Vec<Force> = mixes.iter().map(|m| m.force(1_800)).collect();
    for (i, army) in armies.iter().enumerate() {
        assert!(!armies[..i].contains(army), "{army:?} repeats");
    }
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
        false,
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
        table.contains("composition search at 600 scrap: 6 even-split mixes"),
        "{table}"
    );
    serde_json::to_string(&outcome).expect("the outcome serializes");
}

#[test]
fn a_heavy_mix_propped_up_by_small_partners_is_not_dominant() {
    use UnitKind::{Breaker, Lancer, Talon};
    let member = |kinds: Vec<UnitKind>, weight, counter_score| {
        let mix = Mix::even(kinds);
        Blended {
            army: mix.force(1_800),
            mix,
            weight,
            counter: Mix::even(vec![Lancer]),
            counter_score,
        }
    };
    // Lancers beat the Breaker alone; only the 1% Talon partner covers it.
    let propped = [
        member(vec![Breaker], 0.99, 0.3),
        member(vec![Talon], 0.01, 0.5),
    ];
    assert!(!dominant(&propped));
    assert!(dominant(&[member(vec![Breaker], 1.0, 0.0)]));
}
