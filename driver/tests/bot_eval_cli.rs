//! End-to-end contract coverage for `aftermine-driver bot-eval`.

#[path = "support/cli.rs"]
mod cli;

use cli::{Scratch, driver, jsonl, refused, succeeded};
use serde_json::Value;

#[test]
fn retired_controller_flags_are_rejected() {
    for flag in [
        "--against-overseer",
        "--overseer-policy-seed",
        "--controller",
        "--opponent-controller",
    ] {
        assert_skirmish_bot_eval_refuses(&[flag], &["unexpected argument", flag]);
    }
}

#[test]
fn replay_evidence_requires_its_jsonl_sidecar() {
    let dir = Scratch::new("bot-eval-replay-sidecar");
    let stderr = refused(
        driver()
            .args([
                "bot-eval",
                "skirmish",
                "--ticks",
                "1",
                "--candidate",
                "candidate-a",
                "--replay-dir",
            ])
            .arg(dir.join("replays")),
    );
    assert!(
        stderr.contains("--out") && stderr.contains("required"),
        "replay-only refusal should name the required sidecar: {stderr}"
    );
    assert!(!dir.join("replays").exists());
}

#[test]
fn bot_eval_supports_a_severed_ground_map() {
    succeeded(driver().args([
        "bot-eval",
        concat!(env!("CARGO_MANIFEST_DIR"), "/../scenarios/severance.json"),
        "--ticks",
        "1",
    ]));
}

#[test]
fn a_zero_stall_loop_limit_disables_the_early_stop() {
    let rows = jsonl(&succeeded(driver().args([
        "bot-eval",
        "skirmish",
        "--ticks",
        "1",
        "--stall-loop-limit",
        "0",
    ])));
    let row = &rows[0];
    assert_eq!(row["stall_loop_limit"], Value::Null);
    assert_eq!(row["termination"], "tick_limit");
}

fn assert_skirmish_bot_eval_refuses(args: &[&str], expected: &[&str]) {
    let stderr = refused(
        driver()
            .args(["bot-eval", "skirmish", "--ticks", "1"])
            .args(args),
    );
    for fragment in expected {
        assert!(
            stderr.contains(fragment),
            "refusal did not contain {fragment:?}: {stderr}"
        );
    }
}

#[test]
fn paired_cell_emits_two_compact_rows_with_profiles_exchanged() {
    let rows = jsonl(&succeeded(driver().args([
        "bot-eval",
        "skirmish",
        "--ticks",
        "1",
        "--personality-seed-base",
        "40",
        "--difficulty",
        "prime",
        "--stance",
        "aggressive",
        "--paired",
    ])));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["leg"], "forward");
    assert_eq!(rows[1]["leg"], "swapped");
    for row in &rows {
        assert_eq!(row["candidate"], "ad-hoc");
        assert_eq!(row["tick_limit"], 1);
        assert!(
            row["scenario_fingerprint"]
                .as_str()
                .unwrap()
                .starts_with("fnv1a64:")
        );
        assert_eq!(row["duration_ticks"], 1);
        assert_eq!(row["termination"], "tick_limit");
        assert!(row["result"].is_null());
        assert_eq!(row["winner_seats"], serde_json::json!([]));
        assert!(row["final_hash"].as_str().unwrap().starts_with("0x"));
        assert_eq!(row["evidence"].as_array().unwrap().len(), 2);
    }

    let seed = |row: usize, seat: usize| {
        rows[row]["seats"][seat]["config"]["personality_seed"]
            .as_u64()
            .unwrap()
    };
    assert_eq!((seed(0, 0), seed(0, 1)), (40, 41));
    assert_eq!((seed(1, 0), seed(1, 1)), (41, 40));
    assert_eq!(
        rows[0]["seats"][0]["profile"], rows[1]["seats"][1]["profile"],
        "the complete resolved profile moves with its seed"
    );
    assert_eq!(
        rows[0]["seats"][1]["profile"], rows[1]["seats"][0]["profile"],
        "the complete resolved profile moves with its seed"
    );
}

