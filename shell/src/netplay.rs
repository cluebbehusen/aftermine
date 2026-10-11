//! LAN play: the lobby that gathers machines and starts a match, and the
//! link that carries a running match between them. The frame loop polls
//! both; sockets run on `oxide_net::tcp` threads.

use crate::bot_label::{BotLabelStyle, bot_label};
use crate::game::Game;
use crate::game::network::{CLIENT_BUFFER, NetRole};
use crate::seating::{Occupant, Seating};
use anyhow::{Context, Result, ensure};
use macroquad::prelude::Vec2;
use oxide_net::discovery::Beacon;
use oxide_net::{
    Advertiser, ClientEnd, ClientSession, Closed, Connection, DEFAULT_PORT, Desk, DeskEvent, Exit,
    Gathering, GoneReason, Guest, GuestEnd, GuestEvent, HostEvent, HostSession, Listener,
    LobbyEvent, MemberId, PlayerName, Release, RoomSeat, SeatHolder, SeatNotice, SeatState,
    StartFailed, Token, Visit, VisitEnd, VisitEvent, VisitorId,
};
use oxide_sim::scenario::{BotDifficulty, BotStance};
use oxide_sim::{PlayerId, Scenario, Tick};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// After this long without a new tick, the waiting player is told why.
const WAIT_NOTICE: Duration = Duration::from_millis(500);

/// How long a host keeps finished connections open so every client hears
/// the last lines: a desync halt, or the batches that decided the match.
const CLOSE_GRACE: Duration = Duration::from_secs(2);

/// Connections awaiting Hello at once. Each holds two transport threads, so
/// later ones wait unaccepted until a slot frees.
const MAX_GREETING: usize = 8;

/// Why a running match ended for this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum End {
    /// The host is gone and the match was not decided.
    HostLost,
    /// The host dropped this machine's seat, which surrendered.
    Dropped,
    /// The host left before the match was decided.
    HostLeft,
    /// The host handed this machine's seat to a bot.
    Replaced,
    /// Rejoining found the seat gone from the match.
    SeatGone,
    /// Rejoining found the match over.
    Over,
    /// A hash report disagreed and every machine halted.
    Desync {
        /// The report tick that disagreed.
        tick: Tick,
    },
}

impl End {
    /// What to tell the player.
    pub(crate) fn notice(self) -> String {
        match self {
            End::HostLost => "Lost connection to the host.".to_owned(),
            End::Dropped => "The host dropped you from the match.".to_owned(),
            End::HostLeft => "The host left the match.".to_owned(),
            End::Replaced => "The host gave your seat to a bot.".to_owned(),
            End::SeatGone => "Your seat left the match while you were away.".to_owned(),
            End::Over => "That match is over.".to_owned(),
            End::Desync { tick } => {
                format!("Desync at tick {tick}: the match halted and its replay was saved.")
            }
        }
    }
}

/// A running match's connections and session.
pub(crate) enum Link {
    /// This machine hosts.
    Host(HostLink),
    /// This machine joined a host.
    Client(ClientLink),
}

/// Who a running LAN match is waiting for or has lost track of, and
/// whether its host paused it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NetStatus {
    /// Seats the match waits for, with how long it has waited.
    pub(crate) waiting: Vec<(PlayerId, Duration)>,
    /// Seats the host has heard nothing from lately; play continues.
    pub(crate) quiet: Vec<PlayerId>,
    /// The host paused the match. Only a client is told.
    pub(crate) host_paused: bool,
}

impl NetStatus {
    fn hosted(session: &HostSession, now: Duration) -> Self {
        Self::from_notices(session.seats(now), Duration::ZERO, false)
    }

    fn joined(session: &ClientSession, now: Duration) -> Self {
        let (seats, heard) = session.seats();
        Self::from_notices(
            seats.to_vec(),
            now.saturating_sub(heard),
            session.host_paused(),
        )
    }

    fn from_notices(notices: Vec<SeatNotice>, since: Duration, host_paused: bool) -> Self {
        let mut status = Self {
            host_paused,
            ..Self::default()
        };
        for notice in notices {
            match notice.state {
                SeatState::Quiet => status.quiet.push(notice.seat),
                SeatState::Away { waited_ms } => status
                    .waiting
                    .push((notice.seat, Duration::from_millis(waited_ms) + since)),
            }
        }
        status
    }

    /// The pinned HUD line for this status, if it calls for one.
    fn notice(&self, game: &Game) -> Option<String> {
        if !self.quiet.is_empty() {
            Some(format!(
                "Lost connection to {}",
                seat_names(game, &self.quiet)
            ))
        } else if !self.waiting.is_empty() {
            let seats: Vec<PlayerId> = self.waiting.iter().map(|(seat, _)| *seat).collect();
            Some(format!("Waiting for {}", seat_names(game, &seats)))
        } else {
            self.host_paused.then(|| "Paused by the host".to_owned())
        }
    }
}

/// The seats' names as a sentence.
fn seat_names(game: &Game, seats: &[PlayerId]) -> String {
    let names: Vec<&str> = seats
        .iter()
        .map(|seat| game.scenario.players[usize::from(seat.0)].name.as_str())
        .collect();
    sentence(&names)
}

