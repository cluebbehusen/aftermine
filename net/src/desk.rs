//! A host's front desk while its match runs: fresh connections that probe
//! a token or rejoin a seat, before any of them joins the session.
//!
//! Like the rest of the crate, the desk does no I/O: the caller feeds it
//! lines from each visitor's connection, takes the snapshot it asks for,
//! and sends what it queues.

use crate::host::HostSession;
use crate::message::{GoneReason, JoinMessage, LobbyMessage, same_build};
use crate::name::PlayerName;
use crate::snapshot;
use crate::{GREETING_TIMEOUT, HEARTBEAT_INTERVAL, MAX_JOIN_LINE_BYTES, START_TIMEOUT};
use aftermine_sim::{PlayerId, Tick};
use std::time::Duration;

/// Identifies one visitor's connection to the desk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisitorId(u64);

/// Something the desk's caller must act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeskEvent {
    /// The match waits at [`HostSession::tick`] for `visitor` to rejoin
    /// `seat`: snapshot it and call [`Desk::send_snapshot`].
    Snapshot {
        /// The visitor.
        visitor: VisitorId,
        /// The seat it takes back.
        seat: PlayerId,
    },
    /// The visitor restored the match and took `seat` back: once its queued
    /// lines are sent, its connection belongs to the session as that seat.
    Admitted {
        /// The visitor.
        visitor: VisitorId,
        /// Its seat.
        seat: PlayerId,
    },
    /// The visitor was answered or turned away: send its queued lines, then
    /// close its connection.
    Done {
        /// The visitor.
        visitor: VisitorId,
    },
}

#[derive(Debug)]
enum Stage {
    /// The host's Hello is out; waiting for the visitor's.
    Greeting,
    /// Waiting to hear what the visitor wants.
    Asking,
    /// Rejoining `seat`; `hash` is the snapshot's, once it went out. The
    /// visitor has `allowed` from `since` to restore it.
    Restoring {
        seat: PlayerId,
        attempt: u32,
        hash: Option<u64>,
        since: Duration,
        allowed: Duration,
    },
}

#[derive(Debug)]
struct Visitor {
    id: VisitorId,
    stage: Stage,
    connected: Duration,
    sent: Duration,
    spoke: bool,
}

/// The host's half of probes and rejoins.
#[derive(Debug)]
pub struct Desk {
    commit: String,
    host: PlayerName,
    map: String,
    visitors: Vec<Visitor>,
    next_id: u64,
    attempt: u32,
    events: Vec<DeskEvent>,
    outgoing: Vec<(VisitorId, String)>,
}

impl Desk {
    /// A desk for the match of build `commit` hosted by `host` on `map`.
    pub fn new(commit: &str, host: PlayerName, map: &str) -> Self {
        Self {
            commit: commit.to_owned(),
            host,
            map: map.to_owned(),
            visitors: Vec::new(),
            next_id: 0,
            attempt: 0,
            events: Vec::new(),
            outgoing: Vec::new(),
        }
    }

    /// A connection arrived; the host's Hello is queued for it.
    pub fn connected(&mut self, now: Duration) -> VisitorId {
        let id = VisitorId(self.next_id);
        self.next_id += 1;
        self.visitors.push(Visitor {
            id,
            stage: Stage::Greeting,
            connected: now,
            sent: now,
            spoke: false,
        });
        self.send(id, LobbyMessage::hello(&self.commit).encode());
        id
    }

    /// How many visitors are at the desk.
    pub fn len(&self) -> usize {
        self.visitors.len()
    }

    /// Whether nobody is at the desk.
    pub fn is_empty(&self) -> bool {
        self.visitors.is_empty()
    }

