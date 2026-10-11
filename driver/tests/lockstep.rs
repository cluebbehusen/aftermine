//! In-process lockstep: a host and two clients exchange `oxide-net` lines
//! through delayed links on a virtual clock. Every seat's "player" is a
//! bot controller running on that seat's own machine, so real orders cross
//! the wire. Every machine must execute identical batches.

use chassis::rng::Pcg32;
use oxide_kit::controller::{SeatController, record_events, seat_controllers};
use oxide_kit::{GameReplay, bot_execution, runner};
use oxide_net::{
    ClientEnd, ClientSession, Connection, Desk, DeskEvent, Guest, GuestEvent, HostEvent,
    HostSession, InstallId, JoinMessage, Listener, LobbyMessage, PROGRESS_TIMEOUT, PlayerName,
    RoomSeat, SILENCE_TIMEOUT, SeatHolder, SeatNotice, SeatState, StartBarrier, StartContent,
    Token, Visit, VisitEvent, reports_hash, same_build,
};
use oxide_sim::scenario::{BotConfig, BotDifficulty, BotStance};
use oxide_sim::{Command, PlayerCommand, PlayerId, SIM_VERSION, Scenario, State, Tick, TickReport};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const STEP: Duration = Duration::from_millis(10);
const TICK: Duration = Duration::from_millis(50);
const HOST: PlayerId = PlayerId(0);
const CLIENTS: [PlayerId; 2] = [PlayerId(1), PlayerId(2)];

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

fn player_config() -> BotConfig {
    BotConfig::new(BotDifficulty::Standard, BotStance::Balanced, 0)
}

/// Twin Forges with seats 0 to 2 played by people and seat 3 a host bot.
fn scenario() -> Scenario {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scenarios/twin-forges.json");
    let mut scenario = Scenario::load(&path).unwrap();
    for (seat, player) in scenario.players.iter_mut().enumerate() {
        player.bot = seat == 3;
        player.bot_config = Some(player_config());
    }
    scenario
}

/// One direction of a connection: in-order delivery after a jittered delay.
struct Link {
    latency: Duration,
    jitter: u32,
    rng: Pcg32,
    queue: VecDeque<(Duration, String)>,
    last: Duration,
    closed: bool,
}

impl Link {
    fn new(latency: Duration, jitter_ms: u32, stream: u64) -> Self {
        assert!(latency >= ms(u64::from(jitter_ms)));
        Self {
            latency,
            jitter: jitter_ms,
            rng: Pcg32::new(57, stream),
            queue: VecDeque::new(),
            last: Duration::ZERO,
            closed: false,
        }
    }

    fn send(&mut self, now: Duration, line: String) {
        if self.closed {
            return;
        }
        let spread = u64::from(self.rng.next_below(2 * self.jitter + 1));
        let at = (now + self.latency + ms(spread))
            .checked_sub(ms(u64::from(self.jitter)))
            .unwrap()
            .max(self.last);
        self.last = at;
        self.queue.push_back((at, line));
    }

    fn deliver(&mut self, now: Duration) -> Vec<String> {
        let mut lines = Vec::new();
        while self.queue.front().is_some_and(|(at, _)| *at <= now) {
            lines.push(self.queue.pop_front().unwrap().1);
        }
        lines
    }

    fn close(&mut self) {
        self.closed = true;
        self.queue.clear();
    }
}

/// A machine's world, its record of executed batches, and its seat's player.
struct Machine {
    seat: PlayerId,
    state: State,
    replay: GameReplay,
    player: SeatController,
    acted: Option<Tick>,
    hashes: Vec<(Tick, u64)>,
}

impl Machine {
    fn new(scenario: &Scenario, seat: PlayerId) -> Self {
        // The seat's player, as if it alone were a bot.
        let mut alone = scenario.clone();
        for (index, player) in alone.players.iter_mut().enumerate() {
            player.bot = index == usize::from(seat.0);
            player.bot_config = player.bot.then(player_config);
        }
        Self {
            seat,
            state: scenario.build().unwrap(),
            replay: GameReplay::new(SIM_VERSION, "test", scenario.clone()),
            player: seat_controllers(&alone).unwrap().remove(0),
            acted: None,
            hashes: Vec::new(),
        }
    }

