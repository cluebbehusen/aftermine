//! JSON-lines wire messages between a host and its clients. A connection
//! speaks [`JoinMessage`] and [`LobbyMessage`] until Go, then
//! [`ClientMessage`] and [`HostMessage`].

use crate::PROTOCOL_VERSION;
use crate::name::PlayerName;
use crate::token::{InstallId, Token};
use oxide_sim::scenario::{BotConfig, BotDifficulty, BotStance};
use oxide_sim::{Command, PlayerCommand, PlayerId, SIM_VERSION, Scenario, Tick};
use serde::{Deserialize, Serialize};

/// A line a joining client sends before the match starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum JoinMessage {
    /// The client's wire protocol, sim and build commit. Its shape never
    /// changes, so mismatched builds can still read each other's Hello.
    Hello {
        /// The client's [`PROTOCOL_VERSION`].
        protocol: u32,
        /// The client's [`SIM_VERSION`].
        sim: u32,
        /// The commit the client was built from.
        commit: String,
    },
    /// The player's name, sent once, after a host Hello that matched.
    Introduce {
        /// What the lobby shows and the seat is renamed to.
        name: PlayerName,
        /// The joining installation, for the host alone: it is never
        /// relayed to other members.
        install: InstallId,
    },
    /// The client built the match for start `attempt` and its world at
    /// tick zero hashes to `hash`.
    Ready {
        /// `State::hash` at tick zero.
        hash: u64,
        /// The start this answers, so a late Ready cannot count toward a
        /// later start.
        attempt: u32,
    },
    /// Sent when the client has sent nothing else recently.
    Heartbeat,
    /// Asks a host whether `token` still names a seat in its running match,
    /// in place of an introduction.
    Probe {
        /// The token the host gave at Go.
        token: Token,
    },
    /// Asks to take back the seat `token` names in the host's running match,
    /// in place of an introduction.
    Rejoin {
        /// The token the host gave at Go.
        token: Token,
    },
}

/// A line the host sends to a joining client before the match starts, or
/// to a client rejoining its running match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum LobbyMessage {
    /// The host's wire protocol, sim and build commit, in the same fixed
    /// shape as the client's Hello.
    Hello {
        /// The host's [`PROTOCOL_VERSION`].
        protocol: u32,
        /// The host's [`SIM_VERSION`].
        sim: u32,
        /// The commit the host was built from.
        commit: String,
    },
    /// The map the lobby plays, sent once after the host accepts the
    /// client's Introduce. The map stays fixed while the lobby is open.
    Map {
        /// The map as authored; [`LobbyMessage::Room`] says who plays it.
        scenario: Box<Scenario>,
    },
    /// Who sits where, sent whenever it changes for this client.
    Room {
        /// One entry per seat of the map, in seat order.
        seats: Vec<RoomSeat>,
        /// The receiving client's seat.
        seat: PlayerId,
    },
    /// Refused: every seat already holds a person.
    Full,
    /// The host removed this client from the lobby.
    Removed,
    /// Sent when the host has sent nothing else recently.
    Heartbeat,
    /// The frozen roster's match: the client's seat and the scenario every
    /// machine builds.
    Start {
        /// The seat this client plays.
        seat: PlayerId,
        /// The match every machine builds.
        scenario: Box<Scenario>,
        /// Which start this is; the client's Ready repeats it.
        attempt: u32,
    },
    /// Every client is ready; the match begins. `token` lets this client
    /// take its seat back if it loses its connection.
    Go {
        /// The client's rejoin token.
        token: Token,
    },
    /// The probed token names a seat the running match is waiting for.
    Waiting {
        /// The host's name.
        host: PlayerName,
        /// The match's map.
        map: String,
    },
    /// The token names no seat to take back; the connection closes next.
    Gone {
        /// Why.
        reason: GoneReason,
    },
    /// The match already started, so a new player cannot join; the
    /// connection closes next.
    Underway,
    /// The match as it stands for a rejoining client, in `parts` lines of
    /// [`LobbyMessage::Part`] that follow.
    Resume {
        /// The seat the client takes back.
        seat: PlayerId,
        /// The world tick the snapshot holds; batches resume from it.
        tick: Tick,
        /// The snapshot's length in bytes.
        bytes: u64,
        /// How many parts carry it.
        parts: u32,
        /// Which rejoin this is; the client's Ready repeats it.
        attempt: u32,
    },
    /// The next piece of a [`LobbyMessage::Resume`] snapshot, as hex.
    Part {
        /// The bytes, two hex digits each.
        data: String,
    },
}

