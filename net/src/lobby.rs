//! The lobby before Go: greeting and naming joining clients, showing every
//! machine who sits where, dropping members that fall silent, and running
//! start attempts. Like the session core, it performs no I/O and reads no
//! clock; callers pass received lines and a monotonic `now`, and collect
//! lines to send. Which member sits in which seat is the caller's policy:
//! the lobby only carries it.

use crate::host::HostSession;
use crate::message::{GoneReason, JoinMessage, LobbyMessage, RoomSeat, SeatHolder, same_build};
use crate::name::PlayerName;
use crate::snapshot::Assembler;
use crate::start::{StartBarrier, StartContent, StartFailed};
use crate::token::InstallId;
use crate::token::Token;
use crate::{GREETING_TIMEOUT, HEARTBEAT_INTERVAL, MAX_JOIN_LINE_BYTES, SILENCE_TIMEOUT};
use oxide_sim::{PlayerId, Scenario, Tick};
use std::time::Duration;

/// A connection the host's lobby knows, numbered in arrival order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemberId(u32);

/// Why a member left the lobby.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveReason {
    /// The transport reported the connection closed.
    Closed,
    /// Its Hello named another build.
    OtherBuild,
    /// It did not introduce itself within [`GREETING_TIMEOUT`].
    Unintroduced,
    /// Nothing arrived from it for [`SILENCE_TIMEOUT`].
    Silent,
    /// It sent a line that failed to decode or broke the protocol.
    Protocol,
    /// It failed a start: never ready, or a different world at tick zero.
    FailedStart,
}

/// Something the host's caller must act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LobbyEvent {
    /// A member introduced itself: seat it, or refuse it.
    Joined {
        /// The member.
        member: MemberId,
        /// Its name.
        name: PlayerName,
        /// Its installation.
        install: InstallId,
    },
    /// A member is gone: free its seat and close its connection.
    Left {
        /// The member.
        member: MemberId,
        /// Why it left.
        reason: LeaveReason,
    },
    /// A connection asked after a running match the lobby does not have,
    /// and was told so: finish its connection after sending.
    Turned {
        /// The connection.
        member: MemberId,
    },
}

#[derive(Debug)]
enum Stage {
    /// The host's Hello is out; waiting for the member's.
    Greeting,
    /// The builds match; waiting for the member's Introduce.
    Introducing,
    /// Named and in the room, from an installation.
    Joined(PlayerName, InstallId),
}

#[derive(Debug)]
struct Member {
    id: MemberId,
    stage: Stage,
    connected: Duration,
    heard: Duration,
    sent: Duration,
    /// Whether a line was queued for the member since the last poll.
    spoke: bool,
    /// Whether the map went out.
    mapped: bool,
    /// The last room the member was sent.
    shown: Option<(Vec<RoomSeat>, PlayerId)>,
}

#[derive(Debug)]
struct Starting {
    barrier: StartBarrier,
    seating: Vec<(MemberId, PlayerId)>,
    /// The first failure, once one happened.
    failed: Option<StartFailed>,
    /// Every seat whose client failed it, in order.
    culprits: Vec<PlayerId>,
    /// Introductions that completed mid-start, released if it fails.
    held: Vec<LobbyEvent>,
}

impl Starting {
    fn fail(&mut self, failed: StartFailed) {
        self.failed.get_or_insert(failed);
        if let StartFailed::Mismatch { seat } | StartFailed::Protocol { seat } = failed
            && !self.culprits.contains(&seat)
        {
            self.culprits.push(seat);
        }
    }
}

/// The host's half of the lobby.
///
/// Each frame the host feeds received lines to [`Gathering::receive`],
/// seats or refuses members that [`Gathering::poll`] reports joined, calls
/// [`Gathering::publish`] with the room, and sends
/// [`Gathering::take_outgoing`]. [`Gathering::start`] freezes the seating;
/// [`Gathering::poll_start`] then yields the session, or a failure that
/// drops only the members who caused it and returns the rest to the room.
#[derive(Debug)]
pub struct Gathering {
    commit: String,
    /// The encoded Map line every member receives once.
    map: String,
    members: Vec<Member>,
    next_id: u32,
    attempt: u32,
    starting: Option<Starting>,
    events: Vec<LobbyEvent>,
    outgoing: Vec<(MemberId, String)>,
}