    /// The player's orders from this machine's current world, once per tick.
    fn orders(&mut self) -> Vec<Command> {
        let tick = self.state.current_tick();
        if self.acted.replace(tick) == Some(tick) {
            return Vec::new();
        }
        self.player
            .act(&self.state)
            .into_iter()
            .map(|order| {
                assert_eq!(order.player, self.seat);
                order.command
            })
            .collect()
    }

    /// Executes `batch`, returning its report for any other bots this
    /// machine hosts.
    fn execute(&mut self, batch: &[PlayerCommand]) -> TickReport {
        let report = runner::record_and_tick(&mut self.state, batch, Some(&mut self.replay));
        record_events(std::slice::from_mut(&mut self.player), &report);
        let tick = self.state.current_tick();
        if reports_hash(tick) {
            self.hashes.push((tick, self.state.hash()));
        }
        report
    }

    fn commands(&self) -> Vec<(Tick, PlayerCommand)> {
        self.replay
            .commands
            .iter()
            .map(|timed| (timed.tick, timed.command.clone()))
            .collect()
    }
}

struct Host {
    session: HostSession,
    machine: Machine,
    bots: Vec<SeatController>,
    next_due: Duration,
    frozen: bool,
    blocked: usize,
    events: Vec<(Duration, HostEvent)>,
}

struct Client {
    session: ClientSession,
    machine: Machine,
    up: Link,
    down: Link,
    /// Receives and heartbeats but executes nothing, like a client that
    /// cannot keep up.
    stuck: Option<(Duration, Duration)>,
    /// Does nothing at all, like a suspended app.
    silent: Option<(Duration, Duration)>,
    doctor_at: Option<Tick>,
    end: Option<(Duration, ClientEnd)>,
}

impl Client {
    fn stuck(&self, now: Duration) -> bool {
        self.stuck
            .is_some_and(|(from, until)| from <= now && now < until)
    }

    fn silent(&self, now: Duration) -> bool {
        self.silent
            .is_some_and(|(from, until)| from <= now && now < until)
    }

    fn step(&mut self, now: Duration) {
        if self.end.is_some() || self.down.closed {
            return;
        }
        for line in self.down.deliver(now) {
            if let Err(end) = self.session.receive(&line, now) {
                self.end = Some((now, end));
                return;
            }
        }
        if !self.stuck(now) {
            while let Some(mut batch) = self.session.next_batch() {
                for order in self.machine.orders() {
                    self.session.send(order);
                }
                if self.doctor_at == Some(self.machine.state.current_tick()) {
                    batch.push(PlayerCommand {
                        player: self.machine.seat,
                        command: Command::Surrender,
                    });
                }
                self.machine.execute(&batch);
                let state = &self.machine.state;
                self.session.executed(|| state.hash());
            }
        }
        if let Err(end) = self.session.poll(now) {
            self.end = Some((now, end));
            return;
        }
        for line in self.session.take_outgoing() {
            self.up.send(now, line);
        }
    }
}

struct Net {
    now: Duration,
    host: Host,
    clients: Vec<Client>,
}

impl Net {
    /// Clients 1 and 2 with the given one-way latency and jitter per link.
    fn new(links: [(u64, u32); 2]) -> Self {
        let scenario = scenario();
        let host = Host {
            session: HostSession::new(HOST, &CLIENTS, Duration::ZERO).with_tokens(
                CLIENTS
                    .iter()
                    .map(|&seat| (seat, Token::new(u128::from(seat.0))))
                    .collect(),
            ),
            machine: Machine::new(&scenario, HOST),
            bots: seat_controllers(&scenario).unwrap(),
            next_due: Duration::ZERO,
            frozen: false,
            blocked: 0,
            events: Vec::new(),
        };
        assert_eq!(host.bots.len(), 1, "only seat 3 is a host bot");
        let clients = CLIENTS
            .iter()
            .zip(links)
            .zip(0u64..)
            .map(|((&seat, (latency, jitter)), stream)| Client {
                session: ClientSession::new(Duration::ZERO),
                machine: Machine::new(&scenario, seat),
                up: Link::new(ms(latency), jitter, 2 * stream),
                down: Link::new(ms(latency), jitter, 2 * stream + 1),
                stuck: None,
                silent: None,
                doctor_at: None,
                end: None,
            })
            .collect();
        Self {
            now: Duration::ZERO,
            host,
            clients,
        }
    }

