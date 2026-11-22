use super::*;
use crate::bot_ladder::tests::pair;
use crate::seat_summary::SeatEvidence;
use BotDifficulty::{Prime, Standard};

/// A row whose digests name its game, so the same game in two runs plays
/// identically until a test changes it.
fn diff_row(scored: ScoredLadderRow) -> DiffRow {
    let game = format!("{} {}", scored.ladder.run, scored.leg.name());
    DiffRow {
        final_hash: format!("final {game}"),
        command_hash: format!("commands {game}"),
        evaluation_fingerprint: format!("plan {game}"),
        scored,
    }
}

/// Seat-swapped pairs, one a run, each leg won by the rung given.
fn rows(pairs: &[(Option<BotDifficulty>, Option<BotDifficulty>)]) -> Vec<DiffRow> {
    pairs
        .iter()
        .zip(0..)
        .flat_map(|(&(forward, swapped), run)| pair(run, forward, swapped))
        .map(diff_row)
        .collect()
}

fn error(base: &[DiffRow], branch: &[DiffRow]) -> String {
    diff(base, branch).unwrap_err().to_string()
}

#[test]
fn the_same_rows_play_every_game_identically() {
    let base = rows(&[(Some(Prime), Some(Prime)), (Some(Prime), Some(Standard))]);
    let report = diff(&base, &base).unwrap();
    assert_eq!((report.games, report.identical, report.changed), (4, 4, 0));
    let [comparison] = &report.comparisons[..] else {
        panic!("{report:?}");
    };
    assert_eq!((comparison.games, comparison.identical), (4, 4));
    assert!(comparison.flips.is_empty());
    assert_eq!(comparison.base, comparison.branch);
    assert!(
        comparison.worth.is_empty(),
        "ledgers with no worth samples give no change"
    );
    assert!(
        report
            .rungs
            .iter()
            .flat_map(|rung| &rung.measures)
            .all(|measure| measure.change.is_none_or(|change| change == 0.0))
    );
}

#[test]
fn a_changed_winner_is_a_flip_with_its_direction() {
    let base = rows(&[(Some(Prime), Some(Prime)), (Some(Prime), Some(Standard))]);
    let mut branch = rows(&[(Some(Standard), Some(Prime)), (Some(Prime), None)]);
    for game in &mut branch[..1] {
        game.command_hash = "other commands".into();
    }
    branch[1].final_hash = "other final state".into();
    branch[0].scored.duration_ticks = 2_400;
    let report = diff(&base, &branch).unwrap();
    assert_eq!((report.identical, report.changed), (2, 2));
    let comparison = &report.comparisons[0];
    let flips: Vec<(
        u64,
        EvaluationLeg,
        Option<BotDifficulty>,
        Option<BotDifficulty>,
    )> = comparison
        .flips
        .iter()
        .map(|flip| (flip.run, flip.leg, flip.base_winner, flip.branch_winner))
        .collect();
    assert_eq!(
        flips,
        [
            (0, EvaluationLeg::Forward, Some(Prime), Some(Standard)),
            (1, EvaluationLeg::Swapped, Some(Standard), None),
        ]
    );
    assert_eq!(
        (comparison.base.higher_wins, comparison.base.decided),
        (3, 4)
    );
    assert_eq!(
        (comparison.branch.higher_wins, comparison.branch.decided),
        (2, 3)
    );
    let text = report.render();
    assert!(
        text.contains("skirmish balanced run 0 forward: prime won at 0:50 -> standard won at 2:00"),
        "{text}"
    );
    assert!(
        text.contains("run 1 swapped: standard won at 0:50 -> undecided at 0:50"),
        "{text}"
    );
}

#[test]
fn missing_repeated_and_replanned_games_are_refused() {
    let base = rows(&[(Some(Prime), Some(Prime)), (Some(Prime), Some(Prime))]);
    let error_text = error(&base, &base[..3]);
    assert!(
        error_text.contains("run 1, swapped leg is missing from the branch rows"),
        "{error_text}"
    );
    let error_text = error(&base[..3], &base);
    assert!(
        error_text.contains("missing from the base rows"),
        "{error_text}"
    );
    let mut repeated = base.clone();
    repeated.push(base[0].clone());
    let error_text = error(&base, &repeated);
    assert!(
        error_text.contains("run 0, forward leg repeats in the branch rows"),
        "{error_text}"
    );
    let mut replanned = base.clone();
    replanned[2].evaluation_fingerprint = "another plan".into();
    let error_text = error(&base, &replanned);
    assert!(
        error_text.contains("ladder skirmish prime against standard, balanced run 1, forward leg")
            && error_text.contains("fingerprints differ"),
        "{error_text}"
    );
}