impl Gathering {
    /// A lobby for builds of `commit` playing `map`.
    pub fn new(commit: &str, map: &Scenario) -> Self {
        let map = LobbyMessage::Map {
            scenario: Box::new(map.clone()),
        };
        Self {
            commit: commit.to_owned(),
            map: map.encode(),
            members: Vec::new(),
            next_id: 0,
            attempt: 0,
            starting: None,
            events: Vec::new(),
            outgoing: Vec::new(),
        }
    }

    /// Registers a new connection and queues the host's Hello, which goes
    /// first so a client of another build can say why it cannot join.
    pub fn connected(&mut self, now: Duration) -> MemberId {
        let id = MemberId(self.next_id);
        self.next_id += 1;
        self.members.push(Member {
            id,
            stage: Stage::Greeting,
            connected: now,
            heard: now,
            sent: now,
            spoke: false,
            mapped: false,
            shown: None,
        });
        self.send(id, LobbyMessage::hello(&self.commit).encode());
        id
    }

    /// Handles one line received from `member`. Lines from a member the
    /// lobby already dropped are ignored.
    pub fn receive(&mut self, member: MemberId, line: &str, now: Duration) {
        let Some(index) = self.index(member) else {
            return;
        };
        self.members[index].heard = now;
        if line.len() > MAX_JOIN_LINE_BYTES {
            self.drop_member(index, LeaveReason::Protocol);
            return;
        }
        if let Some(starting) = &mut self.starting
            && let Some(&(_, seat)) = starting.seating.iter().find(|(id, _)| *id == member)
        {
            if let Err(failed) = starting.barrier.receive(seat, line) {
                starting.fail(failed);
            }
            return;
        }
        let (greeting, introducing) = match self.members[index].stage {
            Stage::Greeting => (true, false),
            Stage::Introducing => (false, true),
            Stage::Joined(..) => (false, false),
        };
        match JoinMessage::decode(line) {
            Ok(JoinMessage::Hello {
                protocol,
                sim,
                commit,
            }) if greeting => {
                if same_build(protocol, sim, &commit, &self.commit) {
                    self.members[index].stage = Stage::Introducing;
                } else {
                    self.drop_member(index, LeaveReason::OtherBuild);
                }
            }
            Ok(JoinMessage::Probe { .. } | JoinMessage::Rejoin { .. }) if introducing => {
                let gone = LobbyMessage::Gone {
                    reason: GoneReason::NoMatch,
                };
                self.forget(member, &gone);
                self.events.push(LobbyEvent::Turned { member });
            }
            Ok(JoinMessage::Introduce { name, install }) if introducing => {
                self.members[index].stage = Stage::Joined(name.clone(), install);
                let joined = LobbyEvent::Joined {
                    member,
                    name,
                    install,
                };
                match &mut self.starting {
                    Some(starting) => starting.held.push(joined),
                    None => self.events.push(joined),
                }
            }
            // A Ready can outlive its abandoned start.
            Ok(JoinMessage::Heartbeat) if !greeting => {}
            Ok(JoinMessage::Ready { .. }) if !greeting && !introducing => {}
            _ => self.drop_member(index, LeaveReason::Protocol),
        }
    }

    /// The transport reported `member`'s connection closed.
    pub fn disconnected(&mut self, member: MemberId) {
        if let Some(index) = self.index(member) {
            self.drop_member(index, LeaveReason::Closed);
        }
    }

    /// Refuses a joined member, every seat being taken: queues `Full` and
    /// forgets it. The caller finishes its connection after sending.
    pub fn refuse(&mut self, member: MemberId) {
        self.forget(member, &LobbyMessage::Full);
    }

    /// Removes a member at the host's request: queues `Removed` and forgets
    /// it, failing a running start that seated it. The caller finishes its
    /// connection after sending.
    pub fn remove(&mut self, member: MemberId) {
        self.forget(member, &LobbyMessage::Removed);
    }