/// Names as a sentence: "Alex", "Alex and Sam", "Alex, Sam and Kim".
pub(crate) fn sentence(names: &[&str]) -> String {
    match names.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// The host's connections, one per client seat.
pub(crate) struct HostLink {
    session: HostSession,
    peers: Vec<(PlayerId, Connection)>,
    /// Takes the connections of players probing or rejoining their seats.
    listener: Option<Listener>,
    desk: Desk,
    visitors: Vec<(VisitorId, Connection)>,
    /// Finished connections of seats that left, flushing their last lines.
    departing: Vec<(Connection, Duration)>,
    /// When the host finished every connection, once the match was decided
    /// and every client had acknowledged it, or on a desync.
    closing: Option<(Duration, Option<End>)>,
    clock: Pacing,
}

/// The client's connection to its host.
pub(crate) struct ClientLink {
    session: ClientSession,
    connection: Option<Connection>,
    closed: bool,
    clock: Pacing,
    /// Where the seat lives, to take it back from.
    home: Home,
    /// Taking the seat back after losing the connection.
    rejoin: Option<Rejoin>,
    /// The match as restored by a rejoin, for the app to install.
    rejoined: Option<Box<Game>>,
}

/// The host a client's seat lives on, and the token that proves it.
#[derive(Clone)]
struct Home {
    address: String,
    commit: String,
    token: Token,
    /// The host's name, for the player.
    host: String,
}

impl Home {
    fn from_ticket(ticket: &crate::rejoin_ticket::Ticket) -> Self {
        Self {
            address: ticket.address.clone(),
            commit: ticket.build.clone(),
            token: ticket.token,
            host: ticket.host.clone(),
        }
    }
}

/// How long a client keeps trying to take its seat back.
const REJOIN_WINDOW: Duration = Duration::from_secs(30);

/// The pause between rejoin attempts.
const REJOIN_PAUSE: Duration = Duration::from_secs(1);

/// How long one rejoin attempt waits to connect.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// A client taking its seat back.
struct Rejoin {
    home: Home,
    since: Duration,
    attempt: Attempt,
}

enum Attempt {
    Connecting(JoinHandle<io::Result<Connection>>),
    /// Connected; `game` is the restored match, once the snapshot arrived.
    Calling {
        connection: Connection,
        visit: Box<Visit>,
        game: Option<Box<Game>>,
    },
    /// Waiting to try again.
    Resting(Duration),
}

/// Where a rejoin stands.
enum RejoinPoll {
    Trying,
    /// The seat is back: the restored match and the connection to play it on.
    Back(Box<Game>, Connection),
    Failed(End),
}

impl Rejoin {
    fn start(home: Home, now: Duration) -> Self {
        let attempt = Attempt::Connecting(connect(&home.address));
        Self {
            home,
            since: now,
            attempt,
        }
    }

    fn poll(&mut self, now: Duration) -> RejoinPoll {
        let expired = now.saturating_sub(self.since) >= REJOIN_WINDOW;
        let retry = Attempt::Resting(now + REJOIN_PAUSE);
        match std::mem::replace(&mut self.attempt, Attempt::Resting(now)) {
            Attempt::Connecting(handle) if !handle.is_finished() => {
                self.attempt = Attempt::Connecting(handle);
            }
            Attempt::Connecting(handle) => {
                self.attempt = match handle.join() {
                    Ok(Ok(connection)) => Attempt::Calling {
                        connection,
                        visit: Box::new(Visit::rejoin(&self.home.commit, self.home.token, now)),
                        game: None,
                    },
                    _ => retry,
                };
            }
            Attempt::Calling {
                connection,
                mut visit,
                mut game,
            } => match call(&connection, &mut visit, &mut game, now) {
                Ok(true) => {
                    if let Some(game) = game {
                        return RejoinPoll::Back(game, connection);
                    }
                    self.attempt = retry;
                }
                Ok(false) => {
                    self.attempt = Attempt::Calling {
                        connection,
                        visit,
                        game,
                    };
                }
                Err(Some(end)) => return RejoinPoll::Failed(end),
                Err(None) => self.attempt = retry,
            },
            Attempt::Resting(until) if now >= until => {
                self.attempt = Attempt::Connecting(connect(&self.home.address));
            }
            resting @ Attempt::Resting(_) => self.attempt = resting,
        }
        if expired {
            RejoinPoll::Failed(End::HostLost)
        } else {
            RejoinPoll::Trying
        }
    }
}

/// Moves one frame of a rejoin call: true once the seat is back, an end
/// when the host refused it for good, or `None` to try again.
fn call(
    connection: &Connection,
    visit: &mut Visit,
    game: &mut Option<Box<Game>>,
    now: Duration,
) -> Result<bool, Option<End>> {
    loop {
        let line = match connection.try_recv() {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(Closed) => return Err(None),
        };
        match visit.receive(&line, now) {
            Ok(Some(VisitEvent::Snapshot { seat, bytes, .. })) => {
                let restored = snapshot::decode(&bytes)
                    .and_then(|snapshot| Game::rejoined(snapshot, seat, crate::render::viewport()));
                let Ok(restored) = restored else {
                    return Err(None);
                };
                visit.ready(restored.state.hash());
                *game = Some(Box::new(restored));
            }
            Ok(Some(VisitEvent::Go)) => {
                for line in visit.take_outgoing() {
                    connection.send(&line);
                }
                return Ok(true);
            }
            Ok(Some(VisitEvent::Waiting { .. }) | None) => {}
            Err(VisitEnd::Gone(GoneReason::SeatTaken)) => return Err(Some(End::SeatGone)),
            Err(VisitEnd::Gone(GoneReason::Over | GoneReason::NoMatch)) => {
                return Err(Some(End::Over));
            }
            Err(VisitEnd::OtherBuild { .. }) => return Err(Some(End::HostLost)),
            Err(VisitEnd::Silent | VisitEnd::Protocol) => return Err(None),
        }
    }
    visit.poll(now).map_err(|_| None)?;
    for line in visit.take_outgoing() {
        connection.send(&line);
    }
    Ok(false)
}

/// Connects to `address` off the frame loop.
fn connect(address: &str) -> JoinHandle<io::Result<Connection>> {
    let address = address.to_owned();
    thread::spawn(move || {
        let target = address
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::other("the address names no host"))?;
        Connection::connect_timeout(&target, CONNECT_TIMEOUT)
    })
}

/// Frame timing and the last time the match advanced.
struct Pacing {
    last: Duration,
    tick: Tick,
    moved: Duration,
}

impl Pacing {
    fn new(now: Duration) -> Self {
        Self {
            last: now,
            tick: 0,
            moved: now,
        }
    }

    /// Seconds since the previous frame.
    fn step(&mut self, now: Duration) -> f32 {
        let dt = now.saturating_sub(self.last).as_secs_f32();
        self.last = now;
        dt
    }

    /// Whether the match has not advanced for a while.
    fn stalled(&mut self, game: &Game, now: Duration) -> bool {
        let tick = game.state.current_tick();
        if tick != self.tick {
            self.tick = tick;
            self.moved = now;
        }
        game.state.result().is_none()
            && !game.clock.paused
            && now.saturating_sub(self.moved) >= WAIT_NOTICE
    }
}

impl Link {
    pub(crate) fn host(started: Started, game: &Game, now: Duration) -> Self {
        let host = game.presentation.human;
        let name = shown_host_name(None, &game.scenario, host);
        Self::Host(HostLink {
            session: started.session,
            peers: started.peers,
            listener: started.listener,
            desk: Desk::new(&started.commit, name, &game.scenario.name),
            visitors: Vec::new(),
            departing: Vec::new(),
            closing: None,
            clock: Pacing::new(now),
        })
    }

    fn client(session: ClientSession, connection: Connection, home: Home, now: Duration) -> Self {
        Self::Client(ClientLink {
            session,
            connection: Some(connection),
            closed: false,
            clock: Pacing::new(now),
            home,
            rejoin: None,
            rejoined: None,
        })
    }

    /// The ticket for the seat this client plays, so a relaunch can take it
    /// back; none for a host, or while the client is out of touch.
    pub(crate) fn ticket(&self, game: &Game) -> Option<crate::rejoin_ticket::Ticket> {
        let Link::Client(client) = self else {
            return None;
        };
        if client.rejoin.is_some() || client.connection.is_none() {
            return None;
        }
        Some(crate::rejoin_ticket::Ticket {
            address: client.home.address.clone(),
            host: client.home.host.clone(),
            map: game.scenario.name.clone(),
            seat: game.presentation.human.0,
            token: client.home.token,
            build: client.home.commit.clone(),
            contact: 0,
        })
    }

