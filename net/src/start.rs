//! The start barrier: from a frozen roster to tick zero on every machine.

use crate::START_TIMEOUT;
use crate::host::HostSession;
use crate::message::{JoinMessage, LobbyMessage};
use crate::token::Token;
use oxide_sim::{PlayerId, Scenario};
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
    hash: u64,
    attempt: u32,
    deadline: Duration,
    ready: Vec<bool>,
    started: bool,
    outgoing: Vec<(PlayerId, String)>,
}

impl StartBarrier {
    /// Starts `scenario` with the host on `host` and clients on `clients`,
    /// which together must hold exactly the scenario's human seats. Each
    /// client's token reaches it with Go. `host_hash` is the host's
    /// `State::hash` at tick zero, and `attempt` numbers this start.
    pub fn new(
        scenario: &Scenario,
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
        let humans: Vec<PlayerId> = scenario
            .players
            .iter()
            .zip(0..)
            .filter(|(player, _)| !player.bot)
            .map(|(_, seat)| PlayerId(seat))
            .collect();
        assert_eq!(
            seated, humans,
            "the host and clients must hold exactly the scenario's human seats"
        );
        let outgoing = clients
            .iter()
            .map(|&seat| {
                let start = LobbyMessage::Start {
                    seat,
                    scenario: Box::new(scenario.clone()),
                    attempt,
                };
                (seat, start.encode())
            })
            .collect();
        Self {
            host,
            ready: vec![false; clients.len()],
            clients,
            tokens,
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
            let session =
                HostSession::new(self.host, &self.clients, now).with_tokens(seats.collect());
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
