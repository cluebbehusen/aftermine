use super::*;
use crate::HASH_INTERVAL;
use oxide_sim::UnitId;

const HOST: PlayerId = PlayerId(0);
const A: PlayerId = PlayerId(1);
const B: PlayerId = PlayerId(2);

fn stop(unit: u32) -> Command {
    Command::Stop {
        units: vec![UnitId(unit)],
    }
}

fn from(seat: PlayerId, command: Command) -> PlayerCommand {
    PlayerCommand {
        player: seat,
        command,
    }
}

fn command_line(unit: u32) -> String {
    ClientMessage::Command {
        command: stop(unit),
    }
    .encode()
}

fn ack_line(tick: Tick, hash: u64) -> String {
    ClientMessage::Ack {
        tick,
        hash: reports_hash(tick).then_some(hash),
    }
    .encode()
}

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

/// Seals, publishes and executes one tick with no bot commands, with the
/// host hashing to `hash`.
fn step(host: &mut HostSession, now: Duration, hash: u64) -> Option<Vec<PlayerCommand>> {
    let batch = host.seal(now)?;
    host.publish(&batch);
    host.executed(|| hash);
    Some(batch)
}

/// What the host queued, heartbeats aside.
fn sent_lines(host: &mut HostSession) -> Vec<(PlayerId, String)> {
    let heartbeat = HostMessage::Heartbeat.encode();
    host.take_outgoing()
        .into_iter()
        .filter(|(_, line)| *line != heartbeat)
        .collect()
}

fn ack_through(host: &mut HostSession, seat: PlayerId, from: Tick, to: Tick, now: Duration) {
    for tick in from..=to {
        host.receive(seat, &ack_line(tick, 0), now);
    }
}

#[test]
fn commands_join_the_next_sealed_tick_in_rotated_seat_order() {
    let mut host = HostSession::new(HOST, &[B, A], secs(0));
    let submit_all = |host: &mut HostSession| {
        host.receive(B, &command_line(20), secs(0));
        host.receive(A, &command_line(10), secs(0));
        host.submit(stop(0));
        host.receive(A, &command_line(11), secs(0));
    };
    let expected = |order: [PlayerId; 3]| -> Vec<PlayerCommand> {
        order
            .iter()
            .flat_map(|&seat| match seat.0 {
                0 => vec![from(HOST, stop(0))],
                1 => vec![from(A, stop(10)), from(A, stop(11))],
                _ => vec![from(B, stop(20))],
            })
            .collect()
    };
    submit_all(&mut host);
    assert_eq!(step(&mut host, secs(0), 0).unwrap(), expected([HOST, A, B]));
    submit_all(&mut host);
    assert_eq!(step(&mut host, secs(0), 0).unwrap(), expected([A, B, HOST]));
    submit_all(&mut host);
    assert_eq!(step(&mut host, secs(0), 0).unwrap(), expected([B, HOST, A]));
    assert_eq!(step(&mut host, secs(0), 0).unwrap(), Vec::new());
}

#[test]
fn published_batches_carry_the_complete_batch_to_every_connected_client() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    let mut batch = host.seal(secs(0)).unwrap();
    batch.push(from(PlayerId(3), stop(30)));
    host.publish(&batch);
    let line = HostMessage::Batch {
        tick: 0,
        commands: batch,
    }
    .encode();
    assert_eq!(host.take_outgoing(), vec![(A, line.clone()), (B, line)]);
}

#[test]
fn the_lead_cap_waits_for_the_slowest_live_client() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    for _ in 0..LEAD_CAP {
        assert!(step(&mut host, secs(0), 0).is_some());
    }
    assert!(step(&mut host, secs(0), 0).is_none());
    ack_through(&mut host, A, 1, 5, secs(0));
    assert!(step(&mut host, secs(0), 0).is_none(), "B has acked nothing");
    ack_through(&mut host, B, 1, 1, secs(0));
    assert!(step(&mut host, secs(0), 0).is_some());
    assert!(step(&mut host, secs(0), 0).is_none());
    assert!(host.poll(secs(0)).is_empty());
}

