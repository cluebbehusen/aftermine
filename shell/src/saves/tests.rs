use super::*;
use crate::game::GameReplay;

#[test]
fn shelf_hints_offer_only_the_gestures_the_hands_in_use_have() {
    use crate::platform::{ALL_HANDS, assert_copy_fits};
    assert_eq!(
        entry_hint(Some(("watches", "watch")), ALL_HANDS[0]),
        "{confirm} watches | {delete} twice deletes"
    );
    for hands in ALL_HANDS {
        for action in [("watches", "watch"), ("loads paused", "load paused")] {
            let hint = entry_hint(Some(action), hands);
            assert_copy_fits(hands, &hint);
            assert_eq!(hint.contains("{delete}"), hands.keys, "{hint}");
        }
        let hint = entry_hint(None, hands);
        assert_eq!(hint.is_empty(), !hands.keys, "{hint}");
    }
}

#[test]
fn the_shelf_tells_another_version_from_damage() {
    let dir = std::env::temp_dir().join(format!(
        "oxide-shelf-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let scenario = oxide_sim::Scenario::skirmish();
    let ours: GameReplay = chassis::replay::Replay::new(SIM_VERSION, "test", scenario.clone());
    ours.save(dir.join("ours.json")).unwrap();
    let mut foreign: GameReplay = chassis::replay::Replay::new(SIM_VERSION, "test", scenario);
    foreign.meta.sim_version = SIM_VERSION + 1;
    foreign.save(dir.join("foreign.json")).unwrap();
    let mut damaged: GameReplay =
        chassis::replay::Replay::new(SIM_VERSION, "test", oxide_sim::Scenario::skirmish());
    damaged.record(
        42,
        oxide_sim::PlayerCommand {
            player: oxide_sim::PlayerId(0),
            command: oxide_sim::Command::Surrender,
        },
    );
    damaged.meta.ticks = Some(10);
    damaged.save(dir.join("damaged.json")).unwrap();

    let mut out = Vec::new();
    scan(&dir, &mut out);
    let entry = |name: &str| {
        out.iter()
            .map(|(_, e)| e)
            .find(|e| e.path.file_stem().unwrap() == name)
            .expect("scanned")
    };
    assert!(entry("ours").available());
    assert_eq!(entry("foreign").problem, Some(Unavailable::OtherVersion));
    assert_eq!(
        entry("damaged").problem,
        Some(Unavailable::Unreadable),
        "a record from this version that fails its checks is damaged, not foreign"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn kinds_read_the_metadata_tag() {
    let dir = std::env::temp_dir().join(format!(
        "oxide-kinds-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let scenario = oxide_sim::Scenario::skirmish();
    // A tagged save under a neutral filename: the tag wins.
    let mut named: GameReplay = chassis::replay::Replay::new(SIM_VERSION, "test", scenario.clone());
    named.meta.kind = Some("save".to_string());
    named.meta.description = Some("before the push".to_string());
    named.save(dir.join("anything.json")).unwrap();
    let finished: GameReplay = chassis::replay::Replay::new(SIM_VERSION, "test", scenario);
    finished.save(dir.join("match-0000000099.json")).unwrap();

    let mut out = Vec::new();
    scan(&dir, &mut out);
    let entry = |name: &str| {
        out.iter()
            .map(|(_, e)| e)
            .find(|e| e.path.file_stem().unwrap() == name)
            .expect("scanned")
    };
    assert_eq!(entry("anything").kind, RecordKind::Save);
    assert_eq!(
        (
            entry("anything").title.as_str(),
            entry("anything").detail.as_str()
        ),
        ("before the push", "Skirmish Basin"),
        "a named save leads with its name and keeps its map beside it"
    );
    let finished = entry("match-0000000099");
    assert_eq!(finished.kind, RecordKind::Match);
    assert_eq!(
        (finished.title.as_str(), finished.detail.as_str()),
        ("Skirmish Basin", "1v1"),
        "an unnamed match leads with its map and says its format"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn shelf_skips_oversized_records_without_hiding_valid_neighbors() {
    let dir = std::env::temp_dir().join(format!(
        "oxide-shelf-bounds-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let valid: GameReplay =
        chassis::replay::Replay::new(SIM_VERSION, "test", oxide_sim::Scenario::skirmish());
    valid.save(dir.join("valid.json")).unwrap();
    std::fs::File::create(dir.join("oversized.json"))
        .unwrap()
        .set_len(chassis::replay::MAX_REPLAY_BYTES as u64 + 1)
        .unwrap();

    let mut out = Vec::new();
    scan(&dir, &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].1.path.file_stem().unwrap(), "valid");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn catalog_orders_autosaves_by_saved_time_then_mtime_then_path() {
    let dir = std::env::temp_dir().join(format!("oxide-save-order-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let game = crate::game::Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    for (name, saved_at, modified) in [
        ("copied", 99, 900),
        ("older", 100, 100),
        ("a", 100, 101),
        ("b", 100, 101),
        ("newest", 101, 1),
    ] {
        let mut meta = game.recorder.meta.clone();
        meta.kind = Some("autosave".into());
        meta.ticks = Some(game.state.current_tick());
        meta.saved_at = Some(saved_at);
        let path = dir.join(format!("{name}.oxsave"));
        crate::saved_game::write_capture(game.capture_save(), meta, &path).unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(modified))
            .unwrap();
    }
    for reverse in [false, true] {
        let mut found = Vec::new();
        scan(&dir, &mut found);
        if reverse {
            found.reverse();
        }
        let candidates = newest_first(found)
            .into_iter()
            .filter(|entry| entry.available() && entry.kind == RecordKind::Autosave)
            .map(|entry| {
                entry
                    .path
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(candidates, ["newest", "b", "a", "older", "copied"]);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_calendar_is_honest_without_a_time_crate() {
    assert_eq!(civil(0), (1970, 1, 1));
    assert_eq!(civil(86_399), (1970, 1, 1), "last second of day one");
    assert_eq!(civil(86_400), (1970, 1, 2));
    // Leap handling around the century rule: 2000-02-29 existed.
    // 951_782_400 = 2000-02-29T00:00:00Z; 951_868_800 = 2000-03-01.
    assert_eq!(civil(951_782_400), (2000, 2, 29));
    assert_eq!(civil(951_868_800), (2000, 3, 1));
    // A modern spot check: 2026-07-22T12:00:00Z.
    assert_eq!(civil(1_784_721_600), (2026, 7, 22));
}

#[test]
fn saved_times_read_as_ages_then_dates() {
    let now = 1_784_721_600; // 2026-07-22T12:00:00Z
    let ago = |secs: u64| saved_ago(now - secs, now);
    assert_eq!(ago(0), "just now");
    assert_eq!(ago(59), "just now");
    assert_eq!(ago(60), "1 min ago");
    assert_eq!(ago(59 * 60 + 59), "59 min ago");
    assert_eq!(ago(3600), "1 h ago");
    assert_eq!(ago(23 * 3600), "23 h ago");
    assert_eq!(ago(86_400), "1 day ago");
    assert_eq!(ago(6 * 86_400), "6 days ago");
    assert_eq!(ago(7 * 86_400), "Jul 15", "a week back reads as a date");
    assert_eq!(saved_ago(1_784_721_600 - 365 * 86_400, now), "Jul 22, 2025");
    assert_eq!(
        saved_ago(now + 600, now),
        "just now",
        "a clock moved back never reads as the future"
    );
}

#[test]
fn scan_uses_record_metadata_without_trusting_malformed_neighbors() {
    let dir = std::env::temp_dir().join(format!(
        "oxide-shelf-metadata-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp directory");
    let scenario = oxide_sim::Scenario::skirmish();
    let mut save: GameReplay = chassis::replay::Replay::new(SIM_VERSION, "test", scenario);
    save.meta.kind = Some("save".to_string());
    save.meta.description = Some("before the push".to_string());
    save.meta.sim_version = SIM_VERSION + 1;
    save.meta.saved_at = Some(u64::MAX);
    save.record(
        42,
        oxide_sim::PlayerCommand {
            player: oxide_sim::PlayerId(0),
            command: oxide_sim::Command::Surrender,
        },
    );
    save.save(dir.join("neutral.json")).expect("save record");
    std::fs::write(dir.join("broken.json"), b"not json").expect("bad neighbor");
    std::fs::write(dir.join("ignored.txt"), b"not a replay").expect("other extension");

    let mut out = Vec::new();
    scan(&dir, &mut out);
    assert_eq!(out.len(), 1, "bad neighbors do not hide the good record");
    let entry = &out[0].1;
    assert_eq!(entry.kind, RecordKind::Save, "the metadata tag wins");
    assert_eq!(entry.problem, Some(Unavailable::OtherVersion));
    assert_eq!(entry.title, "before the push");
    assert_eq!(entry.clock, "0:02", "duration comes from the command tail");
    assert_ne!(
        out[0].0.saved_at,
        std::time::UNIX_EPOCH,
        "overflowing saved_at falls back to mtime"
    );

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn lan_saves_are_their_own_kinds_and_resume_rather_than_play_back() {
    let replay: GameReplay =
        chassis::replay::Replay::new(SIM_VERSION, "test", oxide_sim::Scenario::skirmish());
    let mut meta = replay.meta;
    for (tag, kind) in [
        ("lan", RecordKind::LanSave),
        ("lan-autosave", RecordKind::LanAutosave),
    ] {
        meta.kind = Some(tag.to_owned());
        assert_eq!(record_kind(&meta), kind);
        assert!(kind.resumable(), "{kind:?}");
    }
    assert!(!RecordKind::Match.resumable());
    let continues: Vec<RecordKind> = [
        RecordKind::Autosave,
        RecordKind::Save,
        RecordKind::Match,
        RecordKind::LanSave,
        RecordKind::LanAutosave,
    ]
    .into_iter()
    .filter(|kind| kind.continues())
    .collect();
    assert_eq!(
        continues,
        [RecordKind::Autosave, RecordKind::LanAutosave],
        "Continue picks up the newest autosave, local or LAN"
    );
}
