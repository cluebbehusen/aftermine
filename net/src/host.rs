//! The host side: stamping, sealing order, the progress gate, desync
//! detection, and client presence.

use crate::message::{ClientMessage, GoneReason, HostMessage, Release, SeatNotice, SeatState};
use crate::token::Token;
use crate::{
    BATCH_COMMAND_BYTES, CLIENT_PENDING_BYTES, HEARTBEAT_INTERVAL, LEAD_CAP, MAX_CLIENT_LINE_BYTES,
    PROGRESS_TIMEOUT, QUIET_AFTER, QUIET_LEAD_CAP, SILENCE_TIMEOUT, SUSPEND_GAP, reports_hash,
};
use oxide_sim::scenario::BotConfig;
use oxide_sim::{Command, PlayerCommand, PlayerId, Tick};
use std::collections::VecDeque;
use std::time::Duration;

/// How a client seat left the match for good.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// The host dropped the seat, which surrendered.
    Dropped,
    /// The client left, surrendering its seat.
    Left,
    /// The host handed the seat to a bot.
    Replaced,
    /// The seat can no longer act in the world; its client may stay on to
    /// watch.
    Defeated,
}

/// Something the host's caller must act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostEvent {
    /// The seat left the match: send its queued lines, then close its
    /// connection.
    Released {
        /// The client's seat.
        seat: PlayerId,
        /// How it left.
        exit: Exit,
    },
    /// The seat's connection broke the protocol: close it. The match waits
    /// for the seat's player as for any lost connection.
    Lost {
        /// The client's seat.
        seat: PlayerId,
    },
    /// The client's hash report disagreed with the host; the session has
    /// halted and every connected client has been told.
    Desync {
        /// The reporting client's seat.
        seat: PlayerId,
        /// The report tick that disagreed.
        tick: Tick,
    },
}

/// Whether a seat still playing is keeping up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presence {
    Live,
    /// Heard nothing for [`QUIET_AFTER`]: play continues, within
    /// [`QUIET_LEAD_CAP`].
    Quiet,
    /// Silent for [`SILENCE_TIMEOUT`], blocking for [`PROGRESS_TIMEOUT`], or
    /// without a connection: the match waits.
    Away {
        since: Duration,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    Playing(Presence),
    /// Taking the seat back on a fresh connection; the match waits, as it
    /// has since `away_since`.
    Rejoining {
        away_since: Duration,
    },
    Out(Exit),
}

/// A client connection's progress and liveness.
#[derive(Debug)]
struct Link {
    /// World tick after the last batch the client acknowledged.
    acked: Tick,
    /// When the host last received a line from the client.
    heard: Duration,
    /// When the host last sent the client a line, as seen by `poll`.
    sent: Duration,
    /// Whether a line was queued for the client since the last `poll`.
    spoke: bool,
    /// When a failed seal started waiting on this client.
    blocked_since: Option<Duration>,
}

#[derive(Debug)]
struct Client {
    seat: PlayerId,
    /// The seat's connection, while it has one.
    link: Option<Link>,
    standing: Standing,
}

impl Client {
    /// Still in the match, connected or not.
    fn playing(&self) -> bool {
        matches!(
            self.standing,
            Standing::Playing(_) | Standing::Rejoining { .. }
        )
    }

    /// The match waits for it.
    fn away(&self) -> bool {
        matches!(
            self.standing,
            Standing::Playing(Presence::Away { .. }) | Standing::Rejoining { .. }
        )
    }
}

/// Where the match stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Course {
    Playing,
    /// The match has a result: nobody is waited on any more.
    Concluded,
    /// A hash report disagreed: nothing more is sealed.
    Halted,
}

/// A command waiting to be sealed, with the bytes it adds to a batch line.
#[derive(Debug)]
struct Pending {
    command: PlayerCommand,
    bytes: usize,
}

impl Pending {
    fn new(player: PlayerId, command: Command) -> Self {
        let command = PlayerCommand { player, command };
        // The batch line carries the attributed command plus a separator.
        let bytes = serde_json::to_string(&command).map_or(0, |entry| entry.len() + 1);
        Self { command, bytes }
    }
}

