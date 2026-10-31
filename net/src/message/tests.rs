use super::*;
use aftermine_sim::{PlayerId, UnitId};

/// A seat of every kind.
fn room() -> Vec<RoomSeat> {
    vec![
        RoomSeat {
            holder: SeatHolder::Host {
                name: PlayerName::new("Connor").unwrap(),
            },
            team: Some(0),
        },
        RoomSeat {
            holder: SeatHolder::Player {
                name: PlayerName::new("Alex").unwrap(),
            },
            team: None,
        },
        RoomSeat {
            holder: SeatHolder::Bot {
                difficulty: BotDifficulty::Standard,
                stance: BotStance::Balanced,
            },
            team: Some(1),
        },
        RoomSeat {
            holder: SeatHolder::Open,
            team: None,
        },
        RoomSeat {
            holder: SeatHolder::Reserved {
                name: PlayerName::new("Sam").unwrap(),
            },
            team: None,
        },
    ]
}

fn stop(unit: u32) -> Command {
    Command::Stop {
        units: vec![UnitId(unit)],
    }
}

#[test]
fn every_message_survives_a_round_trip() {
    let client = [
        ClientMessage::Command { command: stop(7) },
        ClientMessage::Ack {
            tick: 19,
            hash: None,
        },
        ClientMessage::Ack {
            tick: 20,
            hash: Some(u64::MAX),
        },
        ClientMessage::Heartbeat,
    ];
    for message in client {
        assert_eq!(ClientMessage::decode(&message.encode()).unwrap(), message);
    }
    let host = [
        HostMessage::Batch {
            tick: 3,
            commands: vec![PlayerCommand {
                player: PlayerId(1),
                command: stop(7),
            }],
        },
        HostMessage::Heartbeat,
        HostMessage::Desync { tick: 40 },
    ];
    for message in host {
        assert_eq!(HostMessage::decode(&message.encode()).unwrap(), message);
    }
    for message in [
        JoinMessage::hello("abc"),
        JoinMessage::Introduce {
            name: PlayerName::new("Alex").unwrap(),
            install: crate::InstallId::new(1),
        },
        JoinMessage::Ready {
            hash: 9,
            attempt: 2,
        },
        JoinMessage::Heartbeat,
    ] {
        assert_eq!(JoinMessage::decode(&message.encode()).unwrap(), message);
    }
    for message in [
        LobbyMessage::hello("abc"),
        LobbyMessage::Map {
            scenario: Box::new(Scenario::skirmish()),
        },
        LobbyMessage::Room {
            seats: room(),
            seat: PlayerId(1),
        },
        LobbyMessage::Full,
        LobbyMessage::Removed,
        LobbyMessage::Heartbeat,
        LobbyMessage::Start {
            seat: PlayerId(2),
            scenario: Box::new(Scenario::skirmish()),
            attempt: 3,
        },
        LobbyMessage::Go {
            token: Token::new(9),
        },
        LobbyMessage::Waiting {
            host: PlayerName::new("Connor").unwrap(),
            map: "Broad Front".to_owned(),
        },
        LobbyMessage::Gone {
            reason: GoneReason::SeatTaken,
        },
        LobbyMessage::Underway,
        LobbyMessage::Resume {
            seat: PlayerId(1),
            tick: 400,
            bytes: 1000,
            parts: 1,
            attempt: 2,
        },
        LobbyMessage::Part {
            data: "00ff".to_owned(),
        },
    ] {
        assert_eq!(LobbyMessage::decode(&message.encode()).unwrap(), message);
    }
}

#[test]
fn a_hello_matches_only_the_same_protocol_sim_and_commit() {
    assert!(same_build(PROTOCOL_VERSION, SIM_VERSION, "abc", "abc"));
    assert!(!same_build(PROTOCOL_VERSION + 1, SIM_VERSION, "abc", "abc"));
    assert!(!same_build(PROTOCOL_VERSION, SIM_VERSION + 1, "abc", "abc"));
    assert!(!same_build(PROTOCOL_VERSION, SIM_VERSION, "abd", "abc"));
}

