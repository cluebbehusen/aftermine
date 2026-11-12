//! Saves the app takes on its own: a decided match's record as soon as it
//! ends, and a running match's autosave every minute of live play and as
//! the window goes inactive.

use std::time::Duration;

/// Live play between periodic autosaves, in seconds.
const PERIOD: f32 = 60.0;
/// How long a failed background save waits before trying again.
pub(super) const RETRY: Duration = Duration::from_secs(10);

/// What one presented frame tells the schedule.
#[derive(Debug, Clone, Copy)]
pub(super) struct Frame {
    /// Wall seconds a resumable match played during the frame: its ticks
    /// over its speed, so slow frames and OS suspensions count only what
    /// actually ran.
    pub(super) played: f32,
    /// The native minimize state, when the platform reports one.
    pub(super) minimized: Option<bool>,
    /// The session holds content no record of it has.
    pub(super) unsaved: bool,
}

/// When the app next saves on its own, and how its failures are reported.
#[derive(Debug, Default)]
pub(super) struct Schedule {
    played: f32,
    inactive: bool,
    minimized: bool,
    retry_at: Option<Duration>,
    warned: bool,
}

impl Schedule {
    pub(super) fn observe(&mut self, frame: Frame) {
        self.played += frame.played;
        let minimized = frame.minimized == Some(true);
        if minimized && !self.minimized {
            self.inactive = true;
        }
        self.minimized = minimized;
        if !frame.unsaved {
            self.inactive = false;
        }
    }

    /// Whether a save is owed at `now`: once a retry delay passes, for a
    /// decided match that still lacks its record, or for an undecided
    /// match this machine can resume (`resumable`) with unsaved content
    /// once the window went inactive or a minute of live play passed.
    pub(super) fn due(&self, decided: bool, resumable: bool, now: Duration) -> bool {
        self.retry_at.is_none_or(|at| now >= at)
            && (decided || (resumable && (self.inactive || self.played >= PERIOD)))
    }

    /// Whether the window went inactive since the last save began.
    pub(super) fn inactive(&self) -> bool {
        self.inactive
    }

    /// A save began, so the minute of live play restarts.
    pub(super) fn started(&mut self) {
        self.played = 0.0;
        self.inactive = false;
    }

    /// A save failed at `now`: it is owed again once the retry delay passes.
    /// Returns whether to tell the player, which happens once until a save
    /// succeeds.
    pub(super) fn failed(&mut self, now: Duration) -> bool {
        self.retry_at = Some(now + RETRY);
        self.played = self.played.max(PERIOD);
        !std::mem::replace(&mut self.warned, true)
    }

    pub(super) fn succeeded(&mut self) {
        self.retry_at = None;
        self.warned = false;
    }
}

#[cfg(test)]
mod tests;