#[test]
fn out_of_order_or_misreported_acks_are_protocol_violations() {
    type Violation = fn(&mut HostSession);
    let cases: [(&str, Violation); 5] = [
        ("skipped", |host| host.receive(A, &ack_line(2, 0), secs(0))),
        ("duplicate", |host| {
            host.receive(A, &ack_line(1, 0), secs(0));
            host.receive(A, &ack_line(1, 0), secs(0));
        }),
        ("unpublished", |host| {
            ack_through(host, A, 1, 3, secs(0));
            host.receive(A, &ack_line(4, 0), secs(0));
        }),
        ("unexpected hash", |host| {
            host.receive(
                A,
                &ClientMessage::Ack {
                    tick: 1,
                    hash: Some(1),
                }
                .encode(),
                secs(0),
            );
        }),
        ("garbage", |host| host.receive(A, "{", secs(0))),
    ];
    for (name, break_protocol) in cases {
        let mut host = HostSession::new(HOST, &[A], secs(0));
        for _ in 0..3 {
            step(&mut host, secs(0), 0).unwrap();
        }
        break_protocol(&mut host);
        assert_eq!(
            host.poll(secs(0)),
            vec![HostEvent::Lost { seat: A }],
            "{name}"
        );
        assert!(host.waiting(), "{name}: the match waits for A's player");
    }
}

#[test]
fn a_report_tick_ack_without_a_hash_is_a_protocol_violation() {
    let mut host = HostSession::new(HOST, &[A], secs(0));
    for _ in 0..HASH_INTERVAL {
        step(&mut host, secs(0), 0).unwrap();
    }
    ack_through(&mut host, A, 1, HASH_INTERVAL - 1, secs(0));
    host.receive(
        A,
        &ClientMessage::Ack {
            tick: HASH_INTERVAL,
            hash: None,
        }
        .encode(),
        secs(0),
    );
    assert_eq!(host.poll(secs(0)), vec![HostEvent::Lost { seat: A }]);
}

#[test]
fn a_client_holding_the_host_back_for_the_progress_timeout_makes_the_match_wait() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    for _ in 0..LEAD_CAP {
        step(&mut host, secs(0), 0).unwrap();
    }
    ack_through(&mut host, B, 1, LEAD_CAP, secs(0));
    assert!(step(&mut host, secs(1), 0).is_none());
    let deadline = secs(1) + PROGRESS_TIMEOUT;
    let mut now = secs(1);
    while now < deadline {
        host.receive(A, &ClientMessage::Heartbeat.encode(), now);
        host.receive(B, &ClientMessage::Heartbeat.encode(), now);
        assert!(step(&mut host, now, 0).is_none());
        assert!(host.poll(now).is_empty());
        assert!(!host.waiting(), "waiting early at {now:?}");
        now += Duration::from_millis(250);
    }
    host.receive(A, &ClientMessage::Heartbeat.encode(), deadline);
    host.receive(B, &ClientMessage::Heartbeat.encode(), deadline);
    assert!(host.poll(deadline).is_empty(), "nobody is dropped");
    assert!(host.waiting());
    assert_eq!(
        host.seats(deadline + secs(3)),
        [SeatNotice {
            seat: A,
            state: SeatState::Away { waited_ms: 3000 }
        }]
    );
    assert!(step(&mut host, deadline, 0).is_none());

    // A catches up, and play resumes.
    ack_through(&mut host, A, 1, LEAD_CAP, deadline);
    assert!(host.poll(deadline).is_empty());
    assert!(!host.waiting());
    assert_eq!(
        step(&mut host, deadline, 0),
        Some(Vec::new()),
        "no surrender"
    );
}