/// Each seat's net worth, the same at every sample.
fn worth(game: &mut DiffRow, seats: [u64; 2]) {
    game.scored.evidence = seats
        .iter()
        .map(|&worth| {
            let mut evidence = SeatEvidence::default();
            evidence.ledger.worth = vec![worth; 6];
            evidence
        })
        .collect();
}

#[test]
fn the_worth_change_pairs_each_game_with_itself() {
    let base_rows: Vec<DiffRow> = pair(0, Some(Prime), Some(Prime))
        .into_iter()
        .map(diff_row)
        .collect();
    let mut base = base_rows.clone();
    let mut branch = base_rows;
    // Prime holds seat zero in the forward leg and seat one in the swapped.
    worth(&mut base[0], [600, 400]);
    worth(&mut branch[0], [700, 300]);
    worth(&mut base[1], [500, 500]);
    worth(&mut branch[1], [200, 800]);
    let report = diff(&base, &branch).unwrap();
    let changes = &report.comparisons[0].worth;
    assert_eq!(changes.len(), WORTH_TICKS.len());
    for change in changes {
        // Changes of 0.1 and 0.3: mean 0.2, standard error 0.1.
        assert_eq!(change.games, 2);
        assert!((change.mean - 0.2).abs() < 1e-9, "{change:?}");
        let [low, high] = change.interval.unwrap();
        assert!(
            (low - 0.004).abs() < 1e-9 && (high - 0.396).abs() < 1e-9,
            "{change:?}"
        );
    }
    assert!(report.render().contains(
        "change in prime share of net worth, by game: 6k +20.0 pts [+0.4 to +39.6] (2 games)"
    ));
}

#[test]
fn rung_measures_sit_side_by_side_with_their_change() {
    let base = rows(&[(Some(Prime), Some(Prime))]);
    let mut branch = base.clone();
    for game in &mut branch {
        let seat = usize::from(game.scored.leg == EvaluationLeg::Swapped);
        game.scored.evidence[seat].failures.idle_army.incidents = 3;
        game.scored.evidence[seat].reactivity.anti_air.arose = 2;
        game.scored.evidence[seat].reactivity.anti_air.answered = 1;
    }
    let report = diff(&base, &branch).unwrap();
    let prime = report.rungs.iter().find(|rung| rung.rung == Prime).unwrap();
    assert_eq!((prime.base_seat_games, prime.branch_seat_games), (2, 2));
    let measure = |name: &str| {
        prime
            .measures
            .iter()
            .find(|measure| measure.name == name)
            .unwrap()
    };
    let idle = measure("idle army per seat-game");
    assert_eq!(
        (idle.base.value, idle.branch.value, idle.change),
        (Some(0.0), Some(3.0), Some(3.0))
    );
    let anti_air = measure("anti-air answered");
    assert_eq!(
        (
            anti_air.base.value,
            anti_air.branch.value,
            anti_air.branch.over
        ),
        (None, Some(0.5), 4)
    );
    assert_eq!(anti_air.change, None);
    let standard = report
        .rungs
        .iter()
        .find(|rung| rung.rung == Standard)
        .unwrap();
    assert!(
        standard
            .measures
            .iter()
            .all(|measure| measure.change.is_none_or(|change| change == 0.0))
    );
    let text = report.render();
    assert!(text.contains("ladder: prime seats, 2 seat-games on base, 2 on the branch"));
    assert!(text.contains("anti-air answered"), "{text}");
    assert!(
        !text.contains("cradle answered"),
        "a situation that never arose is left out: {text}"
    );
}

#[test]
fn published_ladder_rows_load_and_diff_against_themselves() {
    use crate::bot_ladder::{LadderManifest, LadderOptions, expand, run_ladder};
    let manifest: LadderManifest = serde_json::from_value(serde_json::json!({
        "name": "ladder",
        "tick_limit": 30,
        "runs": 1,
        "personality_seed_base": 0,
        "stances": ["balanced"],
        "comparisons": [{"higher": "prime", "lower": "standard", "gate": 650}],
        "min_decided_pairs": 1,
        "maps": [{"path": "skirmish", "family": "open"}],
    }))
    .unwrap();
    let legs = expand(&manifest, &[aftermine_sim::Scenario::skirmish()]).unwrap();
    let published = run_ladder(
        &legs,
        manifest.tick_limit,
        &LadderOptions {
            candidate: "test",
            jobs: std::num::NonZeroUsize::MIN,
            replay_dir: None,
        },
    )
    .unwrap();
    let dir = std::env::temp_dir().join(format!("ladder-diff-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rows.jsonl");
    let text: String = published
        .iter()
        .map(|row| serde_json::to_string(row).unwrap() + "\n")
        .collect();
    std::fs::write(&path, text).unwrap();
    let loaded = load(&path);
    std::fs::remove_dir_all(&dir).ok();
    let rows = loaded.unwrap();
    let report = diff(&rows, &rows).unwrap();
    assert_eq!((report.games, report.identical), (2, 2));
}
