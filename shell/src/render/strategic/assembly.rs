//! How a Spire's gantry or a Lattice's arm assembles a round over its
//! build time: every move plays at the pace its frames were drawn for, and
//! the hook or arm waits at the box while the next part is made there.

use super::*;

/// Ticks each gantry or arm frame holds, the pace the art was drawn for.
const FRAME_TICKS: usize = 4;

/// Steps a lock or clamp takes to close, one frame each.
const CLOSE_STEPS: usize = 4;

/// How an assembly rig's frames break down, as
/// `tools/production_sprite_sources` draws them: each of `parts` parts
/// takes `steps` frames, the first `lead` of which bring the empty hook or
/// hand back from the previous seat; `tail` frames return it after the
/// last part is seated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RigLayout {
    pub(super) parts: usize,
    pub(super) steps: usize,
    pub(super) lead: usize,
    pub(super) tail: usize,
}

/// The Spire gantry: five sections, the hook swinging back over the bay
/// for two frames of each, and four frames to park after the nose.
pub(super) const SPIRE_GANTRY: RigLayout = RigLayout {
    parts: 5,
    steps: 8,
    lead: 2,
    tail: 4,
};

/// The Lattice arm: three parts, one frame letting go of the last seat,
/// and two frames to drop back under the cover after the nose.
pub(super) const LATTICE_ARM: RigLayout = RigLayout {
    parts: 3,
    steps: 4,
    lead: 1,
    tail: 2,
};

/// Where a round's assembly stands at one tick of its build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Assembly {
    /// The 1-based gantry or arm frame drawn; `None` while the hook or arm
    /// waits at the box.
    pub(super) frame: Option<usize>,
    /// Parts standing seated.
    pub(super) seated: usize,
    /// The box is making the next part.
    pub(super) making: bool,
}

impl RigLayout {
    /// The frame that seats the last part; its carry is the nose's.
    pub(super) fn last_carry(self) -> usize {
        self.parts * self.steps
    }

    fn carry_ticks(self) -> usize {
        (self.steps - self.lead) * FRAME_TICKS
    }

    fn lead_ticks(self) -> usize {
        self.lead * FRAME_TICKS
    }

    /// Ticks after the last seat: the return, or the closing lock or
    /// clamp if that takes longer.
    fn finish_ticks(self) -> usize {
        self.tail.max(CLOSE_STEPS) * FRAME_TICKS
    }

    /// Ticks the box spends making each part: whatever the moves leave of
    /// the round, shared evenly.
    pub(super) fn wait_ticks(self, round_ticks: usize) -> usize {
        let moving = self.parts * self.carry_ticks()
            + (self.parts - 1) * self.lead_ticks()
            + self.finish_ticks();
        round_ticks.saturating_sub(moving) / self.parts
    }

    /// The tick of the round at which `part`, counted from zero, stands
    /// seated.
    pub(super) fn seated_at(self, part: usize, round_ticks: usize) -> usize {
        (part + 1) * (self.wait_ticks(round_ticks) + self.carry_ticks()) + part * self.lead_ticks()
    }

    /// Closing steps shown by a lock or clamp that shuts as `part` is
    /// seated.
    pub(super) fn closing(self, part: usize, progress: usize, round_ticks: usize) -> usize {
        progress
            .checked_sub(self.seated_at(part, round_ticks))
            .map_or(0, |since| (since / FRAME_TICKS + 1).min(CLOSE_STEPS))
    }