#[test]
fn wire_shape_is_stable() {
    assert_eq!(
        ClientMessage::Command { command: stop(7) }.encode(),
        r#"{"type":"command","command":{"type":"stop","units":[7]}}"#
    );
    assert_eq!(
        ClientMessage::Ack {
            tick: 19,
            hash: None
        }
        .encode(),
        r#"{"type":"ack","tick":19}"#
    );
    assert_eq!(
        ClientMessage::Ack {
            tick: 20,
            hash: Some(5)
        }
        .encode(),
        r#"{"type":"ack","tick":20,"hash":5}"#
    );
    assert_eq!(
        HostMessage::Batch {
            tick: 3,
            commands: vec![PlayerCommand {
                player: PlayerId(1),
                command: stop(7),
            }],
        }
        .encode(),
        r#"{"type":"batch","tick":3,"commands":[{"player":1,"command":{"type":"stop","units":[7]}}]}"#
    );
    assert_eq!(HostMessage::Heartbeat.encode(), r#"{"type":"heartbeat"}"#);
    assert_eq!(
        HostMessage::Desync { tick: 40 }.encode(),
        r#"{"type":"desync","tick":40}"#
    );
    assert_eq!(ClientMessage::Leave.encode(), r#"{"type":"leave"}"#);
    assert_eq!(
        HostMessage::Seats {
            seats: vec![
                SeatNotice {
                    seat: PlayerId(1),
                    state: SeatState::Quiet
                },
                SeatNotice {
                    seat: PlayerId(2),
                    state: SeatState::Away { waited_ms: 1500 }
                }
            ],
            paused: false
        }
        .encode(),
        r#"{"type":"seats","seats":[{"seat":1,"state":{"kind":"quiet"}},{"seat":2,"state":{"kind":"away","waited_ms":1500}}],"paused":false}"#
    );
    assert_eq!(
        HostMessage::Released {
            reason: Release::Dropped
        }
        .encode(),
        r#"{"type":"released","reason":"dropped"}"#
    );
    assert_eq!(HostMessage::Ended.encode(), r#"{"type":"ended"}"#);
    assert_eq!(
        HostMessage::Replaced {
            seat: PlayerId(2),
            config: BotConfig::new(BotDifficulty::Prime, BotStance::Turtle, 9)
        }
        .encode(),
        r#"{"type":"replaced","seat":2,"config":{"difficulty":"prime","stance":"turtle","personality_seed":9}}"#
    );
    assert_eq!(
        HostMessage::Released {
            reason: Release::Replaced
        }
        .encode(),
        r#"{"type":"released","reason":"replaced"}"#
    );
    let hello = format!(
        r#"{{"type":"hello","protocol":{PROTOCOL_VERSION},"sim":{SIM_VERSION},"commit":"abc"}}"#
    );
    assert_eq!(JoinMessage::hello("abc").encode(), hello);
    assert_eq!(LobbyMessage::hello("abc").encode(), hello);
    assert_eq!(
        JoinMessage::Ready {
            hash: 9,
            attempt: 2
        }
        .encode(),
        r#"{"type":"ready","hash":9,"attempt":2}"#
    );
    assert_eq!(
        JoinMessage::Introduce {
            name: PlayerName::new("Alex").unwrap(),
            install: crate::InstallId::new(1),
        }
        .encode(),
        r#"{"type":"introduce","name":"Alex","install":"00000000000000000000000000000001"}"#
    );
    assert_eq!(
        LobbyMessage::Room {
            seats: room(),
            seat: PlayerId(1)
        }
        .encode(),
        concat!(
            r#"{"type":"room","seats":[{"holder":{"kind":"host","name":"Connor"},"team":0},"#,
            r#"{"holder":{"kind":"player","name":"Alex"}},"#,
            r#"{"holder":{"kind":"bot","difficulty":"standard","stance":"balanced"},"team":1},"#,
            r#"{"holder":{"kind":"open"}},{"holder":{"kind":"reserved","name":"Sam"}}],"seat":1}"#
        )
    );
    assert_eq!(LobbyMessage::Full.encode(), r#"{"type":"full"}"#);
    assert_eq!(
        LobbyMessage::Go {
            token: Token::new(0xab)
        }
        .encode(),
        r#"{"type":"go","token":"000000000000000000000000000000ab"}"#
    );
    assert_eq!(
        JoinMessage::Rejoin {
            token: Token::new(1)
        }
        .encode(),
        r#"{"type":"rejoin","token":"00000000000000000000000000000001"}"#
    );
    assert_eq!(
        JoinMessage::Probe {
            token: Token::new(1)
        }
        .encode(),
        r#"{"type":"probe","token":"00000000000000000000000000000001"}"#
    );
    assert_eq!(
        LobbyMessage::Waiting {
            host: PlayerName::new("Connor").unwrap(),
            map: "Broad Front".to_owned()
        }
        .encode(),
        r#"{"type":"waiting","host":"Connor","map":"Broad Front"}"#
    );
    assert_eq!(
        LobbyMessage::Gone {
            reason: GoneReason::Over
        }
        .encode(),
        r#"{"type":"gone","reason":"over"}"#
    );
    assert_eq!(
        LobbyMessage::Gone {
            reason: GoneReason::TooLarge
        }
        .encode(),
        r#"{"type":"gone","reason":"too_large"}"#
    );
    assert_eq!(LobbyMessage::Underway.encode(), r#"{"type":"underway"}"#);
    assert_eq!(
        LobbyMessage::Resume {
            seat: PlayerId(1),
            tick: 400,
            bytes: 1000,
            parts: 1,
            attempt: 2
        }
        .encode(),
        r#"{"type":"resume","seat":1,"tick":400,"bytes":1000,"parts":1,"attempt":2}"#
    );
    assert_eq!(
        LobbyMessage::Part {
            data: "00ff".to_owned()
        }
        .encode(),
        r#"{"type":"part","data":"00ff"}"#
    );
    let start = LobbyMessage::Start {
        seat: PlayerId(2),
        scenario: Box::new(Scenario::skirmish()),
        attempt: 1,
    }
    .encode();
    assert!(start.starts_with(r#"{"type":"start","seat":2,"scenario":{"#));
    assert!(start.ends_with(r#","attempt":1}"#));
}

#[test]
fn malformed_lines_are_rejected() {
    for line in [
        r#"{"type":"ack","tick":1,"extra":true}"#,
        r#"{"type":"acknowledge","tick":1}"#,
        r#"{"type":"ack"}"#,
        r#"{"type":"heartbeat"}{"type":"heartbeat"}"#,
        r#"{"type":"command","command":{"type":"stop","units":[7],"extra":true}}"#,
        r#"{"type":"command","command":{"type":"harvest","units":[7],"node":{"x":1,"y":2,"z":3}}}"#,
        "not json",
    ] {
        assert!(ClientMessage::decode(line).is_err(), "{line}");
    }
    for line in [
        r#"{"type":"batch","tick":1,"commands":[],"extra":true}"#,
        r#"{"type":"halt","tick":1}"#,
        r#"{"type":"batch","commands":[]}"#,
    ] {
        assert!(HostMessage::decode(line).is_err(), "{line}");
    }
    for line in [
        r#"{"type":"hello","protocol":1}"#,
        r#"{"type":"ready","hash":1,"seat":0}"#,
        r#"{"type":"ready","hash":1}"#,
        r#"{"type":"introduce","name":"\u0007"}"#,
        r#"{"type":"introduce","name":"seventeen chars!!"}"#,
        r#"{"type":"ack","tick":1}"#,
    ] {
        assert!(JoinMessage::decode(line).is_err(), "{line}");
    }
    for line in [
        r#"{"type":"hello","protocol":1,"commit":"abc","extra":true}"#,
        r#"{"type":"start","seat":1}"#,
        r#"{"type":"room","seats":[{"holder":{"kind":"robot"}}],"seat":0}"#,
        r#"{"type":"room","seats":[{"holder":{"kind":"open"},"extra":1}],"seat":0}"#,
        r#"{"type":"batch","tick":0,"commands":[]}"#,
    ] {
        assert!(LobbyMessage::decode(line).is_err(), "{line}");
    }
}
