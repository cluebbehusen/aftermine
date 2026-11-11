//! The ticket a client keeps while it plays a LAN match, so that if the app
//! stops without leaving the match, Home can offer to take the seat back. Home only
//! offers it once the host confirms the seat still waits.

use aftermine_net::{Connection, Token, Visit, VisitEnd, VisitEvent};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
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
/// Every write and deletion goes through that one thread, so none can
/// interleave with another.
pub(crate) struct Keeper {
    writer: Option<Sender<Write>>,
    /// Writes sent to the thread and not yet done.
    unwritten: Arc<AtomicUsize>,
    /// The ticket last written this session, and when.
    kept: Option<(Ticket, Duration)>,
}

impl Keeper {
    /// Keeps the ticket at `path`; `None` keeps nothing.
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        let unwritten = Arc::new(AtomicUsize::new(0));
        let writer = path.map(|path| {
            let (writer, wanted) = mpsc::channel::<Write>();
            let unwritten = Arc::clone(&unwritten);
            thread::spawn(move || {
                for write in wanted {
                    let _ = apply(&path, write);
                    unwritten.fetch_sub(1, Ordering::Release);
                }
            });
            writer
        });
        Self {
            writer,
            unwritten,
            kept: None,
        }
    }

    /// Whether every write sent so far is done, or failed. The process
    /// ending stops the writer, so a quit waits on this, briefly.
    pub(crate) fn settled(&self) -> bool {
        self.unwritten.load(Ordering::Acquire) == 0
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
        self.send(Write::Keep(ticket.clone()));
        self.kept = Some((ticket, now));
    }

    /// Drops the ticket this session kept: the seat is no longer this
    /// player's to take back.
    pub(crate) fn clear(&mut self) {
        if self.kept.take().is_some() {
            self.send(Write::Clear);
        }
    }

    /// Drops `ticket` if it is still the one on disk: a probe found its seat
    /// gone, or the ticket stale.
    pub(crate) fn forget(&self, ticket: &Ticket) {
        self.send(Write::Forget(ticket.token));
    }

    fn send(&self, write: Write) {
        if let Some(writer) = &self.writer {
            self.unwritten.fetch_add(1, Ordering::AcqRel);
            if writer.send(write).is_err() {
                self.unwritten.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }
}

/// What the keeper's thread does to the ticket file.
enum Write {
    Keep(Ticket),
    Clear,
    /// Deletes the file if it still holds this token.
    Forget(Token),
}

/// Does `write` to the ticket file at `path`.
fn apply(path: &Path, write: Write) -> std::io::Result<()> {
    match write {
        Write::Keep(ticket) => chassis::fsx::write_atomic(path, |file| {
            serde_json::to_writer(file, &ticket).map_err(std::io::Error::other)
        }),
        Write::Clear => remove(path),
        // Another match's ticket may have replaced it since.
        Write::Forget(token) if read(path).is_some_and(|kept| kept.token == token) => remove(path),
        Write::Forget(_) => Ok(()),
    }
}

fn remove(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// What asking the ticket's host found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Probed {
    /// No ticket to ask about.
    Missing,
    /// The seat waits: offer to rejoin.
    Waiting(Ticket),
    /// The seat or the match is gone, or the ticket is stale: forget it.
    Gone(Ticket),
    /// The host did not answer; ask again later.
    Silent,
}

/// Asks the host on the ticket at `path` whether the seat still waits, off
/// the frame loop. A ticket that expired, came from a build other than
/// `build`, or names a seat that is gone comes back as gone, for the
/// [`Keeper`] to forget.
pub(crate) fn probe(path: PathBuf, build: String) -> JoinHandle<Probed> {
    thread::spawn(move || {
        let Some(ticket) = read(&path) else {
            return Probed::Missing;
        };
        if ticket.expired(now()) || ticket.build != build {
            return Probed::Gone(ticket);
        }
        ask(&ticket)
    })
}

fn ask(ticket: &Ticket) -> Probed {
    let Ok(connection) = Connection::connect_any(ticket.address.as_str(), PROBE_TIMEOUT) else {
        return Probed::Silent;
    };
    let deadline = Instant::now() + PROBE_TIMEOUT;
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
                    Ok(Some(VisitEvent::Waiting)) => return Probed::Waiting(ticket.clone()),
                    Ok(_) => {}
                    Err(VisitEnd::Gone(_) | VisitEnd::OtherBuild { .. }) => {
                        return Probed::Gone(ticket.clone());
                    }
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
