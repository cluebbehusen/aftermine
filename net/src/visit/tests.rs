use super::*;

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn greeted(errand: fn(&str, Token, Duration) -> Visit) -> Visit {
    let mut visit = errand("abc", Token::new(5), secs(0));
    visit
        .receive(&LobbyMessage::hello("abc").encode(), secs(0))
        .unwrap();
    visit
}

#[test]
fn a_visit_says_hello_then_asks_what_it_came_for() {
    let mut probe = greeted(Visit::probe);
    assert_eq!(
        probe.take_outgoing(),
        [
            JoinMessage::hello("abc").encode(),
            JoinMessage::Probe {
                token: Token::new(5)
            }
            .encode()
        ]
    );
    let mut rejoin = greeted(Visit::rejoin);
    assert_eq!(
        rejoin.take_outgoing().last(),
        Some(
            &JoinMessage::Rejoin {
                token: Token::new(5)
            }
            .encode()
        )
    );
}

#[test]
fn another_build_or_a_stray_line_ends_the_visit() {
    let mut visit = Visit::rejoin("abc", Token::new(5), secs(0));
    assert_eq!(
        visit.receive(&LobbyMessage::hello("zzz").encode(), secs(0)),
        Err(VisitEnd::OtherBuild {
            commit: "zzz".to_owned()
        })
    );
    let cases = [
        LobbyMessage::Underway.encode(),
        LobbyMessage::Go {
            token: Token::new(5),
        }
        .encode(),
        LobbyMessage::Resume {
            seat: PlayerId(1),
            tick: 0,
            bytes: 10,
            parts: 4,
            attempt: 1,
        }
        .encode(),
        "garbage".to_owned(),
    ];
    for line in cases {
        let mut visit = greeted(Visit::rejoin);
        assert_eq!(
            visit.receive(&line, secs(0)),
            Err(VisitEnd::Protocol),
            "{line}"
        );
    }
    let mut probe = greeted(Visit::probe);
    let resume = LobbyMessage::Resume {
        seat: PlayerId(1),
        tick: 0,
        bytes: 1,
        parts: 1,
        attempt: 1,
    };
    assert_eq!(
        probe.receive(&resume.encode(), secs(0)),
        Err(VisitEnd::Protocol),
        "a probe takes no snapshot"
    );
}

#[test]
fn go_needs_the_restored_world_and_the_visits_own_token() {
    let resume = LobbyMessage::Resume {
        seat: PlayerId(1),
        tick: 9,
        bytes: 2,
        parts: 1,
        attempt: 4,
    };
    let part = LobbyMessage::Part {
        data: "0102".to_owned(),
    };
    let restored = || {
        let mut visit = greeted(Visit::rejoin);
        visit.receive(&resume.encode(), secs(0)).unwrap();
        assert_eq!(
            visit.receive(&part.encode(), secs(0)),
            Ok(Some(VisitEvent::Snapshot {
                seat: PlayerId(1),
                tick: 9,
                bytes: vec![1, 2]
            }))
        );
        visit
    };
    let go = |token| LobbyMessage::Go { token }.encode();
    let mut early = restored();
    assert_eq!(
        early.receive(&go(Token::new(5)), secs(0)),
        Err(VisitEnd::Protocol)
    );
    let mut stranger = restored();
    stranger.ready(1);
    assert_eq!(
        stranger.receive(&go(Token::new(6)), secs(0)),
        Err(VisitEnd::Protocol)
    );
    let mut visit = restored();
    visit.take_outgoing();
    visit.ready(1);
    assert_eq!(
        visit.take_outgoing(),
        [JoinMessage::Ready {
            hash: 1,
            attempt: 4
        }
        .encode()]
    );
    assert_eq!(
        visit.receive(&go(Token::new(5)), secs(0)),
        Ok(Some(VisitEvent::Go))
    );
}

#[test]
fn a_silent_host_ends_the_visit_but_its_own_pause_does_not() {
    let mut visit = Visit::rejoin("abc", Token::new(5), secs(0));
    visit.poll(secs(0)).unwrap();
    visit.poll(secs(30)).unwrap();
    let mut now = secs(30);
    while now < secs(30) + SILENCE_TIMEOUT {
        visit.poll(now).unwrap();
        now += secs(1);
    }
    assert_eq!(visit.poll(now), Err(VisitEnd::Silent));

    // A line received in the pump that sees the gap is not moved into the
    // future: silence from it ends the visit on time.
    let mut visit = Visit::rejoin("abc", Token::new(5), secs(0));
    visit.poll(secs(0)).unwrap();
    visit
        .receive(&LobbyMessage::hello("abc").encode(), secs(30))
        .unwrap();
    visit.poll(secs(30)).unwrap();
    let silent =
        (31..=30 + 2 * SILENCE_TIMEOUT.as_secs()).find(|&second| visit.poll(secs(second)).is_err());
    assert_eq!(silent, Some(30 + SILENCE_TIMEOUT.as_secs()));
}
