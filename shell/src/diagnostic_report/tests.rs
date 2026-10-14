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