/// The host's half of a lockstep session.
///
/// Each game-loop step, the host calls [`HostSession::seal`]; if it yields
/// human commands, the host appends its bot commands, calls
/// [`HostSession::publish`] with the complete batch, executes that batch, and
/// calls [`HostSession::executed`].
#[derive(Debug)]
pub struct HostSession {
    host: PlayerId,
    /// Every human seat, fixed at creation: the seats commands may come from.
    humans: Vec<PlayerId>,
    clients: Vec<Client>,
    /// Orders in arrival order, sealed oldest first.
    pending: Vec<Pending>,
    /// The tick the next sealed batch executes on.
    next_tick: Tick,
    sealed: bool,
    paused: bool,
    course: Course,
    /// The host's own hashes at report ticks some connected client has yet
    /// to acknowledge.
    hashes: VecDeque<(Tick, u64)>,
    /// The seat states and pause last shown to clients.
    shown: Option<(Vec<SeatNotice>, bool)>,
    /// Each client's rejoin token.
    tokens: Vec<(PlayerId, Token)>,
    last_poll: Option<Duration>,
    events: Vec<HostEvent>,
    outgoing: Vec<(PlayerId, String)>,
}

impl HostSession {
    /// A session at tick zero for the host's own seat and its clients' seats.
    pub fn new(host: PlayerId, clients: &[PlayerId], now: Duration) -> Self {
        let mut humans: Vec<PlayerId> = clients.iter().copied().chain([host]).collect();
        humans.sort_unstable();
        let seats = humans.len();
        humans.dedup();
        assert_eq!(humans.len(), seats, "human seats must be distinct");
        Self {
            host,
            humans,
            clients: clients
                .iter()
                .map(|&seat| Client {
                    seat,
                    link: Some(Link {
                        acked: 0,
                        heard: now,
                        sent: now,
                        spoke: false,
                        blocked_since: None,
                    }),
                    standing: Standing::Playing(Presence::Live),
                })
                .collect(),
            pending: Vec::new(),
            next_tick: 0,
            sealed: false,
            paused: false,
            course: Course::Playing,
            hashes: VecDeque::new(),
            shown: None,
            tokens: Vec::new(),
            last_poll: None,
            events: Vec::new(),
            outgoing: Vec::new(),
        }
    }

    /// The same session, knowing each client's rejoin token.
    #[must_use]
    pub fn with_tokens(mut self, tokens: Vec<(PlayerId, Token)>) -> Self {
        self.tokens = tokens;
        self
    }

    /// The seat `token` lets a client take back, or why it cannot: the seat
    /// must still be in a match that has not ended.
    pub fn token_seat(&self, token: Token) -> Result<PlayerId, GoneReason> {
        if self.course != Course::Playing {
            return Err(GoneReason::Over);
        }
        let seat = self
            .tokens
            .iter()
            .find(|(_, issued)| *issued == token)
            .map(|&(seat, _)| seat)
            .ok_or(GoneReason::Over)?;
        if self.client(seat).playing() {
            Ok(seat)
        } else {
            Err(GoneReason::SeatTaken)
        }
    }

    /// The token issued to `seat`.
    pub fn token(&self, seat: PlayerId) -> Option<Token> {
        self.tokens
            .iter()
            .find(|(issued, _)| *issued == seat)
            .map(|&(_, token)| token)
    }

    /// The tick the next batch executes on, which is the world tick a
    /// snapshot taken now holds.
    pub fn tick(&self) -> Tick {
        self.next_tick
    }

    /// A client is taking `seat` back on a fresh connection: any old
    /// connection is lost, the seat's unsealed orders go, and the match
    /// waits until [`HostSession::readmit`] or
    /// [`HostSession::rejoin_failed`]. False when the seat is not in play.
    pub fn rejoining(&mut self, seat: PlayerId, now: Duration) -> bool {
        let index = self.client_index(seat);
        let away_since = match self.clients[index].standing {
            Standing::Playing(Presence::Away { since }) => since,
            Standing::Rejoining { away_since } => away_since,
            Standing::Playing(Presence::Live | Presence::Quiet) => now,
            Standing::Out(_) => return false,
        };
        if self.clients[index].link.take().is_some() {
            self.events.push(HostEvent::Lost { seat });
        }
        self.pending
            .retain(|pending| pending.command.player != seat);
        self.clients[index].standing = Standing::Rejoining { away_since };
        self.prune_hashes();
        true
    }