    /// Handles one line from `visitor`.
    pub fn receive(
        &mut self,
        session: &mut HostSession,
        visitor: VisitorId,
        line: &str,
        now: Duration,
    ) {
        let Some(index) = self.index(visitor) else {
            return;
        };
        if line.len() > MAX_JOIN_LINE_BYTES {
            self.dismiss(session, index);
            return;
        }
        let message = JoinMessage::decode(line);
        match (&self.visitors[index].stage, message) {
            (_, Ok(JoinMessage::Heartbeat)) => {}
            (
                Stage::Greeting,
                Ok(JoinMessage::Hello {
                    protocol,
                    sim,
                    commit,
                }),
            ) if same_build(protocol, sim, &commit, &self.commit) => {
                self.visitors[index].stage = Stage::Asking;
            }
            (Stage::Asking, Ok(JoinMessage::Probe { token })) => {
                let answer = match session.token_seat(token) {
                    Ok(_) => LobbyMessage::Waiting {
                        host: self.host.clone(),
                        map: self.map.clone(),
                    },
                    Err(reason) => LobbyMessage::Gone { reason },
                };
                self.send(visitor, answer.encode());
                self.dismiss(session, index);
            }
            (Stage::Asking, Ok(JoinMessage::Rejoin { token })) => match session.token_seat(token) {
                Ok(seat) => {
                    // A visitor already rejoining the seat loses it to this one.
                    if let Some(rival) = self.visitors.iter().position(|other| {
                        other.id != visitor
                            && matches!(other.stage, Stage::Restoring { seat: taken, .. } if taken == seat)
                    }) {
                        let rival_id = self.visitors[rival].id;
                        self.visitors.remove(rival);
                        self.events.push(DeskEvent::Done { visitor: rival_id });
                    }
                    session.rejoining(seat, now);
                    self.attempt += 1;
                    let index = self.index(visitor).expect("the visitor is still here");
                    self.visitors[index].stage = Stage::Restoring {
                        seat,
                        attempt: self.attempt,
                        hash: None,
                        since: now,
                        allowed: START_TIMEOUT,
                    };
                    self.events.push(DeskEvent::Snapshot { visitor, seat });
                }
                Err(reason) => {
                    self.send(visitor, LobbyMessage::Gone { reason }.encode());
                    self.dismiss(session, index);
                }
            },
            (Stage::Asking, Ok(JoinMessage::Introduce { .. })) => {
                self.send(visitor, LobbyMessage::Underway.encode());
                self.dismiss(session, index);
            }
            (
                &Stage::Restoring {
                    seat,
                    attempt,
                    hash: Some(expected),
                    ..
                },
                Ok(JoinMessage::Ready {
                    hash,
                    attempt: answered,
                }),
            ) if answered == attempt => {
                if hash == expected && session.readmit(seat, now) {
                    if let Some(token) = session.token(seat) {
                        self.send(visitor, LobbyMessage::Go { token }.encode());
                    }
                    self.visitors.remove(index);
                    self.events.push(DeskEvent::Admitted { visitor, seat });
                } else {
                    self.dismiss(session, index);
                }
            }
            _ => self.dismiss(session, index),
        }
    }

    /// Sends a rejoining visitor its snapshot: `bytes` encode the world at
    /// `tick`, which hashes to `hash`. A snapshot no client would accept
    /// turns the visitor away instead.
    pub fn send_snapshot(
        &mut self,
        session: &mut HostSession,
        visitor: VisitorId,
        tick: Tick,
        hash: u64,
        bytes: &[u8],
        now: Duration,
    ) {
        if bytes.is_empty() || bytes.len() as u64 > snapshot::MAX_SNAPSHOT_BYTES {
            self.refuse(session, visitor, GoneReason::TooLarge);
            return;
        }
        let Some(index) = self.index(visitor) else {
            return;
        };
        let Stage::Restoring {
            seat,
            attempt,
            hash: expected,
            since,
            allowed,
        } = &mut self.visitors[index].stage
        else {
            return;
        };
        *expected = Some(hash);
        *since = now;
        // A large snapshot on a slow link takes a while to arrive.
        *allowed = START_TIMEOUT + snapshot::transfer_time(bytes.len() as u64);
        let (seat, attempt) = (*seat, *attempt);
        let parts = snapshot::parts(bytes);
        let resume = LobbyMessage::Resume {
            seat,
            tick,
            bytes: bytes.len() as u64,
            parts: u32::try_from(parts.len()).unwrap_or(u32::MAX),
            attempt,
        };
        self.send(visitor, resume.encode());
        for data in parts {
            self.send(visitor, LobbyMessage::Part { data }.encode());
        }
    }

