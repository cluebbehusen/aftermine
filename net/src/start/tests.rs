use super::*;

const HOST: PlayerId = PlayerId(0);
const CLIENTS: [PlayerId; 2] = [PlayerId(1), PlayerId(2)];
const HASH: u64 = 0xfeed;

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

/// Twin Forges with seats 0 to 2 human and seat 3 a bot.
fn scenario() -> Scenario {
    let mut scenario =
        Scenario::from_json(include_str!("../../../fixtures/scenarios/twin-forges.json"))
            .expect("the shipped map parses");
    for (seat, player) in scenario.players.iter_mut().enumerate() {
        player.bot = seat == 3;
    }
    scenario
}

/// Each seat with its rejoin token.
fn seated(clients: &[PlayerId]) -> Vec<(PlayerId, Token)> {
    clients.iter().map(|&seat| (seat, token(seat))).collect()
}

fn token(seat: PlayerId) -> Token {
    Token::new(0x100 + u128::from(seat.0))
}

fn ready(hash: u64) -> String {
    JoinMessage::Ready { hash, attempt: 1 }.encode()
}

#[test]
fn each_client_gets_its_own_start_and_go_waits_for_every_ready() {
    let scenario = scenario();
    let mut barrier = StartBarrier::new(
        StartContent::Fresh(&scenario),
        HOST,
        &seated(&CLIENTS),
        HASH,
        1,
        secs(0),
    );
    let starts = barrier.take_outgoing();
    assert_eq!(starts.len(), CLIENTS.len());
    for (seat, line) in starts {
        assert_eq!(
            LobbyMessage::decode(&line).unwrap(),
            LobbyMessage::Start {
                seat,
                scenario: Box::new(scenario.clone()),
                attempt: 1,
            }
        );
    }
    barrier.receive(CLIENTS[1], &ready(HASH)).unwrap();
    assert!(barrier.poll(secs(1)).unwrap().is_none());
    assert!(
        barrier.take_outgoing().is_empty(),
        "no Go before every Ready"
    );
    barrier.receive(CLIENTS[0], &ready(HASH)).unwrap();
    let mut session = barrier
        .poll(secs(2))
        .unwrap()
        .expect("every client is ready");
    let go = |seat| LobbyMessage::Go { token: token(seat) }.encode();
    assert_eq!(
        barrier.take_outgoing(),
        vec![(CLIENTS[0], go(CLIENTS[0])), (CLIENTS[1], go(CLIENTS[1]))],
        "each client gets its own token"
    );
    assert_eq!(session.token_seat(token(CLIENTS[1])), Ok(CLIENTS[1]));
    assert!(
        session.seal(secs(2)).is_some(),
        "the session starts at tick zero"
    );
}

#[test]
fn a_mismatched_hash_or_a_stray_line_fails_the_start() {
    let scenario = scenario();
    let fresh = || {
        StartBarrier::new(
            StartContent::Fresh(&scenario),
            HOST,
            &seated(&CLIENTS),
            HASH,
            1,
            secs(0),
        )
    };
    let seat = CLIENTS[0];
    assert_eq!(
        fresh().receive(seat, &ready(HASH + 1)),
        Err(StartFailed::Mismatch { seat })
    );
    let mut barrier = fresh();
    barrier.receive(seat, &ready(HASH)).unwrap();
    assert_eq!(
        barrier.receive(seat, &ready(HASH)),
        Err(StartFailed::Protocol { seat }),
        "a second Ready"
    );
    for line in [JoinMessage::hello("abc").encode(), "{".to_owned()] {
        assert_eq!(
            fresh().receive(seat, &line),
            Err(StartFailed::Protocol { seat }),
            "{line}"
        );
    }
}

#[test]
fn heartbeats_and_readys_for_another_attempt_are_ignored() {
    let scenario = scenario();
    let mut barrier = StartBarrier::new(
        StartContent::Fresh(&scenario),
        HOST,
        &seated(&CLIENTS),
        HASH,
        2,
        secs(0),
    );
    let seat = CLIENTS[0];
    barrier
        .receive(seat, &JoinMessage::Heartbeat.encode())
        .unwrap();
    // A late Ready from the abandoned first attempt neither counts nor fails.
    barrier.receive(seat, &ready(HASH + 1)).unwrap();
    assert_eq!(barrier.unready(), CLIENTS.to_vec());
    let current = JoinMessage::Ready {
        hash: HASH,
        attempt: 2,
    };
    barrier.receive(seat, &current.encode()).unwrap();
    assert_eq!(barrier.unready(), vec![CLIENTS[1]]);
}