    /// The assembly `progress` ticks into a round of `round_ticks`.
    pub(super) fn at(self, progress: usize, round_ticks: usize) -> Assembly {
        let wait = self.wait_ticks(round_ticks);
        let moving = |frame: usize, seated: usize| Assembly {
            frame: Some(frame),
            seated,
            making: false,
        };
        let mut t = progress;
        for part in 0..self.parts {
            if t < wait {
                return Assembly {
                    frame: None,
                    seated: part,
                    making: true,
                };
            }
            t -= wait;
            if t < self.carry_ticks() {
                let frame = part * self.steps + self.lead + 1 + t / FRAME_TICKS;
                return moving(frame, part);
            }
            t -= self.carry_ticks();
            if part + 1 < self.parts {
                if t < self.lead_ticks() {
                    return moving((part + 1) * self.steps + 1 + t / FRAME_TICKS, part + 1);
                }
                t -= self.lead_ticks();
            }
        }
        Assembly {
            frame: (t < self.tail * FRAME_TICKS).then(|| self.last_carry() + 1 + t / FRAME_TICKS),
            seated: self.parts,
            making: false,
        }
    }
}

/// Ticks between the moments an arc may strike.
const SPARK_SLOT: f32 = 2.0;
/// The longest a spark lives, in ticks.
const SPARK_LIFE: f32 = 11.0;

/// A box's working edge, where the next part is welded, as two ends in art
/// pixels of the 128px footprint canvas.
pub(super) type Mouth = (Vec2, Vec2);

/// Welding sparks spitting from a box while it makes the next part: in
/// about half of the moments, an arc flashes somewhere along a mouth and
/// throws a small fan of embers toward the art's north that cool and fade.
/// `art` maps an art pixel to the screen and `pixel` is one art pixel's
/// size there. Pure in `now`, so pausing freezes them.
pub(super) fn draw_box_sparks(
    art: impl Fn(Vec2) -> Vec2,
    pixel: f32,
    mouths: &[Mouth],
    seed: u32,
    now: f32,
    alpha: f32,
) {
    if mouths.is_empty() || alpha <= 0.0 {
        return;
    }
    let first = numeric::to_u32(((now - SPARK_LIFE) / SPARK_SLOT).floor().max(0.0));
    let last = numeric::to_u32((now / SPARK_SLOT).floor().max(0.0));
    for slot in first..=last {
        let key = seed.wrapping_mul(7919) ^ slot;
        if scatter(key, 0) > 0.55 {
            continue;
        }
        let age = now - slot as f32 * SPARK_SLOT;
        if age < 0.0 {
            continue;
        }
        let pick = numeric::to_usize(scatter(key, 1) * mouths.len() as f32).min(mouths.len() - 1);
        let (a, b) = mouths[pick];
        let origin = a.lerp(b, scatter(key, 2));
        if age < 1.5 {
            let flash = (1.0 - age / 1.5) * alpha;
            let at = art(origin);
            soft_disc(at, pixel * 7.0, Color::new(1.0, 0.78, 0.45, 0.5 * flash));
            fill_rect(
                Rect::new(at.x - pixel, at.y - pixel, pixel * 2.0, pixel * 2.0),
                Color::new(1.0, 0.97, 0.88, flash),
            );
        }
        let embers = 2 + numeric::to_u32(scatter(key, 3) * 3.0);
        for ember in 0..embers {
            let index = 4 + ember * 5;
            let life = SPARK_LIFE * (0.55 + 0.45 * scatter(key, index));
            if age >= life {
                continue;
            }
            let spread = (scatter(key, index + 1) - 0.5) * 2.2;
            let speed = 1.2 + 1.8 * scatter(key, index + 2);
            let heading = vec2(spread, -1.0).normalize();
            let travel = age - 0.035 * age * age;
            let at = art(origin + heading * speed * travel + vec2(0.0, 0.05 * age * age));
            let cooled = age / life;
            let size = (pixel * if cooled < 0.5 { 2.0 } else { 1.5 }).max(1.5);
            fill_rect(
                Rect::new(at.x - size * 0.5, at.y - size * 0.5, size, size),
                Color::new(
                    1.0,
                    0.95 - 0.4 * cooled,
                    0.75 - 0.6 * cooled,
                    (1.0 - cooled * cooled) * alpha,
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests;