    /// The rejoining client restored the match at [`HostSession::tick`]:
    /// its new connection joins the session, acknowledging batches from
    /// there. False unless the seat was rejoining.
    pub fn readmit(&mut self, seat: PlayerId, now: Duration) -> bool {
        let index = self.client_index(seat);
        if !matches!(self.clients[index].standing, Standing::Rejoining { .. }) {
            return false;
        }
        self.clients[index].link = Some(Link {
            acked: self.next_tick,
            heard: now,
            sent: now,
            spoke: false,
            blocked_since: None,
        });
        self.clients[index].standing = Standing::Playing(Presence::Live);
        // The new connection hears the seat states afresh.
        self.shown = None;
        true
    }

    /// The rejoin did not complete; the match waits for the seat as before.
    pub fn rejoin_failed(&mut self, seat: PlayerId) {
        let index = self.client_index(seat);
        if let Standing::Rejoining { away_since } = self.clients[index].standing {
            self.clients[index].standing = Standing::Playing(Presence::Away { since: away_since });
        }
    }

    /// Whether `seat` is taking its seat back.
    pub fn is_rejoining(&self, seat: PlayerId) -> bool {
        matches!(self.client(seat).standing, Standing::Rejoining { .. })
    }

    /// Queues an order from the host's own seat for the next sealed tick.
    pub fn submit(&mut self, command: Command) {
        self.pending.push(Pending::new(self.host, command));
    }

    /// Handles one line received from `seat`'s connection. Lines that
    /// arrive after the seat lost or gave up its connection are ignored.
    /// `seat` must be one of this session's clients.
    pub fn receive(&mut self, seat: PlayerId, line: &str, now: Duration) {
        let index = self.client_index(seat);
        let client = &mut self.clients[index];
        let Some(link) = &mut client.link else {
            return;
        };
        link.heard = now;
        if client.standing == Standing::Playing(Presence::Quiet) {
            client.standing = Standing::Playing(Presence::Live);
        }
        if line.len() > MAX_CLIENT_LINE_BYTES {
            self.lose(index, now);
            return;
        }
        match ClientMessage::decode(line) {
            Ok(ClientMessage::Command { command }) => {
                if !self.clients[index].playing() {
                    return;
                }
                let pending = Pending::new(seat, command);
                let queued: usize = self
                    .pending
                    .iter()
                    .filter(|queued| queued.command.player == seat)
                    .map(|queued| queued.bytes)
                    .sum();
                if queued + pending.bytes > CLIENT_PENDING_BYTES {
                    self.lose(index, now);
                } else {
                    self.pending.push(pending);
                }
            }
            Ok(ClientMessage::Ack { tick, hash }) => self.acknowledge(index, tick, hash, now),
            Ok(ClientMessage::Heartbeat) => {}
            Ok(ClientMessage::Leave) => self.release(index, Exit::Left),
            Err(_) => self.lose(index, now),
        }
    }

    /// The transport reported `seat`'s connection closed. A seat still
    /// playing makes the match wait for its player.
    pub fn disconnected(&mut self, seat: PlayerId, now: Duration) {
        let index = self.client_index(seat);
        if self.clients[index].link.take().is_some() {
            self.go_away(index, now);
            self.prune_hashes();
        }
    }

    /// Pausing stops sealing. Time spent paused never counts toward a
    /// client's progress timeout. Repeating the current state changes
    /// nothing.
    pub fn set_paused(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        for link in self
            .clients
            .iter_mut()
            .filter_map(|client| client.link.as_mut())
        {
            link.blocked_since = None;
        }
    }

    /// Drops `seat` at the host's request: it surrenders, and its client
    /// is told before its connection closes.
    pub fn drop_seat(&mut self, seat: PlayerId) {
        let index = self.client_index(seat);
        if self.clients[index].playing() {
            self.release(index, Exit::Dropped);
        }
    }