    /// The match a rejoin restored since the last call, to install in place
    /// of the stale one.
    pub(crate) fn take_rejoined(&mut self) -> Option<Box<Game>> {
        match self {
            Link::Client(client) => client.rejoined.take(),
            Link::Host(_) => None,
        }
    }

    /// Who the match is waiting for or has lost track of.
    pub(crate) fn status(&self, now: Duration) -> NetStatus {
        match self {
            Link::Host(host) => NetStatus::hosted(&host.session, now),
            Link::Client(client) => NetStatus::joined(&client.session, now),
        }
    }

    /// Whether this machine hosts the match.
    pub(crate) fn hosting(&self) -> bool {
        matches!(self, Link::Host(_))
    }

    /// Drops a seat the match is waiting for: it surrenders and play
    /// resumes. Only the host can.
    pub(crate) fn drop_seat(&mut self, seat: PlayerId) {
        if let Link::Host(host) = self {
            host.session.drop_seat(seat);
        }
    }

    /// Hands a seat the match is waiting for to a bot, and play resumes.
    /// Only the host can. The bot and the session change together, so no
    /// order from the seat's player seals beside the bot's.
    pub(crate) fn replace_seat(
        &mut self,
        game: &mut Game,
        seat: PlayerId,
        difficulty: BotDifficulty,
        stance: BotStance,
    ) -> Result<()> {
        if let Link::Host(host) = self {
            let config = game.take_over_seat(seat, difficulty, stance)?;
            host.session.replace_seat(seat, config);
            let name = &game.scenario.players[usize::from(seat.0)].name;
            game.presentation
                .toast(format!("A bot took over {name}'s seat"));
        }
        Ok(())
    }

    /// Moves lines, runs due ticks, and reports how the match ended, once.
    pub(crate) fn pump(&mut self, game: &mut Game, now: Duration) -> Option<End> {
        match self {
            Link::Host(host) => host.pump(game, now),
            Link::Client(client) => client.pump(game, now),
        }
    }

    /// Leaves the match, handing back finished connections to flush their
    /// last lines. A host of a decided match lets a client still behind
    /// receive the deciding batches; an undecided host tells its clients it
    /// left. A client tells its host it left, surrendering its seat.
    pub(crate) fn leave(self, game: &Game, now: Duration) -> Option<Lingering> {
        let peers = match self {
            Link::Host(mut host) => {
                if game.state.result().is_none() {
                    host.session.abandon();
                    host.send_outgoing();
                }
                host.peers
                    .into_iter()
                    .map(|(_, connection)| connection)
                    .chain(host.departing.into_iter().map(|(connection, _)| connection))
                    .chain(host.visitors.into_iter().map(|(_, connection)| connection))
                    .collect()
            }
            Link::Client(mut client) => {
                let connection = client.connection.take()?;
                client.session.leave();
                for line in client.session.take_outgoing() {
                    connection.send(&line);
                }
                vec![connection]
            }
        };
        Some(Lingering {
            peers: peers
                .into_iter()
                .map(|mut connection| {
                    connection.finish();
                    connection
                })
                .collect(),
            since: now,
        })
    }
}

/// A departed machine's finished connections, flushing their last lines.
pub(crate) struct Lingering {
    peers: Vec<Connection>,
    since: Duration,
}

impl Lingering {
    /// Drains the connections; false once every one closed or the grace ran
    /// out.
    pub(crate) fn pump(&mut self, now: Duration) -> bool {
        self.peers.retain(|connection| drain(connection).is_ok());
        !self.peers.is_empty() && now.saturating_sub(self.since) < CLOSE_GRACE
    }
}

impl HostLink {
    fn pump(&mut self, game: &mut Game, now: Duration) -> Option<End> {
        let dt = self.clock.step(now);
        if let Some((since, end)) = self.closing {
            self.peers
                .retain(|(_, connection)| drain(connection).is_ok());
            let done = self.peers.is_empty() || now.saturating_sub(since) >= CLOSE_GRACE;
            if done && end.is_some() {
                self.peers.clear();
                self.closing = Some((since, None));
                return end;
            }
            return None;
        }

        let mut closed = Vec::new();
        for (seat, connection) in &self.peers {
            loop {
                match connection.try_recv() {
                    Ok(Some(line)) => self.session.receive(*seat, &line, now),
                    Ok(None) => break,
                    Err(Closed) => {
                        self.session.disconnected(*seat, now);
                        closed.push(*seat);
                        break;
                    }
                }
            }
        }
        self.peers.retain(|(seat, _)| !closed.contains(seat));
        self.serve_visitors(game, now);
        for order in game.take_outbox() {
            self.session.submit(order);
        }

        game.host_frame(&mut self.session, dt, now);
        for seat in self.session.playing_seats() {
            if !game.state.accepts_commands(seat) {
                self.session.retire_seat(seat);
            }
        }
        if game.state.result().is_some() {
            self.session.conclude();
        }

        let mut end = None;
        let mut released = Vec::new();
        for event in self.session.poll(now) {
            match event {
                HostEvent::Released { seat, exit } => {
                    released.push(seat);
                    if exit == Exit::Left {
                        let name = &game.scenario.players[usize::from(seat.0)].name;
                        game.presentation.toast(format!("{name} left the match"));
                    }
                }
                HostEvent::Lost { seat } => self.peers.retain(|(peer, _)| *peer != seat),
                HostEvent::Desync { tick, .. } => end = Some(End::Desync { tick }),
            }
        }
        game.presentation.net_notice = NetStatus::hosted(&self.session, now).notice(game);
        self.send_outgoing();
        for seat in released {
            if let Some(index) = self.peers.iter().position(|(peer, _)| *peer == seat) {
                let (_, mut connection) = self.peers.remove(index);
                connection.finish();
                self.departing.push((connection, now));
            }
        }
        self.departing.retain(|(connection, since)| {
            drain(connection).is_ok() && now.saturating_sub(*since) < CLOSE_GRACE
        });
        // A decided match still waits for the final acknowledgements, so
        // a divergence in the last report ticks is caught, not dropped.
        let settled = game.state.result().is_some() && self.session.caught_up();
        if end.is_some() || settled {
            for (_, connection) in &mut self.peers {
                connection.finish();
            }
            self.closing = Some((now, end));
        } else if self.clock.stalled(game, now) && game.presentation.net_notice.is_none() {
            game.presentation.toast("Waiting for players...");
        }
        None
    }