    fn client(&mut self, seat: PlayerId) -> &mut Client {
        &mut self.clients[usize::from(seat.0) - 1]
    }

    fn run_until(&mut self, until: Duration) {
        while self.now < until {
            self.step();
            self.now += STEP;
        }
    }

    /// Runs until the host has executed `ticks` ticks.
    fn run_ticks(&mut self, ticks: Tick) {
        while self.host.machine.state.current_tick() < ticks {
            self.step();
            self.now += STEP;
            assert!(
                self.now < Duration::from_secs(120),
                "the host never got there"
            );
        }
    }

    /// Stops sealing and lets every live client execute what is in flight.
    fn drain(&mut self) {
        self.host.session.set_paused(true);
        self.run_until(self.now + Duration::from_secs(2));
    }

    fn step(&mut self) {
        let now = self.now;
        if !self.host.frozen {
            self.host_step(now);
        }
        for client in &mut self.clients {
            if !client.silent(now) {
                client.step(now);
            }
        }
    }

    fn host_step(&mut self, now: Duration) {
        let host = &mut self.host;
        for (client, &seat) in self.clients.iter_mut().zip(&CLIENTS) {
            for line in client.up.deliver(now) {
                host.session.receive(seat, &line, now);
            }
        }
        for event in host.session.poll(now) {
            if let HostEvent::Released { seat, .. } | HostEvent::Lost { seat } = event {
                let client = &mut self.clients[usize::from(seat.0) - 1];
                client.up.close();
                client.down.close();
            }
            host.events.push((now, event));
        }
        if now >= host.next_due {
            for order in host.machine.orders() {
                host.session.submit(order);
            }
            match host.session.seal(now) {
                Some(mut batch) => {
                    batch.extend(bot_execution::commands(&host.machine.state, &mut host.bots));
                    host.session.publish(&batch);
                    let report = host.machine.execute(&batch);
                    record_events(&mut host.bots, &report);
                    let state = &host.machine.state;
                    host.session.executed(|| state.hash());
                    host.next_due = now + TICK;
                }
                None => host.blocked += 1,
            }
        }
        for (seat, line) in host.session.take_outgoing() {
            self.clients[usize::from(seat.0) - 1].down.send(now, line);
        }
    }

    /// The host and the named clients executed identical batches and agree
    /// on every report-tick hash.
    fn assert_in_sync(&self, seats: &[PlayerId]) {
        let host = &self.host.machine;
        for &seat in seats {
            let client = &self.clients[usize::from(seat.0) - 1].machine;
            assert_eq!(
                client.state.current_tick(),
                host.state.current_tick(),
                "{seat:?}"
            );
            assert_eq!(client.commands(), host.commands(), "{seat:?}");
            assert_eq!(client.hashes, host.hashes, "{seat:?}");
            assert_eq!(client.state.hash(), host.state.hash(), "{seat:?}");
        }
    }

    fn surrender_tick(&self, seat: PlayerId) -> Option<Tick> {
        self.host
            .machine
            .commands()
            .into_iter()
            .find(|(_, order)| order.player == seat && order.command == Command::Surrender)
            .map(|(tick, _)| tick)
    }
}

#[test]
fn healthy_links_never_stall_and_every_machine_agrees() {
    let mut net = Net::new([(40, 20), (90, 20)]);
    net.run_ticks(400);
    assert_eq!(net.host.blocked, 0, "the lead cap absorbed normal latency");
    net.drain();
    assert!(net.host.events.is_empty(), "{:?}", net.host.events);
    assert!(net.clients.iter().all(|client| client.end.is_none()));
    assert!(
        net.host
            .machine
            .commands()
            .iter()
            .any(|(_, order)| CLIENTS.contains(&order.player)),
        "client orders crossed the wire"
    );
    net.assert_in_sync(&CLIENTS);
}