#[test]
fn cross_difficulty_cells_share_personality_and_swap_complete_configs() {
    let rows = jsonl(&succeeded(driver().args([
        "bot-eval",
        "skirmish",
        "--ticks",
        "1",
        "--runs",
        "2",
        "--personality-seed-base",
        "40",
        "--difficulty",
        "prime",
        "--opponent-difficulty",
        "scrapheap",
        "--same-personality-seed",
        "--paired",
    ])));
    assert_eq!(rows.len(), 4);

    let (cells, remainder) = rows.as_chunks::<2>();
    assert!(remainder.is_empty());
    for (run, legs) in cells.iter().enumerate() {
        let expected_personality_seed = 40 + run as u64;
        assert_eq!(legs[0]["leg"], "forward");
        assert_eq!(legs[1]["leg"], "swapped");

        let config = |leg: usize, seat: usize| &legs[leg]["seats"][seat]["config"];
        assert_eq!(config(0, 0)["difficulty"], "prime");
        assert_eq!(config(0, 1)["difficulty"], "scrapheap");
        assert_eq!(config(1, 0)["difficulty"], "scrapheap");
        assert_eq!(config(1, 1)["difficulty"], "prime");
        for leg in 0..2 {
            for seat in 0..2 {
                assert_eq!(
                    config(leg, seat)["personality_seed"],
                    expected_personality_seed
                );
            }
        }
        assert_eq!(
            legs[0]["seats"][0]["profile"], legs[1]["seats"][1]["profile"],
            "the complete primary profile moves to the opposite seat"
        );
        assert_eq!(
            legs[0]["seats"][1]["profile"], legs[1]["seats"][0]["profile"],
            "the complete opponent profile moves to the opposite seat"
        );
    }
}

#[test]
fn every_persisted_row_replays_to_its_reported_terminal_state() {
    let dir = Scratch::new("bot-eval-replay-integrity");
    let replay_dir = dir.join("replays");
    let out = dir.join("rows.jsonl");
    succeeded(
        driver()
            .args([
                "bot-eval",
                "skirmish",
                "--ticks",
                "3",
                "--runs",
                "2",
                "--personality-seed-base",
                "40",
                "--difficulty",
                "prime",
                "--stance",
                "aggressive",
                "--opponent-difficulty",
                "veteran",
                "--opponent-stance",
                "turtle",
                "--paired",
                "--candidate",
                "candidate-a",
                "--out",
            ])
            .arg(&out)
            .arg("--replay-dir")
            .arg(&replay_dir),
    );

    let rows = jsonl(&std::fs::read_to_string(&out).unwrap());
    assert_eq!(rows.len(), 4, "two runs times two paired legs");

    for row in &rows {
        let replay_path = row["replay"]
            .as_str()
            .map(std::path::PathBuf::from)
            .expect("every persisted row points to its replay");
        assert!(replay_path.is_file(), "missing replay: {replay_path:?}");
        let replay = aftermine_kit::load_replay(&replay_path).expect("published replay loads");

        assert_eq!(replay.setup.name, row["scenario"]);
        assert_eq!(
            aftermine_driver::bot_eval::scenario_fingerprint(&replay.setup).unwrap(),
            row["scenario_fingerprint"]
        );
        assert_eq!(replay.meta.ticks, row["duration_ticks"].as_u64());

        let row_seats = row["seats"].as_array().expect("seat configurations");
        assert_eq!(row_seats.len(), replay.setup.players.len());
        for (seat, player) in replay.setup.players.iter().enumerate() {
            assert_eq!(row_seats[seat]["seat"], seat as u64);
            assert_eq!(
                row_seats[seat]["config"],
                serde_json::to_value(player.bot_config).unwrap(),
                "seat {seat} config differs from the associated replay"
            );
        }

        let state =
            aftermine_kit::runner::run_replay(&replay, None).expect("published replay reproduces");
        assert_eq!(state.current_tick(), row["duration_ticks"]);
        assert_eq!(
            aftermine_protocol::hash_hex(state.hash()),
            row["final_hash"],
            "row hash must come from its associated replay"
        );
        assert_eq!(serde_json::to_value(state.result()).unwrap(), row["result"]);
        assert_eq!(
            state
                .winners()
                .into_iter()
                .map(|seat| seat.0)
                .collect::<Vec<_>>(),
            serde_json::from_value::<Vec<u8>>(row["winner_seats"].clone()).unwrap()
        );
    }

    assert_eq!(std::fs::read_dir(&replay_dir).unwrap().count(), rows.len());
}

#[test]
fn opponent_override_refuses_an_ambiguous_multiseat_scenario() {
    let scenario =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scenarios/compass-grand.json");
    let stderr = refused(driver().arg("bot-eval").arg(scenario).args([
        "--ticks",
        "1",
        "--opponent-difficulty",
        "prime",
    ]));
    assert!(
        stderr.contains("require exactly two seats"),
        "multiseat refusal was unclear: {stderr}"
    );
}

#[test]
fn persisted_evidence_requires_an_explicit_candidate() {
    let dir = Scratch::new("bot-eval-candidate-required");
    let stderr = refused(
        driver()
            .args(["bot-eval", "skirmish", "--ticks", "1", "--out"])
            .arg(dir.join("rows.jsonl")),
    );
    assert!(stderr.contains("--candidate is required"));
    assert!(!dir.join("rows.jsonl").exists());
}

