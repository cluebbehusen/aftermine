use super::*;
use crate::START_TIMEOUT;
use oxide_sim::scenario::{BotDifficulty, BotStance};

const COMMIT: &str = "abc";
const HOST: PlayerId = PlayerId(0);
const HASH: u64 = 0xfeed;

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

/// The installation of the `index`th guest to connect.
fn install(index: usize) -> InstallId {
    InstallId::new(0xA000 + index as u128)
}

fn name(name: &str) -> PlayerName {
    PlayerName::new(name).unwrap()
}

/// Twin Forges with seats 0 to 2 human and seat 3 a bot.
fn map() -> Scenario {
    let mut scenario = Scenario::from_json(include_str!("../../../scenarios/twin-forges.json"))
        .expect("the shipped map parses");
    for (seat, player) in scenario.players.iter_mut().enumerate() {
        player.bot = seat == 3;
    }
    scenario
}

/// The room with the host on seat 0, `players` on the seats after it, and
/// the rest open, then the bot.
fn room(players: &[&str]) -> Vec<RoomSeat> {
    let mut seats = vec![RoomSeat {
        holder: SeatHolder::Host { name: name("Host") },
        team: Some(0),
    }];
    for seat in 1..3 {
        let holder = players
            .get(seat - 1)
            .map_or(SeatHolder::Open, |player| SeatHolder::Player {
                name: name(player),
            });
        seats.push(RoomSeat {
            holder,
            team: Some(u8::from(seat == 2)),
        });
    }
    seats.push(RoomSeat {
        holder: SeatHolder::Bot {
            difficulty: BotDifficulty::default(),
            stance: BotStance::default(),
        },
        team: Some(1),
    });
    seats
}

fn token(seat: PlayerId) -> Token {
    Token::new(0x100 + u128::from(seat.0))
}

/// Hands out each seated member's token in seat order.
fn seat_token(seating: &[(MemberId, PlayerId)]) -> impl FnMut() -> Token + '_ {
    let mut seats = seating.iter().map(|&(_, seat)| seat);
    move || token(seats.next().expect("one token per seat"))
}

/// One host lobby and its guests, passing lines between them in memory.
struct Lan {
    host: Gathering,
    guests: Vec<(MemberId, Guest)>,
    ended: Vec<Option<GuestEnd>>,
    events: Vec<LobbyEvent>,
    now: Duration,
}

impl Lan {
    fn new() -> Self {
        Self {
            host: Gathering::new(COMMIT, &map()),
            guests: Vec::new(),
            ended: Vec::new(),
            events: Vec::new(),
            now: secs(0),
        }
    }

    /// Connects a guest of build `commit` named `player`.
    fn connect(&mut self, player: &str, commit: &str) -> usize {
        let member = self.host.connected(self.now);
        let install = install(self.guests.len());
        self.guests
            .push((member, Guest::new(commit, name(player), install, self.now)));
        self.ended.push(None);
        self.guests.len() - 1
    }

    fn join(&mut self, player: &str) -> usize {
        self.connect(player, COMMIT)
    }

    fn member(&self, guest: usize) -> MemberId {
        self.guests[guest].0
    }

    /// Moves lines both ways until none are left, polling the host, and
    /// returns each guest's events in order.
    fn settle(&mut self) -> Vec<Vec<GuestEvent>> {
        let mut heard = vec![Vec::new(); self.guests.len()];
        loop {
            let mut moved = false;
            // Guests first: a guest's Hello leaves on connect, before the
            // host's arrives.
            for index in 0..self.guests.len() {
                let (member, guest) = &mut self.guests[index];
                for line in guest.take_outgoing() {
                    moved = true;
                    self.host.receive(*member, &line, self.now);
                }
            }
            for (member, line) in self.host.take_outgoing() {
                let Some(index) = self.guests.iter().position(|(id, _)| *id == member) else {
                    continue;
                };
                if self.ended[index].is_some() {
                    continue;
                }
                moved = true;
                match self.guests[index].1.receive(&line, self.now) {
                    Ok(Some(event)) => heard[index].push(event),
                    Ok(None) => {}
                    Err(end) => self.ended[index] = Some(end),
                }
            }
            self.events.extend(self.host.poll(self.now));
            if !moved {
                return heard;
            }
        }
    }