    /// Takes new visitors, reads them, snapshots the match for a rejoin,
    /// and hands an admitted visitor's connection to the session. The desk's
    /// lines go out first, so Go precedes anything the session says.
    fn serve_visitors(&mut self, game: &Game, now: Duration) {
        if let Some(listener) = &self.listener {
            while self.visitors.len() < MAX_GREETING {
                let Ok(Some(connection)) = listener.try_accept() else {
                    break;
                };
                let visitor = self.desk.connected(now);
                self.visitors.push((visitor, connection));
            }
        }
        let mut closed = Vec::new();
        for (visitor, connection) in &self.visitors {
            loop {
                match connection.try_recv() {
                    Ok(Some(line)) => self.desk.receive(&mut self.session, *visitor, &line, now),
                    Ok(None) => break,
                    Err(Closed) => {
                        self.desk.disconnected(&mut self.session, *visitor);
                        closed.push(*visitor);
                        break;
                    }
                }
            }
        }
        self.visitors
            .retain(|(visitor, _)| !closed.contains(visitor));
        let mut admitted = Vec::new();
        let mut done = Vec::new();
        for event in self.desk.poll(&mut self.session, now) {
            match event {
                // The session waits, so the world sits at its tick.
                DeskEvent::Snapshot { visitor, .. } => {
                    let snapshot = game
                        .world_snapshot()
                        .and_then(|snapshot| snapshot::encode(&snapshot));
                    if let Ok(bytes) = snapshot
                        && game.state.current_tick() == self.session.tick()
                    {
                        let tick = self.session.tick();
                        self.desk
                            .send_snapshot(visitor, tick, game.state.hash(), &bytes, now);
                    }
                }
                DeskEvent::Admitted { visitor, seat } => admitted.push((visitor, seat)),
                DeskEvent::Done { visitor } => done.push(visitor),
            }
        }
        for (visitor, line) in self.desk.take_outgoing() {
            if let Some((_, connection)) = self.visitors.iter().find(|(id, _)| *id == visitor) {
                connection.send(&line);
            }
        }
        for (visitor, seat) in admitted {
            if let Some(index) = self.visitors.iter().position(|(id, _)| *id == visitor) {
                let (_, connection) = self.visitors.remove(index);
                self.peers.retain(|(peer, _)| *peer != seat);
                self.peers.push((seat, connection));
            }
        }
        for visitor in done {
            if let Some(index) = self.visitors.iter().position(|(id, _)| *id == visitor) {
                let (_, mut connection) = self.visitors.remove(index);
                connection.finish();
                self.departing.push((connection, now));
            }
        }
    }

    fn send_outgoing(&mut self) {
        for (seat, line) in self.session.take_outgoing() {
            if let Some((_, connection)) = self.peers.iter().find(|(peer, _)| *peer == seat) {
                connection.send(&line);
            }
        }
    }
}

impl ClientLink {
    fn pump(&mut self, game: &mut Game, now: Duration) -> Option<End> {
        let dt = self.clock.step(now);
        if let Some(rejoin) = &mut self.rejoin {
            match rejoin.poll(now) {
                RejoinPoll::Trying => {
                    game.presentation.net_notice = Some("Reconnecting to the host...".to_owned());
                    return None;
                }
                RejoinPoll::Back(restored, connection) => {
                    let tick = restored.state.current_tick();
                    self.session = ClientSession::at(tick, now);
                    self.connection = Some(connection);
                    self.closed = false;
                    self.rejoin = None;
                    self.rejoined = Some(restored);
                    return None;
                }
                RejoinPoll::Failed(end) => {
                    self.rejoin = None;
                    return Some(end);
                }
            }
        }
        let connection = self.connection.as_ref()?;
        if !self.closed {
            loop {
                match connection.try_recv() {
                    Ok(Some(line)) => {
                        if let Err(end) = self.session.receive(&line, now) {
                            return self.lost(end, game, now);
                        }
                    }
                    Ok(None) => break,
                    Err(Closed) => {
                        self.closed = true;
                        break;
                    }
                }
            }
        }
        for (seat, config) in self.session.take_replaced() {
            game.mirror_bot(seat, config);
            let name = &game.scenario.players[usize::from(seat.0)].name;
            game.presentation
                .toast(format!("A bot took over {name}'s seat"));
        }
        for order in game.take_outbox() {
            self.session.send(order);
        }

        let keep = if self.closed { 0 } else { CLIENT_BUFFER };
        game.client_frame(&mut self.session, dt, keep);

        if self.closed {
            if self.session.backlog() > 0 {
                return None;
            }
            return self.lost(ClientEnd::HostSilent, game, now);
        }
        if let Err(end) = self.session.poll(now) {
            return self.lost(end, game, now);
        }
        for line in self.session.take_outgoing() {
            connection.send(&line);
        }
        game.presentation.net_notice = NetStatus::joined(&self.session, now).notice(game);
        if self.clock.stalled(game, now) && game.presentation.net_notice.is_none() {
            game.presentation.toast("Waiting for the host...");
        }
        None
    }

    /// The session ended. A host gone from an undecided match is rejoined
    /// rather than given up on; a decided one ends quietly.
    fn lost(&mut self, end: ClientEnd, game: &Game, now: Duration) -> Option<End> {
        let gone = matches!(end, ClientEnd::HostSilent | ClientEnd::Protocol);
        if gone && game.state.result().is_some() {
            self.connection = None;
            return None;
        }
        if gone {
            self.connection = None;
            self.rejoin = Some(Rejoin::start(self.home.clone(), now));
            return None;
        }
        Some(self.end(end))
    }

    fn end(&mut self, end: ClientEnd) -> End {
        self.connection = None;
        match end {
            ClientEnd::Desync { tick } => End::Desync { tick },
            ClientEnd::HostSilent | ClientEnd::Protocol => End::HostLost,
            ClientEnd::Released(Release::Dropped) => End::Dropped,
            ClientEnd::Released(Release::Replaced) => End::Replaced,
            ClientEnd::HostLeft => End::HostLeft,
        }
    }
}

/// Reads and discards whatever arrived, reporting whether the connection
/// is still open.
fn drain(connection: &Connection) -> Result<(), Closed> {
    while connection.try_recv()?.is_some() {}
    Ok(())
}

/// Gathering machines until a match starts.
pub(crate) enum Lobby {
    /// This machine hosts a fixed scenario.
    Host(Box<ScenarioHost>),
    /// This machine joins a host.
    Client(ClientLobby),
    /// This machine takes back a seat in a running match.
    Rejoin(Box<Returning>),
}

/// A client taking back its seat after the app was closed, from its ticket.
pub(crate) struct Returning {
    home: Home,
    rejoin: Rejoin,
    /// Why it gave up, once it did.
    failed: Option<String>,
}

impl Returning {
    /// Starts taking back the seat on `ticket`.
    pub(crate) fn new(ticket: &crate::rejoin_ticket::Ticket, now: Duration) -> Self {
        let home = Home::from_ticket(ticket);
        Self {
            rejoin: Rejoin::start(home.clone(), now),
            home,
            failed: None,
        }
    }