#[test]
fn a_short_client_stall_makes_the_host_wait_without_dropping_anyone() {
    let mut net = Net::new([(40, 20), (40, 20)]);
    net.client(CLIENTS[0]).stuck = Some((Duration::from_secs(2), Duration::from_secs(6)));
    net.run_ticks(300);
    assert!(net.host.blocked > 0, "the host waited at the lead cap");
    net.drain();
    assert!(net.host.events.is_empty(), "{:?}", net.host.events);
    net.assert_in_sync(&CLIENTS);
}

#[test]
fn a_stuck_client_with_a_live_network_makes_the_match_wait_until_the_host_drops_it() {
    let mut net = Net::new([(40, 20), (40, 20)]);
    let stuck = CLIENTS[1];
    net.client(stuck).stuck = Some((Duration::from_secs(2), Duration::MAX));
    net.run_until(Duration::from_secs(20));
    assert!(net.host.events.is_empty(), "{:?}", net.host.events);
    assert!(net.host.session.waiting());
    let [
        SeatNotice {
            seat,
            state: SeatState::Away { waited_ms },
        },
    ] = net.host.session.seats(net.now)[..]
    else {
        panic!("the stuck seat is away");
    };
    assert_eq!(seat, stuck);
    let waited_from = net.now.checked_sub(ms(waited_ms)).unwrap();
    assert!(waited_from + ms(500) >= Duration::from_secs(4) + PROGRESS_TIMEOUT);
    assert!(
        net.clients[0].end.is_none(),
        "the waiting host kept heartbeating"
    );
    let stuck_tick = net.clients[1].machine.state.current_tick();
    assert_eq!(
        net.host.machine.state.current_tick(),
        stuck_tick + oxide_net::LEAD_CAP,
        "the match stopped at the lead cap"
    );

    net.host.session.drop_seat(stuck);
    net.run_until(net.now + Duration::from_secs(2));
    assert_eq!(
        net.surrender_tick(stuck),
        Some(stuck_tick + oxide_net::LEAD_CAP),
        "the first tick after the wait carries the Surrender"
    );
    assert!(!net.host.machine.state.accepts_commands(stuck));
    net.drain();
    net.assert_in_sync(&CLIENTS[..1]);
    let stuck_commands = net.clients[1].machine.commands();
    assert!(net.host.machine.commands().starts_with(&stuck_commands));
}

#[test]
fn a_briefly_silent_client_lets_play_run_on_and_catches_up() {
    let mut net = Net::new([(40, 20), (40, 20)]);
    let quiet = CLIENTS[1];
    net.client(quiet).silent = Some((Duration::from_secs(2), Duration::from_secs(7)));
    net.run_until(Duration::from_secs(2));
    let before = net.host.machine.state.current_tick();
    net.run_until(Duration::from_secs(7));
    assert!(
        net.host.machine.state.current_tick() > before + oxide_net::LEAD_CAP,
        "play ran past the usual lead"
    );
    assert!(!net.host.session.waiting());
    net.run_until(Duration::from_secs(9));
    assert!(net.host.events.is_empty(), "{:?}", net.host.events);
    assert!(net.host.session.seats(net.now).is_empty(), "back to normal");
    net.drain();
    net.assert_in_sync(&CLIENTS);
}

#[test]
fn clients_end_the_session_when_the_host_falls_silent() {
    let mut net = Net::new([(40, 20), (90, 20)]);
    net.run_until(Duration::from_secs(2));
    net.host.frozen = true;
    let frozen_at = net.now;
    net.run_until(frozen_at + SILENCE_TIMEOUT + Duration::from_secs(1));
    for client in &net.clients {
        let (ended_at, end) = client.end.expect("the client noticed");
        assert_eq!(end, ClientEnd::HostSilent);
        assert!(ended_at >= frozen_at + SILENCE_TIMEOUT, "ended early");
        assert!(
            ended_at <= frozen_at + SILENCE_TIMEOUT + ms(200),
            "ended late"
        );
    }
}

