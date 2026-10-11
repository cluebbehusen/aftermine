use super::*;
use crate::visit::{Visit, VisitEnd, VisitEvent};
use crate::{ClientMessage, HostEvent, Token, reports_hash};
use oxide_sim::Tick;

const HOST: PlayerId = PlayerId(0);
const A: PlayerId = PlayerId(1);
const B: PlayerId = PlayerId(2);
const COMMIT: &str = "abc";

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn token(seat: PlayerId) -> Token {
    Token::new(0x100 + u128::from(seat.0))
}

fn session() -> HostSession {
    HostSession::new(HOST, &[A, B], secs(0)).with_tokens(vec![(A, token(A)), (B, token(B))])
}

fn desk() -> Desk {
    Desk::new(COMMIT, PlayerName::new("Connor").unwrap(), "Broad Front")
}

/// Runs the host `ticks` ticks with every client acknowledging.
fn play(session: &mut HostSession, ticks: Tick, now: Duration) {
    for _ in 0..ticks {
        let batch = session.seal(now).expect("nobody holds the match");
        session.publish(&batch);
        session.executed(|| 7);
        let tick = session.tick();
        for seat in [A, B] {
            let ack = ClientMessage::Ack {
                tick,
                hash: reports_hash(tick).then_some(7),
            };
            session.receive(seat, &ack.encode(), now);
        }
    }
    session.take_outgoing();
}

/// A visitor at the desk and the visit on the other end.
struct Call {
    desk: Desk,
    visit: Visit,
    visitor: VisitorId,
    heard: Vec<VisitEvent>,
    ended: Option<VisitEnd>,
    events: Vec<DeskEvent>,
}

impl Call {
    fn new(desk: Desk, visit: Visit, now: Duration) -> Self {
        let mut desk = desk;
        let visitor = desk.connected(now);
        Self {
            desk,
            visit,
            visitor,
            heard: Vec::new(),
            ended: None,
            events: Vec::new(),
        }
    }

    /// Moves lines both ways until none are left.
    fn settle(&mut self, session: &mut HostSession, now: Duration) {
        loop {
            self.events.extend(self.desk.poll(session, now));
            let mut moved = false;
            for line in self.visit.take_outgoing() {
                moved = true;
                self.desk.receive(session, self.visitor, &line, now);
            }
            for (visitor, line) in self.desk.take_outgoing() {
                assert_eq!(visitor, self.visitor);
                if self.ended.is_some() {
                    continue;
                }
                moved = true;
                match self.visit.receive(&line, now) {
                    Ok(Some(event)) => self.heard.push(event),
                    Ok(None) => {}
                    Err(end) => self.ended = Some(end),
                }
            }
            if !moved {
                return;
            }
        }
    }
}

#[test]
fn a_probe_hears_whether_its_seat_is_still_in_play() {
    let mut session = session();
    session.disconnected(A, secs(1));
    let mut call = Call::new(desk(), Visit::probe(COMMIT, token(A), secs(1)), secs(1));
    call.settle(&mut session, secs(1));
    assert_eq!(
        call.heard,
        [VisitEvent::Waiting {
            host: PlayerName::new("Connor").unwrap(),
            map: "Broad Front".to_owned()
        }]
    );
    assert_eq!(
        call.events,
        [DeskEvent::Done {
            visitor: call.visitor
        }]
    );
    assert!(session.waiting(), "a probe changes nothing");

    session.drop_seat(A);
    let mut call = Call::new(desk(), Visit::probe(COMMIT, token(A), secs(1)), secs(1));
    call.settle(&mut session, secs(1));
    assert_eq!(call.ended, Some(VisitEnd::Gone(GoneReason::SeatTaken)));

    let mut call = Call::new(
        desk(),
        Visit::probe(COMMIT, Token::new(1), secs(1)),
        secs(1),
    );
    call.settle(&mut session, secs(1));
    assert_eq!(
        call.ended,
        Some(VisitEnd::Gone(GoneReason::Over)),
        "an unknown token"
    );

    session.conclude();
    let mut call = Call::new(desk(), Visit::probe(COMMIT, token(B), secs(1)), secs(1));
    call.settle(&mut session, secs(1));
    assert_eq!(
        call.ended,
        Some(VisitEnd::Gone(GoneReason::Over)),
        "the match ended"
    );
}

#[test]
fn a_rejoining_client_restores_the_match_and_takes_its_seat_back() {
    let mut session = session();
    play(&mut session, 30, secs(0));
    session.disconnected(A, secs(1));
    session.poll(secs(1));
    let mut call = Call::new(desk(), Visit::rejoin(COMMIT, token(A), secs(1)), secs(1));
    call.settle(&mut session, secs(1));
    assert_eq!(
        call.events,
        [DeskEvent::Snapshot {
            visitor: call.visitor,
            seat: A
        }]
    );
    assert!(session.waiting() && session.is_rejoining(A));
    let snapshot: Vec<u8> = (0..1000u32).map(|byte| byte.to_le_bytes()[0]).collect();
    call.desk
        .send_snapshot(call.visitor, session.tick(), 77, &snapshot, secs(2));
    call.events.clear();
    call.settle(&mut session, secs(2));
    assert_eq!(
        call.heard,
        [VisitEvent::Snapshot {
            seat: A,
            tick: 30,
            bytes: snapshot
        }]
    );
    call.visit.ready(77);
    call.settle(&mut session, secs(3));
    assert_eq!(
        call.events,
        [DeskEvent::Admitted {
            visitor: call.visitor,
            seat: A
        }]
    );
    assert_eq!(call.heard.last(), Some(&VisitEvent::Go));
    assert!(call.desk.is_empty());
    assert!(!session.waiting());

    // The new connection acknowledges from the snapshot's tick.
    session.receive(B, &ClientMessage::Heartbeat.encode(), secs(3));
    play(&mut session, 25, secs(3));
    assert!(
        session.poll(secs(3)).is_empty(),
        "hash reports still check out"
    );
}

