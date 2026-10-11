use super::*;
use crate::game::Game;
use crate::game::network::NetRole;
use macroquad::prelude::vec2;
use oxide_sim::{PlayerId, Scenario};

fn hosted() -> Game {
    let mut scenario = Scenario::skirmish();
    for player in &mut scenario.players {
        player.bot = false;
        player.bot_config = None;
    }
    let mut host =
        Game::networked(scenario, PlayerId(0), NetRole::Host, vec2(1280.0, 800.0)).unwrap();
    for _ in 0..40 {
        host.run_batch(&[]);
    }
    host
}

#[test]
fn a_snapshot_rebuilds_the_hosts_world_on_the_client() {
    let host = hosted();
    let bytes = encode(&host.world_snapshot().unwrap()).unwrap();
    let client = Game::rejoined(decode(&bytes).unwrap(), PlayerId(1), vec2(1280.0, 800.0)).unwrap();
    assert_eq!(client.state.hash(), host.state.hash());
    assert_eq!(client.state.current_tick(), 40);
    assert_eq!(client.net_role(), Some(NetRole::Client));
    assert_eq!(client.presentation.human, PlayerId(1));
    assert_eq!(client.recorder.start_tick(), 40, "its replay starts here");
}

#[test]
fn a_damaged_or_inflated_snapshot_is_refused() {
    let host = hosted();
    let bytes = encode(&host.world_snapshot().unwrap()).unwrap();
    assert!(decode(&bytes[..4]).is_err());
    let mut flipped = bytes.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 0xff;
    assert!(decode(&flipped).is_err(), "the frame checksum fails");
    let mut inflated = bytes;
    inflated[..8].copy_from_slice(&(MAX_DECODED + 1).to_le_bytes());
    assert!(decode(&inflated).is_err());
}

#[test]
fn a_client_cannot_rejoin_a_seat_the_match_lacks() {
    let host = hosted();
    let mut snapshot = host.world_snapshot().unwrap();
    snapshot = decode(&encode(&snapshot).unwrap()).unwrap();
    assert!(Game::rejoined(snapshot, PlayerId(9), vec2(1280.0, 800.0)).is_err());
}