    fn forget(&mut self, member: MemberId, farewell: &LobbyMessage) {
        if let Some(index) = self.index(member) {
            self.outgoing.push((member, farewell.encode()));
            self.members.remove(index);
            self.leave_start(member);
        }
    }

    /// A joined member's name.
    pub fn name(&self, member: MemberId) -> Option<&PlayerName> {
        self.members
            .iter()
            .find(|candidate| candidate.id == member)
            .and_then(|member| match &member.stage {
                Stage::Joined(name, _) => Some(name),
                Stage::Greeting | Stage::Introducing => None,
            })
    }

    /// A joined member's installation.
    pub fn install(&self, member: MemberId) -> Option<InstallId> {
        self.members
            .iter()
            .find(|candidate| candidate.id == member)
            .and_then(|member| match member.stage {
                Stage::Joined(_, install) => Some(install),
                Stage::Greeting | Stage::Introducing => None,
            })
    }

    /// Shows the room to every joined member `seat_of` seats: the map the
    /// first time, then the room whenever its view changed. Nothing goes out
    /// while a start runs, since a room tells a guest its start is off.
    pub fn publish(&mut self, seats: &[RoomSeat], seat_of: impl Fn(MemberId) -> Option<PlayerId>) {
        if self.starting.is_some() {
            return;
        }
        let mut lines = Vec::new();
        for member in &mut self.members {
            if !matches!(member.stage, Stage::Joined(..)) {
                continue;
            }
            let Some(seat) = seat_of(member.id) else {
                continue;
            };
            if !member.mapped {
                member.mapped = true;
                lines.push((member.id, self.map.clone()));
            }
            if member
                .shown
                .as_ref()
                .is_none_or(|(shown, at)| *at != seat || shown.as_slice() != seats)
            {
                member.shown = Some((seats.to_vec(), seat));
                let room = LobbyMessage::Room {
                    seats: seats.to_vec(),
                    seat,
                };
                lines.push((member.id, room.encode()));
            }
        }
        for (member, line) in lines {
            self.send(member, line);
        }
    }

    /// Drops members that never introduced themselves or fell silent,
    /// heartbeats the rest, and returns what happened since the last poll.
    pub fn poll(&mut self, now: Duration) -> Vec<LobbyEvent> {
        let mut index = 0;
        while index < self.members.len() {
            let member = &mut self.members[index];
            let expired = match member.stage {
                Stage::Greeting | Stage::Introducing => {
                    now.saturating_sub(member.connected) >= GREETING_TIMEOUT
                }
                // A seated member may go quiet while it builds or restores
                // the start; the start's own deadline covers it.
                Stage::Joined(..) => {
                    now.saturating_sub(member.heard) >= SILENCE_TIMEOUT
                        && !self.starting.as_ref().is_some_and(|starting| {
                            starting
                                .seating
                                .iter()
                                .any(|&(seated, _)| seated == member.id)
                        })
                }
            };
            if expired {
                let reason = if matches!(member.stage, Stage::Joined(..)) {
                    LeaveReason::Silent
                } else {
                    LeaveReason::Unintroduced
                };
                self.drop_member(index, reason);
                continue;
            }
            if std::mem::take(&mut member.spoke) {
                member.sent = now;
            } else if matches!(member.stage, Stage::Joined(..))
                && now.saturating_sub(member.sent) >= HEARTBEAT_INTERVAL
            {
                member.sent = now;
                self.outgoing
                    .push((member.id, LobbyMessage::Heartbeat.encode()));
            }
            index += 1;
        }
        std::mem::take(&mut self.events)
    }

    /// Lines to send, each addressed to a member.
    pub fn take_outgoing(&mut self) -> Vec<(MemberId, String)> {
        std::mem::take(&mut self.outgoing)
    }