#[test]
fn the_start_times_out_at_its_deadline() {
    let scenario = scenario();
    let mut barrier = StartBarrier::new(
        StartContent::Fresh(&scenario),
        HOST,
        &seated(&CLIENTS),
        HASH,
        1,
        secs(5),
    );
    barrier.receive(CLIENTS[0], &ready(HASH)).unwrap();
    assert_eq!(barrier.unready(), vec![CLIENTS[1]]);
    let deadline = secs(5) + START_TIMEOUT;
    assert!(
        barrier
            .poll(deadline.checked_sub(Duration::from_millis(1)).unwrap())
            .unwrap()
            .is_none()
    );
    assert_eq!(barrier.poll(deadline).unwrap_err(), StartFailed::TimedOut);
}

#[test]
fn the_seats_must_be_exactly_the_scenarios_humans() {
    let scenario = scenario();
    for clients in [
        &[PlayerId(1)][..],
        &[PlayerId(1), PlayerId(2), PlayerId(3)],
        &[PlayerId(1), PlayerId(3)],
    ] {
        let result = std::panic::catch_unwind(|| {
            StartBarrier::new(
                StartContent::Fresh(&scenario),
                HOST,
                &seated(clients),
                HASH,
                1,
                secs(0),
            )
        });
        assert!(result.is_err(), "{clients:?}");
    }
}

#[test]
#[should_panic(expected = "the match already started")]
fn polling_after_the_start_is_a_caller_bug() {
    let scenario = scenario();
    let mut barrier = StartBarrier::new(
        StartContent::Fresh(&scenario),
        HOST,
        &seated(&CLIENTS),
        HASH,
        1,
        secs(0),
    );
    for seat in CLIENTS {
        barrier.receive(seat, &ready(HASH)).unwrap();
    }
    barrier.poll(secs(0)).unwrap();
    let _ = barrier.poll(secs(0));
}

#[test]
#[should_panic(expected = "is not a client seat")]
fn lines_from_unknown_seats_are_a_caller_bug() {
    let scenario = scenario();
    let _ = StartBarrier::new(
        StartContent::Fresh(&scenario),
        HOST,
        &seated(&CLIENTS),
        HASH,
        1,
        secs(0),
    )
    .receive(PlayerId(3), &ready(HASH));
}

#[test]
fn a_resume_start_ships_the_snapshot_and_seals_from_the_saved_tick() {
    let snapshot = [7_u8; 10];
    let pending = [PlayerCommand {
        player: CLIENTS[0],
        command: oxide_sim::Command::Surrender,
    }];
    let content = StartContent::Resume {
        seats: &[HOST, CLIENTS[0], CLIENTS[1]],
        tick: 640,
        snapshot: &snapshot,
        pending: &pending,
    };
    let mut barrier = StartBarrier::new(content, HOST, &seated(&CLIENTS), HASH, 1, secs(0));
    let lines = |seat| {
        let resume = LobbyMessage::Resume {
            seat,
            tick: 640,
            bytes: 10,
            parts: 1,
            attempt: 1,
        };
        let part = LobbyMessage::Part {
            data: "07".repeat(10),
        };
        [(seat, resume.encode()), (seat, part.encode())]
    };
    assert_eq!(
        barrier.take_outgoing(),
        [lines(CLIENTS[0]), lines(CLIENTS[1])].concat()
    );
    for seat in CLIENTS {
        barrier.receive(seat, &ready(HASH)).unwrap();
    }
    let mut session = barrier
        .poll(secs(1))
        .unwrap()
        .expect("every client is ready");
    assert_eq!(session.tick(), 640);
    assert_eq!(session.seal(secs(1)), Some(pending.to_vec()));
}

#[test]
fn a_resume_start_gives_its_snapshot_time_to_reach_every_client() {
    let snapshot = vec![7_u8; 1 << 20];
    let content = StartContent::Resume {
        seats: &[HOST, CLIENTS[0], CLIENTS[1]],
        tick: 640,
        snapshot: &snapshot,
        pending: &[],
    };
    let mut barrier = StartBarrier::new(content, HOST, &seated(&CLIENTS), HASH, 1, secs(0));
    let lines = barrier.take_outgoing();
    let parts: Vec<&String> = lines
        .iter()
        .filter(|(seat, _)| *seat == CLIENTS[0])
        .map(|(_, line)| line)
        .skip(1)
        .collect();
    let others: Vec<&String> = lines
        .iter()
        .filter(|(seat, _)| *seat == CLIENTS[1])
        .map(|(_, line)| line)
        .skip(1)
        .collect();
    assert_eq!(parts, others, "every client gets the same parts");
    // Two clients share the host's link for 2 MiB of snapshot.
    let crossing = snapshot::transfer_time(2 << 20);
    assert_eq!(crossing, secs(4));
    assert!(barrier.poll(START_TIMEOUT).unwrap().is_none());
    assert!(matches!(
        barrier.poll(START_TIMEOUT + crossing),
        Err(StartFailed::TimedOut)
    ));
}
