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
    Advertiser, ClientEnd, ClientSession, Closed, Connection, DEFAULT_PORT, Gathering, Guest,
    GuestEnd, GuestEvent, HostEvent, HostSession, Listener, LobbyEvent, MemberId, PlayerName,
    RoomSeat, SeatHolder, StartFailed,
};
use oxide_sim::{PlayerId, Scenario, Tick};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
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

/// The host's connections, one per client seat.
pub(crate) struct HostLink {
    session: HostSession,
    peers: Vec<(PlayerId, Connection)>,
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
    pub(crate) fn host(
        session: HostSession,
        peers: Vec<(PlayerId, Connection)>,
        now: Duration,
    ) -> Self {
        Self::Host(HostLink {
            session,
            peers,
            closing: None,
            clock: Pacing::new(now),
        })
    }

    fn client(session: ClientSession, connection: Connection, now: Duration) -> Self {
        Self::Client(ClientLink {
            session,
            connection: Some(connection),
            closed: false,
            clock: Pacing::new(now),
        })
    }

    /// Moves lines, runs due ticks, and reports how the match ended, once.
    pub(crate) fn pump(&mut self, game: &mut Game, now: Duration) -> Option<End> {
        match self {
            Link::Host(host) => host.pump(game, now),
            Link::Client(client) => client.pump(game, now),
        }
    }

    /// Leaves the match. A host of a decided match finishes its connections
    /// and hands them back to flush, so a client still behind receives the
    /// deciding batches instead of losing its host. Anything else drops at
    /// once: a client's seat surrenders, and an undecided host ends the match.
    pub(crate) fn leave(self, game: &Game, now: Duration) -> Option<Lingering> {
        match self {
            Link::Host(host) if game.state.result().is_some() => Some(Lingering {
                peers: host
                    .peers
                    .into_iter()
                    .map(|(_, mut connection)| {
                        connection.finish();
                        connection
                    })
                    .collect(),
                since: now,
            }),
            _ => None,
        }
    }
}

/// A departed host's finished connections, flushing their last lines.
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
                        self.session.disconnected(*seat);
                        closed.push(*seat);
                        break;
                    }
                }
            }
        }
        self.peers.retain(|(seat, _)| !closed.contains(seat));
        for order in game.take_outbox() {
            self.session.submit(order);
        }

        game.host_frame(&mut self.session, dt, now);

        let mut end = None;
        for event in self.session.poll(now) {
            match event {
                HostEvent::Dropped { seat, .. } => {
                    self.peers.retain(|(peer, _)| *peer != seat);
                    let name = &game.scenario.players[usize::from(seat.0)].name;
                    game.presentation.toast(format!("{name} left the match"));
                }
                HostEvent::Desync { tick, .. } => end = Some(End::Desync { tick }),
            }
        }
        for (seat, line) in self.session.take_outgoing() {
            if let Some((_, connection)) = self.peers.iter().find(|(peer, _)| *peer == seat) {
                connection.send(&line);
            }
        }
        // A decided match still waits for the final acknowledgements, so
        // a divergence in the last report ticks is caught, not dropped.
        let settled = game.state.result().is_some() && self.session.caught_up();
        if end.is_some() || settled {
            for (_, connection) in &mut self.peers {
                connection.finish();
            }
            self.closing = Some((now, end));
        } else if self.clock.stalled(game, now) {
            game.presentation.toast("Waiting for players...");
        }
        None
    }
}

impl ClientLink {
    fn pump(&mut self, game: &mut Game, now: Duration) -> Option<End> {
        let dt = self.clock.step(now);
        let connection = self.connection.as_ref()?;
        if !self.closed {
            loop {
                match connection.try_recv() {
                    Ok(Some(line)) => {
                        if let Err(end) = self.session.receive(&line, now) {
                            return Some(self.end(end));
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
        for order in game.take_outbox() {
            self.session.send(order);
        }

        let keep = if self.closed { 0 } else { CLIENT_BUFFER };
        game.client_frame(&mut self.session, dt, keep);

        if self.closed {
            if self.session.backlog() > 0 {
                return None;
            }
            self.connection = None;
            return game.state.result().is_none().then_some(End::HostLost);
        }
        if let Err(end) = self.session.poll(now) {
            return Some(self.end(end));
        }
        for line in self.session.take_outgoing() {
            connection.send(&line);
        }
        if self.clock.stalled(game, now) {
            game.presentation.toast("Waiting for the host...");
        }
        None
    }

    fn end(&mut self, end: ClientEnd) -> End {
        self.connection = None;
        match end {
            ClientEnd::Desync { tick } => End::Desync { tick },
            ClientEnd::HostSilent | ClientEnd::Protocol => End::HostLost,
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
}

impl Lobby {
    /// Polls the lobby; returns the match once it starts.
    pub(crate) fn poll(&mut self, now: Duration, viewport: Vec2) -> Option<(Game, Link)> {
        match self {
            Lobby::Host(host) => host.poll(now, viewport),
            Lobby::Client(client) => client.poll(now, viewport),
        }
    }

    /// One line describing where the lobby stands.
    pub(crate) fn status(&self) -> String {
        match self {
            Lobby::Host(host) => host.status(),
            Lobby::Client(client) => client.status(),
        }
    }

    /// One line per seat, once the lobby knows who sits where.
    pub(crate) fn roster(&self) -> Vec<String> {
        match self {
            Lobby::Host(host) => host.roster(),
            Lobby::Client(client) => client.roster(),
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
    listener: Listener,
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

/// Where a running start stands.
pub(crate) enum StartPoll {
    /// Waiting for every seated member to be ready.
    Waiting,
    /// Every machine is at tick zero: the session and each seated
    /// member's connection.
    Started(HostSession, Vec<(PlayerId, Connection)>),
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
        let advertiser = (!bound.ip().is_loopback())
            .then(Advertiser::new)
            .and_then(Result::ok);
        Ok(Self {
            listener,
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
        let events = self.gathering.poll(now);
        for event in &events {
            if let LobbyEvent::Left { member, .. } = event {
                self.connections.retain(|(id, _)| id != member);
            }
        }
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
            let Ok(Some(connection)) = self.listener.try_accept() else {
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
        self.gathering.start(scenario, host, seated, host_hash, now);
        self.flush(now);
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
                StartPoll::Started(session, peers)
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
            StartPoll::Started(session, peers) => {
                let game = self.starting.take()?;
                Some((*game, Link::host(session, peers, now)))
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
                        Ok(Some(GuestEvent::Go)) => {
                            for line in guest.take_outgoing() {
                                connection.send(&line);
                            }
                            let Some((built, _)) = game else {
                                return failed("The host sent something unexpected".to_owned());
                            };
                            let link = Link::client(ClientSession::new(now), connection, now);
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
            GuestEnd::Silent => "Lost contact with the host".to_owned(),
            GuestEnd::Protocol => "The host sent something unexpected".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests;