#[test]
fn time_spent_paused_never_counts_toward_the_progress_timeout() {
    let mut host = HostSession::new(HOST, &[A], secs(0));
    for _ in 0..LEAD_CAP {
        step(&mut host, secs(0), 0).unwrap();
    }
    assert!(step(&mut host, secs(1), 0).is_none());
    host.set_paused(true);
    for second in 2..30 {
        host.receive(A, &ClientMessage::Heartbeat.encode(), secs(second));
        assert!(step(&mut host, secs(second), 0).is_none());
        host.poll(secs(second));
        assert!(!host.waiting());
    }
    host.set_paused(false);
    assert!(step(&mut host, secs(30), 0).is_none());
    for second in 30..40 {
        host.receive(A, &ClientMessage::Heartbeat.encode(), secs(second));
        host.poll(secs(second));
        assert!(!host.waiting(), "{second}");
    }
    host.receive(A, &ClientMessage::Heartbeat.encode(), secs(40));
    host.poll(secs(40));
    assert!(host.waiting());
}

#[test]
fn repeating_the_pause_state_keeps_the_progress_timer() {
    let mut host = HostSession::new(HOST, &[A], secs(0));
    for _ in 0..LEAD_CAP {
        step(&mut host, secs(0), 0).unwrap();
    }
    assert!(step(&mut host, secs(1), 0).is_none());
    for second in 2..11 {
        host.set_paused(false);
        host.receive(A, &ClientMessage::Heartbeat.encode(), secs(second));
        assert!(step(&mut host, secs(second), 0).is_none());
        host.poll(secs(second));
        assert!(!host.waiting());
    }
    host.set_paused(false);
    host.receive(A, &ClientMessage::Heartbeat.encode(), secs(11));
    host.poll(secs(11));
    assert!(host.waiting());
}

#[test]
fn a_silent_client_goes_quiet_then_away_and_heartbeats_keep_it() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    let mut now = secs(0);
    while now < SILENCE_TIMEOUT {
        host.receive(A, &ClientMessage::Heartbeat.encode(), now);
        assert!(host.poll(now).is_empty());
        let expected: &[SeatNotice] = if now >= QUIET_AFTER {
            &[SeatNotice {
                seat: B,
                state: SeatState::Quiet,
            }]
        } else {
            &[]
        };
        assert_eq!(host.seats(now), expected, "{now:?}");
        assert!(!host.waiting(), "{now:?}");
        now += Duration::from_millis(500);
    }
    host.receive(A, &ClientMessage::Heartbeat.encode(), SILENCE_TIMEOUT);
    assert!(host.poll(SILENCE_TIMEOUT).is_empty(), "nobody is dropped");
    assert!(host.waiting());
}

#[test]
fn a_lost_seat_waits_until_the_host_drops_it_then_surrenders_once() {
    type LoseA = fn(&mut HostSession, Duration);
    let later = PROGRESS_TIMEOUT;
    let losses: [(&str, LoseA); 4] = [
        ("closed", |host, now| host.disconnected(A, now)),
        ("protocol", |host, now| host.receive(A, "{", now)),
        ("silent", |_, _| {}),
        ("stalled", |host, now| {
            host.receive(A, &ClientMessage::Heartbeat.encode(), now);
        }),
    ];
    for (name, lose_a) in losses {
        let mut host = HostSession::new(HOST, &[A, B], secs(0));
        for _ in 0..LEAD_CAP {
            step(&mut host, secs(0), 0).unwrap();
        }
        ack_through(&mut host, B, 1, LEAD_CAP, secs(0));
        assert!(step(&mut host, secs(0), 0).is_none(), "blocked on A");
        host.take_outgoing();
        host.receive(B, &ClientMessage::Heartbeat.encode(), later);
        lose_a(&mut host, later);
        host.poll(later);
        assert!(host.waiting(), "{name}");
        assert!(
            step(&mut host, later, 0).is_none(),
            "{name}: the match waits"
        );

        host.drop_seat(A);
        let connected = matches!(name, "silent" | "stalled");
        let released = host
            .take_outgoing()
            .into_iter()
            .any(|(seat, line)| seat == A && line.contains("released"));
        assert_eq!(released, connected, "{name}: a connected client hears why");
        let events = host.poll(later);
        assert_eq!(
            events.contains(&HostEvent::Released {
                seat: A,
                exit: Exit::Dropped
            }),
            connected,
            "{name}"
        );
        host.receive(A, &command_line(10), later);
        host.drop_seat(A);
        assert!(!host.waiting(), "{name}");
        let batch = step(&mut host, later, 0).expect("A left the gate");
        assert_eq!(batch, vec![from(A, Command::Surrender)], "{name}");
        assert!(
            host.take_outgoing().iter().all(|(seat, _)| *seat == B),
            "{name}: a dropped client is sent nothing"
        );
    }
}