    /// Hands `seat` to a bot at the host's request: its unsealed orders are
    /// dropped, its client is told before its connection closes, and every
    /// other client hears which bot took it. The caller runs the bot.
    pub fn replace_seat(&mut self, seat: PlayerId, config: BotConfig) {
        let index = self.client_index(seat);
        if !self.clients[index].playing() {
            return;
        }
        self.pending
            .retain(|pending| pending.command.player != seat);
        self.clients[index].standing = Standing::Out(Exit::Replaced);
        if self.clients[index].link.take().is_some() {
            self.outgoing.push((
                seat,
                HostMessage::Released {
                    reason: Release::Replaced,
                }
                .encode(),
            ));
            self.events.push(HostEvent::Released {
                seat,
                exit: Exit::Replaced,
            });
        }
        self.broadcast(&HostMessage::Replaced { seat, config }.encode());
        self.prune_hashes();
    }

    /// Takes `seat` out of the match once it can no longer act in the
    /// world. Its client may stay connected to watch.
    pub fn retire_seat(&mut self, seat: PlayerId) {
        let index = self.client_index(seat);
        if self.clients[index].playing() {
            self.clients[index].standing = Standing::Out(Exit::Defeated);
        }
    }

    /// The match has a result: nobody is waited on, and the seat states
    /// clear.
    pub fn conclude(&mut self) {
        if self.course == Course::Playing {
            self.course = Course::Concluded;
        }
    }

    /// Tells every connected client the host left before the match ended.
    /// The caller finishes the connections after sending.
    pub fn abandon(&mut self) {
        self.broadcast(&HostMessage::Ended.encode());
    }

    /// The human commands for the next tick in sealed order, or `None` while
    /// paused, halted, waiting on an away seat, or held at a lead cap.
    /// Commands past [`BATCH_COMMAND_BYTES`] wait for a later tick. A sealed
    /// batch must be published before the next seal.
    pub fn seal(&mut self, now: Duration) -> Option<Vec<PlayerCommand>> {
        assert!(
            !self.sealed,
            "publish the sealed batch before sealing again"
        );
        if self.paused || self.course == Course::Halted || self.waiting() {
            return None;
        }
        let tick = self.next_tick;
        let mut blocked = false;
        for client in &mut self.clients {
            let Some(link) = &mut client.link else {
                continue;
            };
            let cap = match client.standing {
                Standing::Playing(Presence::Live) => LEAD_CAP,
                Standing::Playing(Presence::Quiet) => QUIET_LEAD_CAP,
                _ => continue,
            };
            if tick >= link.acked + cap {
                blocked = true;
                if cap == LEAD_CAP {
                    link.blocked_since.get_or_insert(now);
                }
            } else {
                link.blocked_since = None;
            }
        }
        if blocked {
            return None;
        }
        for link in self
            .clients
            .iter_mut()
            .filter_map(|client| client.link.as_mut())
        {
            link.blocked_since = None;
        }
        self.sealed = true;
        let mut budget = BATCH_COMMAND_BYTES;
        let fits = self
            .pending
            .iter()
            .take_while(|pending| {
                let fits = pending.bytes <= budget;
                budget = budget.saturating_sub(pending.bytes);
                fits
            })
            .count();
        let taken = fits.max(usize::from(!self.pending.is_empty()));
        let mut batch: Vec<PlayerCommand> = self
            .pending
            .drain(..taken)
            .map(|pending| pending.command)
            .collect();
        // Seats still in play take turns going first; a seat that left
        // seals its last orders behind them.
        let in_play: Vec<PlayerId> = self
            .humans
            .iter()
            .copied()
            .filter(|&seat| seat == self.host || self.client(seat).playing())
            .collect();
        let seats = in_play.len() as Tick;
        let first = tick % seats;
        batch.sort_by_key(|command| {
            assert!(
                self.humans.binary_search(&command.player).is_ok(),
                "pending commands come from human seats"
            );
            match in_play.iter().position(|&seat| seat == command.player) {
                Some(index) => (0, (index as Tick + seats - first) % seats),
                None => (1, 0),
            }
        });
        Some(batch)
    }

    /// Sends the complete sealed batch (sealed humans, then bots) to every
    /// connected client.
    pub fn publish(&mut self, batch: &[PlayerCommand]) {
        assert!(
            std::mem::take(&mut self.sealed),
            "seal a batch before publishing"
        );
        let line = HostMessage::Batch {
            tick: self.next_tick,
            commands: batch.to_vec(),
        }
        .encode();
        self.next_tick += 1;
        self.broadcast(&line);
    }

