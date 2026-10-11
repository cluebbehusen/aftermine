//! The start barrier: from a frozen roster to tick zero on every machine.

use crate::START_TIMEOUT;
use crate::host::HostSession;
use crate::message::{JoinMessage, LobbyMessage};
use crate::snapshot;
use crate::token::Token;
use oxide_sim::{PlayerCommand, PlayerId, Scenario, Tick};
use std::time::Duration;

/// Why a start was abandoned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartFailed {
    /// Not every client was ready within [`START_TIMEOUT`];
    /// [`StartBarrier::unready`] names them.
    TimedOut,
    /// The client's world at tick zero hashed differently from the host's.
    Mismatch {
        /// The client's seat.
        seat: PlayerId,
    },
    /// The client sent something other than heartbeats and one decodable
    /// Ready.
    Protocol {
        /// The client's seat.
        seat: PlayerId,
    },
}

/// What a start sets up on every machine.
#[derive(Debug, Clone, Copy)]
pub enum StartContent<'a> {
    /// A new match at tick zero.
    Fresh(&'a Scenario),
    /// A saved match, carried as a snapshot whose bytes are opaque here.
    Resume {
        /// The human seats that take part, which the roster fills: a seat
        /// whose player was out of the match when it was saved takes none.
        seats: &'a [PlayerId],
        /// The tick the saved world is at.
        tick: Tick,
        /// The encoded snapshot every client restores.
        snapshot: &'a [u8],
        /// Orders the saved match had not sealed, in arrival order.
        pending: &'a [PlayerCommand],
    },
}

impl StartContent<'_> {
    /// The human seats the roster must fill, in seat order.
    fn humans(&self) -> Vec<PlayerId> {
        match self {
            StartContent::Fresh(scenario) => scenario
                .players
                .iter()
                .zip(0..)
                .filter(|(player, _)| !player.bot)
                .map(|(_, seat)| PlayerId(seat))
                .collect(),
            StartContent::Resume { seats, .. } => {
                let mut seats = seats.to_vec();
                seats.sort_unstable();
                seats
            }
        }
    }

    /// The lines that start `seat`'s client.
    fn lines(&self, seat: PlayerId, attempt: u32) -> Vec<String> {
        match *self {
            StartContent::Fresh(scenario) => vec![
                LobbyMessage::Start {
                    seat,
                    scenario: Box::new(scenario.clone()),
                    attempt,
                }
                .encode(),
            ],
            StartContent::Resume { tick, snapshot, .. } => {
                let parts = snapshot::parts(snapshot);
                let resume = LobbyMessage::Resume {
                    seat,
                    tick,
                    bytes: snapshot.len() as u64,
                    parts: u32::try_from(parts.len()).unwrap_or(u32::MAX),
                    attempt,
                };
                std::iter::once(resume.encode())
                    .chain(
                        parts
                            .into_iter()
                            .map(|data| LobbyMessage::Part { data }.encode()),
                    )
                    .collect()
            }
        }
    }
}

/// The host's half of starting a match once the roster is frozen.
///
/// The barrier queues each client's Start, collects every client's Ready,
/// and compares each tick-zero hash with the host's. Once all are in, it
/// queues Go and hands over the [`HostSession`], whose liveness clocks start
/// then. Send the Go lines before anything the session queues.
///
/// Each start carries an attempt number its Readys repeat, so a Ready that
/// arrives after its start was abandoned is ignored rather than counted.
#[derive(Debug)]
pub struct StartBarrier {
    host: PlayerId,
    clients: Vec<PlayerId>,
    /// Each client's rejoin token, sent with its Go.
    tokens: Vec<Token>,
    /// Where the session starts: the saved tick and unsealed orders of a
    /// resumed match, tick zero and none for a new one.
    from: (Tick, Vec<PlayerCommand>),
    hash: u64,
    attempt: u32,
    deadline: Duration,
    ready: Vec<bool>,
    started: bool,
    outgoing: Vec<(PlayerId, String)>,
}

