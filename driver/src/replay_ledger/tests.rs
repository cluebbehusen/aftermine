use super::*;
use aftermine_sim::Scenario;
use aftermine_sim::scenario::{BotDifficulty, BotStance};
use chassis::replay::Replay;

#[test]
fn seats_name_the_controllers_their_setup_records() {
    let prime = BotConfig::new(BotDifficulty::Prime, BotStance::Balanced, 9);
    let mut scenario = Scenario::skirmish();
    scenario.players[0].bot = true;
    scenario.players[0].bot_config = Some(prime);
    scenario.players[1].bot = false;
    scenario.players[1].bot_config = None;
    let mut replay: GameReplay = Replay::new(SIM_VERSION, "test", scenario);
    replay.meta.ticks = Some(12);
    let seats: Vec<SeatPlayer> = ledger(Path::new("unit.json"), &replay)
        .unwrap()
        .seats
        .into_iter()
        .map(|seat| seat.controller)
        .collect();
    assert_eq!(
        seats,
        [SeatPlayer::Bot { config: prime }, SeatPlayer::Human]
    );
}