#[test]
fn a_closed_connection_makes_the_match_wait_until_the_host_drops_it() {
    let mut net = Net::new([(40, 20), (40, 20)]);
    let gone = CLIENTS[1];
    net.run_until(Duration::from_secs(3));
    net.client(gone).up.close();
    net.client(gone).down.close();
    net.host.session.disconnected(gone, net.now);
    net.run_until(Duration::from_secs(4));
    let held = net.host.machine.state.current_tick();
    net.run_until(Duration::from_secs(8));
    assert!(net.host.events.is_empty(), "{:?}", net.host.events);
    assert!(net.host.session.waiting());
    assert_eq!(
        net.host.machine.state.current_tick(),
        held,
        "the match waits"
    );
    assert!(net.surrender_tick(gone).is_none());

    net.host.session.drop_seat(gone);
    net.run_until(Duration::from_secs(9));
    assert!(net.surrender_tick(gone).is_some());
    assert!(!net.host.machine.state.accepts_commands(gone));
    net.drain();
    net.assert_in_sync(&CLIENTS[..1]);
}

#[test]
fn a_seat_handed_to_a_bot_plays_on_in_sync() {
    let mut net = Net::new([(40, 20), (40, 20)]);
    let gone = CLIENTS[1];
    net.run_until(Duration::from_secs(3));
    net.client(gone).up.close();
    net.client(gone).down.close();
    net.host.session.disconnected(gone, net.now);
    net.run_until(Duration::from_secs(4));
    assert!(net.host.session.waiting());
    let held = net.host.machine.state.current_tick();

    let config = BotConfig::new(BotDifficulty::Standard, BotStance::Balanced, 11);
    net.host.session.replace_seat(gone, config);
    let setup = scenario();
    let opponent_map = oxide_kit::controller::OpponentMap::new(&setup);
    let controller = SeatController::configured(gone, config, &opponent_map).unwrap();
    let at = net.host.bots.partition_point(|bot| bot.player() < gone);
    net.host.bots.insert(at, controller);
    // Early in the match a fresh bot has little to order; give it time.
    net.run_until(Duration::from_secs(40));
    assert!(!net.host.session.waiting());
    assert!(
        net.host
            .machine
            .commands()
            .iter()
            .any(|(tick, order)| order.player == gone && *tick > held),
        "the bot plays the seat"
    );
    assert!(net.surrender_tick(gone).is_none());
    net.drain();
    net.assert_in_sync(&CLIENTS[..1]);
}

#[test]
fn a_lost_client_rejoins_from_a_snapshot_and_stays_in_sync() {
    let mut net = Net::new([(40, 20), (40, 20)]);
    let gone = CLIENTS[1];
    net.run_until(Duration::from_secs(3));
    net.client(gone).up.close();
    net.client(gone).down.close();
    net.host.session.disconnected(gone, net.now);
    net.run_until(Duration::from_secs(4));
    assert!(net.host.session.waiting());

    // The handshake, in memory: the snapshot is the host's world as JSON.
    let now = net.now;
    let mut desk = Desk::new("test", PlayerName::new("Host").unwrap(), "Twin Forges");
    let visitor = desk.connected(now);
    let mut visit = Visit::rejoin("test", Token::new(u128::from(gone.0)), now);
    let mut restored = None;
    let mut admitted = false;
    while !admitted {
        for line in visit.take_outgoing() {
            desk.receive(&mut net.host.session, visitor, &line, now);
        }
        for event in desk.poll(&mut net.host.session, now) {
            match event {
                DeskEvent::Snapshot { .. } => {
                    let state = &net.host.machine.state;
                    let bytes = serde_json::to_vec(state).unwrap();
                    let tick = net.host.session.tick();
                    desk.send_snapshot(
                        &mut net.host.session,
                        visitor,
                        tick,
                        state.hash(),
                        &bytes,
                        now,
                    );
                }
                DeskEvent::Admitted { seat, .. } => {
                    assert_eq!(seat, gone);
                    admitted = true;
                }
                DeskEvent::Done { .. } => panic!("the rejoin was turned away"),
            }
        }
        for (_, line) in desk.take_outgoing() {
            match visit.receive(&line, now).unwrap() {
                Some(VisitEvent::Snapshot { bytes, tick, .. }) => {
                    let state: State = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(state.current_tick(), tick);
                    visit.ready(state.hash());
                    restored = Some(state);
                }
                Some(VisitEvent::Go | VisitEvent::Waiting { .. }) | None => {}
            }
        }
    }
    let state = restored.expect("the snapshot arrived");
    let tick = state.current_tick();
    let client = net.client(gone);
    client.machine.state = state;
    client.session = ClientSession::at(tick, now);
    client.up = Link::new(ms(40), 20, 8);
    client.down = Link::new(ms(40), 20, 9);
    client.end = None;
    let rejoined_hashes = client.machine.hashes.len();

    net.run_until(Duration::from_secs(10));
    assert!(!net.host.session.waiting());
    assert!(net.host.events.is_empty(), "{:?}", net.host.events);
    net.drain();
    net.assert_in_sync(&CLIENTS[..1]);
    let host = &net.host.machine;
    let client = &net.clients[1].machine;
    assert_eq!(client.state.current_tick(), host.state.current_tick());
    assert_eq!(client.state.hash(), host.state.hash());
    let reported = &client.hashes[rejoined_hashes..];
    assert!(!reported.is_empty(), "the rejoined client kept reporting");
    assert!(reported.iter().all(|report| host.hashes.contains(report)));
}