#[test]
fn idle_clients_get_heartbeats_and_batches_reset_the_timer() {
    let mut host = HostSession::new(HOST, &[A], secs(0));
    let heartbeat = (A, HostMessage::Heartbeat.encode());
    assert!(host.poll(Duration::from_millis(499)).is_empty());
    assert!(host.take_outgoing().is_empty());
    host.poll(HEARTBEAT_INTERVAL);
    assert_eq!(host.take_outgoing(), vec![heartbeat.clone()]);
    host.receive(A, &ClientMessage::Heartbeat.encode(), secs(1));
    step(&mut host, secs(1), 0).unwrap();
    host.poll(secs(1));
    assert_eq!(host.take_outgoing().len(), 1, "only the batch");
    host.poll(secs(1) + Duration::from_millis(499));
    assert!(host.take_outgoing().is_empty());
    host.poll(secs(1) + HEARTBEAT_INTERVAL);
    assert_eq!(host.take_outgoing(), vec![heartbeat]);
}

#[test]
fn a_mismatched_hash_halts_the_session_and_tells_every_client() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    for _ in 0..HASH_INTERVAL {
        step(&mut host, secs(0), 7).unwrap();
    }
    host.take_outgoing();
    for tick in 1..=HASH_INTERVAL {
        host.receive(A, &ack_line(tick, 7), secs(0));
    }
    assert!(host.poll(secs(0)).is_empty(), "a matching report is quiet");
    assert_eq!(host.hashes.len(), 1, "B has yet to report");
    for tick in 1..=HASH_INTERVAL {
        host.receive(B, &ack_line(tick, 8), secs(0));
    }
    assert!(host.hashes.is_empty(), "every live client reported");
    assert_eq!(
        host.poll(secs(0)),
        vec![HostEvent::Desync {
            seat: B,
            tick: HASH_INTERVAL
        }]
    );
    let desync = HostMessage::Desync {
        tick: HASH_INTERVAL,
    }
    .encode();
    assert_eq!(host.take_outgoing(), vec![(A, desync.clone()), (B, desync)]);
    assert!(host.seal(secs(0)).is_none());
}

#[test]
fn a_host_is_caught_up_once_every_client_it_is_not_waiting_on_acknowledged_every_batch() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    assert!(host.caught_up());
    for _ in 0..3 {
        step(&mut host, secs(0), 0).unwrap();
    }
    ack_through(&mut host, A, 1, 3, secs(0));
    ack_through(&mut host, B, 1, 2, secs(0));
    assert!(!host.caught_up(), "B is a batch behind");
    host.poll(secs(2));
    assert!(
        host.seats(secs(2))
            .iter()
            .any(|notice| notice.seat == B && notice.state == SeatState::Quiet)
    );
    assert!(
        !host.caught_up(),
        "a quiet client may still report a final hash"
    );
    host.disconnected(B, secs(2));
    assert!(host.caught_up(), "an away client is not counted");
}

#[test]
fn a_host_without_clients_is_never_gated() {
    let mut host = HostSession::new(HOST, &[], secs(0));
    for _ in 0..(3 * LEAD_CAP) {
        step(&mut host, secs(0), 0).unwrap();
    }
    assert!(host.hashes.is_empty());
}

#[test]
#[should_panic(expected = "publish the sealed batch")]
fn sealing_twice_without_publishing_is_a_caller_bug() {
    let mut host = HostSession::new(HOST, &[], secs(0));
    host.seal(secs(0));
    host.seal(secs(0));
}

