//! The shell's own pacing, for a platform that presents frames faster than
//! the interval asked of it, as when a driver setting forces vsync off or
//! holds it to every refresh. It watches how long frames take and, once
//! they come faster than asked, sleeps before each frame.

use super::Refresh;
use std::time::Duration;

/// How long frames run under a new interval or refresh before they count,
/// so a change has reached the screen before it is judged.
const SETTLE: Duration = Duration::from_millis(500);

/// How much frame time one check takes the median of.
const SPAN: Duration = Duration::from_secs(1);

/// The most frames one check keeps; a check ends early when frames come
/// thousands a second.
const MOST_FRAMES: usize = 2048;

/// One presented frame, as the fallback watches it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Frame {
    /// The wall time from this frame's start to the next's, unclamped.
    pub(crate) took: Duration,
    /// How many refreshes the platform was asked to present it after.
    pub(crate) interval: u32,
    /// The display's refresh while it ran.
    pub(crate) refresh: Option<Refresh>,
    /// Whether the window was minimized or in the background, where a
    /// platform may present without waiting for the display.
    pub(crate) hidden: bool,
}

/// How a platform that ignores the asked interval waits for the display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vsync {
    /// Each frame still waits for a refresh, but never more than one.
    EveryRefresh,
    /// No frame waits for a refresh.
    Off,
}

/// Whether the platform presents frames as far apart as asked and, once it
/// does not, when the shell lets each frame present instead.
#[derive(Debug)]
pub(crate) struct Fallback {
    /// The refresh frames are watched under; a change starts over.
    refresh: Option<Refresh>,
    phase: Phase,
}

#[derive(Debug)]
enum Phase {
    /// Watching frames asked to span `interval` refreshes: the first
    /// [`SETTLE`] of them pass, then `frames` collects a [`SPAN`].
    Watching {
        interval: u32,
        watched: Duration,
        frames: Vec<Duration>,
    },
    /// Frames came faster than asked, so the shell paces them until the
    /// refresh changes. `aim` is when the last paced frame meant to
    /// present.
    Engaged { vsync: Vsync, aim: Option<Duration> },
}

impl Phase {
    fn watching(interval: u32) -> Self {
        Self::Watching {
            interval,
            watched: Duration::ZERO,
            frames: Vec::new(),
        }
    }
}

impl Default for Fallback {
    fn default() -> Self {
        Self {
            refresh: None,
            phase: Phase::watching(1),
        }
    }
}

impl Fallback {
    /// Whether this build watches for frames the platform did not pace: on
    /// Windows and Linux a driver setting can override the swap interval.
    /// Apple platforms pace through the display's own link.
    pub(crate) const WATCHED: bool = cfg!(any(target_os = "windows", target_os = "linux"));

    /// Watches one presented frame. Once the median of a second of settled
    /// frames comes in under four fifths of the time asked, the fallback
    /// engages and stays engaged until the refresh changes, when it watches
    /// afresh. Frames slower
    /// than asked, under a heavy load or a limiter outside the game, never
    /// engage it.
    pub(crate) fn observe(&mut self, frame: Frame) {
        if frame.refresh != self.refresh {
            self.refresh = frame.refresh;
            self.phase = Phase::watching(frame.interval);
            return;
        }
        let Some(refresh) = self.refresh else {
            return;
        };
        let Phase::Watching {
            interval,
            watched,
            frames,
        } = &mut self.phase
        else {
            return;
        };
        if frame.hidden || frame.interval != *interval {
            self.phase = Phase::watching(frame.interval);
            return;
        }
        *watched += frame.took;
        if *watched <= SETTLE {
            return;
        }
        frames.push(frame.took);
        if *watched < SETTLE + SPAN && frames.len() < MOST_FRAMES {
            return;
        }
        let middle = frames.len() / 2;
        let median = *frames.select_nth_unstable(middle).1;
        if let Some(vsync) = judge(median, *interval, refresh) {
            self.phase = Phase::Engaged { vsync, aim: None };
        } else {
            frames.clear();
            *watched = SETTLE;
        }
    }

    /// When to wake to present the frame that began at `start`, as of
    /// `now` on the same clock, or none to present at once. Only an engaged
    /// fallback sleeps. At an interval of one it sleeps only without vsync,
    /// holding the full rate to the refresh; the caller skips uncapped
    /// sessions, which run at that interval too.
    ///
    /// While each frame still waits for a refresh, a frame wakes half a
    /// refresh before the refresh it aims for, so that wait lands it there.
    /// While frames wait for none, a frame wakes at its deadline, and a
    /// frame that falls a whole frame behind starts the deadlines over.
    pub(crate) fn wake(
        &mut self,
        interval: u32,
        start: Duration,
        now: Duration,
    ) -> Option<Duration> {
        let (Some(refresh), Phase::Engaged { vsync, aim }) = (self.refresh, &mut self.phase) else {
            return None;
        };
        if interval <= 1 && matches!(vsync, Vsync::EveryRefresh) {
            *aim = None;
            return None;
        }
        let interval = interval.max(1);
        let period = refresh.period();
        let frame = period * interval;
        let wake = match vsync {
            Vsync::EveryRefresh => {
                // A frame that begins no earlier than a quarter refresh
                // before the last aim began after its swap waited for that
                // refresh, so the next aim counts from the display's own
                // beat. A driver that queues frames ahead returns from the
                // swap at once, and the aims keep their own beat.
                let from = match *aim {
                    Some(last) if start + period / 4 < last => last,
                    _ => start,
                };
                let target = from + frame;
                *aim = Some(target);
                target.saturating_sub(period / 2)
            }
            Vsync::Off => {
                let mut deadline = aim.unwrap_or(start) + frame;
                if now >= deadline + frame {
                    deadline = now;
                }
                *aim = Some(deadline);
                deadline
            }
        };
        (wake > now).then_some(wake)
    }
}

/// How the platform waits, judged from the median time of frames asked to
/// span `interval` refreshes, or none when they came no faster than four
/// fifths of that. A median within four fifths of one refresh means each
/// frame still waits for a refresh.
fn judge(median: Duration, interval: u32, refresh: Refresh) -> Option<Vsync> {
    let period = refresh.period();
    if median * 5 >= period * interval * 4 {
        None
    } else if median * 5 >= period * 4 {
        Some(Vsync::EveryRefresh)
    } else {
        Some(Vsync::Off)
    }
}

#[cfg(test)]
mod tests;
