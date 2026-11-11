use super::*;
use std::time::{Duration, Instant};

fn completion(worker: &mut Worker) -> (u64, Result<Output>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(result) = worker.poll() {
            return result;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}

#[test]
fn cancellation_keeps_admission_until_the_worker_releases_its_capture() {
    let mut worker = Worker::new().unwrap();
    let (started, observed) = mpsc::sync_channel(1);
    let (release, blocked) = mpsc::sync_channel(1);
    let id = worker
        .start(Box::new(move |cancel| {
            started.send(()).unwrap();
            blocked.recv().unwrap();
            anyhow::ensure!(!cancel.cancelled(), "cancelled");
            Ok(Output::Saved(None))
        }))
        .unwrap();
    observed.recv().unwrap();
    assert!(worker.cancel());
    assert!(worker.start(Box::new(|_| Ok(Output::Saved(None)))).is_err());
    release.send(()).unwrap();
    let (finished, result) = completion(&mut worker);
    assert_eq!(id, finished);
    assert!(result.is_err());
    assert!(!worker.busy());
    let next = worker
        .start(Box::new(|cancel| {
            cancel.commit()?;
            assert!(!cancel.cancel());
            Ok(Output::Saved(None))
        }))
        .unwrap();
    assert_ne!(id, next);
    assert!(completion(&mut worker).1.is_ok());
}

#[test]
fn a_failed_job_does_not_poison_the_worker() {
    let mut worker = Worker::new().unwrap();
    worker
        .start(Box::new(|_| panic!("injected worker failure")))
        .unwrap();
    assert!(completion(&mut worker).1.is_err());
    worker.start(Box::new(|_| Ok(Output::Saved(None)))).unwrap();
    assert!(completion(&mut worker).1.is_ok());
}

#[test]
fn continue_skips_a_corrupt_newest_payload_and_reuses_the_restored_older_save() {
    let dir =
        std::env::temp_dir().join(format!("aftermine-continue-worker-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut game = Game::new(Scenario::skirmish()).unwrap();
    game.advance_ticks(120);
    let mut meta = game.recorder.meta.clone();
    meta.kind = Some("autosave".into());
    meta.ticks = Some(game.state.current_tick());
    let older = dir.join("older.amsave");
    crate::saved_game::write_capture(game.capture_save(), meta, &older).unwrap();
    let newer = dir.join("newer.amsave");
    let mut corrupt = std::fs::read(&older).unwrap();
    *corrupt.last_mut().unwrap() ^= 1;
    std::fs::write(&newer, corrupt).unwrap();
    assert!(
        crate::saved_game::inspect(&newer)
            .unwrap()
            .problem
            .is_none()
    );
    let Output::Loaded(restored) =
        load_candidates(vec![newer, older.clone()], &Cancellation::default()).unwrap()
    else {
        panic!()
    };
    let mut restored = restored.install();
    for _ in 0..120 {
        assert_eq!(game.do_tick().events, restored.do_tick().events);
        assert_eq!(game.hash_hex(), restored.hash_hex());
    }
    let cancelled = Cancellation::default();
    assert!(cancelled.cancel());
    assert!(load_candidates(vec![older], &cancelled).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_shelf_replay_opens_on_the_worker_and_a_bad_one_says_so() {
    let dir = std::env::temp_dir().join(format!("aftermine-watch-worker-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut game = Game::new(Scenario::skirmish()).unwrap();
    game.advance_ticks(60);
    let good = dir.join("match.amreplay");
    let mut record = game.recorder.clone();
    record.meta.ticks = Some(game.state.current_tick());
    aftermine_kit::save_recording(
        &good,
        &record,
        &aftermine_kit::keyframes::Keyframes::default(),
    )
    .unwrap();
    let Output::Recording(recording) = open_recording(&good, &Cancellation::default()).unwrap()
    else {
        panic!("a recording opens")
    };
    let viewer = watch(
        *recording,
        Screen::Replays(screens::shelf::Shelf::open()),
        |error| panic!("{error:#}"),
    );
    let Screen::Playback { back, .. } = viewer else {
        panic!("a viewer opens")
    };
    assert!(
        matches!(*back, Screen::Replays(_)),
        "leaving the viewer lands back on the shelf"
    );

    let bad = dir.join("broken.amreplay");
    std::fs::write(&bad, b"not a replay").unwrap();
    let Err(error) = open_recording(&bad, &Cancellation::default()) else {
        panic!("a broken file opens nothing")
    };
    assert_eq!(
        failure_line(&Intent::Watch(bad), &error),
        "Could not open the replay"
    );
    let cancelled = Cancellation::default();
    assert!(cancelled.cancel());
    assert!(
        open_recording(&good, &cancelled).is_err(),
        "a cancelled open delivers nothing"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn main_menu_leaves_at_once_while_the_finished_record_saves() {
    let mut game = Game::new(Scenario::skirmish()).unwrap();
    let undecided = BackgroundJob::of(1, &game);
    game.issue(aftermine_sim::Command::Surrender);
    game.advance_ticks(1);
    assert!(
        game.state.result().is_some(),
        "premise: the match is decided"
    );
    let finished = BackgroundJob::of(2, &game);
    let main_menu = Intent::Leave(screens::pause::LeaveVerb::MainMenu);
    assert!(leaves_at_once(&main_menu, Some(&finished), &game.slot));
    assert!(
        !leaves_at_once(&main_menu, Some(&undecided), &game.slot),
        "an autosave from before the end leaves the finished record owed"
    );
    assert!(!leaves_at_once(&main_menu, None, &game.slot));
    let other = Game::new(Scenario::skirmish()).unwrap();
    assert!(
        !leaves_at_once(&main_menu, Some(&finished), &other.slot),
        "another match's save"
    );
    for intent in [
        Intent::Leave(screens::pause::LeaveVerb::Quit),
        Intent::Rematch,
    ] {
        assert!(!leaves_at_once(&intent, Some(&finished), &game.slot));
    }

    // A match started from Home before that save lands still hears it fail.
    assert!(failure_outlives_its_match(&finished, &other.slot));
    assert!(
        !failure_outlives_its_match(&finished, &game.slot),
        "while its match stays, Home retries it instead"
    );
    assert!(
        !failure_outlives_its_match(&undecided, &other.slot),
        "a replaced match's autosave has nothing left to save"
    );
}

#[test]
fn quitting_from_home_keeps_a_left_lan_matchs_resumable_save() {
    let dir = std::env::temp_dir().join(format!(
        "aftermine-home-quit-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut duel = Scenario::skirmish();
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
    for _ in 0..12 {
        game.run_batch(&[]);
    }
    let manifest =
        crate::game::checkpoint::LanManifest::new(&game.scenario, game.presentation.human, &[]);
    let job = autosave::SaveJob::lan(
        &game,
        game.capture_save().for_lan(Vec::new(), manifest),
        None,
    );
    let mark = job.mark().unwrap();
    job.run_in(Some(dir.clone()), std::slice::from_ref(&dir))
        .unwrap();
    game.mark_saved(&mark);
    // The host left with Main menu; the match ran on past that save, and
    // its link is gone.
    for _ in 0..3 {
        game.run_batch(&[]);
    }
    assert!(!game.is_saved());
    let before: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(before.len(), 1);

    let intent = Intent::quit(Screen::Home(HomeScreen::open()).holds_live_match());
    if let Some(job) = save_job(&intent, &game, None) {
        job.run_in(Some(dir.clone()), std::slice::from_ref(&dir))
            .unwrap();
    }
    let after: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(
        after, before,
        "the LAN autosave stays and no match record is written"
    );

    assert!(
        save_job(
            &Intent::quit(Screen::Playing.holds_live_match()),
            &game,
            None
        )
        .is_some(),
        "a quit over a live match still saves it"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_quit_leaves_its_lan_match_then_waits_for_what_goes_out_up_to_a_bound() {
    let early = std::time::Duration::ZERO;
    let almost = EXIT_WAIT.saturating_sub(std::time::Duration::from_millis(1));
    assert_eq!(departure(true, true, early), Departure::Leave);
    assert_eq!(departure(true, false, EXIT_WAIT), Departure::Leave);
    assert_eq!(departure(false, true, early), Departure::Wait);
    assert_eq!(departure(false, true, almost), Departure::Wait);
    assert_eq!(departure(false, true, EXIT_WAIT), Departure::Exit);
    assert_eq!(departure(false, false, early), Departure::Exit);
}

#[test]
fn a_failed_load_with_a_quit_pending_quits_and_a_failed_save_asks() {
    use screens::pause::LeaveVerb::{MainMenu, Quit};
    let load = Intent::Continue;
    let save = Intent::Leave(Quit);
    assert_eq!(after_failure(&load, true, false), AfterFailure::Exit);
    assert_eq!(after_failure(&load, true, true), AfterFailure::Exit);
    assert_eq!(
        after_failure(&save, true, false),
        AfterFailure::SaveFailed(Quit)
    );
    assert_eq!(
        after_failure(&Intent::Named("keep".into()), true, false),
        AfterFailure::SaveFailed(Quit)
    );
    assert_eq!(
        after_failure(&Intent::Rematch, false, true),
        AfterFailure::Leave
    );
    assert_eq!(
        after_failure(&Intent::Leave(MainMenu), false, false),
        AfterFailure::SaveFailed(MainMenu)
    );
    assert_eq!(after_failure(&load, false, false), AfterFailure::Back);
    assert_eq!(
        after_failure(&Intent::Named("keep".into()), false, false),
        AfterFailure::Back
    );
}

#[test]
fn a_record_already_gone_counts_as_deleted_and_a_real_failure_names_its_kind() {
    let dir = std::env::temp_dir().join(format!("aftermine-delete-record-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("save-0000000010-1700000000.amsave");
    std::fs::write(&file, b"x").unwrap();
    delete_record(&file).unwrap();
    assert!(!file.exists());
    delete_record(&file).expect("another device deleted it first");
    assert!(
        delete_record(&dir).is_err(),
        "a folder is no record to delete"
    );
    std::fs::remove_dir_all(&dir).unwrap();

    let entry = |kind| crate::saves::ReplayEntry {
        path: file.clone(),
        title: "x".to_owned(),
        map: None,
        format: "1v1".to_owned(),
        clock: "1:00".to_owned(),
        saved_secs: 0,
        problem: None,
        kind,
    };
    assert_eq!(
        Deletion::of(&entry(crate::saves::RecordKind::Save)).failure,
        "Could not delete that save"
    );
    assert_eq!(
        Deletion::of(&entry(crate::saves::RecordKind::Match)).failure,
        "Could not delete that replay"
    );
}
