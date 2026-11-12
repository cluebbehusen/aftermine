use super::*;

#[test]
fn run_all_bots_is_an_explicit_complete_match_mode() {
    let cli = Cli::try_parse_from(["aftermine-driver", "run", "skirmish", "--all-bots"])
        .expect("all-bots run parses");
    let Cmd::Run { bots, all_bots, .. } = cli.cmd else {
        panic!("run parsed as another command")
    };
    assert!(!bots);
    assert!(all_bots);
    assert!(
        Cli::try_parse_from([
            "aftermine-driver",
            "run",
            "skirmish",
            "--bots",
            "--all-bots",
        ])
        .is_err(),
        "scenario-configured and all-seat modes are mutually exclusive"
    );
}

#[test]
fn bot_pressure_defaults_to_the_shipped_scenarios() {
    let cli = Cli::try_parse_from(["aftermine-driver", "bot-pressure"]).expect("parses");
    let Cmd::BotPressure {
        dir,
        json,
        replay_dir,
    } = cli.cmd
    else {
        panic!("bot-pressure parsed as another command")
    };
    assert_eq!(dir, PathBuf::from("driver/evaluation/pressure"));
    assert_eq!((json, replay_dir), (false, None));
}

#[test]
fn bot_summary_requires_rows() {
    assert!(Cli::try_parse_from(["aftermine-driver", "bot-summary"]).is_err());
    let cli = Cli::try_parse_from(["aftermine-driver", "bot-summary", "a.jsonl", "--json"])
        .expect("bot-summary parses");
    let Cmd::BotSummary { rows, json } = cli.cmd else {
        panic!("bot-summary parsed as another command")
    };
    assert_eq!(rows, [PathBuf::from("a.jsonl")]);
    assert!(json);
}

/// Whether `aftermine-driver <args>` parses.
fn parses(args: &[&str]) -> bool {
    Cli::try_parse_from(std::iter::once("aftermine-driver").chain(args.iter().copied())).is_ok()
}

#[test]
fn tick_scan_refuses_an_empty_window_or_no_passes() {
    assert!(parses(&["tick-scan", "match.json"]));
    for refused in [
        ["tick-scan", "match.json", "--window", "0"],
        ["tick-scan", "match.json", "--passes", "0"],
    ] {
        assert!(!parses(&refused), "{refused:?}");
    }
}

#[test]
fn tick_profile_requires_its_start_and_at_least_one_tick() {
    assert!(parses(&["tick-profile", "match.json", "--from", "40"]));
    for refused in [
        &["tick-profile", "match.json"][..],
        &["tick-profile", "match.json", "--from", "40", "--ticks", "0"],
    ] {
        assert!(!parses(refused), "{refused:?}");
    }
}

#[test]
fn bot_cost_takes_a_named_workload_or_a_scenario_with_a_window() {
    let cli = Cli::try_parse_from(["aftermine-driver", "bot-cost", "mature-armies", "--json"])
        .expect("named workload parses");
    let Cmd::BotCost {
        workload,
        scenario,
        ticks,
        json,
        ..
    } = cli.cmd
    else {
        panic!("bot-cost parsed as another command")
    };
    assert_eq!(
        workload,
        Some(aftermine_driver::bot_cost::Workload::MatureArmies)
    );
    assert_eq!((scenario, ticks), (None, None));
    assert!(json);

    assert!(
        Cli::try_parse_from([
            "aftermine-driver",
            "bot-cost",
            "--scenario",
            "a.json",
            "--ticks",
            "9"
        ])
        .is_ok()
    );
    for invalid in [
        &["aftermine-driver", "bot-cost"][..],
        &["aftermine-driver", "bot-cost", "--scenario", "a.json"],
        &[
            "aftermine-driver",
            "bot-cost",
            "duel",
            "--scenario",
            "a.json",
            "--ticks",
            "9",
        ],
        &["aftermine-driver", "bot-cost", "marathon"],
    ] {
        assert!(Cli::try_parse_from(invalid).is_err(), "{invalid:?}");
    }
}

#[test]
fn profile_shell_parses_an_exact_replay_window() {
    let cli = Cli::try_parse_from([
        "aftermine-driver",
        "profile-shell",
        "match.json",
        "--from",
        "4500",
        "--to",
        "5750",
        "--speed",
        "8",
    ])
    .expect("profile command parses");
    let Cmd::ProfileShell {
        replay,
        from,
        to,
        speed,
        dev,
        ..
    } = cli.cmd
    else {
        panic!("profile-shell parsed as another command")
    };
    assert_eq!(replay, PathBuf::from("match.json"));
    assert_eq!((from, to), (4500, 5750));
    assert_eq!(speed, 8.0);
    assert!(!dev);
}