#[test]
#[should_panic(expected = "seal a batch")]
fn publishing_without_sealing_is_a_caller_bug() {
    HostSession::new(HOST, &[], secs(0)).publish(&[]);
}

#[test]
#[should_panic(expected = "must be distinct")]
fn duplicate_seats_are_a_caller_bug() {
    HostSession::new(HOST, &[A, A], secs(0));
}

#[test]
#[should_panic(expected = "is not a client seat")]
fn lines_from_unknown_seats_are_a_caller_bug() {
    HostSession::new(HOST, &[A], secs(0)).receive(B, "{}", secs(0));
}

/// A Stop order whose client line is exactly `bytes` long, padded with
/// unit ids.
fn bulky_line(bytes: usize) -> String {
    let mut units = Vec::new();
    let line = |units: &[UnitId]| {
        ClientMessage::Command {
            command: Command::Stop {
                units: units.to_vec(),
            },
        }
        .encode()
    };
    while line(&units).len() + 2 <= bytes {
        units.push(UnitId(1));
    }
    let mut padded = line(&units);
    while padded.len() < bytes {
        let last = units.last_mut().expect("padding grows a unit id");
        last.0 = last.0 * 10 + 1;
        padded = line(&units);
    }
    assert_eq!(padded.len(), bytes, "padding lands exactly");
    padded
}

#[test]
fn an_oversized_client_line_loses_the_connection() {
    let mut host = HostSession::new(HOST, &[A], secs(0));
    host.receive(A, &bulky_line(crate::MAX_CLIENT_LINE_BYTES + 1), secs(0));
    assert_eq!(host.poll(secs(0)), [HostEvent::Lost { seat: A }]);
}

#[test]
fn a_client_flooding_a_paused_host_loses_its_connection() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    host.set_paused(true);
    let line = bulky_line(crate::MAX_CLIENT_LINE_BYTES);
    let fit = crate::CLIENT_PENDING_BYTES / line.len();
    for _ in 0..fit {
        host.receive(A, &line, secs(0));
        host.receive(B, &command_line(1), secs(0));
    }
    assert_eq!(host.poll(secs(0)), []);
    host.receive(A, &line, secs(0));
    assert_eq!(host.poll(secs(0)), [HostEvent::Lost { seat: A }]);
}

#[test]
fn pending_bytes_cover_the_published_batch() {
    let pending =
        [Command::Surrender, stop(1), stop(70_000)].map(|command| Pending::new(A, command));
    let batch = |commands: Vec<PlayerCommand>| HostMessage::Batch { tick: 9, commands }.encode();
    let empty = batch(Vec::new()).len();
    let full = batch(
        pending
            .iter()
            .map(|queued| queued.command.clone())
            .collect(),
    )
    .len();
    let counted: usize = pending.iter().map(|queued| queued.bytes).sum();
    assert!(full <= empty + counted, "{full} > {empty} + {counted}");
}

#[test]
fn every_published_batch_fits_one_line() {
    let clients = [1, 2, 3, 4, 5, 6].map(PlayerId);
    let mut host = HostSession::new(HOST, &clients, secs(0));
    let line = bulky_line(crate::MAX_CLIENT_LINE_BYTES);
    let per_client = crate::CLIENT_PENDING_BYTES / line.len();
    for _ in 0..per_client {
        for seat in clients {
            host.receive(seat, &line, secs(0));
        }
    }
    let sent = per_client * clients.len();
    let mut sealed = 0;
    let mut tick = 0;
    while sealed < sent {
        let batch = step(&mut host, secs(0), 0).expect("no client lags yet");
        sealed += batch.len();
        tick += 1;
        for (_, out) in host.take_outgoing() {
            assert!(out.len() <= crate::MAX_LINE_BYTES, "{} bytes", out.len());
        }
        for seat in clients {
            ack_through(&mut host, seat, tick, tick, secs(0));
        }
    }
    assert_eq!(sealed, sent);
    assert!(tick > 1, "the flood spans several ticks");
}