    /// Starts `content` with the host on `host` and each joined member of
    /// `seating` on its seat; together they must hold exactly the
    /// scenario's human seats. `host_hash` is the host's `State::hash` at
    /// the starting tick. Introductions that complete during the start wait
    /// for its outcome.
    pub fn start(
        &mut self,
        content: StartContent<'_>,
        host: PlayerId,
        seating: &[(MemberId, PlayerId)],
        host_hash: u64,
        now: Duration,
        mut token: impl FnMut() -> Token,
    ) {
        assert!(self.starting.is_none(), "a start is already running");
        for (member, _) in seating {
            assert!(self.name(*member).is_some(), "{member:?} has not joined");
        }
        self.attempt += 1;
        let clients: Vec<(PlayerId, Token)> =
            seating.iter().map(|&(_, seat)| (seat, token())).collect();
        let mut barrier = StartBarrier::new(content, host, &clients, host_hash, self.attempt, now);
        let lines = barrier.take_outgoing();
        let starting = Starting {
            barrier,
            seating: seating.to_vec(),
            failed: None,
            culprits: Vec::new(),
            held: Vec::new(),
        };
        for (seat, line) in lines {
            self.send(member_at(&starting.seating, seat), line);
        }
        self.starting = Some(starting);
    }

    /// The session once every seated member is ready, with Go queued for
    /// each; `None` while waiting. A failed start drops the members who
    /// caused it (reported by the next [`Gathering::poll`]), returns the
    /// rest to the room, and shows them the room again on the next publish.
    pub fn poll_start(&mut self, now: Duration) -> Result<Option<HostSession>, StartFailed> {
        let starting = self.starting.as_mut().expect("no start is running");
        let failed = match starting.failed {
            Some(failed) => failed,
            None => match starting.barrier.poll(now) {
                Ok(None) => return Ok(None),
                Ok(Some(session)) => {
                    let lines = starting.barrier.take_outgoing();
                    let starting = self.starting.take().expect("checked above");
                    for (seat, line) in lines {
                        self.send(member_at(&starting.seating, seat), line);
                    }
                    return Ok(Some(session));
                }
                Err(failed) => {
                    starting.fail(failed);
                    failed
                }
            },
        };
        let starting = self.starting.take().expect("checked above");
        let culprits = match failed {
            StartFailed::TimedOut => starting.barrier.unready(),
            StartFailed::Mismatch { .. } | StartFailed::Protocol { .. } => starting.culprits,
        };
        for seat in culprits {
            if let Some(index) = self.index(member_at(&starting.seating, seat)) {
                self.drop_member(index, LeaveReason::FailedStart);
            }
        }
        for member in &mut self.members {
            member.shown = None;
        }
        self.events.extend(starting.held);
        Err(failed)
    }

    /// Abandons a running start at the host's request: every seated member
    /// goes back to the room, shown it again on the next publish, and a
    /// late Ready for the abandoned attempt counts for nothing.
    pub fn cancel_start(&mut self) {
        let starting = self.starting.take().expect("no start is running");
        for member in &mut self.members {
            member.shown = None;
        }
        self.events.extend(starting.held);
    }

    fn index(&self, member: MemberId) -> Option<usize> {
        self.members
            .iter()
            .position(|candidate| candidate.id == member)
    }

    fn send(&mut self, member: MemberId, line: String) {
        if let Some(index) = self.index(member) {
            self.members[index].spoke = true;
        }
        self.outgoing.push((member, line));
    }

    fn drop_member(&mut self, index: usize, reason: LeaveReason) {
        let member = self.members.remove(index);
        self.leave_start(member.id);
        self.events.push(LobbyEvent::Left {
            member: member.id,
            reason,
        });
    }

    /// Fails a running start that seated `member`, who is gone, and drops
    /// the introduction it completed mid-start.
    fn leave_start(&mut self, member: MemberId) {
        if let Some(starting) = &mut self.starting {
            if let Some(&(_, seat)) = starting.seating.iter().find(|(id, _)| *id == member) {
                starting.fail(StartFailed::Protocol { seat });
            }
            starting.held.retain(
                |held| !matches!(held, LobbyEvent::Joined { member: id, .. } if *id == member),
            );
        }
    }
}

/// The member a start seated on `seat`.
fn member_at(seating: &[(MemberId, PlayerId)], seat: PlayerId) -> MemberId {
    seating
        .iter()
        .find(|(_, at)| *at == seat)
        .map(|&(member, _)| member)
        .expect("the barrier only names seated clients")
}