    /// Records the host's own hash once it has executed the published batch.
    /// `hash` runs only when the resulting tick is a report tick.
    pub fn executed(&mut self, hash: impl FnOnce() -> u64) {
        let tick = self.next_tick;
        if reports_hash(tick) && self.clients.iter().any(|client| client.link.is_some()) {
            self.hashes.push_back((tick, hash()));
        }
    }

    /// Whether every client the host is not waiting on has acknowledged
    /// every published batch, and so reported every hash the host expects.
    pub fn caught_up(&self) -> bool {
        self.clients
            .iter()
            .filter(|client| {
                !matches!(
                    client.standing,
                    Standing::Playing(Presence::Quiet | Presence::Away { .. })
                        | Standing::Rejoining { .. }
                )
            })
            .filter_map(|client| client.link.as_ref())
            .all(|link| link.acked == self.next_tick)
    }

    /// The client seats still playing.
    pub fn playing_seats(&self) -> Vec<PlayerId> {
        self.clients
            .iter()
            .filter(|client| client.playing())
            .map(|client| client.seat)
            .collect()
    }

    /// Whether the match is held for a seat that is away.
    pub fn waiting(&self) -> bool {
        self.course == Course::Playing && self.clients.iter().any(Client::away)
    }

    /// Every seat still playing that is not keeping up, as of `now`.
    pub fn seats(&self, now: Duration) -> Vec<SeatNotice> {
        if self.course != Course::Playing {
            return Vec::new();
        }
        self.clients
            .iter()
            .filter_map(|client| {
                let state = match client.standing {
                    Standing::Playing(Presence::Quiet) => SeatState::Quiet,
                    Standing::Playing(Presence::Away { since })
                    | Standing::Rejoining { away_since: since } => SeatState::Away {
                        waited_ms: millis(now.saturating_sub(since)),
                    },
                    _ => return None,
                };
                Some(SeatNotice {
                    seat: client.seat,
                    state,
                })
            })
            .collect()
    }

    /// Applies presence timeouts, queues heartbeats and seat changes, and
    /// returns what happened since the last poll. Call it from the game
    /// loop, never from a network thread, so a hung host stops heartbeating.
    pub fn poll(&mut self, now: Duration) -> Vec<HostEvent> {
        // Time the host itself was suspended is nobody's silence.
        let gap = self
            .last_poll
            .map_or(Duration::ZERO, |last| now.saturating_sub(last));
        self.last_poll = Some(now);
        for client in &mut self.clients {
            let Some(link) = &mut client.link else {
                continue;
            };
            if gap > SUSPEND_GAP {
                link.heard += gap;
                if let Some(since) = &mut link.blocked_since {
                    *since += gap;
                }
            }
            let silence = now.saturating_sub(link.heard);
            let stalled = link
                .blocked_since
                .is_some_and(|since| now.saturating_sub(since) >= PROGRESS_TIMEOUT);
            let caught_up = self.next_tick < link.acked + LEAD_CAP;
            match client.standing {
                Standing::Playing(Presence::Live | Presence::Quiet)
                    if silence >= SILENCE_TIMEOUT || stalled =>
                {
                    link.blocked_since = None;
                    client.standing = Standing::Playing(Presence::Away { since: now });
                }
                Standing::Playing(Presence::Live) if silence >= QUIET_AFTER => {
                    client.standing = Standing::Playing(Presence::Quiet);
                }
                Standing::Playing(Presence::Away { .. }) if silence < QUIET_AFTER && caught_up => {
                    client.standing = Standing::Playing(Presence::Live);
                }
                _ => {}
            }
            if std::mem::take(&mut link.spoke) {
                link.sent = now;
            } else if now.saturating_sub(link.sent) >= HEARTBEAT_INTERVAL {
                link.sent = now;
                self.outgoing
                    .push((client.seat, HostMessage::Heartbeat.encode()));
            }
        }
        self.show_seats(now);
        std::mem::take(&mut self.events)
    }

    /// Lines to send, each addressed to a client seat.
    pub fn take_outgoing(&mut self) -> Vec<(PlayerId, String)> {
        std::mem::take(&mut self.outgoing)
    }

