//! The ticket a client keeps while it plays a LAN match, so that after the
//! app is closed or killed, Home can offer to take the seat back. Home only
//! offers it once the host confirms the seat still waits.

use oxide_net::{Connection, Token, Visit, VisitEnd, VisitEvent};
use serde::{Deserialize, Serialize};
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A ticket the host has not been heard from in this long is dropped.
const EXPIRY: Duration = Duration::from_hours(1);

/// How often a playing client refreshes its ticket's last contact.
const REFRESH: Duration = Duration::from_secs(5);

/// How long a probe waits for the host.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Where a client's seat lives and what proves it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ticket {
    /// The host's address, as joined.
    pub(crate) address: String,
    /// The host's name.
    pub(crate) host: String,
    /// The match's map.
    pub(crate) map: String,
    /// The seat.
    pub(crate) seat: u8,
    /// The rejoin token Go gave.
    pub(crate) token: Token,
    /// The build that joined; only it can rejoin.
    pub(crate) build: String,
    /// When the client last heard its host, in seconds since the epoch.
    pub(crate) contact: u64,
}

impl Ticket {
    /// Whether the host went unheard so long the seat is surely gone.
    pub(crate) fn expired(&self, now: u64) -> bool {
        now.saturating_sub(self.contact) >= EXPIRY.as_secs()
    }

    /// Home's row for it.
    pub(crate) fn label(&self) -> String {
        format!("Rejoin {}'s match on {}", self.host, self.map)
    }
}

/// Seconds since the epoch.
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Reads the ticket at `path`, if one is there and readable.
pub(crate) fn read(path: &Path) -> Option<Ticket> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Keeps the ticket file current from the frame loop. A writer thread does
/// the disk work, so a slow disk never stalls a frame or the save worker.
pub(crate) struct Keeper {
    writer: Option<Sender<Option<Ticket>>>,
    /// The ticket last written this session, and when.
    kept: Option<(Ticket, Duration)>,
}

impl Keeper {
    /// Keeps the ticket at `path`; `None` keeps nothing.
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        let writer = path.map(|path| {
            let (writer, wanted) = mpsc::channel::<Option<Ticket>>();
            thread::spawn(move || {
                for ticket in wanted {
                    let _ = match ticket {
                        Some(ticket) => chassis::fsx::write_atomic(&path, |file| {
                            serde_json::to_writer(file, &ticket).map_err(std::io::Error::other)
                        }),
                        None => match std::fs::remove_file(&path) {
                            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                                Err(error)
                            }
                            _ => Ok(()),
                        },
                    };
                }
            });
            writer
        });
        Self { writer, kept: None }
    }

    /// Records the seat being played, refreshing its last contact every few
    /// seconds.
    pub(crate) fn keep(&mut self, mut ticket: Ticket, now: Duration) {
        let fresh = self.kept.as_ref().is_some_and(|(kept, at)| {
            kept.token == ticket.token
                && kept.address == ticket.address
                && now.saturating_sub(*at) < REFRESH
        });
        if fresh {
            return;
        }
        ticket.contact = self::now();
        self.send(Some(ticket.clone()));
        self.kept = Some((ticket, now));
    }

    /// Drops the ticket this session kept: the seat is no longer this
    /// player's to take back.
    pub(crate) fn clear(&mut self) {
        if self.kept.take().is_some() {
            self.send(None);
        }
    }

    fn send(&self, ticket: Option<Ticket>) {
        if let Some(writer) = &self.writer {
            let _ = writer.send(ticket);
        }
    }
}

/// What asking the ticket's host found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Probed {
    /// No ticket to ask about.
    Missing,
    /// The seat waits: offer to rejoin.
    Waiting(Ticket),
    /// The seat or the match is gone; the ticket was deleted.
    Gone,
    /// The host did not answer; ask again later.
    Silent,
}

/// Asks the host on the ticket at `path` whether the seat still waits, off
/// the frame loop. A ticket that expired, came from a build other than
/// `build`, or names a seat that is gone is deleted.
pub(crate) fn probe(path: PathBuf, build: String) -> JoinHandle<Probed> {
    thread::spawn(move || {
        let Some(ticket) = read(&path) else {
            return Probed::Missing;
        };
        if ticket.expired(now()) || ticket.build != build {
            let _ = std::fs::remove_file(&path);
            return Probed::Gone;
        }
        let probed = ask(&ticket);
        if probed == Probed::Gone {
            let _ = std::fs::remove_file(&path);
        }
        probed
    })
}

fn ask(ticket: &Ticket) -> Probed {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let Some(target) = ticket
        .address
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
    else {
        return Probed::Silent;
    };
    let Ok(connection) = Connection::connect_timeout(&target, PROBE_TIMEOUT) else {
        return Probed::Silent;
    };
    let started = Instant::now();
    let elapsed = || started.elapsed();
    let mut visit = Visit::probe(&ticket.build, ticket.token, elapsed());
    while Instant::now() < deadline {
        for line in visit.take_outgoing() {
            connection.send(&line);
        }
        loop {
            match connection.try_recv() {
                Ok(Some(line)) => match visit.receive(&line, elapsed()) {
                    Ok(Some(VisitEvent::Waiting { .. })) => return Probed::Waiting(ticket.clone()),
                    Ok(_) => {}
                    Err(VisitEnd::Gone(_) | VisitEnd::OtherBuild { .. }) => return Probed::Gone,
                    Err(VisitEnd::Silent | VisitEnd::Protocol) => return Probed::Silent,
                },
                Ok(None) => break,
                Err(_) => return Probed::Silent,
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    Probed::Silent
}

#[cfg(test)]
mod tests;