/// A guest already in the room on `seat` of `scenario`, its greeting sent.
fn seated_guest(scenario: &Scenario, seat: PlayerId, now: Duration) -> Guest {
    let name = |seat: u8| PlayerName::new(&format!("Player {seat}")).unwrap();
    let mut guest = Guest::new(
        "test",
        name(seat.0),
        InstallId::new(u128::from(seat.0)),
        now,
    );
    let seats = scenario
        .players
        .iter()
        .zip(0..)
        .map(|(player, index)| RoomSeat {
            holder: if player.bot {
                SeatHolder::Bot {
                    difficulty: BotDifficulty::Standard,
                    stance: BotStance::Balanced,
                }
            } else if PlayerId(index) == HOST {
                SeatHolder::Host { name: name(index) }
            } else {
                SeatHolder::Player { name: name(index) }
            },
            team: None,
        })
        .collect();
    for message in [
        LobbyMessage::hello("test"),
        LobbyMessage::Map {
            scenario: Box::new(scenario.clone()),
        },
        LobbyMessage::Room { seats, seat },
    ] {
        guest.receive(&message.encode(), now).unwrap();
    }
    guest.take_outgoing();
    guest
}

#[test]
fn a_resumed_match_starts_from_its_snapshot_and_stays_in_sync() {
    let mut net = Net::new([(40, 20), (40, 20)]);
    net.run_until(Duration::from_secs(3));
    net.drain();
    net.assert_in_sync(&CLIENTS);

    // The saved match: the host's world as JSON, and an order it had not
    // sealed yet.
    let now = net.now;
    let scenario = scenario();
    let state = net.host.machine.state.clone();
    let tick = state.current_tick();
    let snapshot = serde_json::to_vec(&state).unwrap();
    let pending = [PlayerCommand {
        player: CLIENTS[0],
        command: Command::Stop { units: Vec::new() },
    }];
    let tokens: Vec<(PlayerId, Token)> = CLIENTS
        .iter()
        .map(|&seat| (seat, Token::new(0x50 + u128::from(seat.0))))
        .collect();
    let content = StartContent::Resume {
        seats: &[HOST, CLIENTS[0], CLIENTS[1]],
        tick,
        snapshot: &snapshot,
        pending: &pending,
    };
    let mut barrier = StartBarrier::new(content, HOST, &tokens, state.hash(), 1, now);
    let mut guests: Vec<Guest> = CLIENTS
        .iter()
        .map(|&seat| seated_guest(&scenario, seat, now))
        .collect();
    for (seat, line) in barrier.take_outgoing() {
        let guest = &mut guests[usize::from(seat.0) - 1];
        if let Some(GuestEvent::Resume {
            seat: restored_seat,
            tick: at,
            bytes,
        }) = guest.receive(&line, now).unwrap()
        {
            assert_eq!((restored_seat, at), (seat, tick));
            let restored: State = serde_json::from_slice(&bytes).unwrap();
            guest.ready(restored.hash());
            net.client(seat).machine.state = restored;
        }
    }
    for (guest, &seat) in guests.iter_mut().zip(&CLIENTS) {
        for line in guest.take_outgoing() {
            barrier.receive(seat, &line).unwrap();
        }
    }
    net.host.session = barrier.poll(now).unwrap().expect("every client is ready");
    for (seat, line) in barrier.take_outgoing() {
        let go = guests[usize::from(seat.0) - 1].receive(&line, now);
        assert!(matches!(go, Ok(Some(GuestEvent::Go { .. }))), "{go:?}");
    }
    for (stream, &seat) in (10_u64..).step_by(2).zip(&CLIENTS) {
        let client = net.client(seat);
        client.session = ClientSession::at(tick, now);
        client.up = Link::new(ms(40), 20, stream);
        client.down = Link::new(ms(40), 20, stream + 1);
    }

    net.run_until(now + Duration::from_secs(5));
    net.drain();
    assert!(net.host.events.is_empty(), "{:?}", net.host.events);
    assert!(net.host.machine.state.current_tick() > tick + 40);
    net.assert_in_sync(&CLIENTS);
    assert!(
        net.host
            .machine
            .commands()
            .contains(&(tick, pending[0].clone())),
        "the unsealed order seals on the saved tick"
    );
}