    fn client_index(&self, seat: PlayerId) -> usize {
        self.clients
            .iter()
            .position(|client| client.seat == seat)
            .unwrap_or_else(|| panic!("{seat:?} is not a client seat"))
    }

    fn client(&self, seat: PlayerId) -> &Client {
        &self.clients[self.client_index(seat)]
    }

    fn acknowledge(&mut self, index: usize, tick: Tick, hash: Option<u64>, now: Duration) {
        let Some(link) = &mut self.clients[index].link else {
            return;
        };
        if tick != link.acked + 1 || tick > self.next_tick || hash.is_some() != reports_hash(tick) {
            self.lose(index, now);
            return;
        }
        link.acked = tick;
        if let Some(hash) = hash {
            let expected = self
                .hashes
                .iter()
                .find(|(report, _)| *report == tick)
                .map(|(_, hash)| *hash)
                .expect("the host records its hash before a client can report that tick");
            if hash != expected {
                self.halt(self.clients[index].seat, tick);
            }
        }
        self.prune_hashes();
    }

    /// A seat still playing waits for its player from `now`.
    fn go_away(&mut self, index: usize, now: Duration) {
        let client = &mut self.clients[index];
        if matches!(
            client.standing,
            Standing::Playing(Presence::Live | Presence::Quiet)
        ) {
            client.standing = Standing::Playing(Presence::Away { since: now });
        }
    }

    /// Closes a connection that broke the protocol.
    fn lose(&mut self, index: usize, now: Duration) {
        if self.clients[index].link.take().is_some() {
            self.go_away(index, now);
            let seat = self.clients[index].seat;
            self.events.push(HostEvent::Lost { seat });
            self.prune_hashes();
        }
    }

    /// Takes a seat out of the match for good. A seat that was still
    /// playing surrenders; a dropped client hears why.
    fn release(&mut self, index: usize, exit: Exit) {
        let client = &mut self.clients[index];
        let seat = client.seat;
        if client.playing() {
            client.standing = Standing::Out(exit);
            self.pending.push(Pending::new(seat, Command::Surrender));
        }
        if exit == Exit::Dropped && self.clients[index].link.is_some() {
            self.outgoing.push((
                seat,
                HostMessage::Released {
                    reason: Release::Dropped,
                }
                .encode(),
            ));
        }
        if self.clients[index].link.take().is_some() {
            self.events.push(HostEvent::Released { seat, exit });
        }
        self.prune_hashes();
    }

    fn halt(&mut self, seat: PlayerId, tick: Tick) {
        self.course = Course::Halted;
        self.events.push(HostEvent::Desync { seat, tick });
        self.broadcast(&HostMessage::Desync { tick }.encode());
    }

    /// Tells every connected client which seats are not keeping up, and
    /// whether the host paused, whenever either changed.
    fn show_seats(&mut self, now: Duration) {
        let seats = self.seats(now);
        let key = |seats: &[SeatNotice]| -> Vec<(PlayerId, bool)> {
            seats
                .iter()
                .map(|notice| (notice.seat, notice.state == SeatState::Quiet))
                .collect()
        };
        let changed = self
            .shown
            .as_ref()
            .is_none_or(|(shown, paused)| key(shown) != key(&seats) || *paused != self.paused);
        // Nothing to say until something is out of the ordinary.
        if changed && (self.shown.is_some() || !seats.is_empty() || self.paused) {
            self.broadcast(
                &HostMessage::Seats {
                    seats: seats.clone(),
                    paused: self.paused,
                }
                .encode(),
            );
            self.shown = Some((seats, self.paused));
        }
    }

    fn broadcast(&mut self, line: &str) {
        for client in &mut self.clients {
            if let Some(link) = &mut client.link {
                link.spoke = true;
                self.outgoing.push((client.seat, line.to_owned()));
            }
        }
    }

    fn prune_hashes(&mut self) {
        let oldest = self
            .clients
            .iter()
            .filter_map(|client| client.link.as_ref())
            .map(|link| link.acked)
            .min();
        match oldest {
            Some(oldest) => self.hashes.retain(|(tick, _)| *tick > oldest),
            None => self.hashes.clear(),
        }
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