#[test]
fn a_rejoin_replaces_a_connection_the_host_still_trusts() {
    let mut session = session();
    let mut call = Call::new(desk(), Visit::rejoin(COMMIT, token(A), secs(0)), secs(0));
    call.settle(&mut session, secs(0));
    assert!(session.is_rejoining(A));
    assert_eq!(session.poll(secs(0)), [HostEvent::Lost { seat: A }]);
    session.receive(A, &ClientMessage::Heartbeat.encode(), secs(0));
    assert!(
        session.is_rejoining(A),
        "the old connection's lines are ignored"
    );
}

#[test]
fn a_restore_that_disagrees_or_stalls_leaves_the_match_waiting() {
    let mut session = session();
    session.disconnected(A, secs(0));
    let mut call = Call::new(desk(), Visit::rejoin(COMMIT, token(A), secs(0)), secs(0));
    call.settle(&mut session, secs(0));
    call.desk
        .send_snapshot(call.visitor, session.tick(), 77, &[1, 2, 3], secs(0));
    call.settle(&mut session, secs(0));
    call.visit.ready(78);
    call.settle(&mut session, secs(0));
    assert_eq!(
        call.events.last(),
        Some(&DeskEvent::Done {
            visitor: call.visitor
        })
    );
    assert!(session.waiting() && !session.is_rejoining(A));

    let mut call = Call::new(desk(), Visit::rejoin(COMMIT, token(A), secs(1)), secs(1));
    call.settle(&mut session, secs(1));
    call.desk
        .send_snapshot(call.visitor, session.tick(), 77, &[1, 2, 3], secs(1));
    call.settle(&mut session, secs(1));
    assert!(session.is_rejoining(A));
    let events = call.desk.poll(&mut session, secs(1) + START_TIMEOUT);
    assert_eq!(
        events,
        [DeskEvent::Done {
            visitor: call.visitor
        }]
    );
    assert!(session.waiting() && !session.is_rejoining(A));
}

#[test]
fn a_seat_dropped_mid_rejoin_turns_the_visitor_away() {
    let mut session = session();
    session.disconnected(A, secs(0));
    let mut call = Call::new(desk(), Visit::rejoin(COMMIT, token(A), secs(0)), secs(0));
    call.settle(&mut session, secs(0));
    session.drop_seat(A);
    call.settle(&mut session, secs(0));
    assert_eq!(call.ended, Some(VisitEnd::Gone(GoneReason::SeatTaken)));
    assert_eq!(
        call.events.last(),
        Some(&DeskEvent::Done {
            visitor: call.visitor
        })
    );
}

#[test]
fn newcomers_other_builds_and_idlers_are_turned_away() {
    let mut session = session();
    let mut desk = desk();
    let newcomer = desk.connected(secs(0));
    desk.take_outgoing();
    desk.receive(
        &mut session,
        newcomer,
        &JoinMessage::hello(COMMIT).encode(),
        secs(0),
    );
    let name = PlayerName::new("Sam").unwrap();
    desk.receive(
        &mut session,
        newcomer,
        &JoinMessage::Introduce { name }.encode(),
        secs(0),
    );
    assert_eq!(
        desk.take_outgoing(),
        [(newcomer, LobbyMessage::Underway.encode())]
    );

    let stranger = desk.connected(secs(0));
    desk.receive(
        &mut session,
        stranger,
        &JoinMessage::hello("zzz").encode(),
        secs(0),
    );
    let idler = desk.connected(secs(0));
    let oversized = "x".repeat(MAX_JOIN_LINE_BYTES + 1);
    let shouter = desk.connected(secs(0));
    desk.receive(&mut session, shouter, &oversized, secs(0));
    let events = desk.poll(&mut session, GREETING_TIMEOUT);
    for visitor in [newcomer, stranger, idler, shouter] {
        assert!(
            events.contains(&DeskEvent::Done { visitor }),
            "{visitor:?} in {events:?}"
        );
    }
    assert!(desk.is_empty());
}

#[test]
fn the_desk_heartbeats_a_visitor_it_has_nothing_to_say_to() {
    let mut session = session();
    let mut desk = desk();
    let visitor = desk.connected(secs(0));
    desk.poll(&mut session, secs(0));
    desk.take_outgoing();
    desk.poll(&mut session, HEARTBEAT_INTERVAL);
    assert_eq!(
        desk.take_outgoing(),
        [(visitor, LobbyMessage::Heartbeat.encode())]
    );
}