impl StartBarrier {
    /// Starts `content` with the host on `host` and clients on `clients`,
    /// which together must hold exactly the human seats that take part. Each
    /// client's token reaches it with Go. `host_hash` is the host's
    /// `State::hash` at the starting tick, and `attempt` numbers this start.
    pub fn new(
        content: StartContent<'_>,
        host: PlayerId,
        clients: &[(PlayerId, Token)],
        host_hash: u64,
        attempt: u32,
        now: Duration,
    ) -> Self {
        let tokens: Vec<Token> = clients.iter().map(|&(_, token)| token).collect();
        let clients: Vec<PlayerId> = clients.iter().map(|&(seat, _)| seat).collect();
        let mut seated: Vec<PlayerId> = clients.iter().copied().chain([host]).collect();
        seated.sort_unstable();
        assert_eq!(
            seated,
            content.humans(),
            "the host and clients must hold exactly the human seats that take part"
        );
        let outgoing = clients
            .iter()
            .flat_map(|&seat| {
                content
                    .lines(seat, attempt)
                    .into_iter()
                    .map(move |line| (seat, line))
            })
            .collect();
        let from = match content {
            StartContent::Fresh(_) => (0, Vec::new()),
            StartContent::Resume { tick, pending, .. } => (tick, pending.to_vec()),
        };
        Self {
            host,
            ready: vec![false; clients.len()],
            clients,
            tokens,
            from,
            hash: host_hash,
            attempt,
            deadline: now + START_TIMEOUT,
            started: false,
            outgoing,
        }
    }

    /// Handles one line received from `seat`'s connection. `seat` must be one
    /// of the clients.
    pub fn receive(&mut self, seat: PlayerId, line: &str) -> Result<(), StartFailed> {
        let index = self
            .clients
            .iter()
            .position(|&client| client == seat)
            .unwrap_or_else(|| panic!("{seat:?} is not a client seat"));
        match JoinMessage::decode(line) {
            Ok(JoinMessage::Heartbeat) => Ok(()),
            Ok(JoinMessage::Ready { attempt, .. }) if attempt != self.attempt => Ok(()),
            Ok(JoinMessage::Ready { hash, .. }) if !self.ready[index] => {
                if hash != self.hash {
                    return Err(StartFailed::Mismatch { seat });
                }
                self.ready[index] = true;
                Ok(())
            }
            _ => Err(StartFailed::Protocol { seat }),
        }
    }

    /// The clients that have not sent their Ready.
    pub fn unready(&self) -> Vec<PlayerId> {
        self.clients
            .iter()
            .zip(&self.ready)
            .filter(|(_, ready)| !**ready)
            .map(|(&seat, _)| seat)
            .collect()
    }

    /// The session once every client is ready, with Go queued for each;
    /// `None` while waiting.
    pub fn poll(&mut self, now: Duration) -> Result<Option<HostSession>, StartFailed> {
        assert!(!self.started, "the match already started");
        if self.ready.iter().all(|&ready| ready) {
            self.started = true;
            let seats = self
                .clients
                .iter()
                .copied()
                .zip(self.tokens.iter().copied());
            self.outgoing.extend(
                seats
                    .clone()
                    .map(|(seat, token)| (seat, LobbyMessage::Go { token }.encode())),
            );
            let (tick, pending) = std::mem::take(&mut self.from);
            let session = HostSession::new(self.host, &self.clients, now)
                .with_tokens(seats.collect())
                .resumed(tick, pending);
            return Ok(Some(session));
        }
        if now >= self.deadline {
            return Err(StartFailed::TimedOut);
        }
        Ok(None)
    }

    /// Lines to send, each addressed to a client seat.
    pub fn take_outgoing(&mut self) -> Vec<(PlayerId, String)> {
        std::mem::take(&mut self.outgoing)
    }
}

#[cfg(test)]
mod tests;
