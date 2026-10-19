use super::*;

/// Points `job` at `dir` for both its record and the match's autosaves.
fn redirect(job: &mut SaveJob, dir: &Path) {
    job.dir = Some(dir.to_owned());
    job.autosaves = Some(dir.to_owned());
}

fn write_record(game: &mut Game, dir: &Path) -> Result<SaveOutcome, SaveError> {
    let mut job = SaveJob::capture(game, None);
    redirect(&mut job, dir);
    let mark = job.mark();
    let result = job.run()?;
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
    redirect(&mut job, dir);
    job.meta.saved_at = Some(saved_at);
    match job.run()? {
        SaveOutcome::Wrote(path) => Ok(path),
        _ => unreachable!(),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oxide-autosave-{name}-{}-{}",
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
    let mut game = Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    game.issue(oxide_sim::Command::Train {
        building: oxide_sim::BuildingId(0),
        kind: oxide_sim::UnitKind::Harvester,
    });
    let Ok(SaveOutcome::Wrote(path)) = write_record(&mut game, &dir) else {
        panic!("pending input must not be discarded")
    };
    let mut restored = crate::saved_game::load(&path).unwrap();
    assert_eq!(*restored.pending, *game.pending);
    assert_eq!(restored.state.current_tick(), 0);
    assert_eq!(restored.do_tick().events, game.do_tick().events);
    restored.issue(oxide_sim::Command::Surrender);
    restored.do_tick();
    let Ok(SaveOutcome::Wrote(finished)) = write_record(&mut restored, &dir) else {
        panic!("finished record writes")
    };
    let record = oxide_kit::load_replay(&finished).unwrap();
    assert_eq!(record.meta.kind.as_deref(), Some("match"));
    let mut playback = oxide_kit::playback::Playback::load(record).unwrap();
    playback.seek(restored.state.current_tick());
    assert_eq!(playback.state.hash(), restored.state.hash());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_lan_match_leaves_only_a_watch_only_recording() {
    let dir = scratch("lan-match");
    let mut duel = oxide_sim::Scenario::skirmish();
    for player in &mut duel.players {
        player.bot = false;
        player.bot_config = None;
    }
    let mut game = Game::networked(
        duel,
        oxide_sim::PlayerId(0),
        crate::game::network::NetRole::Host,
        macroquad::prelude::vec2(1280.0, 800.0),
    )
    .unwrap();
    game.issue(oxide_sim::Command::Train {
        building: oxide_sim::BuildingId(0),
        kind: oxide_sim::UnitKind::Harvester,
    });
    let Ok(SaveOutcome::Wrote(path)) = write_record(&mut game, &dir) else {
        panic!("an undecided LAN match still records its history")
    };
    let record = oxide_kit::load_replay(&path).unwrap();
    assert_eq!(record.meta.kind.as_deref(), Some("match"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn outcomes_distinguish_nothing_already_and_wrote() {
    let dir = scratch("outcomes");
    let mut game = Game::new(oxide_sim::Scenario::skirmish()).expect("game");
    assert!(
        matches!(
            write_record(&mut game, &dir),
            Ok(SaveOutcome::NothingToSave)
        ),
        "an unstarted game writes nothing"
    );
    game.advance_ticks(1);
    let Ok(SaveOutcome::Wrote(path)) = write_record(&mut game, &dir) else {
        panic!("a started game writes its record");
    };
    assert!(path.exists());
    assert!(
        matches!(write_record(&mut game, &dir), Ok(SaveOutcome::AlreadySaved)),
        "one record per session"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn records_carry_their_kind_and_a_named_save_leaves_the_session_recorder_alone() {
    let dir = scratch("kind-stamp");
    let mut game = Game::new(oxide_sim::Scenario::skirmish()).expect("game");
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
    assert!(
        !game.is_saved(),
        "an explicit save is not the session's quit record"
    );
    // A second save on the same tick walks the name instead of
    // overwriting the first.
    let again = write_named(&game, "again", &dir, 1_784_721_601).expect("the twin walks a counter");
    assert_ne!(saved, again);
    // The quit autosave stamps its own kind.
    let Ok(SaveOutcome::Wrote(auto_path)) = write_record(&mut game, &dir) else {
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
    let mut game = Game::new(oxide_sim::Scenario::skirmish()).expect("game");
    game.advance_ticks(1);
    let err = write_record(&mut game, &dir).expect_err("the failure surfaces");
    assert!(matches!(err, SaveError::CreateDir { .. }));
    assert!(err.player_line().is_ascii(), "the menu font is Latin-1");
    assert!(!game.is_saved(), "a failed save still owes a record");
    std::fs::remove_file(&dir).ok();
}

#[test]
fn a_failed_publication_removes_only_its_uncommitted_reservation() {
    let dir = scratch("failed-reservation");
    std::fs::create_dir_all(&dir).unwrap();
    let held = free_path(&dir, "save", 1).unwrap();
    let path = held.path.clone();
    assert!(held.publish(|_| Err::<(), _>("write refused")).is_err());
    assert!(!path.exists());
    let held = free_path(&dir, "save", 1).unwrap();
    let path = held.path.clone();
    assert!(
        held.publish(|path| {
            chassis::fsx::write_atomic(path, |writer| {
                writer.write_all(b"published")?;
                Ok::<_, std::io::Error>(())
            })
            .unwrap();
            Err::<(), _>("late durability failure")
        })
        .is_err()
    );
    assert_eq!(std::fs::read(path).unwrap(), b"published");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn collisions_beyond_one_thousand_remain_exclusively_reserved() {
    let dir = scratch("many-collisions");
    std::fs::create_dir_all(&dir).unwrap();
    let mut game = Game::new(oxide_sim::Scenario::skirmish()).expect("game");
    game.advance_ticks(1);
    let tick = game.state.current_tick();
    for n in 0..1000 {
        let extension = "oxsave";
        let path = if n == 0 {
            dir.join(format!("save-{tick:010}.{extension}"))
        } else {
            dir.join(format!("save-{tick:010}-{n}.{extension}"))
        };
        std::fs::write(path, b"occupied").expect("create known collision");
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
        std::fs::write(dir.join(format!("autosave-000000000{n}.oxsave")), b"{}").unwrap();
    }
    std::fs::write(dir.join("match-0000000100.oxreplay"), b"{}").unwrap();
    std::fs::write(dir.join("save-outpost.json"), b"{}").unwrap();
    std::fs::write(dir.join("autosave-legacy.json"), b"{}").unwrap();
    std::fs::write(dir.join("autosave-0000000001.tmp.42.0"), b"live").unwrap();
    rotate(&dir);
    assert!(
        !dir.join("autosave-0000000000.oxsave").exists(),
        "only the oldest autosave died"
    );
    for n in 1..6 {
        assert!(dir.join(format!("autosave-000000000{n}.oxsave")).exists());
    }
    assert!(dir.join("match-0000000100.oxreplay").exists());
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
        std::fs::write(dir.join(format!("match-{n:010}.oxreplay")), b"{}").unwrap();
    }
    rotate(&dir);
    assert!(!dir.join(format!("match-{:010}.oxreplay", 0)).exists());
    for n in 1..=KEEP_MATCHES {
        assert!(dir.join(format!("match-{n:010}.oxreplay")).exists());
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_directory_at_budget_loses_nothing() {
    let dir = scratch("at-budget");
    std::fs::create_dir_all(&dir).unwrap();
    for n in 0..KEEP_AUTOSAVES {
        std::fs::write(dir.join(format!("autosave-{n:010}.oxsave")), b"{}").unwrap();
    }
    rotate(&dir);
    for n in 0..KEEP_AUTOSAVES {
        assert!(dir.join(format!("autosave-{n:010}.oxsave")).exists());
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn lan_autosaves_rotate_against_their_own_budget() {
    let dir = scratch("lan-autosaves");
    std::fs::create_dir_all(&dir).unwrap();
    for n in 0..=KEEP_AUTOSAVES {
        std::fs::write(dir.join(format!("lan-autosave-{n:010}.oxsave")), b"{}").unwrap();
        std::fs::write(dir.join(format!("autosave-{n:010}.oxsave")), b"{}").unwrap();
    }
    std::fs::write(dir.join("lan-0000000007.oxsave"), b"{}").unwrap();
    rotate(&dir);
    for prefix in ["lan-autosave", "autosave"] {
        assert!(!dir.join(format!("{prefix}-{:010}.oxsave", 0)).exists());
        for n in 1..=KEEP_AUTOSAVES {
            assert!(dir.join(format!("{prefix}-{n:010}.oxsave")).exists());
        }
    }
    assert!(
        dir.join("lan-0000000007.oxsave").exists(),
        "a named LAN save stays"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_lan_host_save_is_a_resumable_checkpoint() {
    let dir = scratch("lan-host");
    let mut game = Game::new(oxide_sim::Scenario::skirmish()).expect("game");
    game.advance_ticks(12);
    let capture = || {
        let manifest =
            crate::game::checkpoint::LanManifest::new(&game.scenario, game.presentation.human, &[]);
        game.capture_save().for_lan(Vec::new(), manifest)
    };
    for (name, kind) in [(None, "lan-autosave"), (Some("raid"), "lan")] {
        let mut job = SaveJob::lan(&game, capture(), name);
        redirect(&mut job, &dir);
        let SaveOutcome::Wrote(path) = job.run().unwrap() else {
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
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_match_overwrites_its_one_slot() {
    let dir = scratch("one-slot");
    let mut game = Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(1);
    let Ok(SaveOutcome::Wrote(first)) = write_record(&mut game, &dir) else {
        panic!("the first autosave lands");
    };
    game.advance_ticks(2);
    let Ok(SaveOutcome::Wrote(second)) = write_record(&mut game, &dir) else {
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
    let mut game = Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(4);
    let Ok(SaveOutcome::Wrote(path)) = write_record(&mut game, &dir) else {
        panic!("the autosave lands");
    };
    let slot = SaveSlot::adopt(&path, &dir).unwrap();
    let crate::saved_game::Loaded::Local(restored) = crate::saved_game::prepare(&path).unwrap()
    else {
        panic!("a local autosave restores locally");
    };
    let mut resumed = restored.with_slot(Some(slot)).install();
    assert!(
        matches!(
            write_record(&mut resumed, &dir),
            Ok(SaveOutcome::AlreadySaved)
        ),
        "leaving right after Continue rewrites nothing"
    );
    resumed.advance_ticks(3);
    let Ok(SaveOutcome::Wrote(again)) = write_record(&mut resumed, &dir) else {
        panic!("the continued match saves");
    };
    assert_eq!(again, path);
    assert_eq!(files(&dir).len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn only_an_autosave_in_the_autosave_folder_lends_its_slot() {
    let dir = Path::new("/data/autosaves");
    let adopt = |path: &str| SaveSlot::adopt(Path::new(path), dir);
    assert_eq!(
        adopt("/data/autosaves/autosave-12.oxsave"),
        Some(SaveSlot("12".into()))
    );
    assert_eq!(
        adopt("/data/autosaves/lan-autosave-12.oxsave"),
        Some(SaveSlot("12".into()))
    );
    for other in [
        "/data/saves/autosave-12.oxsave",
        "/data/autosaves/save-12.oxsave",
        "/data/autosaves/match-12.oxreplay",
        "/data/autosaves/autosave-.oxsave",
        "/data/autosaves/autosave12.oxsave",
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
        let mut game = Game::new(oxide_sim::Scenario::skirmish()).unwrap();
        for ticks in [1, 2] {
            game.advance_ticks(ticks);
            assert!(matches!(
                write_record(&mut game, &dir),
                Ok(SaveOutcome::Wrote(_))
            ));
        }
        slots.push(game.slot.clone());
    }
    let expected: Vec<String> = {
        let mut names: Vec<String> = slots[1..]
            .iter()
            .map(|slot| slot.file("autosave"))
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
    let mut game = Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(oxide_kit::keyframes::KEYFRAME_EVERY + 5);
    assert!(matches!(
        write_record(&mut game, &autosaves),
        Ok(SaveOutcome::Wrote(_))
    ));
    game.issue(oxide_sim::Command::Surrender);
    game.do_tick();
    assert!(game.state.result().is_some());
    let mut job = SaveJob::capture(&game, None);
    assert_eq!(job.dir, crate::paths::matches_dir());
    job.dir = Some(matches.clone());
    job.autosaves = Some(autosaves.clone());
    let mark = job.mark().unwrap();
    let Ok(SaveOutcome::Wrote(record)) = job.run() else {
        panic!("the finished record lands");
    };
    game.mark_saved(&mark);
    assert!(
        files(&autosaves).is_empty(),
        "the record replaces the autosave"
    );
    assert_eq!(files(&matches), vec![game.slot.file("match")]);
    let recording = oxide_kit::load_recording(&record).unwrap();
    assert_eq!(recording.replay.meta.kind.as_deref(), Some("match"));
    assert_eq!(
        recording.keyframes.ticks().collect::<Vec<_>>(),
        [oxide_kit::keyframes::KEYFRAME_EVERY],
        "the record carries the keyframes the match kept"
    );
    assert!(matches!(
        write_record(&mut game, &matches),
        Ok(SaveOutcome::AlreadySaved)
    ));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_match_decided_right_after_its_resume_records_its_whole_history() {
    use oxide_kit::keyframes::KEYFRAME_EVERY;
    let root = scratch("decided-resume");
    let mut game = Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(KEYFRAME_EVERY + 5);
    let mut resumed = game
        .capture_save()
        .checkpoint()
        .unwrap()
        .restore()
        .unwrap()
        .install();
    resumed.issue(oxide_sim::Command::Surrender);
    resumed.do_tick();
    assert!(resumed.state.result().is_some());
    let Ok(SaveOutcome::Wrote(record)) = write_record(&mut resumed, &root) else {
        panic!("the finished record lands");
    };
    let recording = oxide_kit::load_recording(&record).unwrap();
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
    let mut game = Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(1);
    let mut job = SaveJob::capture(&game, None);
    redirect(&mut job, &dir);
    let mark = job.mark().unwrap();
    game.advance_ticks(2);
    assert!(matches!(job.run(), Ok(SaveOutcome::Wrote(_))));
    game.mark_saved(&mark);
    assert!(!game.is_saved());
    let Ok(SaveOutcome::Wrote(path)) = write_record(&mut game, &dir) else {
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
    let mut game = Game::new(oxide_sim::Scenario::skirmish()).unwrap();
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
    let mut game = Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    game.issue(oxide_sim::Command::Surrender);
    game.advance_ticks(2);
    let mut replay = game.recorder.clone();
    replay.meta.ticks = Some(game.state.current_tick());
    let mut watched = Game::from_replay(replay).unwrap();
    assert!(watched.state.result().is_some());
    assert!(matches!(
        write_record(&mut watched, &dir),
        Ok(SaveOutcome::AlreadySaved)
    ));
    assert!(!dir.exists());
}

#[test]
fn a_slot_holds_one_autosave_kind() {
    let dir = scratch("one-kind");
    let mut game = Game::new(oxide_sim::Scenario::skirmish()).unwrap();
    game.advance_ticks(3);
    let manifest =
        crate::game::checkpoint::LanManifest::new(&game.scenario, game.presentation.human, &[]);
    let mut job = SaveJob::lan(
        &game,
        game.capture_save().for_lan(Vec::new(), manifest),
        None,
    );
    redirect(&mut job, &dir);
    assert!(matches!(job.run(), Ok(SaveOutcome::Wrote(_))));
    assert_eq!(files(&dir), vec![game.slot.file("lan-autosave")]);
    game.advance_ticks(1);
    assert!(matches!(
        write_record(&mut game, &dir),
        Ok(SaveOutcome::Wrote(_))
    ));
    assert_eq!(
        files(&dir),
        vec![game.slot.file("autosave")],
        "a host resumed alone keeps one autosave"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn restarting_drops_the_slot_once_the_worker_is_idle() {
    let dir = scratch("discard");
    std::fs::create_dir_all(&dir).unwrap();
    let slot = SaveSlot("9".into());
    for kind in AUTOSAVE_KINDS {
        std::fs::write(dir.join(slot.file(kind)), b"{}").unwrap();
    }
    let mut discarded = vec![slot.clone()];
    discard_when_idle(&mut discarded, true, Some(&dir));
    assert_eq!(
        discarded,
        vec![slot.clone()],
        "a running save may still write it"
    );
    assert_eq!(files(&dir).len(), 2);
    discard_when_idle(&mut discarded, false, Some(&dir));
    assert!(discarded.is_empty());
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
    assert!(matches!(
        write_record(&mut game, &dir),
        Ok(SaveOutcome::NothingToSave)
    ));
    assert!(!dir.exists(), "nothing was written");
}