#[test]
fn a_quiet_client_lets_play_run_up_to_ten_seconds_ahead() {
    let mut host = HostSession::new(HOST, &[A], secs(0));
    for _ in 0..LEAD_CAP {
        step(&mut host, secs(0), 0).unwrap();
    }
    assert!(
        step(&mut host, secs(0), 0).is_none(),
        "a live client holds play"
    );
    host.poll(QUIET_AFTER);
    assert!(!host.waiting());
    let mut sealed = LEAD_CAP;
    while step(&mut host, QUIET_AFTER, 0).is_some() {
        sealed += 1;
    }
    assert_eq!(sealed, QUIET_LEAD_CAP, "a quiet client gets the wider lead");
    let mut now = QUIET_AFTER;
    while now < SILENCE_TIMEOUT {
        host.poll(now);
        assert!(!host.waiting(), "{now:?}");
        now += Duration::from_millis(500);
    }
    host.poll(SILENCE_TIMEOUT);
    assert!(
        host.waiting(),
        "silence past the timeout makes the match wait"
    );
}

#[test]
fn a_recovering_connection_ends_the_wait_on_its_own() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    for _ in 0..LEAD_CAP {
        step(&mut host, secs(0), 0).unwrap();
    }
    ack_through(&mut host, B, 1, LEAD_CAP, secs(0));
    host.receive(B, &ClientMessage::Heartbeat.encode(), SILENCE_TIMEOUT);
    host.poll(SILENCE_TIMEOUT);
    assert!(host.waiting());
    let later = SILENCE_TIMEOUT + secs(1);
    host.receive(B, &ClientMessage::Heartbeat.encode(), later);
    host.receive(A, &ClientMessage::Heartbeat.encode(), later);
    host.poll(later);
    assert!(host.waiting(), "A is heard but a full lead behind");
    ack_through(&mut host, A, 1, 1, later);
    host.take_outgoing();
    host.poll(later);
    assert!(!host.waiting());
    let seats = HostMessage::Seats {
        seats: Vec::new(),
        paused: false,
    }
    .encode();
    assert_eq!(
        sent_lines(&mut host),
        vec![(A, seats.clone()), (B, seats)],
        "everyone hears the wait is over"
    );
}

#[test]
fn seat_changes_and_the_pause_reach_every_connected_client_once() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    host.poll(secs(0));
    assert!(
        sent_lines(&mut host).is_empty(),
        "nothing out of the ordinary"
    );
    host.receive(A, &ClientMessage::Heartbeat.encode(), QUIET_AFTER);
    host.poll(QUIET_AFTER);
    let quiet = HostMessage::Seats {
        seats: vec![SeatNotice {
            seat: B,
            state: SeatState::Quiet,
        }],
        paused: false,
    }
    .encode();
    assert_eq!(sent_lines(&mut host), vec![(A, quiet.clone()), (B, quiet)]);
    host.receive(A, &ClientMessage::Heartbeat.encode(), QUIET_AFTER);
    host.poll(QUIET_AFTER);
    assert!(
        sent_lines(&mut host).is_empty(),
        "unchanged states go unsaid"
    );
    host.receive(B, &ClientMessage::Heartbeat.encode(), QUIET_AFTER);
    host.set_paused(true);
    host.poll(QUIET_AFTER);
    let paused = HostMessage::Seats {
        seats: Vec::new(),
        paused: true,
    }
    .encode();
    assert_eq!(
        sent_lines(&mut host),
        vec![(A, paused.clone()), (B, paused)]
    );
}

