//! Presentation state for a drone host's docks.
//!
//! The simulation counts a host's docked drones but not which dock holds
//! which. Drones fill the docks in a fixed order, rear pair first, and leave
//! front pair first, as the Roc's art launches them. Each dock remembers the
//! tick it last filled or emptied, so its clamps open as a drone lifts away
//! and close once one has settled, and a landing drone glides in from where
//! it was last drawn.

use macroquad::prelude::Vec2;

/// The order drones fill a host's docks; they leave in reverse.
pub(crate) const FILL_ORDER: [usize; 6] = [2, 3, 1, 4, 0, 5];
/// Ticks a launching drone takes to rise clear of its dock.
pub(crate) const LAUNCH_TICKS: f32 = 8.0;
/// Ticks a landing drone takes to glide into its dock.
pub(crate) const LAND_TICKS: f32 = 6.0;
/// Ticks clamps take to swing open after a launch.
const OPEN_TICKS: f32 = 4.0;
/// Ticks after a landing before the clamps start closing, and how long
/// they take.
const CLOSE_DELAY: f32 = 5.0;
const CLOSE_TICKS: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Dock {
    occupied: bool,
    /// The tick the dock last filled or emptied; `None` when it has been
    /// so since the host was first seen.
    since: Option<u64>,
    /// Where a landing drone was last drawn, in world tiles.
    from: Option<Vec2>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DockMotion {
    docks: [Dock; FILL_ORDER.len()],
}

/// A landing drone between where it was last drawn and its dock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Landing {
    pub(crate) from: Vec2,
    pub(crate) progress: f32,
}

fn ramp(t: f32, a: f32, b: f32) -> f32 {
    let u = ((t - a) / (b - a)).clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

impl DockMotion {
    /// Docks at rest with `docked` drones aboard.
    pub(crate) fn settled(docked: u8) -> Self {
        let mut docks = [Dock {
            occupied: false,
            since: None,
            from: None,
        }; FILL_ORDER.len()];
        for &dock in FILL_ORDER.iter().take(usize::from(docked)) {
            docks[dock].occupied = true;
        }
        Self { docks }
    }

    /// Updates the docks for tick `tick`, after which `docked` drones are
    /// aboard. `launched` names this tick's launched drones in id order and
    /// `landed` where this tick's landed drones were last drawn, in id
    /// order. Returns the dock each launched drone left.
    pub(crate) fn observe(
        &mut self,
        tick: u64,
        docked: u8,
        launched: &[u32],
        landed: &[Vec2],
    ) -> Vec<(u32, usize)> {
        let wanted = Self::settled(docked);
        let mut leaving = Vec::new();
        let mut arriving = Vec::new();
        for &dock in &FILL_ORDER {
            match (self.docks[dock].occupied, wanted.docks[dock].occupied) {
                (true, false) => leaving.push(dock),
                (false, true) => arriving.push(dock),
                _ => {}
            }
        }
        leaving.reverse();
        for (&dock, from) in arriving
            .iter()
            .zip(landed.iter().map(Some).chain(std::iter::repeat(None)))
        {
            self.docks[dock] = Dock {
                occupied: true,
                since: Some(tick),
                from: from.copied(),
            };
        }
        for &dock in &leaving {
            self.docks[dock] = Dock {
                occupied: false,
                since: Some(tick),
                from: None,
            };
        }
        launched.iter().copied().zip(leaving).collect()
    }

    fn age(&self, dock: usize, now: f32) -> f32 {
        self.docks[dock]
            .since
            .map_or(f32::INFINITY, |since| now - since as f32)
    }

    /// How far a dock's clamps stand open at `now`, from 0 closed to 1.
    pub(crate) fn spread(&self, dock: usize, now: f32) -> f32 {
        let age = self.age(dock, now);
        if self.docks[dock].occupied {
            1.0 - ramp(age, CLOSE_DELAY, CLOSE_DELAY + CLOSE_TICKS)
        } else {
            ramp(age, 0.0, OPEN_TICKS)
        }
    }

    /// Whether a drone rests in `dock` at `now`, landed and settled.
    pub(crate) fn resting(&self, dock: usize, now: f32) -> bool {
        self.docks[dock].occupied
            && (self.docks[dock].from.is_none() || self.age(dock, now) >= LAND_TICKS)
    }

    /// The drone gliding into `dock` at `now`, if one is.
    pub(crate) fn landing(&self, dock: usize, now: f32) -> Option<Landing> {
        let dock = self.docks[dock];
        let from = dock.from.filter(|_| dock.occupied)?;
        let age = now - dock.since? as f32;
        (age < LAND_TICKS).then(|| Landing {
            from,
            progress: ramp(age, 0.0, LAND_TICKS),
        })
    }
}

/// How far a drone launched `age` ticks ago has risen from its dock to
/// where the simulation has it, from 0 to 1.
pub(crate) fn launch_progress(age: f32) -> f32 {
    ramp(age, 0.0, LAUNCH_TICKS)
}

#[cfg(test)]
mod tests;
