use super::*;
use aftermine_sim::SIM_VERSION;
use aftermine_sim::scenario::ScenarioMode;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct TempReplay(PathBuf);

impl Drop for TempReplay {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn write_fixture(bytes: &[u8]) -> TempReplay {
    let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "aftermine-kit-replay-{}-{id}.json",
        std::process::id()
    ));
    std::fs::write(&path, bytes).expect("fixture is written");
    TempReplay(path)
}

fn current_document() -> Value {
    serde_json::to_value(GameReplay::new(SIM_VERSION, "test", Scenario::skirmish()))
        .expect("current replay serializes")
}

#[test]
fn sandbox_replay_preserves_rules_through_the_strict_wire() {
    let mut setup = Scenario::skirmish();
    setup.mode = ScenarioMode::Sandbox;
    for row in &mut setup.map {
        *row = row.replace(['1', '2'], ".");
    }
    let mut replay = GameReplay::new(SIM_VERSION, "test", setup.clone());
    replay.meta.ticks = Some(20);
    let fixture = write_fixture(&serde_json::to_vec(&replay).unwrap());
    let loaded = load_replay(&fixture.0).unwrap();
    assert_eq!(loaded.setup, setup);
    let world = crate::runner::run_replay(&loaded, None).unwrap();
    assert_eq!(world.current_tick(), 20);
    assert_eq!(world.mode(), ScenarioMode::Sandbox);
    assert!(world.result().is_none());
}

#[test]
fn current_replay_round_trips_through_the_strict_wire() {
    let document = current_document();
    let fixture = write_fixture(&serde_json::to_vec(&document).expect("fixture serializes"));

    let loaded = load_replay(&fixture.0).expect("current replay loads");
    assert_eq!(loaded.meta.sim_version, SIM_VERSION);
    assert_eq!(loaded.setup, Scenario::skirmish());
    assert!(loaded.commands.is_empty());
}

#[test]
fn current_replay_rejects_unknown_fields_at_strict_setup_boundaries() {
    let mut documents = Vec::new();

    let mut replay = current_document();
    replay["unexpected"] = json!(true);
    documents.push(("replay", replay));

    let mut setup = current_document();
    setup["setup"]["unexpected"] = json!(true);
    documents.push(("setup", setup));

    let mut player = current_document();
    player["setup"]["players"][1]["unexpected"] = json!(true);
    documents.push(("player", player));

    let mut config = current_document();
    config["setup"]["players"][1]["bot_config"]["unexpected"] = json!(true);
    documents.push(("bot config", config));

    for (boundary, document) in documents {
        let fixture = write_fixture(&serde_json::to_vec(&document).expect("fixture serializes"));
        assert!(
            load_replay(&fixture.0).is_err(),
            "unknown field was accepted at the {boundary} boundary"
        );
    }
}

#[test]
fn current_replay_rejects_duplicate_bot_config_fields() {
    let json = serde_json::to_string(&GameReplay::new(SIM_VERSION, "test", Scenario::skirmish()))
        .expect("current replay serializes");
    let needle = r#""bot_config":{}"#;
    let replacement = concat!(r#""bot_config":{},"#, r#""bot_config":{}"#);
    let duplicate = json.replacen(needle, replacement, 1);
    assert_ne!(duplicate, json, "fixture must duplicate a real field");
    let fixture = write_fixture(duplicate.as_bytes());

    assert!(load_replay(&fixture.0).is_err());
}

/// A skirmish recorded for `ticks` with the keyframes a live recorder keeps.
fn recording(ticks: u64) -> Recording {
    let mut replay = GameReplay::new(SIM_VERSION, "test", Scenario::skirmish());
    replay.meta.ticks = Some(ticks);
    let mut state = replay.setup.build().unwrap();
    let mut recorder = crate::keyframes::KeyframeRecorder::default();
    while state.current_tick() < ticks {
        let command = aftermine_sim::PlayerCommand {
            player: aftermine_sim::PlayerId(0),
            command: aftermine_sim::Command::Surrender,
        };
        if state.current_tick() == ticks - 1 {
            replay.record(state.current_tick(), command.clone());
            state.tick(&[command]);
        } else {
            state.tick(&[]);
        }
        recorder.observe(&state);
    }
    Recording {
        replay,
        keyframes: recorder.pending().wait(),
    }
}

fn saved(recording: &Recording) -> Vec<u8> {
    let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "aftermine-kit-replay-{}-{id}.amreplay",
        std::process::id()
    ));
    save_recording(&path, &recording.replay, &recording.keyframes).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    bytes
}

#[test]
fn a_recording_round_trips_with_its_keyframes() {
    let original = recording(2_100);
    let bytes = saved(&original);
    assert_eq!(&bytes[..8], MAGIC);
    let fixture = write_fixture(&bytes);
    let loaded = load_recording(&fixture.0).unwrap();
    assert_eq!(
        serde_json::to_value(&loaded.replay).unwrap(),
        serde_json::to_value(&original.replay).unwrap()
    );
    assert_eq!(loaded.keyframes.ticks().collect::<Vec<_>>(), [1_024, 2_048]);
    let origin = loaded.replay.setup.build().unwrap();
    let truth =
        crate::runner::run_replay_observed(&loaded.replay, None, Some(2_048), false, |_, _| {})
            .unwrap();
    assert_eq!(
        loaded.keyframes.decode(1, &origin).unwrap().hash(),
        truth.hash()
    );
    assert_eq!(
        serde_json::to_value(load_replay(&fixture.0).unwrap()).unwrap(),
        serde_json::to_value(&original.replay).unwrap(),
        "replay tools read a recording's record alone"
    );
}

#[test]
fn a_damaged_or_foreign_recording_is_refused() {
    let bytes = saved(&recording(1_100));
    let header_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let header: serde_json::Value = serde_json::from_slice(&bytes[12..12 + header_len]).unwrap();
    let rewrite = |change: &dyn Fn(&mut serde_json::Value)| {
        let mut header = header.clone();
        change(&mut header);
        let header = serde_json::to_vec(&header).unwrap();
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&u32::try_from(header.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&header);
        out.extend_from_slice(&bytes[12 + header_len..]);
        out
    };
    let mut cases = vec![
        ("trailing bytes", [bytes.as_slice(), &[0]].concat()),
        ("truncated", bytes[..bytes.len() - 1].to_vec()),
        ("format", rewrite(&|h| h["version"] = json!(FORMAT + 1))),
        ("unknown field", rewrite(&|h| h["extra"] = json!(1))),
        (
            "record length",
            rewrite(&|h| h["record"]["decoded"] = json!(1)),
        ),
        (
            "off the grid",
            rewrite(&|h| h["keyframes"][0]["tick"] = json!(1_000)),
        ),
        (
            "past the end",
            rewrite(&|h| h["keyframes"][0]["tick"] = json!(2_048)),
        ),
    ];
    let mut flipped = bytes.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 1;
    cases.push(("keyframe checksum", flipped));
    for (case, bytes) in cases {
        let fixture = write_fixture(&bytes);
        let result = load_recording(&fixture.0).and_then(|recording| {
            let origin = recording.replay.setup.build().unwrap();
            recording
                .keyframes
                .decode(0, &origin)
                .map(|_| ())
                .map_err(|error| ReplayError::Invalid(error.to_string()))
        });
        assert!(result.is_err(), "{case} was accepted");
    }
}
