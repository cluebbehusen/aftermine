//! End-to-end contract coverage for `aftermine-driver bot-ladder` and
//! `bot-ladder-report`.

#[path = "support/cli.rs"]
mod cli;

use cli::{Scratch, driver, jsonl, refused, succeeded};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Writes a one-comparison ladder over Skirmish at 24 ticks.
fn manifest(dir: &Path) -> PathBuf {
    let path = dir.join("ladder.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "name": "cli",
            "tick_limit": 24,
            "runs": 1,
            "personality_seed_base": 9000,
            "stances": ["balanced"],
            "comparisons": [{"higher": "prime", "lower": "scrapheap", "gate": 800}],
            "min_decided_pairs": 40,
            "maps": [{"path": "skirmish", "family": "open"}],
        }))
        .unwrap(),
    )
    .unwrap();
    path
}

fn ladder(manifest: &Path, out: &Path, replays: &Path) -> Command {
    let mut command = driver();
    command
        .arg("bot-ladder")
        .arg(manifest)
        .args(["--jobs", "2", "--out"])
        .arg(out)
        .arg("--replay-dir")
        .arg(replays);
    command
}

#[test]
fn a_ladder_writes_paired_rows_and_reports_them_against_the_gate() {
    let dir = Scratch::new("bot-ladder-one-pair");
    let manifest = manifest(&dir);
    let (out, replays) = (dir.join("out"), dir.join("replays"));
    let report = succeeded(&mut ladder(&manifest, &out, &replays));
    assert!(
        report.contains("prime against scrapheap: too few decided pairs"),
        "{report}"
    );
    let rows = jsonl(&std::fs::read_to_string(out.join("rows.jsonl")).unwrap());
    assert_eq!(rows.len(), 2);
    for (row, (leg, rungs)) in rows.iter().zip([
        ("forward", ["prime", "scrapheap"]),
        ("swapped", ["scrapheap", "prime"]),
    ]) {
        assert_eq!(row["leg"], leg);
        assert_eq!(row["ladder"]["higher"], "prime");
        assert_eq!(row["ladder"]["gate"], 800);
        for (seat, rung) in rungs.iter().enumerate() {
            assert_eq!(row["seats"][seat]["profile"]["difficulty"], *rung);
            assert_eq!(row["seats"][seat]["profile"]["personality_seed"], 9000);
        }
        let replay = row["replay"].as_str().unwrap();
        assert!(Path::new(replay).exists());
        let summary = succeeded(driver().args(["replay-summary", replay]));
        for rung in rungs {
            assert!(
                summary.contains(&format!("bot ({rung} / balanced, personality seed 9000)")),
                "the replay names each seat's difficulty: {summary}"
            );
        }
    }
    assert!(replays.join("legs.jsonl").exists());

    let json = succeeded(
        driver()
            .arg("bot-ladder-report")
            .arg(out.join("rows.jsonl"))
            .arg("--json"),
    );
    let json: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(json["comparisons"][0]["overall"]["legs"], 2);
    assert_eq!(json["comparisons"][0]["verdict"], "too_few_pairs");

    let again = refused(&mut ladder(&manifest, &out, &dir.join("replays-again")));
    assert!(
        again.contains("refusing to overwrite"),
        "existing rows are never replaced: {again}"
    );
}

#[test]
fn the_report_refuses_rows_from_bot_eval() {
    let dir = Scratch::new("bot-ladder-not-ladder");
    let rows = dir.join("rows.jsonl");
    succeeded(
        driver()
            .args([
                "bot-eval",
                "skirmish",
                "--ticks",
                "1",
                "--candidate",
                "cli",
                "--out",
            ])
            .arg(&rows),
    );
    let stderr = refused(driver().arg("bot-ladder-report").arg(&rows));
    assert!(stderr.contains("is not a bot-ladder row"), "{stderr}");
}