    fn poll(&mut self, now: Duration) -> Option<(Game, Link)> {
        if self.failed.is_some() {
            return None;
        }
        match self.rejoin.poll(now) {
            RejoinPoll::Trying => None,
            RejoinPoll::Back(game, connection) => {
                let session = ClientSession::at(game.state.current_tick(), now);
                Some((
                    *game,
                    Link::client(session, connection, self.home.clone(), now),
                ))
            }
            RejoinPoll::Failed(end) => {
                self.failed = Some(end.notice());
                None
            }
        }
    }

    fn status(&self) -> String {
        self.failed
            .clone()
            .unwrap_or_else(|| format!("Rejoining {}'s match...", self.home.host))
    }
}

impl Lobby {
    /// Polls the lobby; returns the match once it starts.
    pub(crate) fn poll(&mut self, now: Duration, viewport: Vec2) -> Option<(Game, Link)> {
        match self {
            Lobby::Host(host) => host.poll(now, viewport),
            Lobby::Client(client) => client.poll(now, viewport),
            Lobby::Rejoin(returning) => returning.poll(now),
        }
    }

    /// One line describing where the lobby stands.
    pub(crate) fn status(&self) -> String {
        match self {
            Lobby::Host(host) => host.status(),
            Lobby::Client(client) => client.status(),
            Lobby::Rejoin(returning) => returning.status(),
        }
    }

    /// One line per seat, once the lobby knows who sits where.
    pub(crate) fn roster(&self) -> Vec<String> {
        match self {
            Lobby::Host(host) => host.roster(),
            Lobby::Client(client) => client.roster(),
            Lobby::Rejoin(_) => Vec::new(),
        }
    }

    /// A joiner's view of the host's setup: the map, who sits where, and
    /// its own seat; `None` until both arrive, and for a host.
    pub(crate) fn room_view(&self) -> Option<(&Scenario, &[RoomSeat], PlayerId)> {
        let Lobby::Client(ClientLobby {
            state: Joining::Gathering { guest, .. },
            ..
        }) = self
        else {
            return None;
        };
        let (seats, seat) = guest.room()?;
        Some((guest.map()?, seats, seat))
    }

    /// A client that gave up: the name it joined as, the address it tried,
    /// and why it failed.
    pub(crate) fn failure(&self) -> Option<(&PlayerName, &str, &str)> {
        match self {
            Lobby::Client(ClientLobby {
                address,
                name,
                state: Joining::Failed(reason),
                ..
            }) => Some((name, address, reason)),
            _ => None,
        }
    }
}

/// Whether a host address can contain `ch`: host names, IPv4 and bracketed
/// IPv6 addresses, and a port.
/// Whether a lobby listening on `bound` advertises. Beacons are IPv4
/// broadcasts and joiners dial the address one came from, so only an IPv4
/// listener other machines can reach announces itself.
fn advertises(bound: std::net::SocketAddr) -> bool {
    bound.is_ipv4() && !bound.ip().is_loopback()
}

pub(crate) fn address_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | ':' | '[' | ']')
}

/// A typed host address with its port, the default when it names none, or
/// why it cannot reach a host.
pub(crate) fn parse_join_address(typed: &str) -> Result<String, &'static str> {
    const NOT_AN_ADDRESS: &str = "that isn't an address - try one like 192.168.1.20";
    const BAD_PORT: &str = "the port must be a number from 1 to 65535";
    let typed = typed.trim();
    if typed.is_empty() {
        return Err("enter the host's address");
    }
    if let Ok(socket) = typed.parse::<SocketAddr>() {
        return if socket.port() == 0 {
            Err(BAD_PORT)
        } else {
            Ok(typed.to_owned())
        };
    }
    match typed.parse::<IpAddr>() {
        Ok(IpAddr::V4(_)) => return Ok(format!("{typed}:{DEFAULT_PORT}")),
        Ok(IpAddr::V6(_)) => return Ok(format!("[{typed}]:{DEFAULT_PORT}")),
        Err(_) => {}
    }
    if let Some(inner) = typed.strip_prefix('[') {
        // A bracketed address with a valid port already parsed above.
        return match inner.strip_suffix(']').map(str::parse::<Ipv6Addr>) {
            Some(Ok(_)) => Ok(format!("{typed}:{DEFAULT_PORT}")),
            _ => Err(NOT_AN_ADDRESS),
        };
    }
    let (host, port) = match typed.rsplit_once(':') {
        Some((host, _)) if host.contains(':') => return Err(NOT_AN_ADDRESS),
        Some((host, port)) => match port.parse::<u16>() {
            Ok(port) if port != 0 => (host, port),
            _ => return Err(BAD_PORT),
        },
        None => (typed, DEFAULT_PORT),
    };
    if host_name(host) {
        Ok(format!("{host}:{port}"))
    } else {
        Err(NOT_AN_ADDRESS)
    }
}

/// Whether `host` is a well-formed DNS name. An all-digit last label is
/// refused, which catches unfinished or out-of-range IPv4 addresses such as
/// `192.168.1` and `192.168.1.300`.
fn host_name(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    let label = |label: &str| {
        (1..=63).contains(&label.len())
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    };
    host.len() <= 253
        && host.split('.').all(label)
        && host
            .rsplit('.')
            .next()
            .is_some_and(|last| !last.chars().all(|ch| ch.is_ascii_digit()))
}

/// Where other machines reach a listener bound to `bound`. A wildcard bind
/// shows the addresses of the interfaces this machine routes through to the
/// internet and to Tailscale's service address, which differ only when a
/// tailnet is up.
fn reachable(bound: SocketAddr) -> String {
    if !bound.ip().is_unspecified() {
        return bound.to_string();
    }
    let mut shown: Vec<String> = Vec::new();
    for probe in [
        Ipv4Addr::new(192, 0, 2, 1),
        Ipv4Addr::new(100, 100, 100, 100),
    ] {
        if let Some(ip) = routed_from(probe) {
            let address = SocketAddr::new(ip, bound.port()).to_string();
            if !shown.contains(&address) {
                shown.push(address);
            }
        }
    }
    if shown.is_empty() {
        format!("port {}", bound.port())
    } else {
        shown.join(" or ")
    }
}

/// The local address this machine would send to `probe` from. Connecting a
/// UDP socket only picks a route; it sends nothing.
fn routed_from(probe: Ipv4Addr) -> Option<IpAddr> {
    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .and_then(|socket| {
            socket.connect((probe, 9))?;
            socket.local_addr()
        })
        .ok()
        .map(|local| local.ip())
        .filter(|ip| !ip.is_unspecified())
}

/// A host's lobby sockets: accepting connections, running the `oxide-net`
/// lobby over them, and handing the seated ones to the match. Seating is
/// the caller's policy: it answers [`HostLobby::poll`]'s events and
/// publishes the room.
pub(crate) struct HostLobby {
    /// Moves on to the match once it starts.
    listener: Option<Listener>,
    address: String,
    commit: String,
    /// The TCP port joiners connect to.
    port: u16,
    /// Announces the lobby on the local network; `None` on loopback, which
    /// nobody else can reach, or where the platform refuses the socket.
    advertiser: Option<Advertiser>,
    gathering: Gathering,
    connections: Vec<(MemberId, Connection)>,
    /// Members refused or removed since the last flush, whose connections
    /// finish once their last line is out.
    farewells: Vec<MemberId>,
    /// Finished connections flushing their last line.
    leaving: Vec<(Connection, Duration)>,
}

