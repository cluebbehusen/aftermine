//! LAN discovery: the beacon a hosting lobby broadcasts, and the list of
//! lobbies a joining player has heard from. Pure; [`crate::udp`] moves the
//! datagrams.

use crate::message::same_build;
use crate::name::PlayerName;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

/// The tag every beacon opens with, so other traffic on the port is
/// ignored.
const TAG: &str = "oxide-lan";

/// How often a hosting lobby announces itself.
pub const BEACON_INTERVAL: Duration = Duration::from_secs(1);

/// A lobby unheard for this long drops off the list.
pub const BEACON_EXPIRY: Duration = Duration::from_secs(3);

/// The longest beacon sent or read.
pub const MAX_BEACON_BYTES: usize = 512;

/// The most lobbies the list keeps.
pub const MAX_SIGHTINGS: usize = 32;

/// The longest map name a beacon carries, in characters.
const MAP_CHARS: usize = 48;

/// What a hosting lobby announces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Beacon {
    oxide: String,
    protocol: u32,
    sim: u32,
    commit: String,
    /// The host's name.
    pub host: PlayerName,
    /// The TCP port the lobby listens on.
    pub port: u16,
    /// The map's name.
    pub map: String,
    /// People in the lobby, the host included.
    pub players: u8,
    /// The map's seats: how many people the lobby could hold.
    pub seats: u8,
}

/// The fields every beacon opens with. Their shape never changes, so a
/// lobby of another build still lists, dimmed, under its host's name.
#[derive(Deserialize)]
struct Header {
    oxide: String,
    protocol: u32,
    sim: u32,
    commit: String,
    host: String,
}

/// What a datagram announced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// A lobby of this build.
    Joinable(Beacon),
    /// A lobby of another build, which cannot be joined.
    OtherBuild {
        /// The host's name, as sent.
        host: String,
        /// The host's build.
        commit: String,
    },
}

impl Beacon {
    /// This build's beacon for a lobby on `port`.
    pub fn new(
        commit: &str,
        host: PlayerName,
        port: u16,
        map: &str,
        players: usize,
        seats: usize,
    ) -> Self {
        Self {
            oxide: TAG.to_owned(),
            protocol: crate::PROTOCOL_VERSION,
            sim: aftermine_sim::SIM_VERSION,
            commit: commit.to_owned(),
            host,
            port,
            map: map.chars().take(MAP_CHARS).collect(),
            players: u8::try_from(players).unwrap_or(u8::MAX),
            seats: u8::try_from(seats).unwrap_or(u8::MAX),
        }
    }

    /// The datagram.
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("beacons always serialize")
    }
}

/// Reads a datagram as a beacon, given this build's commit; `None` for
/// anything that is not one.
pub fn hear(datagram: &[u8], commit: &str) -> Option<Heard> {
    if datagram.len() > MAX_BEACON_BYTES {
        return None;
    }
    let header: Header = serde_json::from_slice(datagram).ok()?;
    if header.oxide != TAG {
        return None;
    }
    if !same_build(header.protocol, header.sim, &header.commit, commit) {
        return Some(Heard::OtherBuild {
            host: header.host.chars().take(PlayerName::MAX_CHARS).collect(),
            commit: header.commit.chars().take(40).collect(),
        });
    }
    serde_json::from_slice(datagram).ok().map(Heard::Joinable)
}

/// One lobby the list heard from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighting {
    /// Where it was heard from.
    pub source: IpAddr,
    /// What it last announced.
    pub heard: Heard,
    last: Duration,
}

impl Sighting {
    /// The address to join, for a lobby of this build.
    pub fn address(&self) -> Option<SocketAddr> {
        match &self.heard {
            Heard::Joinable(beacon) => Some(SocketAddr::new(self.source, beacon.port)),
            Heard::OtherBuild { .. } => None,
        }
    }

    fn key(&self) -> (IpAddr, u16) {
        match &self.heard {
            Heard::Joinable(beacon) => (self.source, beacon.port),
            Heard::OtherBuild { .. } => (self.source, 0),
        }
    }

    fn host(&self) -> &str {
        match &self.heard {
            Heard::Joinable(beacon) => beacon.host.as_str(),
            Heard::OtherBuild { host, .. } => host,
        }
    }
}

/// The lobbies heard lately, sorted by host and address.
#[derive(Debug, Default)]
pub struct Sightings {
    list: Vec<Sighting>,
}

impl Sightings {
    /// Records what `source` announced at `now`.
    pub fn hear(&mut self, source: IpAddr, heard: Heard, now: Duration) {
        let sighting = Sighting {
            source,
            heard,
            last: now,
        };
        let key = sighting.key();
        if let Some(known) = self.list.iter_mut().find(|known| known.key() == key) {
            *known = sighting;
        } else if self.list.len() < MAX_SIGHTINGS {
            self.list.push(sighting);
        }
        self.list
            .sort_by(|a, b| a.host().cmp(b.host()).then_with(|| a.key().cmp(&b.key())));
    }

    /// Drops lobbies unheard for [`BEACON_EXPIRY`].
    pub fn expire(&mut self, now: Duration) {
        self.list
            .retain(|sighting| now.saturating_sub(sighting.last) < BEACON_EXPIRY);
    }

    /// The lobbies, sorted by host and address.
    pub fn list(&self) -> &[Sighting] {
        &self.list
    }
}

#[cfg(test)]
mod tests;
