use super::*;
use aftermine_sim::scenario::{BotDifficulty, BotStance};

/// A short recorded skirmish: seat 0 idle, seat 1 a Prime bot.
fn recorded(name: &str, ticks: u64) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aftermine-driver-fork-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let mut scenario = aftermine_sim::Scenario::skirmish();
    aftermine_kit::controller::seat_bots_around(
        &mut scenario,
        PlayerId(0),
        BotDifficulty::Prime,
        BotStance::Balanced,
        9000,
    )
    .unwrap();
    let outcome =
        aftermine_kit::runner::run_scenario(&scenario, ticks, true, Some(&crate::build_identity()))
            .unwrap();
    let path = dir.join("game.json");
    outcome.replay.unwrap().save(&path).unwrap();
    path
}

#[test]
fn a_fork_restores_the_game_and_checks_what_the_bot_kept() {
    let replay = recorded("kept", 400);
    let case = Case {
        name: "quiet".to_string(),
        replay: replay.to_string_lossy().into_owned(),
        at: 300,
        window: 200,
        family: "held-army".to_string(),
        check: Check::Harvesters,
    };
    let row = play(&case, 1, false).unwrap();
    assert_eq!(row.ticks, 500);
    assert!(!row.bot_lost);
    assert!(row.harvesters.0 > 0, "{row:?}");
    assert!(row.pass, "a quiet window keeps the bot's workers: {row:?}");
    assert!(row.foundries > 0);
    assert!((0.0..=1.0).contains(&row.worth_share.1));
    assert_eq!(
        play(&case, 1, false).unwrap().worth_share,
        row.worth_share,
        "deterministic"
    );
    let cold = play(&case, 1, true).unwrap();
    assert_eq!(
        cold.harvesters.0, row.harvesters.0,
        "the fork's state does not depend on the bot's memory"
    );
}

#[test]
fn a_fork_past_the_recording_is_refused() {
    let replay = recorded("short", 100);
    let case = Case {
        name: "late".to_string(),
        replay: replay.to_string_lossy().into_owned(),
        at: 900,
        window: 100,
        family: "held-army".to_string(),
        check: Check::Foundry,
    };
    assert!(play(&case, 0, false).is_err());
}

#[test]
fn a_home_relative_replay_path_resolves() {
    if let Some(home) = std::env::var_os("HOME") {
        assert_eq!(expand("~/a/b.json"), Path::new(&home).join("a/b.json"));
    }
    assert_eq!(expand("/x/y.json"), PathBuf::from("/x/y.json"));
}