#[test]
fn a_leaving_client_surrenders_without_holding_the_match() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    host.receive(A, &command_line(10), secs(0));
    host.receive(A, &ClientMessage::Leave.encode(), secs(0));
    assert_eq!(
        host.poll(secs(0)),
        [HostEvent::Released {
            seat: A,
            exit: Exit::Left
        }]
    );
    assert!(!host.waiting());
    host.take_outgoing();
    let batch = step(&mut host, secs(0), 0).unwrap();
    assert_eq!(
        batch,
        vec![from(A, stop(10)), from(A, Command::Surrender)],
        "its last orders and its surrender seal behind everyone else"
    );
    assert!(host.take_outgoing().iter().all(|(seat, _)| *seat == B));
}

#[test]
fn seats_still_in_play_share_first_place_once_one_leaves() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    host.receive(A, &ClientMessage::Leave.encode(), secs(0));
    step(&mut host, secs(0), 0).unwrap();
    let firsts: Vec<PlayerId> = (1..5)
        .map(|_| {
            host.submit(stop(0));
            host.receive(B, &command_line(20), secs(0));
            step(&mut host, secs(0), 0).unwrap()[0].player
        })
        .collect();
    assert_eq!(
        firsts,
        [B, HOST, B, HOST],
        "turns alternate between the two"
    );
}

#[test]
fn a_quiet_client_catching_up_reports_hashes_the_host_kept() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    host.poll(QUIET_AFTER);
    let mut tick = 0;
    while step(&mut host, QUIET_AFTER, 7).is_some() {
        tick += 1;
        host.receive(B, &ack_line(tick, 7), QUIET_AFTER);
    }
    assert!(tick > 2 * HASH_INTERVAL, "play ran on without A");
    for report in 1..=tick {
        host.receive(A, &ack_line(report, 7), QUIET_AFTER);
    }
    assert!(host.poll(QUIET_AFTER).is_empty(), "no desync, no lost link");
}

#[test]
fn a_defeated_client_watches_without_holding_the_match() {
    let mut host = HostSession::new(HOST, &[A], secs(0));
    host.retire_seat(A);
    host.receive(A, &command_line(10), secs(0));
    for _ in 0..(2 * LEAD_CAP) {
        assert_eq!(step(&mut host, secs(0), 0), Some(Vec::new()));
    }
    host.poll(SILENCE_TIMEOUT);
    assert!(!host.waiting());
    assert!(
        host.take_outgoing().iter().any(|(seat, _)| *seat == A),
        "it still receives batches"
    );
}

#[test]
fn a_concluded_match_waits_on_nobody() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    step(&mut host, secs(0), 0).unwrap();
    ack_through(&mut host, B, 1, 1, secs(0));
    host.disconnected(A, secs(0));
    host.poll(secs(0));
    assert!(host.waiting());
    host.conclude();
    assert!(!host.waiting());
    assert!(host.caught_up());
    assert!(host.seats(secs(1)).is_empty());
}

#[test]
fn an_abandoning_host_tells_every_connected_client() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    host.disconnected(B, secs(0));
    host.abandon();
    assert_eq!(host.take_outgoing(), [(A, HostMessage::Ended.encode())]);
}

#[test]
fn time_the_host_spent_suspended_is_nobodys_silence() {
    let mut host = HostSession::new(HOST, &[A], secs(0));
    host.poll(secs(0));
    host.poll(secs(30));
    assert!(
        host.seats(secs(30)).is_empty(),
        "A is neither quiet nor away"
    );
    host.poll(secs(31));
    assert_eq!(
        host.seats(secs(31)),
        [SeatNotice {
            seat: A,
            state: SeatState::Quiet
        }],
        "silence after the gap still counts"
    );

    // A line that arrives in the pump that sees the gap resets the clock to
    // now, never later.
    let mut host = HostSession::new(HOST, &[A], secs(0));
    host.poll(secs(0));
    host.receive(A, &ClientMessage::Heartbeat.encode(), secs(3600));
    host.poll(secs(3600));
    for second in 3601..=3600 + SILENCE_TIMEOUT.as_secs() {
        host.poll(secs(second));
    }
    assert!(
        host.waiting(),
        "silence after the line counts from when it was heard"
    );
}

