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
fn bot_pressure_defaults_to_the_shipped_scenarios_across_the_variant_grid() {
    use aftermine_sim::scenario::BotDifficulty;
    let cli = Cli::try_parse_from(["aftermine-driver", "bot-pressure"]).expect("parses");
    let Cmd::BotPressure {
        dir,
        difficulties,
        seeds,
        no_mirror,
        jobs,
        json,
        replay_dir,
    } = cli.cmd
    else {
        panic!("bot-pressure parsed as another command")
    };
    assert_eq!(dir, PathBuf::from("driver/evaluation/pressure"));
    assert_eq!(difficulties, BotDifficulty::ALL);
    assert_eq!(seeds, 0..3);
    assert_eq!((no_mirror, jobs.get()), (false, 4));
    assert_eq!((json, replay_dir), (false, None));

    let cli = Cli::try_parse_from([
        "aftermine-driver",
        "bot-pressure",
        "--difficulties",
        "prime,scrapheap",
        "--seeds",
        "4..6",
        "--no-mirror",
        "--jobs",
        "8",
    ])
    .expect("parses");
    let Cmd::BotPressure {
        difficulties,
        seeds,
        no_mirror,
        jobs,
        ..
    } = cli.cmd
    else {
        panic!("bot-pressure parsed as another command")
    };
    assert_eq!(
        difficulties,
        [BotDifficulty::Prime, BotDifficulty::Scrapheap]
    );
    assert_eq!((seeds, no_mirror, jobs.get()), (4..6, true, 8));
    for refused in [
        ["bot-pressure", "--seeds", "3..3"],
        ["bot-pressure", "--jobs", "0"],
        ["bot-pressure", "--difficulties", "hard"],
    ] {
        assert!(!parses(&refused), "{refused:?}");
    }
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

#[test]
fn bot_ladder_diff_takes_a_base_and_a_branch_run() {
    let cli = Cli::try_parse_from([
        "aftermine-driver",
        "bot-ladder-diff",
        "main/rows.jsonl",
        "branch/rows.jsonl",
        "--json",
    ])
    .expect("bot-ladder-diff parses");
    let Cmd::BotLadderDiff { base, branch, json } = cli.cmd else {
        panic!("bot-ladder-diff parsed as another command")
    };
    assert_eq!(
        (base, branch),
        (
            PathBuf::from("main/rows.jsonl"),
            PathBuf::from("branch/rows.jsonl")
        )
    );
    assert!(json);
    assert!(!parses(&["bot-ladder-diff", "main/rows.jsonl"]));
    assert!(!parses(&["bot-ladder-diff", "a", "b", "c"]));
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

#[test]
fn bot_gauntlet_takes_families_maps_and_a_seed_range() {
    let cli = Cli::try_parse_from([
        "aftermine-driver",
        "bot-gauntlet",
        "--families",
        "tuning,ball-rush",
        "--maps",
        "skirmish",
        "--seeds",
        "3..7",
    ])
    .expect("bot-gauntlet parses");
    let Cmd::BotGauntlet {
        families,
        seeds,
        tick_limit,
        difficulties,
        ..
    } = cli.cmd
    else {
        panic!("bot-gauntlet parsed as another command")
    };
    assert_eq!(families, ["tuning", "ball-rush"]);
    assert_eq!((seeds, tick_limit), (3..7, 24_000));
    assert_eq!(
        difficulties,
        [aftermine_sim::scenario::BotDifficulty::Prime]
    );
    for refused in ["7..3", "4..4", "a..b", "5"] {
        assert!(
            !parses(&[
                "bot-gauntlet",
                "--families",
                "held-army",
                "--maps",
                "skirmish",
                "--seeds",
                refused,
            ]),
            "{refused}"
        );
    }
    assert!(!parses(&["bot-gauntlet", "--maps", "skirmish"]));
}