    /// Turns `visitor` away for `reason`. A rejoin it began fails, and the
    /// match waits for the seat as before.
    pub fn refuse(&mut self, session: &mut HostSession, visitor: VisitorId, reason: GoneReason) {
        let Some(index) = self.index(visitor) else {
            return;
        };
        self.send(visitor, LobbyMessage::Gone { reason }.encode());
        self.dismiss(session, index);
    }

    /// The transport reported `visitor`'s connection closed.
    pub fn disconnected(&mut self, session: &mut HostSession, visitor: VisitorId) {
        if let Some(index) = self.index(visitor) {
            if let Stage::Restoring { seat, .. } = self.visitors[index].stage {
                session.rejoin_failed(seat);
            }
            self.visitors.remove(index);
        }
    }

    /// Applies timeouts, heartbeats visitors, and returns what happened.
    /// A visitor that has not asked after [`GREETING_TIMEOUT`], or not
    /// restored the match [`START_TIMEOUT`] after its snapshot, plus the
    /// time the snapshot takes to cross a slow link, is turned away, as is
    /// one whose seat the host dropped meanwhile.
    pub fn poll(&mut self, session: &mut HostSession, now: Duration) -> Vec<DeskEvent> {
        let mut index = 0;
        while index < self.visitors.len() {
            let visitor = &self.visitors[index];
            let expired = match visitor.stage {
                Stage::Greeting | Stage::Asking => {
                    now.saturating_sub(visitor.connected) >= GREETING_TIMEOUT
                }
                Stage::Restoring {
                    seat,
                    since,
                    allowed,
                    ..
                } => !session.is_rejoining(seat) || now.saturating_sub(since) >= allowed,
            };
            if expired {
                if let Stage::Restoring { seat, .. } = visitor.stage
                    && !session.is_rejoining(seat)
                {
                    let id = visitor.id;
                    let gone = LobbyMessage::Gone {
                        reason: GoneReason::SeatTaken,
                    };
                    self.send(id, gone.encode());
                }
                self.dismiss(session, index);
                continue;
            }
            let visitor = &mut self.visitors[index];
            if std::mem::take(&mut visitor.spoke) {
                visitor.sent = now;
            } else if now.saturating_sub(visitor.sent) >= HEARTBEAT_INTERVAL {
                visitor.sent = now;
                self.outgoing
                    .push((visitor.id, LobbyMessage::Heartbeat.encode()));
            }
            index += 1;
        }
        std::mem::take(&mut self.events)
    }

    /// Lines to send, each addressed to a visitor.
    pub fn take_outgoing(&mut self) -> Vec<(VisitorId, String)> {
        std::mem::take(&mut self.outgoing)
    }

    fn index(&self, visitor: VisitorId) -> Option<usize> {
        self.visitors
            .iter()
            .position(|candidate| candidate.id == visitor)
    }

    fn send(&mut self, visitor: VisitorId, line: String) {
        if let Some(index) = self.index(visitor) {
            self.visitors[index].spoke = true;
        }
        self.outgoing.push((visitor, line));
    }

    /// Turns a visitor away; a rejoin it had begun fails.
    fn dismiss(&mut self, session: &mut HostSession, index: usize) {
        let visitor = self.visitors.remove(index);
        if let Stage::Restoring { seat, .. } = visitor.stage {
            session.rejoin_failed(seat);
        }
        self.events.push(DeskEvent::Done {
            visitor: visitor.id,
        });
    }
}

#[cfg(test)]
mod tests;
