//! What a viewer may see of a missile in the sky: the one rule the sky's
//! drawing and its motor loops share.

use macroquad::prelude::Vec2;

/// Whether a missile over `ground`, its world position on the ground
/// track, shows to this viewer now. A viewer's own and allied missiles
/// always show, as their units do; a `hostile` one shows true to fog, only
/// while the tile under it is in sight, or in an omniscient view.
pub(crate) fn missile_seen(game: &crate::game::Scene<'_>, hostile: bool, ground: Vec2) -> bool {
    missile_seen_by(game.presentation, game.state, hostile, ground)
}

/// [`missile_seen`] for a presentation and the state it shows.
pub(crate) fn missile_seen_by(
    presentation: &crate::game::Presentation,
    state: &aftermine_sim::State,
    hostile: bool,
    ground: Vec2,
) -> bool {
    !hostile || ground_seen_by(presentation, state, ground)
}

/// Whether this viewer sees the ground at `ground` itself, whatever flies
/// over it.
pub(crate) fn ground_seen_by(
    presentation: &crate::game::Presentation,
    state: &aftermine_sim::State,
    ground: Vec2,
) -> bool {
    presentation.all_seeing()
        || state
            .vision(presentation.human)
            .visible(crate::numeric::tile_at(ground))
}

/// Ticks over which a warhead eases off its deck's rail: it leaves from
/// rest and runs ahead of the simulation's even pace for a moment, so it
/// stands exactly where the simulation has it from then on.
const RAIL_TICKS: f32 = 20.0;
/// Tiles a warhead runs along before it starts to climb.
const RAIL_CLEAR: f32 = 1.4;
/// Height gained, in tiles, per tile flown while climbing.
const CLIMB: f32 = 0.45;
/// Cruise height, in tiles.
const CRUISE: f32 = 4.5;
/// Height lost, in tiles, per tile left in the dive onto the target.
const DIVE: f32 = 0.9;
/// Shadow offset per tile of height, down and to the right as an
/// aircraft's falls.
pub(crate) const SHADOW_PER_TILE: Vec2 = Vec2::new(0.16, 0.26);
/// A missile's growth per tile of height.
pub(crate) const SCALE_PER_TILE: f32 = 0.05;

/// Lengths, in tiles, from each missile sprite's centre: the warhead's nose
/// and nozzle, the interceptor's, and the gap at which an interceptor's
/// fuse fires.
pub(crate) const WARHEAD_NOSE: f32 = 44.0 / 64.0;
pub(crate) const WARHEAD_TAIL: f32 = 44.0 / 64.0;
pub(crate) const INTERCEPTOR_NOSE: f32 = 27.0 / 64.0;
pub(crate) const INTERCEPTOR_TAIL: f32 = 26.0 / 64.0;
pub(crate) const FUSE_GAP: f32 = 6.0 / 64.0;
/// Tiles per tick an interceptor drifts on into the burst after contact:
/// the warhead's own pace, so neither shoots out of the flash.
const INTO_BURST: f32 = 0.2;

/// A warhead's whole flight as drawn: one straight line from its deck to
/// its target at the simulation's pace, after easing off the rail, with a
/// height that shows only in its shadow and size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Flight {
    pub(crate) launch: Vec2,
    pub(crate) impact: Vec2,
    pub(crate) launched_at: f32,
    pub(crate) arrival: f32,
}

impl Flight {
    pub(crate) fn of(line: aftermine_sim::strike::WarheadLine) -> Self {
        let ticks = aftermine_sim::stats::WARHEAD.flight_ticks(line.launch, line.impact);
        Self {
            launch: crate::game::world_vec(line.launch),
            impact: crate::game::world_vec(line.impact),
            launched_at: line.arrival.saturating_sub(ticks) as f32,
            arrival: line.arrival as f32,
        }
    }