/// Why a token names no seat to take back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoneReason {
    /// The match ended, or is not the one the token came from.
    Over,
    /// The seat was dropped, defeated, or handed to a bot.
    SeatTaken,
    /// The host is not running a match.
    NoMatch,
    /// The match is too large to send.
    TooLarge,
}

/// One seat as the lobby shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomSeat {
    /// Who plays the seat.
    pub holder: SeatHolder,
    /// The seat's team; `None` stands alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<u8>,
}

/// Who plays a seat in the lobby.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SeatHolder {
    /// The hosting player.
    Host {
        /// The host's name.
        name: PlayerName,
    },
    /// A joined player.
    Player {
        /// The player's name.
        name: PlayerName,
    },
    /// A bot.
    Bot {
        /// How well it plays.
        difficulty: BotDifficulty,
        /// Its strategic posture.
        stance: BotStance,
    },
    /// Waiting for a player.
    Open,
    /// Waiting for the player who held the seat in a saved match.
    Reserved {
        /// That player's name.
        name: PlayerName,
    },
}

impl JoinMessage {
    /// This build's Hello.
    pub fn hello(commit: &str) -> Self {
        Self::Hello {
            protocol: PROTOCOL_VERSION,
            sim: SIM_VERSION,
            commit: commit.to_owned(),
        }
    }
}

impl LobbyMessage {
    /// This build's Hello.
    pub fn hello(commit: &str) -> Self {
        Self::Hello {
            protocol: PROTOCOL_VERSION,
            sim: SIM_VERSION,
            commit: commit.to_owned(),
        }
    }
}

/// Whether a peer's Hello matches this build: the same protocol and sim
/// versions and, before launch, the same commit.
pub fn same_build(protocol: u32, sim: u32, peer_commit: &str, commit: &str) -> bool {
    protocol == PROTOCOL_VERSION && sim == SIM_VERSION && peer_commit == commit
}

/// A line a client sends to the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMessage {
    /// An order for the client's bound seat. The host attributes the seat
    /// from the connection, so a client cannot name another seat.
    Command {
        /// The order.
        command: Command,
    },
    /// The client executed the batch that left its world at `tick`.
    Ack {
        /// The world tick after the executed batch.
        tick: Tick,
        /// `State::hash` at `tick`, present exactly when
        /// [`crate::reports_hash`] holds for `tick`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hash: Option<u64>,
    },
    /// Sent when the client has sent nothing else recently.
    Heartbeat,
    /// The client is leaving, surrendering its seat.
    Leave,
}

/// A line the host sends to a client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostMessage {
    /// The complete ordered commands every machine executes on `tick`.
    Batch {
        /// The world tick the batch executes on.
        tick: Tick,
        /// Human commands in sealed order, then bot commands.
        commands: Vec<PlayerCommand>,
    },
    /// Sent when the host has sent nothing else recently.
    Heartbeat,
    /// A client's hash report disagreed with the host at `tick`; the
    /// session has halted.
    Desync {
        /// The report tick that disagreed.
        tick: Tick,
    },
    /// The seats not keeping up, and whether the host paused, replacing the
    /// last such line. Sent whenever either changes.
    Seats {
        /// Every seat still playing that is not keeping up.
        seats: Vec<SeatNotice>,
        /// The host paused the match.
        paused: bool,
    },
    /// The host took this client's seat out of the match; the connection
    /// closes next.
    Released {
        /// Why.
        reason: Release,
    },
    /// The host left before the match ended; the connection closes next.
    Ended,
    /// The host handed `seat` to a bot.
    Replaced {
        /// The seat.
        seat: PlayerId,
        /// The bot now playing it.
        config: BotConfig,
    },
}

/// A seat that is not keeping up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeatNotice {
    /// The seat.
    pub seat: PlayerId,
    /// How it is doing.
    pub state: SeatState,
}

/// How a seat that is not keeping up is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SeatState {
    /// The host heard nothing from it lately; play continues for now.
    Quiet,
    /// The match is waiting for its player.
    Away {
        /// How long the match has waited, as of the line.
        waited_ms: u64,
    },
}

/// Why the host took a client's seat out of the match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Release {
    /// The host dropped the seat, which surrendered.
    Dropped,
    /// The host handed the seat to a bot.
    Replaced,
}

macro_rules! wire_lines {
    ($($message:ty),+) => {$(
        impl $message {
            /// One wire line, without the trailing newline.
            pub fn encode(&self) -> String {
                serde_json::to_string(self).expect("wire messages always serialize")
            }

            /// Parses one wire line.
            pub fn decode(line: &str) -> serde_json::Result<Self> {
                serde_json::from_str(line)
            }
        }
    )+};
}

wire_lines!(ClientMessage, HostMessage, JoinMessage, LobbyMessage);

#[cfg(test)]
mod tests;