/// A started match's session and connections, for its link.
pub(crate) struct Started {
    session: HostSession,
    peers: Vec<(PlayerId, Connection)>,
    listener: Option<Listener>,
    commit: String,
}

/// Where a running start stands.
pub(crate) enum StartPoll {
    /// Waiting for every seated member to be ready.
    Waiting,
    /// Every machine is at tick zero: the session, each seated member's
    /// connection, and the listener the match keeps for rejoins.
    Started(Box<Started>),
    /// The start failed; the members who caused it leave through the next
    /// poll, and the rest are back in the room.
    Failed(StartFailed),
}

impl HostLobby {
    /// Listens on `address` for builds of `commit` to play `map`.
    pub(crate) fn open(address: &str, commit: &str, map: &Scenario) -> Result<Self> {
        let listener =
            Listener::bind(address).with_context(|| format!("cannot listen on {address}"))?;
        let bound = listener
            .local_addr()
            .with_context(|| format!("cannot listen on {address}"))?;
        let advertiser = advertises(bound).then(Advertiser::new).and_then(Result::ok);
        Ok(Self {
            listener: Some(listener),
            address: reachable(bound),
            commit: commit.to_owned(),
            port: bound.port(),
            advertiser,
            gathering: Gathering::new(commit, map),
            connections: Vec::new(),
            farewells: Vec::new(),
            leaving: Vec::new(),
        })
    }

    /// Where other machines reach this lobby.
    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    /// A joined member's name.
    pub(crate) fn name(&self, member: MemberId) -> Option<&PlayerName> {
        self.gathering.name(member)
    }

    /// Announces the lobby on the local network, at most once per beacon
    /// interval: `host`'s lobby on `map`, `players` of `seats` here.
    pub(crate) fn advertise(
        &mut self,
        host: &PlayerName,
        map: &str,
        players: usize,
        seats: usize,
        now: Duration,
    ) {
        if let Some(advertiser) = &mut self.advertiser {
            let beacon = Beacon::new(&self.commit, host.clone(), self.port, map, players, seats);
            advertiser.poll(&beacon, now);
        }
    }

    /// Announces to `target` instead of the whole network.
    #[cfg(test)]
    pub(crate) fn advertise_to(&mut self, target: std::net::SocketAddr) {
        self.advertiser = Advertiser::with_target(target).ok();
    }

    /// How many connections are open, joined or not.
    #[cfg(test)]
    pub(crate) fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Accepts and reads connections and reports who joined and who left;
    /// a leaver's connection is already closed. No one is accepted while a
    /// start runs.
    pub(crate) fn poll(&mut self, now: Duration, starting: bool) -> Vec<LobbyEvent> {
        self.leaving.retain(|(connection, since)| {
            drain(connection).is_ok() && now.saturating_sub(*since) < CLOSE_GRACE
        });
        if !starting {
            self.accept(now);
        }
        for (member, connection) in &self.connections {
            loop {
                match connection.try_recv() {
                    Ok(Some(line)) => self.gathering.receive(*member, &line, now),
                    Ok(None) => break,
                    Err(Closed) => {
                        self.gathering.disconnected(*member);
                        break;
                    }
                }
            }
        }
        let mut events = self.gathering.poll(now);
        for event in &events {
            match event {
                LobbyEvent::Left { member, .. } => {
                    self.connections.retain(|(id, _)| id != member);
                }
                LobbyEvent::Turned { member } => self.farewells.push(*member),
                LobbyEvent::Joined { .. } => {}
            }
        }
        events.retain(|event| !matches!(event, LobbyEvent::Turned { .. }));
        self.flush(now);
        events
    }

    /// Accepts waiting connections while fewer than [`MAX_GREETING`] have
    /// yet to introduce themselves.
    fn accept(&mut self, now: Duration) {
        let mut unnamed = self
            .connections
            .iter()
            .filter(|(member, _)| self.gathering.name(*member).is_none())
            .count();
        while unnamed < MAX_GREETING {
            let Some(Ok(Some(connection))) = self.listener.as_ref().map(Listener::try_accept)
            else {
                break;
            };
            let member = self.gathering.connected(now);
            self.connections.push((member, connection));
            unnamed += 1;
        }
    }

    /// Turns a joined member away: every seat holds a person.
    pub(crate) fn refuse(&mut self, member: MemberId) {
        self.gathering.refuse(member);
        self.farewells.push(member);
    }

    /// Removes a member at the host's request.
    pub(crate) fn remove(&mut self, member: MemberId) {
        self.gathering.remove(member);
        self.farewells.push(member);
    }

    /// Shows the room, then sends everything queued.
    pub(crate) fn publish(
        &mut self,
        room: &[RoomSeat],
        seat_of: impl Fn(MemberId) -> Option<PlayerId>,
        now: Duration,
    ) {
        self.gathering.publish(room, seat_of);
        self.flush(now);
    }

    /// Sends what the lobby queued, then finishes refused and removed
    /// connections.
    fn flush(&mut self, now: Duration) {
        for (member, line) in self.gathering.take_outgoing() {
            if let Some((_, connection)) = self.connections.iter().find(|(id, _)| *id == member) {
                connection.send(&line);
            }
        }
        for member in std::mem::take(&mut self.farewells) {
            if let Some(index) = self.connections.iter().position(|(id, _)| *id == member) {
                let (_, mut connection) = self.connections.remove(index);
                connection.finish();
                self.leaving.push((connection, now));
            }
        }
    }

    /// Starts `scenario` with the host on `host` and each member of
    /// `seated` on its seat; `host_hash` is the host's tick-zero hash.
    pub(crate) fn start(
        &mut self,
        scenario: &Scenario,
        host: PlayerId,
        seated: &[(MemberId, PlayerId)],
        host_hash: u64,
        now: Duration,
    ) {
        self.gathering.start(
            scenario,
            host,
            seated,
            host_hash,
            now,
            crate::entropy::token,
        );
        self.flush(now);
    }

    /// Abandons a running start; everyone returns to the room.
    pub(crate) fn cancel_start(&mut self) {
        self.gathering.cancel_start();
    }

    /// Advances a running start; on success the seated connections leave
    /// with the session.
    pub(crate) fn poll_start(
        &mut self,
        now: Duration,
        seated: &[(MemberId, PlayerId)],
    ) -> StartPoll {
        let started = self.gathering.poll_start(now);
        self.flush(now);
        match started {
            Ok(None) => StartPoll::Waiting,
            Ok(Some(session)) => {
                let mut peers = Vec::new();
                for &(member, seat) in seated {
                    if let Some(index) = self.connections.iter().position(|(id, _)| *id == member) {
                        peers.push((seat, self.connections.remove(index).1));
                    }
                }
                StartPoll::Started(Box::new(Started {
                    session,
                    peers,
                    listener: self.listener.take(),
                    commit: self.commit.clone(),
                }))
            }
            Err(failed) => StartPoll::Failed(failed),
        }
    }
}