    /// The share of the line flown at presentation `tick`.
    pub(crate) fn progress(self, tick: f32) -> f32 {
        let span = (self.arrival - self.launched_at).max(1.0);
        let t = (tick - self.launched_at).clamp(0.0, span);
        let ease = RAIL_TICKS.min(span);
        if t >= ease {
            t / span
        } else {
            let u = t / ease;
            ease * u * u * (2.0 - u) / span
        }
    }

    /// Where it stands at `tick`, world coords.
    pub(crate) fn ground(self, tick: f32) -> Vec2 {
        self.launch + (self.impact - self.launch) * self.progress(tick)
    }

    /// Its unit heading, launch to impact.
    pub(crate) fn heading(self) -> Vec2 {
        (self.impact - self.launch).normalize_or_zero()
    }

    /// Its height in tiles at `tick`: running along the rail, climbing,
    /// cruising, then diving onto the target.
    pub(crate) fn altitude(self, tick: f32) -> f32 {
        let length = self.launch.distance(self.impact);
        let flown = length * self.progress(tick);
        (CLIMB * (flown - RAIL_CLEAR))
            .min(CRUISE)
            .min(DIVE * (length - flown))
            .max(0.0)
    }

    /// How large it draws at `tick`.
    pub(crate) fn scale(self, tick: f32) -> f32 {
        1.0 + SCALE_PER_TILE * self.altitude(tick)
    }

    /// Its nose at `tick`, world coords.
    pub(crate) fn nose(self, tick: f32) -> Vec2 {
        self.ground(tick) + self.heading() * WARHEAD_NOSE * self.scale(tick)
    }
}

/// An interceptor's flight as drawn: one straight line from its pod's pivot
/// to where its nose meets the warhead's, accelerating like a rocket, then
/// drifting on past contact into the burst.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct InterceptorPath {
    /// The pod's pivot, world coords.
    pub(crate) from: Vec2,
    /// Its centre on contact, world coords.
    pub(crate) end: Vec2,
    /// Presentation tick it leaves the pod.
    pub(crate) launch: f32,
    /// Presentation tick the noses meet.
    pub(crate) contact: f32,
    /// Its height on contact, tiles.
    pub(crate) altitude: f32,
}

impl InterceptorPath {
    /// The path from a pod's pivot to an interceptor meeting `contact`, the
    /// point between the two noses, at `altitude` tiles.
    pub(crate) fn to(from: Vec2, contact: Vec2, launch: f32, meet: f32, altitude: f32) -> Self {
        let heading = (contact - from).normalize_or_zero();
        Self {
            from,
            end: contact - heading * (INTERCEPTOR_NOSE + FUSE_GAP / 2.0),
            launch,
            contact: meet,
            altitude,
        }
    }

    /// The share of the line flown at `tick`, past 1 after contact.
    pub(crate) fn share(&self, tick: f32) -> f32 {
        if tick <= self.contact {
            let k = (tick - self.launch) / (self.contact - self.launch).max(1.0);
            k.max(0.0).powf(1.7)
        } else {
            1.0 + (tick - self.contact) * INTO_BURST / self.from.distance(self.end).max(1e-3)
        }
    }

    /// Its centre at `tick`, world coords.
    pub(crate) fn at(&self, tick: f32) -> Vec2 {
        self.from + (self.end - self.from) * self.share(tick)
    }

    /// Its height at `tick`, tiles.
    pub(crate) fn altitude_at(&self, tick: f32) -> f32 {
        self.altitude * self.share(tick).min(1.0)
    }

    /// Its unit heading.
    pub(crate) fn heading(&self) -> Vec2 {
        (self.end - self.from).normalize_or_zero()
    }
}

/// Where a warhead on `line` stands over the ground at presentation `tick`,
/// and how far along its flight it is.
pub(crate) fn warhead_track(line: aftermine_sim::strike::WarheadLine, tick: f32) -> (Vec2, f32) {
    let flight = Flight::of(line);
    (flight.ground(tick), flight.progress(tick))
}

#[cfg(test)]
mod tests;
