use super::*;

/// Runs `job` with `dir` holding both its record and the match's autosaves.
fn run_at(job: SaveJob, dir: &Path) -> Result<Option<PathBuf>, SaveError> {
    job.run_in(Some(dir.to_owned()), &[dir.to_owned()])
}

fn write_record(game: &mut Game, dir: &Path) -> Result<Option<PathBuf>, SaveError> {
    let job = SaveJob::capture(game, None);
    let mark = job.mark();
    let result = run_at(job, dir)?;
    if let Some(mark) = mark {
        game.mark_saved(&mark);
    }
    Ok(result)
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn write_named(game: &Game, name: &str, dir: &Path, saved_at: u64) -> Result<PathBuf, SaveError> {
    let mut job = SaveJob::capture(game, Some(name));
    job.meta.saved_at = Some(saved_at);
    match run_at(job, dir)? {
        Some(path) => Ok(path),
        _ => unreachable!(),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aftermine-autosave-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::remove_dir_all(&dir).ok();
    dir
}

#[test]
fn pending_tick_zero_input_is_saved_and_finished_matches_remain_recordings() {
    let dir = scratch("pending-and-finished");
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
    game.issue(aftermine_sim::Command::Train {
        building: aftermine_sim::BuildingId(0),
        kind: aftermine_sim::UnitKind::Harvester,
    });
    let Ok(Some(path)) = write_record(&mut game, &dir) else {
        panic!("pending input must not be discarded")
    };
    let mut restored = crate::saved_game::tests::load(&path).unwrap();
    assert_eq!(*restored.pending, *game.pending);
    assert_eq!(restored.state.current_tick(), 0);
    assert_eq!(restored.do_tick().events, game.do_tick().events);
    restored.issue(aftermine_sim::Command::Surrender);
    restored.do_tick();
    let Ok(Some(finished)) = write_record(&mut restored, &dir) else {
        panic!("finished record writes")
    };
    let record = aftermine_kit::load_replay(&finished).unwrap();
    assert_eq!(record.meta.kind.as_deref(), Some("match"));
    let mut playback = aftermine_kit::playback::Playback::load(record).unwrap();
    playback.seek(restored.state.current_tick());
    assert_eq!(playback.state.hash(), restored.state.hash());
    assert!(
        SaveJob::capture(&restored, Some("after the end"))
            .mark()
            .is_none(),
        "a named save never stands in for a finished match's record"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_lan_match_leaves_only_a_watch_only_recording() {
    let dir = scratch("lan-match");
    let mut duel = aftermine_sim::Scenario::skirmish();
    for player in &mut duel.players {
        player.bot = false;
        player.bot_config = None;
    }
    let mut game = Game::networked(
        duel,
        aftermine_sim::PlayerId(0),
        crate::game::network::NetRole::Host,
        macroquad::prelude::vec2(1280.0, 800.0),
    )
    .unwrap();
    game.issue(aftermine_sim::Command::Train {
        building: aftermine_sim::BuildingId(0),
        kind: aftermine_sim::UnitKind::Harvester,
    });
    let Ok(Some(path)) = write_record(&mut game, &dir) else {
        panic!("an undecided LAN match still records its history")
    };
    let record = aftermine_kit::load_replay(&path).unwrap();
    assert_eq!(record.meta.kind.as_deref(), Some("match"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_session_writes_one_record_once_it_has_started() {
    let dir = scratch("one-record");
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).expect("game");
    assert!(
        matches!(write_record(&mut game, &dir), Ok(None)),
        "an unstarted game writes nothing"
    );
    game.advance_ticks(1);
    let Ok(Some(path)) = write_record(&mut game, &dir) else {
        panic!("a started game writes its record");
    };
    assert!(path.exists());
    assert!(
        matches!(write_record(&mut game, &dir), Ok(None)),
        "one record per session"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn records_carry_their_kind_and_a_named_save_leaves_the_session_recorder_alone() {
    let dir = scratch("kind-stamp");
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).expect("game");
    game.advance_ticks(1);
    let saved =
        write_named(&game, "before the big push", &dir, 1_784_721_600).expect("a named save lands");
    let record = crate::saved_game::inspect(&saved).expect("loads back");
    assert_eq!(record.meta.kind.as_deref(), Some("save"));
    assert_eq!(
        record.meta.description.as_deref(),
        Some("before the big push")
    );
    assert_eq!(record.meta.saved_at, Some(1_784_721_600));
    assert!(
        game.recorder.meta.description.is_none(),
        "the live recorder never inherits the name"
    );
    // A second save on the same tick walks the name instead of
    // overwriting the first.
    let again = write_named(&game, "again", &dir, 1_784_721_601).expect("the twin walks a counter");
    assert_ne!(saved, again);
    // The quit autosave stamps its own kind.
    let Ok(Some(auto_path)) = write_record(&mut game, &dir) else {
        panic!("the quit record lands");
    };
    let auto = crate::saved_game::inspect(&auto_path).expect("loads back");
    assert_eq!(auto.meta.kind.as_deref(), Some("autosave"));
    assert!(auto.meta.description.is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_unwritable_dir_reports_instead_of_lying() {
    // The would-be directory exists as a file, so create_dir_all
    // refuses and the error must surface.
    let dir = scratch("unwritable");
    std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
    std::fs::write(&dir, b"in the way").unwrap();
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).expect("game");
    game.advance_ticks(1);
    let err = write_record(&mut game, &dir).expect_err("the failure surfaces");
    assert!(matches!(err, SaveError::CreateDir { .. }));
    assert!(err.player_line().is_ascii(), "the menu font is Latin-1");
    assert!(!game.is_saved(), "a failed save still owes a record");
    std::fs::remove_file(&dir).ok();
}

#[test]
fn a_named_save_skips_a_taken_name_and_a_failed_one_leaves_nothing() {
    let dir = scratch("named-names");
    std::fs::create_dir_all(&dir).unwrap();
    let write = |body: &'static [u8]| {
        move |path: &Path| -> anyhow::Result<()> {
            std::fs::write(path, body)?;
            Ok(())
        }
    };
    let first = publish_named(&dir, RecordKind::Save, 7, write(b"first")).unwrap();
    let second = publish_named(&dir, RecordKind::Save, 7, write(b"second")).unwrap();
    assert_ne!(first, second, "a taken name is skipped, never replaced");
    assert_eq!(std::fs::read(&first).unwrap(), b"first");
    assert_eq!(std::fs::read(&second).unwrap(), b"second");
    let failed = publish_named(&dir, RecordKind::Save, 7, |_| {
        Err(anyhow::anyhow!("disk full"))
    });
    assert!(matches!(failed, Err(SaveError::Write { .. })));
    assert_eq!(files(&dir).len(), 2, "a failed save leaves no file behind");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn collisions_beyond_one_thousand_still_find_a_free_name() {
    let dir = scratch("many-collisions");
    std::fs::create_dir_all(&dir).unwrap();
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).expect("game");
    game.advance_ticks(1);
    let tick = game.state.current_tick();
    // The name carries the time, so occupy every name of the next few seconds.
    let now = now_unix();
    for time in now..now + 3 {
        for n in 0..1000 {
            let extension = "amsave";
            let path = if n == 0 {
                dir.join(format!("save-{tick:010}-{time}.{extension}"))
            } else {
                dir.join(format!("save-{tick:010}-{time}-{n}.{extension}"))
            };
            std::fs::write(path, b"occupied").expect("create known collision");
        }
    }

    let first = write_named(&game, "first beyond the cutoff", &dir, 1)
        .expect("the first high-collision save lands");
    let second = write_named(&game, "second beyond the cutoff", &dir, 2)
        .expect("the next high-collision save lands");
    assert_ne!(first, second, "each save owns a distinct destination");
    assert_eq!(
        crate::saved_game::inspect(&first)
            .unwrap()
            .meta
            .description
            .as_deref(),
        Some("first beyond the cutoff")
    );
    assert_eq!(
        crate::saved_game::inspect(&second)
            .unwrap()
            .meta
            .description
            .as_deref(),
        Some("second beyond the cutoff")
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rotation_touches_only_its_own_kind() {
    let dir = scratch("kinds");
    std::fs::create_dir_all(&dir).unwrap();
    // Six autosaves (one over budget), one match, one explicit
    // save, one fresh temp. Names sort in age order so the mtime
    // tie-break stays deterministic.
    for n in 0..6 {
        std::fs::write(dir.join(format!("autosave-000000000{n}.amsave")), b"{}").unwrap();
    }
    std::fs::write(dir.join("match-0000000100.amreplay"), b"{}").unwrap();
    std::fs::write(dir.join("save-outpost.json"), b"{}").unwrap();
    std::fs::write(dir.join("autosave-legacy.json"), b"{}").unwrap();
    std::fs::write(dir.join("autosave-0000000001.tmp.42.0"), b"live").unwrap();
    rotate(&dir);
    assert!(
        !dir.join("autosave-0000000000.amsave").exists(),
        "only the oldest autosave died"
    );
    for n in 1..6 {
        assert!(dir.join(format!("autosave-000000000{n}.amsave")).exists());
    }
    assert!(dir.join("match-0000000100.amreplay").exists());
    assert!(dir.join("autosave-legacy.json").exists());
    assert!(
        dir.join("save-outpost.json").exists(),
        "explicit saves are never rotation's to take"
    );
    assert!(
        dir.join("autosave-0000000001.tmp.42.0").exists(),
        "a fresh temp is a write in flight, not an orphan"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn matches_rotate_against_their_own_budget() {
    let dir = scratch("matches");
    std::fs::create_dir_all(&dir).unwrap();
    for n in 0..=KEEP_MATCHES {
        std::fs::write(dir.join(format!("match-{n:010}.amreplay")), b"{}").unwrap();
    }
    rotate(&dir);
    assert!(!dir.join(format!("match-{:010}.amreplay", 0)).exists());
    for n in 1..=KEEP_MATCHES {
        assert!(dir.join(format!("match-{n:010}.amreplay")).exists());
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_directory_at_budget_loses_nothing() {
    let dir = scratch("at-budget");
    std::fs::create_dir_all(&dir).unwrap();
    for n in 0..KEEP_AUTOSAVES {
        std::fs::write(dir.join(format!("autosave-{n:010}.amsave")), b"{}").unwrap();
    }
    rotate(&dir);
    for n in 0..KEEP_AUTOSAVES {
        assert!(dir.join(format!("autosave-{n:010}.amsave")).exists());
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn lan_autosaves_rotate_against_their_own_budget() {
    let dir = scratch("lan-autosaves");
    std::fs::create_dir_all(&dir).unwrap();
    for n in 0..=KEEP_AUTOSAVES {
        std::fs::write(dir.join(format!("lan-autosave-{n:010}.amsave")), b"{}").unwrap();
        std::fs::write(dir.join(format!("autosave-{n:010}.amsave")), b"{}").unwrap();
    }
    std::fs::write(dir.join("lan-0000000007.amsave"), b"{}").unwrap();
    rotate(&dir);
    for prefix in ["lan-autosave", "autosave"] {
        assert!(!dir.join(format!("{prefix}-{:010}.amsave", 0)).exists());
        for n in 1..=KEEP_AUTOSAVES {
            assert!(dir.join(format!("{prefix}-{n:010}.amsave")).exists());
        }
    }
    assert!(
        dir.join("lan-0000000007.amsave").exists(),
        "a named LAN save stays"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_lan_host_save_is_a_resumable_checkpoint() {
    let dir = scratch("lan-host");
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).expect("game");
    game.advance_ticks(12);
    let capture = || {
        let manifest =
            crate::game::checkpoint::LanManifest::new(&game.scenario, game.presentation.human, &[]);
        game.capture_save().for_lan(Vec::new(), manifest)
    };
    for (name, kind) in [(None, "lan-autosave"), (Some("raid"), "lan")] {
        let job = SaveJob::lan(&game, capture(), name);
        let Some(path) = run_at(job, &dir).unwrap() else {
            panic!("a LAN save lands");
        };
        assert!(
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|file| file.starts_with(&format!("{kind}-")))
        );
        let info = crate::saved_game::inspect(&path).unwrap();
        assert_eq!(info.meta.kind.as_deref(), Some(kind));
        assert_eq!(info.meta.description.as_deref(), name);
    }
    assert_eq!(
        files(&dir).len(),
        2,
        "a named LAN save leaves the LAN autosave"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_named_save_replaces_the_matchs_autosave_and_counts_as_saved() {
    let dir = scratch("named-replaces-autosave");
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(1);
    let Ok(Some(autosave)) = write_record(&mut game, &dir) else {
        panic!("the autosave lands");
    };
    game.advance_ticks(1);
    let job = SaveJob::capture(&game, Some("before the push"));
    let mark = job.mark().expect("a local match counts its named save");
    let Ok(Some(named)) = run_at(job, &dir) else {
        panic!("the named save lands");
    };
    game.mark_saved(&mark);
    assert!(!autosave.exists(), "the named save holds what it held");
    assert!(
        matches!(write_record(&mut game, &dir), Ok(None)),
        "leaving right after a named save writes nothing"
    );
    assert_eq!(files(&dir).len(), 1);
    game.advance_ticks(1);
    let Ok(Some(again)) = write_record(&mut game, &dir) else {
        panic!("play after the named save autosaves");
    };
    assert_eq!(again, autosave);
    assert!(named.exists(), "autosaves never touch a named save");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_failed_named_save_keeps_the_matchs_autosave() {
    let autosaves = scratch("failed-named-autosaves");
    let blocked = scratch("failed-named-target");
    std::fs::create_dir_all(blocked.parent().unwrap()).unwrap();
    std::fs::write(&blocked, b"in the way").unwrap();
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(1);
    let Ok(Some(autosave)) = write_record(&mut game, &autosaves) else {
        panic!("the autosave lands");
    };
    game.advance_ticks(1);
    let error = SaveJob::capture(&game, Some("refused"))
        .run_in(Some(blocked.clone()), std::slice::from_ref(&autosaves))
        .expect_err("the failure surfaces");
    assert!(matches!(error, SaveError::CreateDir { .. }));
    assert!(autosave.exists());
    std::fs::remove_dir_all(&autosaves).ok();
    std::fs::remove_file(&blocked).ok();
}

#[test]
fn leaving_a_loaded_named_save_unplayed_writes_nothing() {
    let dir = scratch("load-named");
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(4);
    let named = write_named(&game, "raid", &dir, 1).unwrap();
    let crate::saved_game::Loaded::Local(restored) = crate::saved_game::prepare(&named).unwrap()
    else {
        panic!("a local save restores locally");
    };
    let mut resumed = restored.install();
    assert!(
        matches!(write_record(&mut resumed, &dir), Ok(None)),
        "the named save already holds this moment"
    );
    resumed.advance_ticks(3);
    let Ok(Some(autosave)) = write_record(&mut resumed, &dir) else {
        panic!("play after the load autosaves");
    };
    assert_ne!(autosave, named, "a named save is never overwritten");
    assert_eq!(files(&dir).len(), 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_match_overwrites_its_one_slot() {
    let dir = scratch("one-slot");
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(1);
    let Ok(Some(first)) = write_record(&mut game, &dir) else {
        panic!("the first autosave lands");
    };
    game.advance_ticks(2);
    let Ok(Some(second)) = write_record(&mut game, &dir) else {
        panic!("the next autosave lands");
    };
    assert_eq!(first, second);
    assert_eq!(files(&dir).len(), 1);
    let saved = crate::saved_game::inspect(&first).unwrap();
    assert_eq!(saved.meta.ticks, Some(3));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn continuing_a_slot_overwrites_it() {
    let dir = scratch("continue-slot");
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(4);
    let Ok(Some(path)) = write_record(&mut game, &dir) else {
        panic!("the autosave lands");
    };
    let slot = SaveSlot::adopt(&path, std::slice::from_ref(&dir)).unwrap();
    let crate::saved_game::Loaded::Local(restored) = crate::saved_game::prepare(&path).unwrap()
    else {
        panic!("a local autosave restores locally");
    };
    let mut resumed = restored.with_slot(Some(slot)).install();
    assert!(
        matches!(write_record(&mut resumed, &dir), Ok(None)),
        "leaving right after Continue rewrites nothing"
    );
    resumed.advance_ticks(3);
    let Ok(Some(again)) = write_record(&mut resumed, &dir) else {
        panic!("the continued match saves");
    };
    assert_eq!(again, path);
    assert_eq!(files(&dir).len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn only_an_autosave_in_an_autosave_folder_lends_its_slot() {
    let folders = [
        PathBuf::from("/cloud/autosaves"),
        PathBuf::from("/data/autosaves"),
    ];
    let adopt = |path: &str| SaveSlot::adopt(Path::new(path), &folders);
    assert_eq!(
        adopt("/data/autosaves/autosave-12.amsave"),
        Some(SaveSlot("12".into()))
    );
    assert_eq!(
        adopt("/cloud/autosaves/autosave-12.amsave"),
        Some(SaveSlot("12".into()))
    );
    assert_eq!(
        adopt("/data/autosaves/lan-autosave-12.amsave"),
        Some(SaveSlot("12".into()))
    );
    for other in [
        "/data/saves/autosave-12.amsave",
        "/data/autosaves/save-12.amsave",
        "/data/autosaves/match-12.amreplay",
        "/data/autosaves/autosave-.amsave",
        "/data/autosaves/autosave12.amsave",
    ] {
        assert_eq!(adopt(other), None, "{other}");
    }
}

#[test]
fn fresh_slots_never_repeat() {
    let slots: Vec<SaveSlot> = (0..64).map(|_| SaveSlot::fresh()).collect();
    for (index, slot) in slots.iter().enumerate() {
        assert!(!slots[index + 1..].contains(slot));
    }
}

#[test]
fn rotation_keeps_the_last_five_matches() {
    let dir = scratch("five-matches");
    let mut slots = Vec::new();
    for _ in 0..=KEEP_AUTOSAVES {
        let mut game = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
        for ticks in [1, 2] {
            game.advance_ticks(ticks);
            assert!(matches!(write_record(&mut game, &dir), Ok(Some(_))));
        }
        slots.push(game.slot.clone());
    }
    let expected: Vec<String> = {
        let mut names: Vec<String> = slots[1..]
            .iter()
            .map(|slot| slot.file(RecordKind::Autosave))
            .collect();
        names.sort();
        names
    };
    assert_eq!(files(&dir), expected, "the oldest match left whole");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_decided_match_writes_its_record_and_drops_its_slot() {
    let root = scratch("decided-slot");
    let (autosaves, matches) = (root.join("autosaves"), root.join("matches"));
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(aftermine_kit::keyframes::KEYFRAME_EVERY + 5);
    assert!(matches!(write_record(&mut game, &autosaves), Ok(Some(_))));
    game.issue(aftermine_sim::Command::Surrender);
    game.do_tick();
    assert!(game.state.result().is_some());
    let job = SaveJob::capture(&game, None);
    assert_eq!(job.kind, RecordKind::Match);
    let mark = job.mark().unwrap();
    let Ok(Some(record)) = job.run_in(Some(matches.clone()), std::slice::from_ref(&autosaves))
    else {
        panic!("the finished record lands");
    };
    game.mark_saved(&mark);
    assert!(
        files(&autosaves).is_empty(),
        "the record replaces the autosave"
    );
    assert_eq!(files(&matches), vec![game.slot.file(RecordKind::Match)]);
    let recording = aftermine_kit::load_recording(&record).unwrap();
    assert_eq!(recording.replay.meta.kind.as_deref(), Some("match"));
    assert_eq!(
        recording.keyframes.ticks().collect::<Vec<_>>(),
        [aftermine_kit::keyframes::KEYFRAME_EVERY],
        "the record carries the keyframes the match kept"
    );
    assert!(matches!(write_record(&mut game, &matches), Ok(None)));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_match_decided_right_after_its_resume_records_its_whole_history() {
    use aftermine_kit::keyframes::KEYFRAME_EVERY;
    let root = scratch("decided-resume");
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(KEYFRAME_EVERY + 5);
    let mut resumed = game
        .capture_save()
        .checkpoint()
        .unwrap()
        .restore()
        .unwrap()
        .install();
    resumed.issue(aftermine_sim::Command::Surrender);
    resumed.do_tick();
    assert!(resumed.state.result().is_some());
    let Ok(Some(record)) = write_record(&mut resumed, &root) else {
        panic!("the finished record lands");
    };
    let recording = aftermine_kit::load_recording(&record).unwrap();
    assert_eq!(
        recording.replay.start_tick(),
        0,
        "the record keeps the match"
    );
    assert_eq!(
        recording.keyframes.ticks().next(),
        Some(KEYFRAME_EVERY),
        "with the keyframes rebuilt before the save"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_save_that_finished_after_more_ticks_does_not_cover_them() {
    let dir = scratch("late-mark");
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(1);
    let job = SaveJob::capture(&game, None);
    let mark = job.mark().unwrap();
    game.advance_ticks(2);
    assert!(matches!(run_at(job, &dir), Ok(Some(_))));
    game.mark_saved(&mark);
    assert!(!game.is_saved());
    let Ok(Some(path)) = write_record(&mut game, &dir) else {
        panic!("the newer ticks still save");
    };
    assert_eq!(
        crate::saved_game::inspect(&path).unwrap().meta.ticks,
        Some(3)
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_mark_for_another_match_changes_nothing() {
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(1);
    game.mark_saved(&SaveMark {
        slot: SaveSlot::fresh(),
        key: game.save_key(),
    });
    assert!(!game.is_saved());
}

#[test]
fn a_match_decided_before_install_writes_no_record() {
    let dir = scratch("decided-replay");
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
    game.issue(aftermine_sim::Command::Surrender);
    game.advance_ticks(2);
    let mut replay = game.recorder.clone();
    replay.meta.ticks = Some(game.state.current_tick());
    let mut watched = Game::from_replay(replay).unwrap();
    assert!(watched.state.result().is_some());
    assert!(matches!(write_record(&mut watched, &dir), Ok(None)));
    assert!(!dir.exists());
}

#[test]
fn a_slot_holds_one_autosave_kind() {
    let dir = scratch("one-kind");
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(3);
    let manifest =
        crate::game::checkpoint::LanManifest::new(&game.scenario, game.presentation.human, &[]);
    let job = SaveJob::lan(
        &game,
        game.capture_save().for_lan(Vec::new(), manifest),
        None,
    );
    assert!(matches!(run_at(job, &dir), Ok(Some(_))));
    assert_eq!(files(&dir), vec![game.slot.file(RecordKind::LanAutosave)]);
    game.advance_ticks(1);
    assert!(matches!(write_record(&mut game, &dir), Ok(Some(_))));
    assert_eq!(
        files(&dir),
        vec![game.slot.file(RecordKind::Autosave)],
        "a host resumed alone keeps one autosave"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_session_left_on_the_device_moves_into_icloud_when_resumed() {
    let root = scratch("moves-to-icloud");
    let (cloud, device) = (root.join("cloud"), root.join("device"));
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(3);
    assert!(matches!(write_record(&mut game, &device), Ok(Some(_))));
    game.advance_ticks(3);
    let both = [cloud.clone(), device.clone()];
    let job = SaveJob::capture(&game, None);
    assert!(matches!(
        job.run_in(Some(cloud.clone()), &both),
        Ok(Some(_))
    ));
    assert_eq!(files(&cloud), vec![game.slot.file(RecordKind::Autosave)]);
    assert!(files(&device).is_empty(), "one match keeps one autosave");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn restarting_drops_the_abandoned_slot() {
    let dir = scratch("discard");
    std::fs::create_dir_all(&dir).unwrap();
    let slot = SaveSlot("9".into());
    for kind in RecordKind::AUTOSAVES {
        std::fs::write(dir.join(slot.file(kind)), b"{}").unwrap();
    }
    discard_in(std::slice::from_ref(&slot), &[]);
    assert_eq!(files(&dir).len(), 2, "no folder, nothing to drop");
    discard_in(&[slot], std::slice::from_ref(&dir));
    assert!(files(&dir).is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_lesson_is_never_saved() {
    let dir = scratch("lesson");
    let mut game = crate::tutorial::Tutorial::begin_at(
        crate::tutorial::Lesson::Scrap,
        macroquad::prelude::vec2(1280.0, 800.0),
    )
    .unwrap();
    game.advance_ticks(20);
    assert!(matches!(write_record(&mut game, &dir), Ok(None)));
    assert!(!dir.exists(), "nothing was written");
}