#[test]
fn a_diverged_client_halts_every_machine_at_the_next_report() {
    let mut net = Net::new([(40, 20), (40, 20)]);
    let report = 2 * oxide_net::HASH_INTERVAL;
    net.client(CLIENTS[0]).doctor_at = Some(report - 5);
    net.run_until(Duration::from_secs(6));
    assert_eq!(
        net.host
            .events
            .iter()
            .map(|(_, event)| *event)
            .collect::<Vec<_>>(),
        vec![HostEvent::Desync {
            seat: CLIENTS[0],
            tick: report
        }]
    );
    for client in &net.clients {
        assert_eq!(
            client.end.map(|(_, end)| end),
            Some(ClientEnd::Desync { tick: report })
        );
    }
    let halted = net.host.machine.state.current_tick();
    net.run_until(net.now + Duration::from_secs(1));
    assert_eq!(
        net.host.machine.state.current_tick(),
        halted,
        "the host stopped sealing"
    );
}

/// Polls until `poll` yields, failing after ten seconds.
fn wait<T>(mut poll: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(value) = poll() {
            return value;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        thread::sleep(ms(1));
    }
}

fn recv(connection: &Connection) -> String {
    wait(|| connection.try_recv().expect("the peer stays connected"))
}

/// Sends host lines to seats, which the host seated in `CLIENTS` order.
fn send_to_seats(seated: &[Connection], lines: Vec<(PlayerId, String)>) {
    for (seat, line) in lines {
        seated[usize::from(seat.0) - 1].send(&line);
    }
}

