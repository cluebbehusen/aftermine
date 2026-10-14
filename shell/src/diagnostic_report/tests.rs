use super::*;
use std::time::{Duration, Instant};

fn finished(job: &mut ReportJob) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(result) = job.poll() {
            return result;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn scratch(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "oxide-report-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn a_report_holds_the_live_matchs_commands_and_the_incident_log() {
    let root = scratch("live");
    std::fs::write(
        root.join(oxide_kit::diagnostics::INCIDENTS),
        br#"{"incidents":[]}"#,
    )
    .unwrap();
    let mut game = crate::game::Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    game.issue(oxide_sim::Command::Train {
        building: oxide_sim::BuildingId(0),
        kind: oxide_sim::UnitKind::Harvester,
    });
    game.advance_ticks(3);

    let mut job = ReportJob::default();
    job.start(&game, Some(root.clone())).unwrap();
    assert!(
        job.start(&game, Some(root.clone())).is_err(),
        "only one export can run at a time"
    );
    finished(&mut job).unwrap();

    let report = std::fs::read_dir(root.join("reports"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let replay = oxide_kit::load_replay(report.join("replay.json")).unwrap();
    assert_eq!(replay.meta.ticks, Some(3));
    assert_eq!(
        serde_json::to_value(&replay.commands).unwrap(),
        serde_json::to_value(&game.recorder.commands).unwrap()
    );
    assert_eq!(
        std::fs::read(report.join(oxide_kit::diagnostics::INCIDENTS)).unwrap(),
        br#"{"incidents":[]}"#
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(report.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(
        manifest["contents"],
        serde_json::json!(["replay.json", "incidents.json"])
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_unplayed_match_without_incidents_has_nothing_to_export() {
    let root = scratch("empty");
    let game = crate::game::Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    let mut job = ReportJob::default();
    job.start(&game, Some(root.clone())).unwrap();
    assert!(
        finished(&mut job)
            .unwrap_err()
            .contains("no match or incident")
    );
    assert!(!root.join("reports").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_report_needs_a_diagnostics_folder() {
    let game = crate::game::Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    assert!(ReportJob::default().start(&game, None).is_err());
}

#[test]
fn a_report_carries_the_newest_crash_dump() {
    let root = scratch("crash");
    let mut crashed = crate::game::Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    crashed.advance_ticks(2);
    let mut replay = crashed.recorder.clone();
    replay.meta.ticks = Some(2);
    oxide_kit::diagnostics::write_crash_dump(
        &root,
        &oxide_kit::diagnostics::CrashDump {
            replay: &replay,
            phase: "collection",
            unfinished: &[],
            message: "boom",
            build: &crate::build_identity(),
        },
    )
    .unwrap();
    let relaunched = crate::game::Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    let mut job = ReportJob::default();
    job.start(&relaunched, Some(root.clone())).unwrap();
    finished(&mut job).unwrap();
    let report = std::fs::read_dir(root.join("reports"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(report.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(
        manifest["contents"],
        serde_json::json!(["crash-replay.json", "crash.json"])
    );
    assert_eq!(
        oxide_kit::load_replay(report.join("crash-replay.json"))
            .unwrap()
            .meta
            .ticks,
        Some(2)
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_failed_export_leaves_no_partial_report() {
    let root = scratch("failed");
    let broken = root.join("crash-0000000000001-1");
    std::fs::create_dir_all(&broken).unwrap();
    // A finished dump whose replay went missing cannot be copied.
    std::fs::write(broken.join("crash.json"), b"{}").unwrap();
    let game = crate::game::Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    let mut job = ReportJob::default();
    job.start(&game, Some(root.clone())).unwrap();
    assert!(
        finished(&mut job)
            .unwrap_err()
            .contains("crash-replay.json")
    );
    assert_eq!(
        std::fs::read_dir(root.join("reports")).unwrap().count(),
        0,
        "the half-written report is removed"
    );
    std::fs::remove_dir_all(root).unwrap();
}
