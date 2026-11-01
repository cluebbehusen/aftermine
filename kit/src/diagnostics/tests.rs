use super::*;

fn temp() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "aftermine-diagnostics-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn build() -> BuildIdentity {
    BuildIdentity::new("fixture", "diagnostic-host", "false")
}

fn until(test: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !test() {
        assert!(Instant::now() < deadline, "diagnostic watchdog timeout");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn context(tick: u64) -> FrameContext {
    FrameContext {
        screen: "playing",
        tick,
        units: 12,
        buildings: 3,
        speed: 1.0,
        width: 1280,
        height: 800,
        dpi: 2.0,
        paused: false,
        minimized: Some(false),
    }
}

fn incidents(directory: &Path) -> Vec<Value> {
    read_log(&directory.join(INCIDENTS))
        .map(|log| log.incidents)
        .unwrap_or_default()
}

fn has(directory: &Path, kind: &str, thread: &str) -> bool {
    incidents(directory)
        .iter()
        .any(|incident| incident["kind"] == kind && incident["thread"] == thread)
}

#[test]
fn a_main_thread_stall_and_its_resumption_land_in_the_incident_log() {
    let root = temp();
    let monitor =
        Monitor::start_with(Some(root.clone()), build(), Duration::from_millis(100)).unwrap();
    monitor.frame(context(7));
    monitor.frame(context(8));
    let simulation = monitor.stage(Stage::Simulation, 8).unwrap();
    until(|| has(&root, "stall", "main"));
    drop(simulation);
    until(|| has(&root, "resumed", "main"));

    let log = incidents(&root);
    let stall = log
        .iter()
        .find(|incident| incident["kind"] == "stall")
        .unwrap();
    assert_eq!(stall["stage"], "simulation");
    assert_eq!(stall["tick"], 8);
    assert!(stall["stalled_ms"].as_u64().unwrap() >= 100);
    assert_eq!(stall["build"]["revision"], "diagnostic-host");
    assert_eq!(stall["context"]["screen"], "playing");
    assert_eq!(stall["context"]["units"], 12);
    assert_eq!(stall["recent"]["frames"].as_array().unwrap().len(), 2);
    assert_eq!(stall["recent"]["seconds"][0]["ticks"], 1);
    let resumed = log
        .iter()
        .find(|incident| incident["kind"] == "resumed")
        .unwrap();
    assert_eq!(resumed["stage"], "simulation");
    assert!(resumed.get("recent").is_none());
    drop(monitor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_panic_while_writing_an_incident_does_not_wait_on_its_own_lock() {
    let root = temp();
    let monitor =
        Monitor::start_with(Some(root.clone()), build(), Duration::from_secs(60)).unwrap();
    let held = monitor.inner.writing.lock().unwrap();
    let outer = Writing::enter();
    let started = Instant::now();
    monitor
        .inner
        .record_panic("inside a write", None, "backtrace".into());
    assert!(started.elapsed() < WRITE_WAIT / 2);
    assert_eq!(incidents(&root)[0]["panic"]["message"], "inside a write");
    drop(outer);
    assert!(
        !Writing::enter().0,
        "leaving the outer write clears the mark"
    );
    drop(held);
    drop(monitor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_busy_bot_seat_reports_its_stall_from_any_thread() {
    let root = temp();
    let monitor =
        Monitor::start_with(Some(root.clone()), build(), Duration::from_millis(100)).unwrap();
    std::thread::scope(|scope| {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let monitor = &monitor;
        let worker = scope.spawn(move || {
            let _seat = monitor.seat(3, 42).unwrap();
            wait.recv().unwrap();
        });
        until(|| has(&root, "stall", "bot seat 3"));
        release.send(()).unwrap();
        worker.join().unwrap();
    });
    until(|| has(&root, "resumed", "bot seat 3"));
    let log = incidents(&root);
    assert_eq!(log[0]["tick"], 42);
    assert_eq!(log[0]["seats"][0]["seat"], 3);
    assert!(
        log.iter().all(|incident| incident["thread"] != "main"),
        "the main thread is unwatched until its first frame"
    );
    assert!(monitor.seat(u8::try_from(SEATS).unwrap(), 0).is_none());
    drop(monitor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_window_minimized_after_its_last_frame_is_not_a_stall() {
    let root = temp();
    let monitor =
        Monitor::start_with(Some(root.clone()), build(), Duration::from_millis(50)).unwrap();
    monitor.frame(context(0));
    let present = monitor.stage(Stage::Present, 0).unwrap();
    monitor.minimized(true);
    std::thread::sleep(Duration::from_millis(300));
    assert!(incidents(&root).is_empty());
    drop(present);
    drop(monitor);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn frame_stage_times_partition_the_frame_interval() {
    let monitor = Monitor::start_with(None, build(), Duration::from_secs(60)).unwrap();
    monitor.frame(context(0));
    {
        let _input = monitor.stage(Stage::Input, 0).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        let _simulation = monitor.stage(Stage::Simulation, 0).unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
    monitor.frame(context(1));
    let recent = monitor.inner.recent.lock().unwrap().json();
    let frame = &recent["frames"][1];
    let stages = frame["stages_us"].as_object().unwrap();
    assert!(stages["input"].as_u64().unwrap() >= 5_000);
    assert!(stages["simulation"].as_u64().unwrap() >= 10_000);
    assert_eq!(
        stages
            .values()
            .map(|value| value.as_u64().unwrap())
            .sum::<u64>(),
        frame["interval_us"].as_u64().unwrap(),
        "nested stages are counted once"
    );
    assert_eq!(recent["seconds"].as_array().unwrap().len(), 1);
}

#[test]
fn other_threads_cannot_move_the_main_thread_heartbeat() {
    let monitor = Monitor::start_with(None, build(), Duration::from_secs(60)).unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            assert!(monitor.stage(Stage::Simulation, 9).is_none());
            monitor.progress(9);
            monitor.frame(context(9));
        });
    });
    assert_eq!(monitor.inner.main.tick.load(Ordering::Relaxed), 0);
    assert!(!monitor.inner.main.armed.load(Ordering::Relaxed));
    assert!(monitor.inner.recent.lock().unwrap().frames.is_empty());
}

#[test]
fn a_dropped_monitor_has_stopped_its_watchdog() {
    // A watchdog still running after the drop could write a stall into a
    // directory its owner is already removing.
    let monitor = Monitor::start_with(None, build(), Duration::from_secs(60)).unwrap();
    let inner = Arc::downgrade(&monitor.inner);
    drop(monitor);
    assert!(
        inner.upgrade().is_none(),
        "the watchdog still holds the monitor"
    );
}

#[test]
fn panics_record_their_message_location_and_bounded_backtrace() {
    let root = temp();
    let monitor =
        Monitor::start_with(Some(root.clone()), build(), Duration::from_secs(60)).unwrap();
    monitor.frame(context(5));
    let simulation = monitor.stage(Stage::Simulation, 5).unwrap();
    monitor
        .inner
        .record_panic("boom", Some("sim/src/tick.rs:1:2"), "é".repeat(20_000));
    let log = incidents(&root);
    let panic = &log[0];
    assert_eq!(panic["kind"], "panic");
    assert_eq!(
        panic["thread"],
        std::thread::current().name().unwrap_or("unnamed")
    );
    assert_eq!(panic["panic"]["message"], "boom");
    assert_eq!(panic["panic"]["location"], "sim/src/tick.rs:1:2");
    let backtrace = panic["panic"]["backtrace"].as_str().unwrap();
    assert!(!backtrace.is_empty() && backtrace.len() <= MAX_BACKTRACE_BYTES);
    assert_eq!(panic["main"]["stage"], "simulation");
    assert_eq!(panic["main"]["tick"], 5);
    drop(simulation);
    drop(monitor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_fatal_error_lands_in_the_incident_log_with_its_message() {
    let root = temp();
    let monitor =
        Monitor::start_with(Some(root.clone()), build(), Duration::from_secs(60)).unwrap();
    monitor.frame(context(3));
    monitor.fatal("loading the sprite atlas: not found");
    let log = incidents(&root);
    assert_eq!(log[0]["kind"], "fatal");
    assert_eq!(
        log[0]["fatal"]["message"],
        "loading the sprite atlas: not found"
    );
    assert_eq!(log[0]["build"]["revision"], "diagnostic-host");
    drop(monitor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "subprocess fixture; invoked by an_installed_monitor_records_a_panic_before_its_process_dies"]
fn panicking_child() {
    let root = PathBuf::from(std::env::var_os("AFTERMINE_DIAGNOSTICS_TEST_ROOT").unwrap());
    let monitor = Monitor::start(Some(root), build()).unwrap();
    assert!(monitor.install().is_ok());
    let _simulation = stage(Stage::Simulation, 77);
    panic!("child boom");
}

#[test]
fn an_installed_monitor_records_a_panic_before_its_process_dies() {
    let root = temp();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "diagnostics::tests::panicking_child",
            "--ignored",
        ])
        .env("AFTERMINE_DIAGNOSTICS_TEST_ROOT", &root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
    let log = incidents(&root);
    let [panic] = log.as_slice() else {
        panic!("one panic incident: {log:?}");
    };
    assert_eq!(panic["kind"], "panic");
    assert_eq!(panic["panic"]["message"], "child boom");
    assert!(
        panic["panic"]["location"]
            .as_str()
            .unwrap()
            .contains("diagnostics")
    );
    assert!(!panic["panic"]["backtrace"].as_str().unwrap().is_empty());
    assert_eq!(panic["main"]["stage"], "simulation");
    assert_eq!(panic["main"]["tick"], 77);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn incident_logs_keep_the_newest_entries_within_their_bound() {
    let root = temp();
    std::fs::create_dir_all(&root).unwrap();
    for index in 0..MAX_INCIDENTS + 4 {
        append(&root, json!({ "kind": "stall", "index": index })).unwrap();
    }
    let log = read_log(&root.join(INCIDENTS)).unwrap();
    assert_eq!(log.dropped, 4);
    assert_eq!(log.incidents.len(), MAX_INCIDENTS);
    assert_eq!(log.incidents[0]["index"], 4);
    std::fs::write(root.join(INCIDENTS), b"not json").unwrap();
    append(&root, json!({ "kind": "panic" })).unwrap();
    assert_eq!(incidents(&root).len(), 1, "a damaged log restarts");
    std::fs::remove_dir_all(root).unwrap();
}

fn crash_replay() -> crate::GameReplay {
    let mut replay = crate::GameReplay::new(
        aftermine_sim::SIM_VERSION,
        "test",
        aftermine_sim::Scenario::skirmish(),
    );
    replay.record(
        0,
        aftermine_sim::PlayerCommand {
            player: aftermine_sim::PlayerId(0),
            command: aftermine_sim::Command::Surrender,
        },
    );
    replay.meta.ticks = Some(1);
    replay
}

#[test]
fn a_crash_dump_holds_a_loadable_replay_and_the_unfinished_batch() {
    let root = temp();
    let replay = crash_replay();
    let unfinished = [aftermine_sim::PlayerCommand {
        player: aftermine_sim::PlayerId(1),
        command: aftermine_sim::Command::Surrender,
    }];
    let directory = write_crash_dump(
        &root,
        &CrashDump {
            replay: &replay,
            phase: "simulation",
            unfinished: &unfinished,
            message: "boom",
            build: &build(),
        },
    )
    .unwrap();
    assert_eq!(newest_crash_dump(&root), Some(directory.clone()));
    let loaded = crate::load_replay(directory.join("replay.json")).unwrap();
    assert_eq!(loaded.meta.ticks, Some(1));
    assert_eq!(loaded.commands.len(), 1);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(directory.join("crash.json")).unwrap()).unwrap();
    assert_eq!(manifest["phase"], "simulation");
    assert_eq!(manifest["message"], "boom");
    assert_eq!(manifest["ticks"], 1);
    assert_eq!(manifest["unfinished"][0]["player"], 1);
    assert_eq!(manifest["build"]["revision"], "diagnostic-host");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn crash_dumps_keep_the_newest_finished_ones() {
    let root = temp();
    for at in 1..=6 {
        let old = root.join(format!("crash-{at:013}-1"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join(CRASH_DUMP_MANIFEST), b"{}").unwrap();
    }
    let replay = crash_replay();
    let newest = write_crash_dump(
        &root,
        &CrashDump {
            replay: &replay,
            phase: "collection",
            unfinished: &[],
            message: "boom",
            build: &build(),
        },
    )
    .unwrap();
    let kept = crash_dumps(&root);
    assert_eq!(kept.len(), KEEP_CRASH_DUMPS);
    assert_eq!(kept.last(), Some(&newest));
    assert!(!root.join(format!("crash-{:013}-1", 2)).exists());
    let unfinished = root.join("crash-9999999999999-1");
    std::fs::create_dir_all(&unfinished).unwrap();
    assert_eq!(
        newest_crash_dump(&root),
        Some(newest),
        "a dump without its manifest never finished"
    );
    std::fs::remove_dir_all(root).unwrap();
}