/// Why a guest's lobby ended without a match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuestEnd {
    /// The host runs another build.
    OtherBuild {
        /// The host's commit.
        commit: String,
    },
    /// Every seat already holds a person.
    Full,
    /// The host removed this player.
    Removed,
    /// The host's match already started.
    Underway,
    /// Nothing arrived from the host for [`SILENCE_TIMEOUT`].
    Silent,
    /// The host sent a line that failed to decode or broke the protocol.
    Protocol,
}

/// What a line from the host changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuestEvent {
    /// The map arrived; [`Guest::map`] has it.
    Map,
    /// Who sits where changed; [`Guest::room`] has it. A room that
    /// arrives during a start means the start failed.
    Room,
    /// Build this match on `seat`, then call [`Guest::ready`].
    Start {
        /// The seat this player plays.
        seat: PlayerId,
        /// The match every machine builds.
        scenario: Box<Scenario>,
    },
    /// Restore this saved match on `seat`, then call [`Guest::ready`] with
    /// the restored world's hash.
    Resume {
        /// The seat this player plays.
        seat: PlayerId,
        /// The tick the saved world is at; the session starts there.
        tick: Tick,
        /// The encoded snapshot.
        bytes: Vec<u8>,
    },
    /// The match begins; lines after this one belong to the session.
    /// `token` takes the seat back if the connection is lost.
    Go {
        /// This player's rejoin token.
        token: Token,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuestStage {
    /// The guest's Hello is out; waiting for the host's.
    Greeting,
    /// Named; waiting in the room.
    Waiting,
    /// Building a start, then waiting for Go once ready.
    Starting { attempt: u32, ready: bool },
    /// Go arrived.
    Started,
}

/// A joining client's half of the lobby.
#[derive(Debug)]
pub struct Guest {
    commit: String,
    name: PlayerName,
    install: InstallId,
    stage: GuestStage,
    heard: Duration,
    sent: Duration,
    spoke: bool,
    map: Option<Box<Scenario>>,
    room: Option<(Vec<RoomSeat>, PlayerId)>,
    /// A resumed match's snapshot arriving: the seat, its tick, and the
    /// parts so far.
    incoming: Option<(PlayerId, Tick, Assembler)>,
    outgoing: Vec<String>,
}

impl Guest {
    /// A guest of build `commit` joining as `name` from installation
    /// `install`; queues its Hello.
    pub fn new(commit: &str, name: PlayerName, install: InstallId, now: Duration) -> Self {
        Self {
            commit: commit.to_owned(),
            name,
            install,
            stage: GuestStage::Greeting,
            heard: now,
            sent: now,
            spoke: true,
            map: None,
            room: None,
            incoming: None,
            outgoing: vec![JoinMessage::hello(commit).encode()],
        }
    }

    /// The map, once it arrived.
    pub fn map(&self) -> Option<&Scenario> {
        self.map.as_deref()
    }

    /// The latest room and this player's seat in it.
    pub fn room(&self) -> Option<(&[RoomSeat], PlayerId)> {
        self.room
            .as_ref()
            .map(|(seats, seat)| (seats.as_slice(), *seat))
    }

    /// Handles one line from the host.
    pub fn receive(&mut self, line: &str, now: Duration) -> Result<Option<GuestEvent>, GuestEnd> {
        self.heard = now;
        let Ok(message) = LobbyMessage::decode(line) else {
            return Err(GuestEnd::Protocol);
        };
        match self.stage {
            GuestStage::Greeting => {
                let LobbyMessage::Hello {
                    protocol,
                    sim,
                    commit,
                } = message
                else {
                    return Err(GuestEnd::Protocol);
                };
                if !same_build(protocol, sim, &commit, &self.commit) {
                    return Err(GuestEnd::OtherBuild { commit });
                }
                self.stage = GuestStage::Waiting;
                let introduce = JoinMessage::Introduce {
                    name: self.name.clone(),
                    install: self.install,
                };
                self.send(introduce.encode());
                Ok(None)
            }
            GuestStage::Waiting | GuestStage::Starting { .. } => self.in_room(message),
            GuestStage::Started => Err(GuestEnd::Protocol),
        }
    }

    fn in_room(&mut self, message: LobbyMessage) -> Result<Option<GuestEvent>, GuestEnd> {
        match message {
            LobbyMessage::Heartbeat => Ok(None),
            LobbyMessage::Full => Err(GuestEnd::Full),
            LobbyMessage::Removed => Err(GuestEnd::Removed),
            LobbyMessage::Map { scenario }
                if self.map.is_none() && !scenario.players.is_empty() =>
            {
                self.map = Some(scenario);
                Ok(Some(GuestEvent::Map))
            }
            LobbyMessage::Room { seats, seat } if self.fits(&seats, seat) => {
                self.room = Some((seats, seat));
                self.stage = GuestStage::Waiting;
                self.incoming = None;
                Ok(Some(GuestEvent::Room))
            }
            LobbyMessage::Start {
                seat,
                scenario,
                attempt,
            } if self.room.is_some() => {
                self.stage = GuestStage::Starting {
                    attempt,
                    ready: false,
                };
                Ok(Some(GuestEvent::Start { seat, scenario }))
            }
            LobbyMessage::Resume {
                seat,
                tick,
                bytes,
                parts,
                attempt,
            } if self.room.is_some() => {
                let assembler = Assembler::new(bytes, parts).ok_or(GuestEnd::Protocol)?;
                self.stage = GuestStage::Starting {
                    attempt,
                    ready: false,
                };
                self.incoming = Some((seat, tick, assembler));
                Ok(None)
            }
            LobbyMessage::Part { data } => {
                let (_, _, assembler) = self.incoming.as_mut().ok_or(GuestEnd::Protocol)?;
                let Some(bytes) = assembler.push(&data).map_err(|_| GuestEnd::Protocol)? else {
                    return Ok(None);
                };
                let (seat, tick, _) = self.incoming.take().expect("checked above");
                Ok(Some(GuestEvent::Resume { seat, tick, bytes }))
            }
            LobbyMessage::Go { token }
                if matches!(self.stage, GuestStage::Starting { ready: true, .. }) =>
            {
                self.stage = GuestStage::Started;
                Ok(Some(GuestEvent::Go { token }))
            }
            LobbyMessage::Underway => Err(GuestEnd::Underway),
            _ => Err(GuestEnd::Protocol),
        }
    }

    /// Whether a room matches the map: a seat per map seat, this player's
    /// seat among them, holding this player.
    fn fits(&self, seats: &[RoomSeat], seat: PlayerId) -> bool {
        let Some(map) = &self.map else {
            return false;
        };
        seats.len() == map.players.len()
            && seats.get(usize::from(seat.0)).is_some_and(
                |own| matches!(&own.holder, SeatHolder::Player { name } if *name == self.name),
            )
    }

    /// Reports the started match built or restored, its world at the
    /// starting tick hashing to `hash`.
    pub fn ready(&mut self, hash: u64) {
        let GuestStage::Starting { attempt, ready } = &mut self.stage else {
            panic!("no start to be ready for");
        };
        *ready = true;
        let ready = JoinMessage::Ready {
            hash,
            attempt: *attempt,
        };
        self.send(ready.encode());
    }

    /// Ends a guest whose host fell silent and heartbeats a quiet one.
    pub fn poll(&mut self, now: Duration) -> Result<(), GuestEnd> {
        if self.stage == GuestStage::Started {
            return Ok(());
        }
        if now.saturating_sub(self.heard) >= SILENCE_TIMEOUT {
            return Err(GuestEnd::Silent);
        }
        if std::mem::take(&mut self.spoke) {
            self.sent = now;
        } else if self.stage != GuestStage::Greeting
            && now.saturating_sub(self.sent) >= HEARTBEAT_INTERVAL
        {
            self.sent = now;
            self.outgoing.push(JoinMessage::Heartbeat.encode());
        }
        Ok(())
    }

    /// Lines to send to the host.
    pub fn take_outgoing(&mut self) -> Vec<String> {
        std::mem::take(&mut self.outgoing)
    }

    fn send(&mut self, line: String) {
        self.spoke = true;
        self.outgoing.push(line);
    }
}

#[cfg(test)]
mod tests;