#[test]
fn a_replaced_seat_drops_its_orders_and_every_client_hears_who_took_it() {
    use oxide_sim::scenario::{BotConfig, BotDifficulty, BotStance};
    let config = BotConfig::new(BotDifficulty::Prime, BotStance::Turtle, 7);
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    host.receive(A, &command_line(10), secs(0));
    host.disconnected(A, secs(0));
    host.poll(secs(0));
    assert!(host.waiting());
    host.take_outgoing();
    host.replace_seat(A, config);
    assert!(!host.waiting());
    assert_eq!(
        sent_lines(&mut host),
        [(B, HostMessage::Replaced { seat: A, config }.encode())]
    );
    assert_eq!(
        step(&mut host, secs(0), 0),
        Some(Vec::new()),
        "no surrender and no stale order"
    );

    let mut host = HostSession::new(HOST, &[A], secs(0));
    host.replace_seat(A, config);
    assert_eq!(
        host.take_outgoing(),
        [(
            A,
            HostMessage::Released {
                reason: Release::Replaced
            }
            .encode()
        )],
        "a connected client hears why before it closes"
    );
    assert_eq!(
        host.poll(secs(0)),
        [HostEvent::Released {
            seat: A,
            exit: Exit::Replaced
        }]
    );
}

#[test]
fn a_resumed_session_seals_the_saved_orders_first_from_the_saved_tick() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0))
        .resumed(500, vec![from(A, stop(1)), from(HOST, stop(2))]);
    host.submit(stop(3));
    assert_eq!(host.tick(), 500);
    let batch = step(&mut host, secs(0), 7).unwrap();
    let mut sealed = batch.clone();
    sealed.sort_by_key(|order| order.player);
    assert_eq!(
        sealed,
        vec![from(HOST, stop(2)), from(HOST, stop(3)), from(A, stop(1))]
    );
    let lines = sent_lines(&mut host);
    assert!(lines.iter().any(|(seat, line)| *seat == A
        && matches!(
            HostMessage::decode(line),
            Ok(HostMessage::Batch { tick: 500, .. })
        )));
    // The clients start acknowledged through the saved tick.
    host.receive(A, &ack_line(501, 7), secs(0));
    host.receive(B, &ack_line(501, 7), secs(0));
    assert_eq!(host.poll(secs(0)), Vec::new());
    assert!(host.caught_up());
}

#[test]
#[should_panic(expected = "a pending order must come from a human seat")]
fn a_resumed_session_refuses_an_order_from_a_bot_seat() {
    let _ = HostSession::new(HOST, &[A], secs(0)).resumed(5, vec![from(B, stop(1))]);
}

#[test]
fn a_rejoin_purges_lines_queued_for_the_old_connection() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    let batch = host.seal(secs(0)).unwrap();
    host.publish(&batch);
    assert!(host.rejoining(A, secs(0)));
    let lines = host.take_outgoing();
    assert!(lines.iter().any(|(seat, _)| *seat == B));
    assert!(lines.iter().all(|(seat, _)| *seat != A));
}

#[test]
fn a_concluded_match_readmits_nobody() {
    let mut host = HostSession::new(HOST, &[A], secs(0));
    assert!(host.rejoining(A, secs(0)));
    host.conclude();
    assert!(!host.readmit(A, secs(0)));
}

#[test]
fn everyone_hears_a_readmitted_seat_is_back() {
    let mut host = HostSession::new(HOST, &[A, B], secs(0));
    host.disconnected(A, secs(0));
    host.poll(secs(0));
    assert!(
        sent_lines(&mut host)
            .iter()
            .any(|(seat, line)| *seat == B && line.contains("away"))
    );
    assert!(host.rejoining(A, secs(0)));
    assert!(host.readmit(A, secs(0)));
    host.poll(secs(0));
    let cleared = HostMessage::Seats {
        seats: Vec::new(),
        paused: false,
    }
    .encode();
    let lines = sent_lines(&mut host);
    assert!(lines.contains(&(B, cleared.clone())), "{lines:?}");
    assert!(lines.contains(&(A, cleared)), "{lines:?}");
}