    /// Seats guest `i` on seat `i + 1` and publishes `room`.
    fn publish(&mut self, room: &[RoomSeat]) {
        let seated: Vec<MemberId> = self.guests.iter().map(|(id, _)| *id).collect();
        self.host.publish(room, |member| {
            seated
                .iter()
                .position(|id| *id == member)
                .map(|index| PlayerId(u8::try_from(index + 1).unwrap()))
        });
    }

    fn take_events(&mut self) -> Vec<LobbyEvent> {
        std::mem::take(&mut self.events)
    }

    /// Starts the map with guest `i` on seat `i + 1`.
    fn start(&mut self) {
        let seating: Vec<(MemberId, PlayerId)> = self
            .guests
            .iter()
            .enumerate()
            .map(|(index, (id, _))| (*id, PlayerId(u8::try_from(index + 1).unwrap())))
            .collect();
        self.host.start(
            StartContent::Fresh(&map()),
            HOST,
            &seating,
            HASH,
            self.now,
            seat_token(&seating),
        );
    }
}

#[test]
fn a_guest_is_greeted_named_and_shown_the_map_once_and_the_room_on_change() {
    let mut lan = Lan::new();
    let alex = lan.join("Alex");
    lan.settle();
    assert_eq!(
        lan.take_events(),
        vec![LobbyEvent::Joined {
            member: lan.member(alex),
            name: name("Alex"),
            install: install(alex),
        }]
    );
    assert_eq!(lan.host.name(lan.member(alex)), Some(&name("Alex")));
    assert_eq!(lan.host.install(lan.member(alex)), Some(install(alex)));

    lan.publish(&room(&["Alex"]));
    let heard = lan.settle();
    assert_eq!(heard[alex], vec![GuestEvent::Map, GuestEvent::Room]);
    let guest = &lan.guests[alex].1;
    assert_eq!(guest.map(), Some(&map()));
    assert_eq!(
        guest.room(),
        Some((room(&["Alex"]).as_slice(), PlayerId(1)))
    );

    lan.publish(&room(&["Alex"]));
    assert!(
        lan.host.take_outgoing().is_empty(),
        "an unchanged room stays put"
    );
    let mut changed = room(&["Alex"]);
    changed[2].team = Some(0);
    lan.publish(&changed);
    assert_eq!(lan.settle()[alex], vec![GuestEvent::Room], "no second map");
}

#[test]
fn a_probe_or_rejoin_at_a_lobby_hears_there_is_no_match() {
    for ask in [
        JoinMessage::Probe {
            token: Token::new(1),
        },
        JoinMessage::Rejoin {
            token: Token::new(1),
        },
    ] {
        let mut host = Gathering::new(COMMIT, &map());
        let member = host.connected(secs(0));
        host.receive(member, &JoinMessage::hello(COMMIT).encode(), secs(0));
        host.take_outgoing();
        host.receive(member, &ask.encode(), secs(0));
        assert_eq!(
            host.take_outgoing(),
            [(
                member,
                LobbyMessage::Gone {
                    reason: GoneReason::NoMatch
                }
                .encode()
            )]
        );
        assert_eq!(host.poll(secs(0)), [LobbyEvent::Turned { member }]);
        assert_eq!(host.name(member), None);
    }
}

#[test]
fn a_guest_hears_the_match_already_started() {
    let mut guest = Guest::new(COMMIT, name("Alex"), install(0), secs(0));
    guest
        .receive(&LobbyMessage::hello(COMMIT).encode(), secs(0))
        .unwrap();
    assert_eq!(
        guest.receive(&LobbyMessage::Underway.encode(), secs(0)),
        Err(GuestEnd::Underway)
    );
}