#[test]
fn a_later_invalid_scenario_publishes_no_partial_evidence() {
    let dir = Scratch::new("bot-eval-prevalidation");
    let invalid = dir.join("invalid.json");
    let shipped = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/scenarios/skirmish.json");
    let mut value: Value =
        serde_json::from_slice(&std::fs::read(shipped).unwrap()).expect("shipped JSON");
    value["players"] = serde_json::json!([]);
    std::fs::write(&invalid, serde_json::to_vec(&value).unwrap()).unwrap();
    let replay_dir = dir.join("replays");
    let out = dir.join("rows.jsonl");

    let stderr = refused(
        driver()
            .args(["bot-eval", "skirmish"])
            .arg(&invalid)
            .args(["--ticks", "1", "--candidate", "candidate-a", "--out"])
            .arg(&out)
            .arg("--replay-dir")
            .arg(&replay_dir),
    );
    assert!(
        stderr.contains("needs 1 to 16 players"),
        "unexpected refusal: {stderr}"
    );
    assert!(!out.exists(), "the JSONL index was not published");
    assert!(
        !replay_dir.exists() || std::fs::read_dir(&replay_dir).unwrap().next().is_none(),
        "the valid first scenario left replay or staging evidence behind"
    );
}

#[test]
fn overflowing_seed_ranges_publish_no_partial_evidence() {
    let cases = [(
        "personality",
        "--personality-seed-base",
        (u64::MAX - 1).to_string(),
        "personality seed range overflows",
    )];

    for (name, option, seed, expected) in cases {
        let dir = Scratch::new(&format!("bot-eval-{name}-seed-overflow"));
        let replay_dir = dir.join("replays");
        let out = dir.join("rows.jsonl");
        let stderr = refused(
            driver()
                .args([
                    "bot-eval",
                    "skirmish",
                    "--ticks",
                    "1",
                    "--runs",
                    "2",
                    option,
                    &seed,
                    "--candidate",
                    "candidate-a",
                    "--out",
                ])
                .arg(&out)
                .arg("--replay-dir")
                .arg(&replay_dir),
        );
        assert!(
            stderr.contains(expected),
            "unexpected {name} seed refusal: {stderr}"
        );
        assert!(!out.exists(), "the JSONL index was not published");
        assert!(
            !replay_dir.exists() || std::fs::read_dir(&replay_dir).unwrap().next().is_none(),
            "the completed first seed cell left evidence behind"
        );
    }
}