#[test]
fn a_match_starts_and_stays_in_sync_over_tcp() {
    const COMMIT: &str = "lockstep-test";
    let scenario = scenario();
    let clock = Instant::now();
    let listener = Listener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let clients: Vec<Connection> = CLIENTS
        .iter()
        .map(|_| {
            let connection = Connection::connect(address).unwrap();
            connection.send(&JoinMessage::hello(COMMIT).encode());
            connection
        })
        .collect();

    // The host seats connections in accept order and answers each Hello.
    let seated: Vec<Connection> = CLIENTS
        .iter()
        .map(|_| {
            let connection = wait(|| listener.try_accept().unwrap());
            let JoinMessage::Hello {
                protocol,
                sim,
                commit,
            } = JoinMessage::decode(&recv(&connection)).unwrap()
            else {
                panic!("a client opens with Hello");
            };
            assert!(same_build(protocol, sim, &commit, COMMIT));
            connection.send(&LobbyMessage::hello(COMMIT).encode());
            connection
        })
        .collect();
    for connection in &clients {
        let LobbyMessage::Hello {
            protocol,
            sim,
            commit,
        } = LobbyMessage::decode(&recv(connection)).unwrap()
        else {
            panic!("the host answers with Hello");
        };
        assert!(same_build(protocol, sim, &commit, COMMIT));
    }

    let mut host = Machine::new(&scenario, HOST);
    let mut bots = seat_controllers(&scenario).unwrap();
    let tokens: Vec<(PlayerId, oxide_net::Token)> = CLIENTS
        .iter()
        .map(|&seat| (seat, oxide_net::Token::new(u128::from(seat.0))))
        .collect();
    let mut barrier = StartBarrier::new(
        StartContent::Fresh(&scenario),
        HOST,
        &tokens,
        host.state.hash(),
        1,
        clock.elapsed(),
    );
    send_to_seats(&seated, barrier.take_outgoing());
    let mut machines: Vec<Machine> = clients
        .iter()
        .map(|connection| {
            let LobbyMessage::Start {
                seat,
                scenario,
                attempt,
            } = LobbyMessage::decode(&recv(connection)).unwrap()
            else {
                panic!("the host starts the match");
            };
            let machine = Machine::new(&scenario, seat);
            let ready = JoinMessage::Ready {
                hash: machine.state.hash(),
                attempt,
            };
            connection.send(&ready.encode());
            machine
        })
        .collect();
    let mut session = wait(|| {
        for (connection, &seat) in seated.iter().zip(&CLIENTS) {
            while let Some(line) = connection.try_recv().unwrap() {
                barrier.receive(seat, &line).unwrap();
            }
        }
        barrier.poll(clock.elapsed()).unwrap()
    });
    send_to_seats(&seated, barrier.take_outgoing());
    let mut sessions: Vec<ClientSession> = clients
        .iter()
        .zip(&tokens)
        .map(|(connection, &(_, token))| {
            assert_eq!(
                LobbyMessage::decode(&recv(connection)).unwrap(),
                LobbyMessage::Go { token }
            );
            ClientSession::new(clock.elapsed())
        })
        .collect();

    let started = Instant::now();
    let mut draining = false;
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the match stalled"
        );
        let now = clock.elapsed();
        for (connection, &seat) in seated.iter().zip(&CLIENTS) {
            while let Some(line) = connection.try_recv().unwrap() {
                session.receive(seat, &line, now);
            }
        }
        assert!(session.poll(now).is_empty(), "no drops or desyncs");
        if !draining {
            for order in host.orders() {
                session.submit(order);
            }
            if let Some(mut batch) = session.seal(now) {
                batch.extend(bot_execution::commands(&host.state, &mut bots));
                session.publish(&batch);
                let report = host.execute(&batch);
                record_events(&mut bots, &report);
                let state = &host.state;
                session.executed(|| state.hash());
            }
            if host.state.current_tick() >= 80 {
                draining = true;
                session.set_paused(true);
            }
        }
        send_to_seats(&seated, session.take_outgoing());
        for ((connection, client), machine) in clients.iter().zip(&mut sessions).zip(&mut machines)
        {
            while let Some(line) = connection.try_recv().unwrap() {
                client.receive(&line, now).unwrap();
            }
            while let Some(batch) = client.next_batch() {
                for order in machine.orders() {
                    client.send(order);
                }
                machine.execute(&batch);
                let state = &machine.state;
                client.executed(|| state.hash());
            }
            client.poll(now).unwrap();
            for line in client.take_outgoing() {
                connection.send(&line);
            }
        }
        let tick = host.state.current_tick();
        if draining
            && machines
                .iter()
                .all(|machine| machine.state.current_tick() == tick)
        {
            break;
        }
        thread::sleep(ms(1));
    }
    for machine in &machines {
        assert_eq!(machine.commands(), host.commands(), "{:?}", machine.seat);
        assert_eq!(machine.hashes, host.hashes, "{:?}", machine.seat);
        assert_eq!(
            machine.state.hash(),
            host.state.hash(),
            "{:?}",
            machine.seat
        );
    }
    assert!(
        host.commands()
            .iter()
            .any(|(_, order)| CLIENTS.contains(&order.player)),
        "client orders crossed the sockets"
    );
}