#[test]
fn a_member_of_another_build_is_dropped_and_its_guest_says_why() {
    let mut lan = Lan::new();
    let other = lan.connect("Alex", "xyz");
    lan.settle();
    assert_eq!(
        lan.ended[other],
        Some(GuestEnd::OtherBuild {
            commit: COMMIT.to_owned()
        })
    );
    assert_eq!(
        lan.take_events(),
        vec![LobbyEvent::Left {
            member: lan.member(other),
            reason: LeaveReason::OtherBuild,
        }]
    );
}

#[test]
fn a_refused_or_removed_member_hears_why_and_is_forgotten() {
    let mut lan = Lan::new();
    let (alex, sam) = (lan.join("Alex"), lan.join("Sam"));
    lan.settle();
    lan.take_events();
    lan.host.refuse(lan.member(alex));
    lan.host.remove(lan.member(sam));
    lan.settle();
    assert_eq!(lan.ended[alex], Some(GuestEnd::Full));
    assert_eq!(lan.ended[sam], Some(GuestEnd::Removed));
    assert_eq!(lan.host.name(lan.member(alex)), None);
    assert!(
        lan.take_events().is_empty(),
        "the host asked; nothing to report"
    );
}

#[test]
fn members_that_never_introduce_or_fall_silent_are_dropped() {
    let mut lan = Lan::new();
    let mute = lan.host.connected(lan.now);
    lan.host.take_outgoing();
    lan.now = GREETING_TIMEOUT
        .checked_sub(Duration::from_millis(1))
        .unwrap();
    assert!(lan.host.poll(lan.now).is_empty());
    lan.now = GREETING_TIMEOUT;
    assert_eq!(
        lan.host.poll(lan.now),
        vec![LobbyEvent::Left {
            member: mute,
            reason: LeaveReason::Unintroduced,
        }]
    );

    let alex = lan.join("Alex");
    lan.settle();
    lan.take_events();
    // Heartbeats both ways keep a quiet lobby alive.
    for _ in 0..30 {
        lan.now += Duration::from_millis(500);
        lan.guests[alex].1.poll(lan.now).unwrap();
        lan.settle();
    }
    assert!(lan.take_events().is_empty());
    assert_eq!(lan.ended[alex], None);
    // A guest that stops talking is dropped.
    lan.now += SILENCE_TIMEOUT;
    assert_eq!(
        lan.host.poll(lan.now),
        vec![LobbyEvent::Left {
            member: lan.member(alex),
            reason: LeaveReason::Silent,
        }]
    );
}

#[test]
fn the_host_heartbeats_only_joined_members_it_has_been_quiet_with() {
    let mut host = Gathering::new(COMMIT, &map());
    let member = host.connected(secs(0));
    host.take_outgoing();
    assert!(host.poll(secs(1)).is_empty());
    assert!(
        host.take_outgoing().is_empty(),
        "no heartbeat before joining"
    );
    host.receive(member, &JoinMessage::hello(COMMIT).encode(), secs(1));
    let introduce = JoinMessage::Introduce {
        name: name("Alex"),
        install: install(0),
    };
    host.receive(member, &introduce.encode(), secs(1));
    host.poll(secs(1));
    host.take_outgoing();
    host.poll(secs(1) + HEARTBEAT_INTERVAL);
    assert_eq!(
        host.take_outgoing(),
        vec![(member, LobbyMessage::Heartbeat.encode())]
    );
}

