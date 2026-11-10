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
fn a_replaced_match_retires_on_the_worker_when_it_is_free() {
    let mut worker = Worker::new().expect("worker");
    worker.retire(Game::new(aftermine_sim::Scenario::skirmish()).expect("game"));
    assert!(
        worker.busy(),
        "the old match winds down off the frame thread"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let result = loop {
        if let Some((_, result)) = worker.poll() {
            break result;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the retire job finishes"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert!(matches!(result, Ok(Output::Retired)));
    assert!(!worker.busy());
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
