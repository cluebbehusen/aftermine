use super::*;
use crate::session::{Playtest, Session};
use aftermine_protocol::Request;
use aftermine_sim::Scenario;
use aftermine_sim::scenario::{BotDifficulty, BotStance};

/// A short playtest against a Standard bot in seat one, as its replay.
fn played(name: &str) -> GameReplay {
    let dir = std::env::temp_dir().join(format!(
        "aftermine-driver-replay-trace-{name}-{}",
        std::process::id()
    ));
    let mut scenario = Scenario::skirmish();
    aftermine_kit::controller::seat_bots_around(
        &mut scenario,
        &[PlayerId(0)],
        BotDifficulty::Standard,
        BotStance::Balanced,
        3,
    )
    .unwrap();
    let out = dir.join("replay.json");
    let mut session = Session::playtest(
        scenario,
        Playtest {
            seats: vec![PlayerId(0)],
            tick_limit: None,
            replay_out: out.clone(),
            min_step: None,
            orders_per_minute: None,
        },
    )
    .unwrap();
    session
        .handle(Request::AdvanceTicks { ticks: 300 })
        .unwrap();
    let replay = aftermine_kit::load_replay(&out).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    replay
}

fn rows(replay: &GameReplay, window: RangeInclusive<u64>) -> Result<Vec<TraceRow>> {
    let mut rows = Vec::new();
    trace(replay, PlayerId(1), window, |row| {
        rows.push(row.clone());
        Ok(())
    })?;
    Ok(rows)
}

#[test]
fn a_replay_traces_its_bot_s_decisions_inside_the_window() {
    let replay = played("window");
    let all = rows(&replay, 0..=u64::MAX).expect("the bot that played traces");
    assert!(!all.is_empty());
    assert!(all.iter().all(|row| row.trace.player == PlayerId(1)));
    assert!(
        all.iter().any(|row| !row.trace.purchases.is_empty()),
        "the bot bought something"
    );
    let late = rows(&replay, 120..=200).unwrap();
    assert!(!late.is_empty());
    assert!(late.iter().all(|row| (120..=200).contains(&row.tick)));
    assert_eq!(
        late,
        all.into_iter()
            .filter(|row| (120..=200).contains(&row.tick))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_record_the_bot_would_not_send_is_refused() {
    let mut replay = played("tampered");
    let forged = replay
        .commands
        .iter_mut()
        .rev()
        .find(|timed| timed.command.player == PlayerId(1))
        .expect("the bot sent commands");
    let tick = forged.tick;
    forged.command.command = aftermine_sim::Command::Stop { units: Vec::new() };
    let error = rows(&replay, 0..=u64::MAX).expect_err("a forged bot command is refused");
    assert!(
        error.to_string().contains(&format!("at tick {tick}")),
        "{error}"
    );
    assert!(tick > 0, "the bot sent commands after its first decision");
    assert!(rows(&replay, 0..=tick - 1).is_ok(), "the window ends first");
    assert!(
        trace(&replay, PlayerId(0), 0..=10, |_| Ok(())).is_err(),
        "seat zero has no bot"
    );
}