#[test]
fn rerunning_one_exact_candidate_refuses_to_replace_its_evidence() {
    let dir = Scratch::new("bot-eval-no-overwrite");
    let replay_dir = dir.join("replays");
    let out = dir.join("rows.jsonl");
    let run = || {
        let mut command = driver();
        command
            .args([
                "bot-eval",
                "skirmish",
                "--ticks",
                "1",
                "--candidate",
                "candidate-a",
                "--out",
            ])
            .arg(&out)
            .arg("--replay-dir")
            .arg(&replay_dir);
        command
    };

    succeeded(&mut run());
    let original_index = std::fs::read(&out).unwrap();
    let replay_paths: Vec<_> = std::fs::read_dir(&replay_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(replay_paths.len(), 1);
    let original_replay = std::fs::read(&replay_paths[0]).unwrap();

    let stderr = refused(&mut run());
    assert!(
        stderr.contains("refusing to overwrite"),
        "unexpected refusal: {stderr}"
    );
    assert_eq!(std::fs::read(&out).unwrap(), original_index);
    assert_eq!(std::fs::read(&replay_paths[0]).unwrap(), original_replay);
    assert_eq!(std::fs::read_dir(&replay_dir).unwrap().count(), 1);
}

#[test]
fn decision_trace_requires_a_persisted_index_and_candidate() {
    let dir = Scratch::new("bot-eval-trace-contract");
    let trace = dir.join("trace.jsonl");
    let out = dir.join("rows.jsonl");

    let stderr = refused(
        driver()
            .args([
                "bot-eval",
                "skirmish",
                "--ticks",
                "1",
                "--candidate",
                "candidate-a",
                "--decision-trace-out",
            ])
            .arg(&trace),
    );
    assert!(
        stderr.contains("--decision-trace-out")
            && stderr.contains("--out")
            && stderr.contains("required"),
        "trace-only refusal was unclear: {stderr}"
    );

    let stderr = refused(
        driver()
            .args(["bot-eval", "skirmish", "--ticks", "1", "--out"])
            .arg(&out)
            .arg("--decision-trace-out")
            .arg(&trace),
    );
    assert!(stderr.contains("--candidate is required"));
    assert!(!out.exists());
    assert!(!trace.exists());
}

#[test]
fn decision_trace_is_deterministic_opt_in_evidence_for_both_seats() {
    let first = Scratch::new("bot-eval-trace-determinism");
    let second = Scratch::new("bot-eval-trace-determinism");

    let run = |dir: &std::path::Path| {
        let out = dir.join("rows.jsonl");
        let trace = dir.join("trace.jsonl");
        succeeded(
            driver()
                .args([
                    "bot-eval",
                    "skirmish",
                    "--ticks",
                    "25",
                    "--paired",
                    "--personality-seed-base",
                    "40",
                    "--candidate",
                    "candidate-a",
                    "--out",
                ])
                .arg(&out)
                .arg("--decision-trace-out")
                .arg(&trace),
        );
        (
            std::fs::read_to_string(out).unwrap(),
            std::fs::read_to_string(trace).unwrap(),
        )
    };

    let (first_rows, first_trace) = run(&first);
    let (second_rows, second_trace) = run(&second);
    assert_eq!(
        first_rows, second_rows,
        "compact rows must stay deterministic"
    );
    assert_eq!(
        first_trace, second_trace,
        "decision trace JSONL must stay deterministic"
    );

    let without_trace = succeeded(driver().args([
        "bot-eval",
        "skirmish",
        "--ticks",
        "25",
        "--paired",
        "--personality-seed-base",
        "40",
        "--candidate",
        "candidate-a",
    ]));
    assert_eq!(
        without_trace, first_rows,
        "enabling trace output must not change normal evaluation rows"
    );

    let rows = jsonl(&first_rows);
    let traces = jsonl(&first_trace);
    assert_eq!(rows.len(), 2);
    assert!(!traces.is_empty());
    assert!(rows.iter().all(|row| row.get("trace").is_none()));
    for trace in &traces {
        let evaluation = rows.iter().find(|row| row["leg"] == trace["leg"]).unwrap();
        assert_eq!(trace["version"], 1);
        assert_eq!(trace["candidate"], evaluation["candidate"]);
        assert_eq!(
            trace["evaluation_fingerprint"],
            evaluation["evaluation_fingerprint"]
        );
        assert!(trace["seat"].as_u64().unwrap() < 2);
        assert_eq!(trace["trace"]["player"], trace["seat"]);
        assert_eq!(trace["tick"], trace["trace"]["tick"]);
        assert!(trace["tick"].as_u64().unwrap() < evaluation["duration_ticks"].as_u64().unwrap());
    }
}

#[test]
fn decision_trace_refuses_overwrite_before_publishing_other_evidence() {
    let dir = Scratch::new("bot-eval-trace-no-overwrite");
    let out = dir.join("rows.jsonl");
    let trace = dir.join("trace.jsonl");
    std::fs::write(&trace, b"earlier trace").unwrap();

    let stderr = refused(
        driver()
            .args([
                "bot-eval",
                "skirmish",
                "--ticks",
                "1",
                "--candidate",
                "candidate-a",
                "--out",
            ])
            .arg(&out)
            .arg("--decision-trace-out")
            .arg(&trace),
    );
    assert!(
        stderr.contains("refusing to overwrite"),
        "unexpected refusal: {stderr}"
    );
    assert_eq!(std::fs::read(&trace).unwrap(), b"earlier trace");
    assert!(!out.exists());
    assert_eq!(std::fs::read_dir(&*dir).unwrap().count(), 1);
}

#[test]
fn staged_trace_is_cleaned_up_when_replay_staging_fails() {
    let dir = Scratch::new("bot-eval-trace-stage-cleanup");
    let out = dir.join("rows.jsonl");
    let trace = dir.join("trace.jsonl");
    let replay_blocker = dir.join("not-a-directory");
    std::fs::write(&replay_blocker, b"keep me").unwrap();

    refused(
        driver()
            .args([
                "bot-eval",
                "skirmish",
                "--ticks",
                "1",
                "--candidate",
                "candidate-a",
                "--out",
            ])
            .arg(&out)
            .arg("--decision-trace-out")
            .arg(&trace)
            .arg("--replay-dir")
            .arg(&replay_blocker),
    );
    assert_eq!(std::fs::read(&replay_blocker).unwrap(), b"keep me");
    assert!(!out.exists());
    assert!(!trace.exists());
    assert_eq!(
        std::fs::read_dir(&*dir).unwrap().count(),
        1,
        "private trace staging files must be removed on another evidence failure"
    );
}