/// The host's notice for a failed start.
pub(crate) fn start_failed_notice(failed: StartFailed) -> &'static str {
    match failed {
        StartFailed::TimedOut => "A player didn't get ready",
        StartFailed::Mismatch { .. } => "A player's game didn't match",
        StartFailed::Protocol { .. } => "A player left",
    }
}

/// A host for a fixed scenario (`--host`): its human seats open, joiners
/// take them (or a bot's, when none is open), leavers free theirs, and the
/// match starts once every seat has someone to play it.
pub(crate) struct ScenarioHost {
    lobby: HostLobby,
    scenario: Scenario,
    host_name: Option<PlayerName>,
    seating: Seating,
    /// The seat order joiners fill: the setup screen's, grouped by team.
    order: Vec<usize>,
    /// The host's match while a start runs.
    starting: Option<Box<Game>>,
    /// A seating whose match could not be built, so it is not retried.
    unbuildable: Option<Seating>,
    notice: Option<String>,
}

impl ScenarioHost {
    /// Listens on `address` for the scenario's human seats other than
    /// `host`'s. `host_name`, when given, names the host's seat.
    pub(crate) fn new(
        address: &str,
        scenario: Scenario,
        host: PlayerId,
        commit: &str,
        host_name: Option<PlayerName>,
    ) -> Result<Self> {
        let humans: Vec<PlayerId> = scenario
            .players
            .iter()
            .zip(0..)
            .filter(|(player, _)| !player.bot)
            .map(|(_, seat)| PlayerId(seat))
            .collect();
        ensure!(
            humans.len() >= 2 && humans.contains(&host),
            "a hosted scenario needs the host's seat and another human seat (seats with \"bot\": false)"
        );
        Ok(Self {
            lobby: HostLobby::open(address, commit, &scenario)?,
            seating: Seating::from_scenario(&scenario, host),
            order: crate::screens::wizard::seat_display_order(&scenario),
            scenario,
            host_name,
            starting: None,
            unbuildable: None,
            notice: None,
        })
    }

    fn status(&self) -> String {
        let place = if self.starting.is_some() {
            "Starting the match...".to_owned()
        } else {
            let open = self
                .seating
                .seats()
                .iter()
                .filter(|occupant| **occupant == Occupant::Open)
                .count();
            let here = 1 + self.seating.members().len();
            format!(
                "Hosting on {}: {here} of {} players here",
                self.lobby.address(),
                here + open
            )
        };
        match &self.notice {
            Some(notice) => format!("{place}. {notice}"),
            None => place,
        }
    }

    /// One line per seat: its number and who plays it.
    fn roster(&self) -> Vec<String> {
        roster_lines(&self.room(), None)
    }

    fn room(&self) -> Vec<RoomSeat> {
        let host = shown_host_name(
            self.host_name.as_ref(),
            &self.scenario,
            self.seating.host_seat(),
        );
        self.seating.room(
            &host,
            |member| self.lobby.name(member).cloned(),
            |seat| self.scenario.players[seat].team,
        )
    }

    fn poll(&mut self, now: Duration, viewport: Vec2) -> Option<(Game, Link)> {
        for event in self.lobby.poll(now, self.starting.is_some()) {
            match event {
                LobbyEvent::Joined { member, .. } => {
                    if self.seating.place(member, &self.order).is_none() {
                        self.lobby.refuse(member);
                    }
                }
                LobbyEvent::Left { member, .. } => {
                    self.seating.vacate(member);
                }
                // The lobby closes these itself.
                LobbyEvent::Turned { .. } => {}
            }
        }
        if self.starting.is_some() {
            return self.poll_start(now);
        }
        let room = self.room();
        let seating = &self.seating;
        self.lobby
            .publish(&room, |member| seating.seat_of(member), now);
        // Joiners can take any seat but the host's, so the map's seats are
        // the lobby's room.
        let here = 1 + self.seating.members().len();
        let host = shown_host_name(
            self.host_name.as_ref(),
            &self.scenario,
            self.seating.host_seat(),
        );
        let seats = self.scenario.players.len();
        self.lobby
            .advertise(&host, &self.scenario.name, here, seats, now);
        if self.seating.ready() && self.unbuildable.as_ref() != Some(&self.seating) {
            self.start(now, viewport);
        }
        None
    }

    fn start(&mut self, now: Duration, viewport: Vec2) {
        let mut scenario = self.scenario.clone();
        let lobby = &self.lobby;
        // Every bot seat here came from the scenario and keeps its seed.
        self.seating.apply(
            &mut scenario,
            self.host_name.as_ref(),
            |member| lobby.name(member).cloned(),
            0,
        );
        let host = self.seating.host_seat();
        match Game::networked(scenario.clone(), host, NetRole::Host, viewport) {
            Ok(game) => {
                let seated = self.seating.members();
                self.lobby
                    .start(&scenario, host, &seated, game.state.hash(), now);
                self.starting = Some(Box::new(game));
                self.notice = None;
            }
            Err(error) => {
                self.unbuildable = Some(self.seating.clone());
                self.notice = Some(format!("Cannot start: {error}"));
            }
        }
    }

    fn poll_start(&mut self, now: Duration) -> Option<(Game, Link)> {
        match self.lobby.poll_start(now, &self.seating.members()) {
            StartPoll::Waiting => None,
            StartPoll::Started(started) => {
                let game = self.starting.take()?;
                let link = Link::host(*started, &game, now);
                Some((*game, link))
            }
            StartPoll::Failed(failed) => {
                self.starting = None;
                self.notice = Some(start_failed_notice(failed).to_owned());
                None
            }
        }
    }
}

/// The host's name as a room shows it: the player's, or else the seat's.
pub(crate) fn shown_host_name(
    host_name: Option<&PlayerName>,
    scenario: &Scenario,
    seat: PlayerId,
) -> PlayerName {
    host_name.cloned().unwrap_or_else(|| {
        PlayerName::new(&scenario.players[usize::from(seat.0)].name)
            .unwrap_or_else(|_| PlayerName::new("Host").expect("a valid name"))
    })
}

/// One line per seat: its number, who plays it, and whether that is the
/// host or the player at `you`.
pub(crate) fn roster_lines(seats: &[RoomSeat], you: Option<PlayerId>) -> Vec<String> {
    seats
        .iter()
        .enumerate()
        .map(|(seat, room)| {
            let mark = if you.is_some_and(|you| usize::from(you.0) == seat) {
                " (you)"
            } else {
                ""
            };
            let who = match &room.holder {
                SeatHolder::Host { name } => format!("{name} (host)"),
                SeatHolder::Player { name } => format!("{name}{mark}"),
                SeatHolder::Bot { difficulty, stance } => {
                    bot_label(*difficulty, *stance, BotLabelStyle::Controller)
                }
                SeatHolder::Open => "Open".to_owned(),
            };
            format!("{}. {who}", seat + 1)
        })
        .collect()
}

