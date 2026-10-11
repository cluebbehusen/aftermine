//! UDP for LAN discovery: a hosting lobby's beacon sender and a joining
//! player's listener. Both are non-blocking sockets polled from the game
//! loop, and a socket error is a state to report, never a panic: a
//! platform may refuse broadcasts outright.

use crate::discovery::{BEACON_INTERVAL, Beacon, MAX_BEACON_BYTES, Sighting, Sightings, hear};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

/// The UDP port lobbies announce themselves on.
pub const DISCOVERY_PORT: u16 = 4200;

/// Datagrams one poll reads at most, so a flood cannot stall a frame.
const READS_PER_POLL: usize = 64;

/// Sends a hosting lobby's beacon once per [`BEACON_INTERVAL`].
#[derive(Debug)]
pub struct Advertiser {
    socket: UdpSocket,
    targets: Vec<SocketAddr>,
    last: Option<Duration>,
    failing: bool,
}

impl Advertiser {
    /// Broadcasts to every machine on the local network: to the limited
    /// broadcast address and to the broadcast address of the subnet this
    /// machine routes through, taken as a /24. macOS can refuse the limited
    /// address when several interfaces are up, and std cannot read a
    /// netmask, so a larger subnet hears only the limited one.
    pub fn new() -> io::Result<Self> {
        let mut targets = vec![SocketAddr::from((Ipv4Addr::BROADCAST, DISCOVERY_PORT))];
        if let Some(local) = routed_local() {
            let [a, b, c, _] = local.octets();
            targets.push(SocketAddr::from((
                Ipv4Addr::new(a, b, c, 255),
                DISCOVERY_PORT,
            )));
        }
        Self::with_targets(targets)
    }

    /// Sends to `target` instead, such as a listener on loopback.
    pub fn with_target(target: SocketAddr) -> io::Result<Self> {
        Self::with_targets(vec![target])
    }

    fn with_targets(targets: Vec<SocketAddr>) -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_broadcast(true)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            targets,
            last: None,
            failing: false,
        })
    }

    /// Sends `beacon` if a beacon is due.
    pub fn poll(&mut self, beacon: &Beacon, now: Duration) {
        if self
            .last
            .is_some_and(|last| now.saturating_sub(last) < BEACON_INTERVAL)
        {
            return;
        }
        self.last = Some(now);
        let datagram = beacon.encode();
        let sent = self
            .targets
            .iter()
            .filter(|target| self.socket.send_to(&datagram, target).is_ok())
            .count();
        self.failing = sent == 0;
    }

    /// Whether the last beacon reached no address.
    pub fn failing(&self) -> bool {
        self.failing
    }
}

/// The private IPv4 address this machine would reach the internet from.
/// Connecting a UDP socket only picks a route; it sends nothing.
fn routed_local() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if ip.is_private() => Some(ip),
        _ => None,
    }
}

/// Listens for beacons and keeps the list of lobbies heard lately.
#[derive(Debug)]
pub struct Scanner {
    socket: UdpSocket,
    commit: String,
    sightings: Sightings,
}

impl Scanner {
    /// Listens on [`DISCOVERY_PORT`] for lobbies of build `commit`.
    pub fn bind(commit: &str) -> io::Result<Self> {
        Self::bind_to((Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT), commit)
    }

    /// Listens on `address` instead, such as an ephemeral loopback port.
    pub fn bind_to(address: impl ToSocketAddrs, commit: &str) -> io::Result<Self> {
        let socket = UdpSocket::bind(address)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            commit: commit.to_owned(),
            sightings: Sightings::default(),
        })
    }

    /// The bound address.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Reads what arrived and returns the lobbies heard lately.
    pub fn poll(&mut self, now: Duration) -> &[Sighting] {
        let mut buffer = [0; MAX_BEACON_BYTES + 1];
        for _ in 0..READS_PER_POLL {
            match self.socket.recv_from(&mut buffer) {
                Ok((length, from)) => {
                    if let Some(heard) = hear(&buffer[..length], &self.commit) {
                        self.sightings.hear(from.ip(), heard, now);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                // Some platforms report a truncated or refused datagram as
                // an error; the next one may still be good.
                Err(_) => {}
            }
        }
        self.sightings.expire(now);
        self.sightings.list()
    }
}

#[cfg(test)]
mod tests;