#[test]
fn a_member_that_breaks_the_protocol_is_dropped() {
    let hello = JoinMessage::hello(COMMIT).encode();
    let introduce = JoinMessage::Introduce {
        name: name("Alex"),
        install: install(0),
    }
    .encode();
    let long = format!(r#"{{"type":"introduce","name":"{}"}}"#, " ".repeat(1100));
    for (before, line) in [
        (vec![], "not json".to_owned()),
        (vec![], JoinMessage::Heartbeat.encode()),
        (vec![hello.clone()], hello.clone()),
        (
            vec![hello.clone()],
            r#"{"type":"introduce","name":""}"#.to_owned(),
        ),
        (vec![hello.clone()], long),
        (vec![hello.clone(), introduce.clone()], introduce.clone()),
    ] {
        let mut host = Gathering::new(COMMIT, &map());
        let member = host.connected(secs(0));
        for earlier in &before {
            host.receive(member, earlier, secs(0));
        }
        host.poll(secs(0));
        host.receive(member, &line, secs(0));
        assert_eq!(
            host.poll(secs(0)),
            vec![LobbyEvent::Left {
                member,
                reason: LeaveReason::Protocol,
            }],
            "{line}"
        );
        host.receive(member, &line, secs(0));
        assert!(host.poll(secs(0)).is_empty(), "a dropped member is ignored");
    }

    let mut host = Gathering::new(COMMIT, &map());
    let member = host.connected(secs(0));
    host.receive(member, &hello, secs(0));
    host.receive(member, &introduce, secs(0));
    host.poll(secs(0));
    let stale = JoinMessage::Ready {
        hash: HASH,
        attempt: 1,
    };
    host.receive(member, &stale.encode(), secs(0));
    assert!(host.poll(secs(0)).is_empty(), "a Ready outside a start");
    host.disconnected(member);
    assert_eq!(
        host.poll(secs(0)),
        vec![LobbyEvent::Left {
            member,
            reason: LeaveReason::Closed,
        }]
    );
}

#[test]
fn a_start_reaches_go_once_every_seated_guest_is_ready() {
    let mut lan = Lan::new();
    let (alex, sam) = (lan.join("Alex"), lan.join("Sam"));
    lan.settle();
    lan.publish(&room(&["Alex", "Sam"]));
    lan.settle();
    lan.start();
    let heard = lan.settle();
    for (guest, seat) in [(alex, PlayerId(1)), (sam, PlayerId(2))] {
        assert_eq!(
            heard[guest],
            vec![GuestEvent::Start {
                seat,
                scenario: Box::new(map()),
            }]
        );
    }
    lan.guests[alex].1.ready(HASH);
    lan.settle();
    assert!(lan.host.poll_start(lan.now).unwrap().is_none());
    let mut changed = room(&["Alex", "Sam"]);
    changed[1].team = Some(1);
    lan.publish(&changed);
    assert!(
        lan.settle().iter().all(Vec::is_empty),
        "no room while starting"
    );
    lan.guests[sam].1.ready(HASH);
    lan.settle();
    assert!(lan.host.poll_start(lan.now).unwrap().is_some());
    let heard = lan.settle();
    assert_eq!(
        heard[alex],
        vec![GuestEvent::Go {
            token: token(PlayerId(1))
        }]
    );
    assert_eq!(
        heard[sam],
        vec![GuestEvent::Go {
            token: token(PlayerId(2))
        }]
    );
    assert!(
        lan.guests[alex].1.poll(lan.now + SILENCE_TIMEOUT).is_ok(),
        "the session owns liveness now"
    );
}

#[test]
fn a_failed_start_drops_only_its_culprit_and_the_rest_wait_for_the_next() {
    let mut lan = Lan::new();
    let (alex, sam) = (lan.join("Alex"), lan.join("Sam"));
    lan.settle();
    lan.publish(&room(&["Alex", "Sam"]));
    lan.settle();
    lan.take_events();
    lan.start();
    lan.settle();
    lan.guests[sam].1.ready(HASH);
    lan.guests[alex].1.ready(HASH + 1);
    lan.settle();
    assert_eq!(
        lan.host.poll_start(lan.now).unwrap_err(),
        StartFailed::Mismatch { seat: PlayerId(1) }
    );
    lan.settle();
    assert_eq!(
        lan.take_events(),
        vec![LobbyEvent::Left {
            member: lan.member(alex),
            reason: LeaveReason::FailedStart,
        }]
    );

    // Sam goes back to the room, and the next start counts only its own
    // Ready: Sam's from the failed attempt does not carry over.
    lan.guests.remove(alex);
    lan.ended.remove(alex);
    lan.publish(&room(&["Sam"]));
    assert_eq!(lan.settle()[0], vec![GuestEvent::Room]);
    let late = JoinMessage::Ready {
        hash: HASH,
        attempt: 1,
    };
    let seating = [(lan.member(0), PlayerId(1))];
    let mut scenario = map();
    scenario.players[2].bot = true;
    lan.host.start(
        StartContent::Fresh(&scenario),
        HOST,
        &seating,
        HASH,
        lan.now,
        seat_token(&seating),
    );
    lan.host.receive(lan.member(0), &late.encode(), lan.now);
    assert!(lan.host.poll_start(lan.now).unwrap().is_none());
    lan.settle();
    lan.guests[0].1.ready(HASH);
    lan.settle();
    assert!(lan.host.poll_start(lan.now).unwrap().is_some());
}

#[test]
fn every_member_that_fails_a_start_is_dropped() {
    let mut lan = Lan::new();
    let (alex, sam) = (lan.join("Alex"), lan.join("Sam"));
    lan.settle();
    lan.publish(&room(&["Alex", "Sam"]));
    lan.settle();
    lan.take_events();
    lan.start();
    lan.settle();
    lan.guests[alex].1.ready(HASH + 1);
    lan.guests[sam].1.ready(HASH + 2);
    lan.settle();
    assert_eq!(
        lan.host.poll_start(lan.now).unwrap_err(),
        StartFailed::Mismatch { seat: PlayerId(1) },
        "the first failure names the start's"
    );
    lan.settle();
    assert_eq!(
        lan.take_events(),
        [alex, sam].map(|guest| LobbyEvent::Left {
            member: lan.member(guest),
            reason: LeaveReason::FailedStart,
        })
    );
}

#[test]
fn removing_a_seated_member_fails_the_start() {
    let mut lan = Lan::new();
    let (alex, sam) = (lan.join("Alex"), lan.join("Sam"));
    lan.settle();
    lan.publish(&room(&["Alex", "Sam"]));
    lan.settle();
    lan.take_events();
    lan.start();
    lan.settle();
    lan.guests[alex].1.ready(HASH);
    lan.guests[sam].1.ready(HASH);
    lan.settle();
    lan.host.remove(lan.member(sam));
    assert_eq!(
        lan.host.poll_start(lan.now).unwrap_err(),
        StartFailed::Protocol { seat: PlayerId(2) },
        "no session goes ahead without Sam"
    );
    lan.settle();
    assert_eq!(lan.ended[sam], Some(GuestEnd::Removed));
    assert!(lan.take_events().is_empty(), "the host already knows");
    assert_eq!(lan.host.name(lan.member(alex)), Some(&name("Alex")));
}

#[test]
fn a_cancelled_start_returns_everyone_to_the_room() {
    let mut lan = Lan::new();
    let (alex, sam) = (lan.join("Alex"), lan.join("Sam"));
    lan.settle();
    lan.publish(&room(&["Alex", "Sam"]));
    lan.settle();
    lan.take_events();
    lan.start();
    lan.settle();
    lan.guests[alex].1.ready(HASH);
    lan.settle();
    lan.host.cancel_start();
    lan.publish(&room(&["Alex", "Sam"]));
    let heard = lan.settle();
    assert_eq!(heard[alex], vec![GuestEvent::Room]);
    assert_eq!(heard[sam], vec![GuestEvent::Room]);
    assert!(lan.take_events().is_empty(), "nobody leaves");

    // The next start counts only its own Readys.
    lan.start();
    lan.host.receive(
        lan.member(sam),
        &JoinMessage::Ready {
            hash: HASH,
            attempt: 1,
        }
        .encode(),
        lan.now,
    );
    lan.settle();
    assert!(lan.host.poll_start(lan.now).unwrap().is_none());
    for guest in [alex, sam] {
        lan.guests[guest].1.ready(HASH);
    }
    lan.settle();
    assert!(lan.host.poll_start(lan.now).unwrap().is_some());
}

#[test]
fn a_start_that_times_out_drops_the_unready_and_a_closed_seat_fails_it() {
    let mut lan = Lan::new();
    let (alex, sam) = (lan.join("Alex"), lan.join("Sam"));
    lan.settle();
    lan.publish(&room(&["Alex", "Sam"]));
    lan.settle();
    lan.take_events();
    lan.start();
    lan.settle();
    lan.guests[alex].1.ready(HASH);
    lan.settle();
    assert_eq!(
        lan.host.poll_start(lan.now + START_TIMEOUT).unwrap_err(),
        StartFailed::TimedOut
    );
    lan.settle();
    assert_eq!(
        lan.take_events(),
        vec![LobbyEvent::Left {
            member: lan.member(sam),
            reason: LeaveReason::FailedStart,
        }]
    );

    let mut lan = Lan::new();
    let (alex, sam) = (lan.join("Alex"), lan.join("Sam"));
    lan.settle();
    lan.publish(&room(&["Alex", "Sam"]));
    lan.settle();
    lan.take_events();
    lan.start();
    lan.host.disconnected(lan.member(sam));
    assert_eq!(
        lan.host.poll_start(lan.now).unwrap_err(),
        StartFailed::Protocol { seat: PlayerId(2) }
    );
    lan.settle();
    assert_eq!(
        lan.take_events(),
        vec![LobbyEvent::Left {
            member: lan.member(sam),
            reason: LeaveReason::Closed,
        }]
    );
    assert_eq!(lan.host.name(lan.member(alex)), Some(&name("Alex")));
}

#[test]
fn an_introduction_during_a_start_waits_for_its_outcome() {
    let mut lan = Lan::new();
    let (alex, sam) = (lan.join("Alex"), lan.join("Sam"));
    lan.settle();
    lan.take_events();
    // Only Alex is seated; Sam waits unseated.
    let seated = lan.member(alex);
    lan.host.publish(&room(&["Alex"]), |member| {
        (member == seated).then_some(PlayerId(1))
    });
    lan.settle();
    let seating = [(seated, PlayerId(1))];
    let mut scenario = map();
    scenario.players[2].bot = true;
    lan.host.start(
        StartContent::Fresh(&scenario),
        HOST,
        &seating,
        HASH,
        lan.now,
        seat_token(&seating),
    );
    let late = lan.join("Riley");
    lan.settle();
    assert!(lan.take_events().is_empty(), "no join while starting");
    lan.guests[alex].1.ready(HASH + 1);
    lan.settle();
    assert!(lan.host.poll_start(lan.now).is_err());
    lan.settle();
    let events = lan.take_events();
    assert!(events.contains(&LobbyEvent::Joined {
        member: lan.member(late),
        name: name("Riley"),
        install: install(late),
    }));
    assert!(!events.iter().any(
        |event| matches!(event, LobbyEvent::Joined { member, .. } if *member == lan.member(sam))
    ));
}

#[test]
fn a_guest_refuses_a_room_that_does_not_fit_the_map_or_seat_it() {
    let start = |line: &str| {
        let mut guest = Guest::new(COMMIT, name("Alex"), install(0), secs(0));
        guest
            .receive(&LobbyMessage::hello(COMMIT).encode(), secs(0))
            .unwrap();
        guest.receive(line, secs(0))
    };
    let map_line = LobbyMessage::Map {
        scenario: Box::new(map()),
    }
    .encode();
    let room_line = |seats: Vec<RoomSeat>, seat: u8| {
        LobbyMessage::Room {
            seats,
            seat: PlayerId(seat),
        }
        .encode()
    };
    assert_eq!(
        start(&room_line(room(&["Alex"]), 1)),
        Err(GuestEnd::Protocol),
        "no map yet"
    );
    assert_eq!(
        start(
            &LobbyMessage::Go {
                token: Token::new(1)
            }
            .encode()
        ),
        Err(GuestEnd::Protocol)
    );
    assert_eq!(start("garbage"), Err(GuestEnd::Protocol));
    assert_eq!(
        start(&LobbyMessage::hello(COMMIT).encode()),
        Err(GuestEnd::Protocol)
    );

    for (seats, seat) in [
        (room(&["Alex"]), 2),
        (room(&["Sam"]), 1),
        (room(&["Alex"]), 9),
        (room(&["Alex"])[..3].to_vec(), 1),
    ] {
        let mut guest = Guest::new(COMMIT, name("Alex"), install(0), secs(0));
        guest
            .receive(&LobbyMessage::hello(COMMIT).encode(), secs(0))
            .unwrap();
        guest.receive(&map_line, secs(0)).unwrap();
        assert_eq!(
            guest.receive(&map_line, secs(0)),
            Err(GuestEnd::Protocol),
            "a second map"
        );
        let mut guest = Guest::new(COMMIT, name("Alex"), install(0), secs(0));
        guest
            .receive(&LobbyMessage::hello(COMMIT).encode(), secs(0))
            .unwrap();
        guest.receive(&map_line, secs(0)).unwrap();
        assert_eq!(
            guest.receive(&room_line(seats, seat), secs(0)),
            Err(GuestEnd::Protocol)
        );
    }
}

#[test]
fn a_guest_heartbeats_once_named_and_gives_up_on_a_silent_host() {
    let mut guest = Guest::new(COMMIT, name("Alex"), install(0), secs(0));
    assert_eq!(
        guest.take_outgoing(),
        vec![JoinMessage::hello(COMMIT).encode()]
    );
    guest.poll(secs(0)).unwrap();
    guest.poll(secs(1)).unwrap();
    assert!(
        guest.take_outgoing().is_empty(),
        "no heartbeat before the host's Hello"
    );
    guest
        .receive(&LobbyMessage::hello(COMMIT).encode(), secs(1))
        .unwrap();
    assert_eq!(
        guest.take_outgoing(),
        vec![
            JoinMessage::Introduce {
                name: name("Alex"),
                install: install(0),
            }
            .encode()
        ]
    );
    guest.poll(secs(1)).unwrap();
    guest.poll(secs(1) + HEARTBEAT_INTERVAL).unwrap();
    assert_eq!(guest.take_outgoing(), vec![JoinMessage::Heartbeat.encode()]);
    assert_eq!(guest.poll(secs(1) + SILENCE_TIMEOUT), Err(GuestEnd::Silent));
}

#[test]
#[should_panic(expected = "no start to be ready for")]
fn readying_without_a_start_is_a_caller_bug() {
    Guest::new(COMMIT, name("Alex"), install(0), secs(0)).ready(HASH);
}

/// A guest seated as Alex on seat 1, in the room.
fn seated_guest() -> Guest {
    let mut guest = Guest::new(COMMIT, name("Alex"), install(0), secs(0));
    for message in [
        LobbyMessage::hello(COMMIT),
        LobbyMessage::Map {
            scenario: Box::new(map()),
        },
        LobbyMessage::Room {
            seats: room(&["Alex"]),
            seat: PlayerId(1),
        },
    ] {
        guest.receive(&message.encode(), secs(0)).unwrap();
    }
    guest.take_outgoing();
    guest
}

fn resume_line(tick: Tick, bytes: u64, parts: u32) -> String {
    LobbyMessage::Resume {
        seat: PlayerId(1),
        tick,
        bytes,
        parts,
        attempt: 1,
    }
    .encode()
}

fn part_line(data: &str) -> String {
    LobbyMessage::Part {
        data: data.to_owned(),
    }
    .encode()
}

#[test]
fn a_resumed_start_reaches_every_guest_as_its_snapshot() {
    let mut lan = Lan::new();
    let alex = lan.join("Alex");
    let sam = lan.join("Sam");
    lan.settle();
    lan.publish(&room(&["Alex", "Sam"]));
    lan.settle();
    let snapshot: Vec<u8> = (0..=u8::MAX)
        .cycle()
        .take(crate::snapshot::PART_BYTES + 10)
        .collect();
    let seating = [
        (lan.member(alex), PlayerId(1)),
        (lan.member(sam), PlayerId(2)),
    ];
    lan.host.start(
        StartContent::Resume {
            scenario: &map(),
            tick: 900,
            snapshot: &snapshot,
            pending: &[],
        },
        HOST,
        &seating,
        HASH,
        lan.now,
        seat_token(&seating),
    );
    let heard = lan.settle();
    for (guest, seat) in [(alex, PlayerId(1)), (sam, PlayerId(2))] {
        assert_eq!(
            heard[guest],
            vec![GuestEvent::Resume {
                seat,
                tick: 900,
                bytes: snapshot.clone(),
            }]
        );
        lan.guests[guest].1.ready(HASH);
    }
    lan.settle();
    let session = lan.host.poll_start(lan.now).unwrap().expect("both ready");
    assert_eq!(session.tick(), 900);
    let heard = lan.settle();
    assert!(
        heard
            .iter()
            .all(|events| matches!(events[..], [GuestEvent::Go { .. }]))
    );
}

#[test]
fn a_guest_refuses_snapshot_parts_it_was_not_promised() {
    assert_eq!(
        seated_guest().receive(&part_line("00"), secs(1)),
        Err(GuestEnd::Protocol),
        "a part with no resume"
    );

    let mut guest = seated_guest();
    guest.receive(&resume_line(9, 1, 1), secs(1)).unwrap();
    assert_eq!(
        guest.receive(&part_line("0011"), secs(1)),
        Err(GuestEnd::Protocol),
        "longer than announced"
    );

    let too_big = crate::snapshot::MAX_SNAPSHOT_BYTES + 1;
    let parts = u32::try_from(crate::snapshot::part_count(too_big)).unwrap();
    assert_eq!(
        seated_guest().receive(&resume_line(9, too_big, parts), secs(1)),
        Err(GuestEnd::Protocol),
        "larger than any snapshot"
    );

    let mut guest = seated_guest();
    guest.receive(&resume_line(9, 1, 1), secs(1)).unwrap();
    let room = LobbyMessage::Room {
        seats: room(&["Alex"]),
        seat: PlayerId(1),
    };
    assert_eq!(
        guest.receive(&room.encode(), secs(1)),
        Ok(Some(GuestEvent::Room)),
        "a room calls the start off"
    );
    assert_eq!(
        guest.receive(&part_line("00"), secs(1)),
        Err(GuestEnd::Protocol)
    );
}

#[test]
fn a_member_quiet_while_it_builds_the_start_is_left_to_the_start() {
    let mut lan = Lan::new();
    let alex = lan.join("Alex");
    let sam = lan.join("Sam");
    lan.settle();
    lan.publish(&room(&["Alex", "Sam"]));
    lan.settle();
    lan.start();
    lan.settle();
    lan.take_events();
    lan.now += SILENCE_TIMEOUT + secs(1);
    assert_eq!(
        lan.host.poll(lan.now),
        [],
        "not dropped as silent mid-start"
    );
    assert!(lan.host.name(lan.member(alex)).is_some());
    lan.now = START_TIMEOUT + secs(1);
    assert_eq!(
        lan.host.poll_start(lan.now).err(),
        Some(StartFailed::TimedOut)
    );
    let left = |member| LobbyEvent::Left {
        member,
        reason: LeaveReason::FailedStart,
    };
    let culprits = [left(lan.member(alex)), left(lan.member(sam))];
    assert_eq!(lan.host.poll(lan.now), culprits);
}