/// Why a connect to `address` failed. Apple platforms report a connect
/// that Local Network privacy blocked as an unreachable host, so there the
/// reason points at the setting.
fn unreachable(address: &str, error: &io::Error, apple: bool) -> String {
    let unreachable = matches!(
        error.kind(),
        io::ErrorKind::HostUnreachable | io::ErrorKind::NetworkUnreachable
    );
    if apple && unreachable {
        format!(
            "Cannot reach {address}: check that you share the host's network and that \
             Oxide is allowed under Privacy & Security > Local Network"
        )
    } else {
        format!("Cannot reach {address}: {error}")
    }
}

/// A client joining a host.
pub(crate) struct ClientLobby {
    address: String,
    commit: String,
    name: PlayerName,
    state: Joining,
}

enum Joining {
    Connecting(JoinHandle<io::Result<Connection>>),
    /// In the host's lobby; `game` is a start being readied.
    Gathering {
        connection: Connection,
        guest: Box<Guest>,
        game: Option<(Box<Game>, PlayerId)>,
    },
    Failed(String),
    Started,
}

impl ClientLobby {
    /// Starts connecting to the host at `address`, to join as `name`.
    pub(crate) fn new(address: &str, commit: &str, name: PlayerName) -> Self {
        let target = address.to_owned();
        Self {
            address: address.to_owned(),
            commit: commit.to_owned(),
            name,
            state: Joining::Connecting(thread::spawn(move || Connection::connect(target))),
        }
    }

    fn status(&self) -> String {
        match &self.state {
            Joining::Connecting(_) => format!("Connecting to {}...", self.address),
            Joining::Gathering {
                game: Some((_, seat)),
                ..
            } => format!("Starting as seat {}...", seat.0 + 1),
            Joining::Gathering { guest, .. } => match guest.room() {
                Some((seats, _)) => {
                    let host = seats.iter().find_map(|seat| match &seat.holder {
                        SeatHolder::Host { name } => Some(name.as_str()),
                        _ => None,
                    });
                    format!(
                        "Waiting for {} to start the match",
                        host.unwrap_or("the host")
                    )
                }
                None => format!("Joining {}...", self.address),
            },
            Joining::Failed(reason) => reason.clone(),
            Joining::Started => "Started".to_owned(),
        }
    }

    /// One line per seat once the room arrived, marking this player.
    fn roster(&self) -> Vec<String> {
        match &self.state {
            Joining::Gathering { guest, .. } => guest
                .room()
                .map(|(seats, you)| roster_lines(seats, Some(you)))
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn poll(&mut self, now: Duration, viewport: Vec2) -> Option<(Game, Link)> {
        let state = std::mem::replace(&mut self.state, Joining::Started);
        let (next, started) = self.advance(state, now, viewport);
        self.state = next;
        started
    }

    fn advance(
        &self,
        state: Joining,
        now: Duration,
        viewport: Vec2,
    ) -> (Joining, Option<(Game, Link)>) {
        let failed = |reason: String| (Joining::Failed(reason), None);
        match state {
            Joining::Connecting(handle) if handle.is_finished() => match handle.join() {
                Ok(Ok(connection)) => {
                    let mut guest = Box::new(Guest::new(&self.commit, self.name.clone(), now));
                    for line in guest.take_outgoing() {
                        connection.send(&line);
                    }
                    (
                        Joining::Gathering {
                            connection,
                            guest,
                            game: None,
                        },
                        None,
                    )
                }
                Ok(Err(error)) => failed(unreachable(
                    &self.address,
                    &error,
                    cfg!(any(target_os = "macos", target_os = "ios")),
                )),
                Err(_) => failed("Connecting failed".to_owned()),
            },
            Joining::Gathering {
                connection,
                mut guest,
                mut game,
            } => {
                loop {
                    let line = match connection.try_recv() {
                        Ok(Some(line)) => line,
                        Ok(None) => break,
                        Err(Closed) => {
                            return failed(if game.is_some() {
                                "The start failed; try joining again".to_owned()
                            } else {
                                "The host closed the lobby".to_owned()
                            });
                        }
                    };
                    match guest.receive(&line, now) {
                        Ok(Some(GuestEvent::Start { seat, scenario })) => {
                            match Game::networked(*scenario, seat, NetRole::Client, viewport) {
                                Ok(built) => {
                                    guest.ready(built.state.hash());
                                    game = Some((Box::new(built), seat));
                                }
                                Err(error) => return failed(format!("Cannot start: {error}")),
                            }
                        }
                        // A room during a start means the start is off.
                        Ok(Some(GuestEvent::Room)) => game = None,
                        // Stop reading at Go: the batches behind it belong to
                        // the link.
                        Ok(Some(GuestEvent::Go { token })) => {
                            for line in guest.take_outgoing() {
                                connection.send(&line);
                            }
                            let Some((built, _)) = game else {
                                return failed("The host sent something unexpected".to_owned());
                            };
                            let host = guest.room().and_then(|(seats, _)| {
                                seats.iter().find_map(|seat| match &seat.holder {
                                    SeatHolder::Host { name } => Some(name.to_string()),
                                    _ => None,
                                })
                            });
                            let home = Home {
                                address: self.address.clone(),
                                commit: self.commit.clone(),
                                token,
                                host: host.unwrap_or_else(|| "the host".to_owned()),
                            };
                            let link = Link::client(ClientSession::new(now), connection, home, now);
                            return (Joining::Started, Some((*built, link)));
                        }
                        Ok(Some(GuestEvent::Map) | None) => {}
                        Err(end) => return failed(self.ended(end)),
                    }
                }
                if let Err(end) = guest.poll(now) {
                    return failed(self.ended(end));
                }
                for line in guest.take_outgoing() {
                    connection.send(&line);
                }
                (
                    Joining::Gathering {
                        connection,
                        guest,
                        game,
                    },
                    None,
                )
            }
            other => (other, None),
        }
    }

    /// Why the lobby ended, for the player.
    fn ended(&self, end: GuestEnd) -> String {
        match end {
            GuestEnd::OtherBuild { commit } => {
                format!(
                    "The host runs build {commit}; this is build {}",
                    self.commit
                )
            }
            GuestEnd::Full => "The match is full".to_owned(),
            GuestEnd::Removed => "The host removed you from the lobby".to_owned(),
            GuestEnd::Underway => "That match already started".to_owned(),
            GuestEnd::Silent => "Lost contact with the host".to_owned(),
            GuestEnd::Protocol => "The host sent something unexpected".to_owned(),
        }
    }
}

mod snapshot;

#[cfg(test)]
mod tests;
