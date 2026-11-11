//! Writes the settings file off the frame thread.

use super::Config;
use crate::platform::{PLATFORM, Platform};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

/// What the writer's thread shares with the frame loop.
#[derive(Default)]
struct Slot {
    /// The newest settings not yet taken up for writing.
    waiting: Option<String>,
    /// Whether the thread is writing now.
    writing: bool,
    /// The latest failed write, until the frame loop reports it.
    failure: Option<std::io::Error>,
    /// The writer is gone: the thread ends once nothing waits.
    closed: bool,
}

#[derive(Default)]
struct Shared {
    slot: Mutex<Slot>,
    changed: Condvar,
}

impl Shared {
    fn slot(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

enum Target {
    /// Apple TV keeps the settings in user defaults, which take them at once.
    Defaults,
    /// A thread writes the settings file. A write syncs the file and its
    /// folder, several milliseconds that a held stick on a Settings row would
    /// otherwise spend from every few frames.
    Thread(Arc<Shared>),
    /// Nowhere to keep them, as on a CI machine without a home.
    Nowhere,
}

/// Keeps this device's copy of the settings current. Saves are latest-wins:
/// one handed over while another is being written replaces any still
/// waiting, so the file only moves forward to the newest settings.
pub(crate) struct Writer {
    target: Target,
}

impl Writer {
    /// The writer for this device's settings.
    pub(crate) fn open() -> Self {
        if PLATFORM == Platform::Tv {
            Self {
                target: Target::Defaults,
            }
        } else {
            Self::new(super::config_path())
        }
    }

    /// Writes the file at `path` atomically; `None` keeps nothing.
    fn new(path: Option<PathBuf>) -> Self {
        let Some(path) = path else {
            return Self {
                target: Target::Nowhere,
            };
        };
        Self::spawn(move |text| {
            chassis::fsx::write_atomic(&path, |writer| writer.write_all(text.as_bytes()))
        })
    }

    /// Hands every write to `write` on a thread of its own.
    fn spawn(mut write: impl FnMut(&str) -> std::io::Result<()> + Send + 'static) -> Self {
        let shared = Arc::new(Shared::default());
        let thread = Arc::clone(&shared);
        std::thread::spawn(move || {
            loop {
                let text = {
                    let mut slot = thread.slot();
                    loop {
                        if let Some(text) = slot.waiting.take() {
                            slot.writing = true;
                            break text;
                        }
                        if slot.closed {
                            return;
                        }
                        slot = thread
                            .changed
                            .wait(slot)
                            .unwrap_or_else(PoisonError::into_inner);
                    }
                };
                // Leaving the app mid-write must not cut the file short.
                let background = aftermine_apple_bridge::application::BackgroundTask::begin();
                let written = write(&text);
                drop(background);
                let mut slot = thread.slot();
                slot.writing = false;
                if let Err(error) = written {
                    slot.failure = Some(error);
                }
                thread.changed.notify_all();
            }
        });
        Self {
            target: Target::Thread(shared),
        }
    }

    /// Keeps `config`, in the background where it goes to a file.
    pub(crate) fn save(&self, config: &Config) {
        match &self.target {
            Target::Defaults => {
                aftermine_apple_bridge::defaults::set(super::DEFAULTS_KEY, &config.text());
            }
            Target::Thread(shared) => {
                shared.slot().waiting = Some(config.text());
                shared.changed.notify_all();
            }
            Target::Nowhere => {}
        }
    }

    /// The latest write that failed since the last call, for the player to
    /// hear about once.
    pub(crate) fn failure(&self) -> Option<std::io::Error> {
        match &self.target {
            Target::Thread(shared) => shared.slot().failure.take(),
            Target::Defaults | Target::Nowhere => None,
        }
    }

    /// Waits until every save handed over so far is on disk, or failed. A
    /// quit calls this: ending the process would stop the thread mid-way.
    pub(crate) fn flush(&self) {
        if let Target::Thread(shared) = &self.target {
            let mut slot = shared.slot();
            while slot.waiting.is_some() || slot.writing {
                slot = shared
                    .changed
                    .wait(slot)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        if let Target::Thread(shared) = &self.target {
            shared.slot().closed = true;
            shared.changed.notify_all();
        }
    }
}

#[cfg(test)]
mod tests;
