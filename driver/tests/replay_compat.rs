//! Checks at the replay file-loading boundary and the `replay` command.

#[path = "support/cli.rs"]
mod cli;

use aftermine_kit::GameReplay;
use aftermine_sim::{SIM_VERSION, Scenario};
use chassis::replay::ReplayError;
use cli::{TempReplay, driver, jsonl, refused, succeeded};

#[test]
fn checkpoint_origins_reach_every_read_only_replay_surface() {
    let scenario = Scenario::skirmish();
    let mut state = scenario.build().unwrap();
    for _ in 0..37 {
        state.tick(&[]);
    }
    let origin = aftermine_kit::recording::WorldOrigin::capture(&scenario, &state).unwrap();
    let mut replay = GameReplay::with_origin(SIM_VERSION, "test", scenario, origin).unwrap();
    replay.meta.ticks = Some(43);
    let file = TempReplay::save("origin", &replay);
    let replay = aftermine_kit::load_replay(&*file).unwrap();
    let report =
        aftermine_driver::replay_inspect::inspect(&replay, &[37, 43], None, false).unwrap();
    assert_eq!(report.start_tick, 37);
    assert_eq!(
        report
            .snapshots
            .iter()
            .map(|snapshot| snapshot.tick)
            .collect::<Vec<_>>(),
        [37, 43]
    );
    assert_eq!(report.command_activity[0].longest_silence.from_tick, 37);
    assert_eq!(report.command_activity[0].longest_silence.duration_ticks, 6);
    assert!(aftermine_driver::replay_inspect::inspect(&replay, &[36], None, false).is_err());
    let options = aftermine_driver::replay_summary::SummaryOptions {
        until: None,
        every: None,
        minimaps: aftermine_driver::replay_summary::MinimapMode::None,
    };
    let report = aftermine_driver::replay_summary::summarize(&replay, &options).unwrap();
    assert_eq!(report.scenario.start_tick, 37);
    assert_eq!(report.scenario.effective_ticks, 43);
    assert!(
        report
            .digests
            .iter()
            .flat_map(|digest| &digest.rows)
            .all(|row| row.explored_tiles > 0 && row.explored_delta == 0)
    );
    let at_origin = aftermine_driver::replay_summary::summarize(
        &replay,
        &aftermine_driver::replay_summary::SummaryOptions {
            until: Some(37),
            ..options
        },
    )
    .unwrap();
    assert_eq!(at_origin.digests.len(), 1);
    assert!(
        at_origin.digests[0]
            .rows
            .iter()
            .all(|row| row.explored_delta == 0)
    );
    assert!(
        report
            .tech_reach
            .iter()
            .flat_map(|seat| &seat.firsts)
            .all(|first| first.tick >= 37)
    );
    let end = aftermine_kit::runner::run_replay(&replay, None).unwrap();
    assert_eq!(end.current_tick(), 43);
    assert!(aftermine_driver::session::Session::resume(replay).is_err());
}

