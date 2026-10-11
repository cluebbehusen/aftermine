//! A client's call on a running match: probing whether its token still
//! names a seat, or rejoining that seat with a snapshot of the match.

use crate::message::{GoneReason, JoinMessage, LobbyMessage, same_build};
use crate::name::PlayerName;
use crate::snapshot::Assembler;
use crate::token::Token;
use crate::{HEARTBEAT_INTERVAL, SILENCE_TIMEOUT, SUSPEND_GAP};
use oxide_sim::{PlayerId, Tick};
use std::time::Duration;

/// What a line from the host brought.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisitEvent {
    /// The token names a seat the match is waiting for.
    Waiting {
        /// The host's name.
        host: PlayerName,
        /// The match's map.
        map: String,
    },
    /// The match as it stands: restore `bytes`, a world at `tick`, on
    /// `seat`, then call [`Visit::ready`].
    Snapshot {
        /// The seat taken back.
        seat: PlayerId,
        /// The world tick the snapshot holds.
        tick: Tick,
        /// The snapshot.
        bytes: Vec<u8>,
    },
    /// The seat is back; lines after this one belong to the session, from
    /// the snapshot's tick.
    Go,
}

/// Why a visit ended without the seat back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisitEnd {
    /// The token names no seat to take back.
    Gone(GoneReason),
    /// The host runs another build.
    OtherBuild {
        /// The host's commit.
        commit: String,
    },
    /// Nothing arrived from the host for [`SILENCE_TIMEOUT`].
    Silent,
    /// The host sent a line that failed to decode or broke the protocol.
    Protocol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Errand {
    Probe,
    Rejoin,
}

#[derive(Debug)]
enum Stage {
    Greeting,
    /// The probe or rejoin is out.
    Asked,
    Receiving {
        seat: PlayerId,
        tick: Tick,
        attempt: u32,
        assembler: Assembler,
    },
    Restoring {
        attempt: u32,
        ready: bool,
    },
    Done,
}

/// A client's half of a probe or rejoin.
#[derive(Debug)]
pub struct Visit {
    commit: String,
    token: Token,
    errand: Errand,
    stage: Stage,
    heard: Duration,
    sent: Duration,
    spoke: bool,
    last_poll: Option<Duration>,
    outgoing: Vec<String>,
}

impl Visit {
    /// Asks whether `token` still names a seat; queues the Hello.
    pub fn probe(commit: &str, token: Token, now: Duration) -> Self {
        Self::new(commit, token, Errand::Probe, now)
    }

    /// Asks to take back the seat `token` names; queues the Hello.
    pub fn rejoin(commit: &str, token: Token, now: Duration) -> Self {
        Self::new(commit, token, Errand::Rejoin, now)
    }

    fn new(commit: &str, token: Token, errand: Errand, now: Duration) -> Self {
        Self {
            commit: commit.to_owned(),
            token,
            errand,
            stage: Stage::Greeting,
            heard: now,
            sent: now,
            spoke: true,
            last_poll: None,
            outgoing: vec![JoinMessage::hello(commit).encode()],
        }
    }

    /// Handles one line from the host.
    pub fn receive(&mut self, line: &str, now: Duration) -> Result<Option<VisitEvent>, VisitEnd> {
        self.heard = now;
        let Ok(message) = LobbyMessage::decode(line) else {
            return Err(VisitEnd::Protocol);
        };
        if message == LobbyMessage::Heartbeat {
            return Ok(None);
        }
        if let LobbyMessage::Gone { reason } = message {
            return Err(VisitEnd::Gone(reason));
        }
        match (&mut self.stage, message) {
            (
                Stage::Greeting,
                LobbyMessage::Hello {
                    protocol,
                    sim,
                    commit,
                },
            ) => {
                if !same_build(protocol, sim, &commit, &self.commit) {
                    return Err(VisitEnd::OtherBuild { commit });
                }
                self.stage = Stage::Asked;
                let token = self.token;
                let ask = match self.errand {
                    Errand::Probe => JoinMessage::Probe { token },
                    Errand::Rejoin => JoinMessage::Rejoin { token },
                };
                self.say(&ask);
                Ok(None)
            }
            (Stage::Asked, LobbyMessage::Waiting { host, map }) if self.errand == Errand::Probe => {
                self.stage = Stage::Done;
                Ok(Some(VisitEvent::Waiting { host, map }))
            }
            (
                Stage::Asked,
                LobbyMessage::Resume {
                    seat,
                    tick,
                    bytes,
                    parts,
                    attempt,
                },
            ) if self.errand == Errand::Rejoin => {
                let assembler = Assembler::new(bytes, parts).ok_or(VisitEnd::Protocol)?;
                self.stage = Stage::Receiving {
                    seat,
                    tick,
                    attempt,
                    assembler,
                };
                Ok(None)
            }
            (
                Stage::Receiving {
                    seat,
                    tick,
                    attempt,
                    assembler,
                },
                LobbyMessage::Part { data },
            ) => {
                let Some(bytes) = assembler.push(&data).map_err(|_| VisitEnd::Protocol)? else {
                    return Ok(None);
                };
                let (seat, tick) = (*seat, *tick);
                self.stage = Stage::Restoring {
                    attempt: *attempt,
                    ready: false,
                };
                Ok(Some(VisitEvent::Snapshot { seat, tick, bytes }))
            }
            (Stage::Restoring { ready: true, .. }, LobbyMessage::Go { token })
                if token == self.token =>
            {
                self.stage = Stage::Done;
                Ok(Some(VisitEvent::Go))
            }
            _ => Err(VisitEnd::Protocol),
        }
    }

    /// Reports the snapshot restored, its world hashing to `hash`.
    pub fn ready(&mut self, hash: u64) {
        let Stage::Restoring { attempt, ready } = &mut self.stage else {
            panic!("no snapshot to be ready for");
        };
        *ready = true;
        let attempt = *attempt;
        self.say(&JoinMessage::Ready { hash, attempt });
    }

    /// Applies the host silence timeout and queues heartbeats. A gap
    /// longer than [`SUSPEND_GAP`] since the last poll, such as restoring
    /// a large snapshot, is not the host's silence.
    pub fn poll(&mut self, now: Duration) -> Result<(), VisitEnd> {
        let gap = self
            .last_poll
            .map_or(Duration::ZERO, |last| now.saturating_sub(last));
        self.last_poll = Some(now);
        if gap > SUSPEND_GAP {
            self.heard += gap;
        }
        if now.saturating_sub(self.heard) >= SILENCE_TIMEOUT {
            return Err(VisitEnd::Silent);
        }
        if std::mem::take(&mut self.spoke) {
            self.sent = now;
        } else if now.saturating_sub(self.sent) >= HEARTBEAT_INTERVAL {
            self.sent = now;
            self.outgoing.push(JoinMessage::Heartbeat.encode());
        }
        Ok(())
    }

    /// Lines to send to the host.
    pub fn take_outgoing(&mut self) -> Vec<String> {
        std::mem::take(&mut self.outgoing)
    }

    fn say(&mut self, message: &JoinMessage) {
        self.spoke = true;
        self.outgoing.push(message.encode());
    }
}

#[cfg(test)]
mod tests;
