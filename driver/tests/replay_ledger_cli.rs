//! End-to-end contract coverage for `aftermine-driver replay-ledger`.

#[path = "support/cli.rs"]
mod cli;

use aftermine_kit::GameReplay;
use aftermine_sim::{SIM_VERSION, Scenario};
use chassis::replay::Replay;
use cli::{Scratch, TempReplay, driver, refused, succeeded};
use serde_json::Value;

#[test]
fn a_replay_ledger_reports_every_seat_as_json_and_text() {
    let mut replay: GameReplay = Replay::new(SIM_VERSION, "test", Scenario::skirmish());
    replay.meta.ticks = Some(3_000);
    let path = TempReplay::save("replay-ledger", &replay);
    let json = succeeded(driver().arg("replay-ledger").arg(&*path).arg("--json"));
    let games: Value = serde_json::from_str(&json).unwrap();
    let [game] = games.as_array().unwrap().as_slice() else {
        panic!("{games}");
    };
    assert_eq!(game["ticks"], 3_000);
    let seats = game["seats"].as_array().unwrap();
    assert_eq!(seats.len(), Scenario::skirmish().players.len());
    for seat in seats {
        let ledger = &seat["ledger"];
        assert_eq!(ledger["worth"].as_array().unwrap().len(), 3);
        let foundry = &ledger["buildings"]["foundry"];
        assert!(foundry["starting"].as_u64().unwrap() > 0);
        assert!(
            foundry["passive"].as_u64().unwrap() > 0,
            "the drip: {foundry}"
        );
    }

    let text = succeeded(driver().arg("replay-ledger").arg(&*path).arg(&*path));
    assert!(text.starts_with("2 games"), "{text}");
    assert!(text.contains("income"), "{text}");
}

#[test]
fn a_player_save_is_refused_for_its_missing_history() {
    let dir = Scratch::new("replay-ledger-save");
    let save = dir.join("game.amsave");
    std::fs::write(&save, b"AFTERSAV\0header").unwrap();
    let error = refused(driver().arg("replay-ledger").arg(&save));
    assert!(error.contains("match recording"), "{error}");
}