#[test]
fn checkpoint_exploration_deltas_count_only_new_scouting() {
    use aftermine_sim::{Command, PlayerCommand, PlayerId};
    use chassis::grid::TilePos;
    let scenario = Scenario::skirmish();
    let mut state = scenario.build().unwrap();
    for _ in 0..37 {
        state.tick(&[]);
    }
    let explored = |state: &aftermine_sim::State| -> u64 {
        (0..state.map().height())
            .flat_map(|y| (0..state.map().width()).map(move |x| TilePos::new(x, y)))
            .filter(|&tile| state.vision(PlayerId(0)).explored(tile))
            .count() as u64
    };
    let initial = explored(&state);
    let mut replay = GameReplay::with_origin(
        SIM_VERSION,
        "test",
        scenario.clone(),
        aftermine_kit::recording::WorldOrigin::capture(&scenario, &state).unwrap(),
    )
    .unwrap();
    let unit = state
        .units()
        .iter()
        .find(|unit| unit.player == PlayerId(0))
        .unwrap()
        .id;
    let command = PlayerCommand {
        player: PlayerId(0),
        command: Command::Run {
            units: vec![unit],
            goal: TilePos::new(18, 8),
            queue: false,
        },
    };
    replay.record(37, command.clone());
    let first = state.tick(&[command]);
    assert!(
        !first
            .events
            .iter()
            .any(|event| matches!(event, aftermine_sim::Event::CommandRejected { .. }))
    );
    for _ in 38..237 {
        state.tick(&[]);
    }
    replay.meta.ticks = Some(237);
    let final_explored = explored(&state);
    assert!(final_explored > initial, "scouting must reveal new ground");
    let options = aftermine_driver::replay_summary::SummaryOptions {
        until: None,
        every: Some(50),
        minimaps: aftermine_driver::replay_summary::MinimapMode::None,
    };
    let report = aftermine_driver::replay_summary::summarize(&replay, &options).unwrap();
    assert_eq!(
        report
            .digests
            .iter()
            .map(|digest| digest.rows[0].explored_delta)
            .sum::<u64>(),
        final_explored - initial
    );
    assert_eq!(
        report.digests.last().unwrap().rows[0].explored_tiles,
        final_explored
    );
    let mut from_start = GameReplay::new(SIM_VERSION, "test", scenario);
    from_start.meta.ticks = Some(1);
    let report = aftermine_driver::replay_summary::summarize(&from_start, &options).unwrap();
    assert_eq!(
        report.digests[0].rows[0].explored_delta, initial,
        "scenario-start summaries retain their initial visibility"
    );
}

#[test]
fn a_foreign_version_replay_is_refused_at_version_validation() {
    let mut replay: GameReplay = GameReplay::new(SIM_VERSION + 1, "test", Scenario::skirmish());
    replay.meta.ticks = Some(0);
    let fixture = TempReplay::save("foreign-version-replay", &replay);
    let replay = aftermine_kit::load_replay(&*fixture).expect("the current setup shape loads");
    assert!(matches!(
        replay.validate(Some(SIM_VERSION)),
        Err(ReplayError::VersionMismatch { .. })
    ));

    let stderr = refused(driver().arg("replay").arg(&*fixture));
    assert!(
        stderr.contains("replay was recorded on sim"),
        "the replay command should refuse at version validation: {stderr}"
    );
}

#[test]
fn hash_every_lines_repeat_exactly_and_end_at_the_final_hash() {
    let mut replay = GameReplay::new(SIM_VERSION, "test", Scenario::skirmish());
    replay.meta.ticks = Some(25);
    let file = TempReplay::save("hash-every", &replay);
    let run = |extra: &[&str]| jsonl(&succeeded(driver().arg("replay").arg(&*file).args(extra)));
    let lines = run(&["--hash-every", "10"]);
    assert_eq!(lines, run(&["--hash-every", "10"]));
    let ticks: Vec<_> = lines.iter().map(|line| line["tick"].as_u64()).collect();
    assert_eq!(ticks, [Some(10), Some(20), Some(25)]);
    assert!(lines.iter().all(|line| line["events"].is_string()));
    let plain = run(&[]);
    assert_eq!(plain.len(), 1);
    assert_eq!(lines[2]["hash"], plain[0]["hash"]);
    assert!(plain[0].get("events").is_none());
}

#[test]
fn until_runs_a_prefix_while_a_short_ticks_override_still_refuses() {
    use aftermine_sim::{Command, PlayerCommand, PlayerId, UnitId};
    use chassis::replay::Replay;
    let mut replay: GameReplay = Replay::new(SIM_VERSION, "test", Scenario::skirmish());
    replay.record(
        100,
        PlayerCommand {
            player: PlayerId(0),
            command: Command::Stop {
                units: vec![UnitId(0)],
            },
        },
    );
    replay.meta.ticks = Some(200);
    let file = TempReplay::save("until", &replay);
    let run = || {
        let mut command = driver();
        command.arg("replay").arg(&*file);
        command
    };
    let prefix = jsonl(&succeeded(run().args([
        "--until",
        "50",
        "--hash-every",
        "25",
    ])));
    let ticks: Vec<_> = prefix.iter().map(|line| line["tick"].as_u64()).collect();
    assert_eq!(ticks, [Some(25), Some(50), Some(50)]);
    assert!(refused(run().args(["--ticks", "50"])).contains("unconsumed"));
}
