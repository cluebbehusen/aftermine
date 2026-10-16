//! Presentation state for a Maw's cutter drum, conveyor and kiln.
//!
//! The simulation says only whether the Maw cut something lately and
//! whether its kiln has wreck to eat. The drum lowers and spins while
//! either holds, and grinds while it cuts; the belt runs while the kiln
//! feeds and until the last load it carries reaches the kiln; the kiln
//! glows while it burns. Distances and positions are in the unit canvas's
//! pixels.

/// How far scrap rides from leaving the hood to dropping into the kiln.
pub(crate) const BELT_RUN: f32 = 77.0;

const DEPLOY_TICKS: f32 = 24.0;
const CUT_SPIN: f32 = 0.30;
const FEED_SPIN: f32 = 0.22;
const SPIN_EASE: f32 = 0.03;
const BELT_SPEED: f32 = 2.0;
const BELT_EASE: f32 = 0.15;
/// Belt travel between successive loads while the kiln feeds.
const LOAD_SPACING: f32 = 12.0;
const HEAT_UP_TICKS: f32 = 16.0;
const COOL_TICKS: f32 = 40.0;
/// How fast the drum's grind swells as it starts cutting and dies once it
/// stops, per tick.
const GRIND_RISE: f32 = 0.5;
const GRIND_FALL: f32 = 0.2;

/// One clump of scrap riding the belt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Load {
    /// Belt travel when it left the hood.
    start: f32,
    pub(crate) variant: usize,
}

#[derive(Debug)]
pub(crate) struct MawMotion {
    tick: u64,
    deploy: [f32; 2],
    spin: [f32; 2],
    spin_rate: f32,
    grind: [f32; 2],
    belt: [f32; 2],
    belt_rate: [f32; 2],
    heat: [f32; 2],
    loads: Vec<Load>,
    next_load: f32,
    issued: usize,
}

impl MawMotion {
    pub(crate) fn new(tick: u64) -> Self {
        Self {
            tick,
            deploy: [0.0; 2],
            spin: [0.0; 2],
            spin_rate: 0.0,
            grind: [0.0; 2],
            belt: [0.0; 2],
            belt_rate: [0.0; 2],
            heat: [0.0; 2],
            loads: Vec::new(),
            next_load: 0.0,
            issued: 0,
        }
    }

    /// A machine first seen at `tick`, as it looks after a while at what it
    /// is doing then: drum lowered and turning while it cuts or feeds, kiln
    /// hot and the belt full while it feeds. A Maw met after a seek or a
    /// load shows its work at once.
    pub(crate) fn settled(tick: u64, cutting: bool, feeding: bool) -> Self {
        let mut motion = Self::new(tick);
        if cutting || feeding {
            motion.deploy = [1.0; 2];
        }
        motion.spin_rate = spin_target(cutting, feeding);
        if cutting {
            motion.grind = [1.0; 2];
        }
        if feeding {
            motion.belt_rate = [BELT_SPEED; 2];
            motion.heat = [1.0; 2];
            let mut along = 0.0;
            while along + LOAD_SPACING < BELT_RUN {
                along += LOAD_SPACING;
            }
            while along >= 0.0 {
                motion.loads.push(Load {
                    start: -along,
                    variant: motion.issued,
                });
                motion.issued += 1;
                along -= LOAD_SPACING;
            }
            motion.next_load = LOAD_SPACING;
        }
        motion
    }

    /// Advances one completed tick. `cutting` holds while the Maw's own
    /// hits are recent; `feeding` while its kiln has wreck to eat.
    pub(crate) fn observe(&mut self, tick: u64, cutting: bool, feeding: bool) {
        if self.tick == tick {
            return;
        }
        self.tick = tick;
        let working = cutting || feeding;
        let deploy = self.deploy[1];
        self.deploy = [
            deploy,
            if working {
                (deploy + 1.0 / DEPLOY_TICKS).min(1.0)
            } else {
                (deploy - 1.0 / DEPLOY_TICKS).max(0.0)
            },
        ];
        self.spin_rate = approach(self.spin_rate, spin_target(cutting, feeding), SPIN_EASE);
        self.spin = [self.spin[1], self.spin[1] + self.spin_rate];
        let grind = self.grind[1];
        self.grind = [
            grind,
            if cutting {
                (grind + GRIND_RISE).min(1.0)
            } else {
                (grind - GRIND_FALL).max(0.0)
            },
        ];

        let carrying = !self.loads.is_empty();
        let belt_target = if feeding || carrying { BELT_SPEED } else { 0.0 };
        let belt_rate = approach(self.belt_rate[1], belt_target, BELT_EASE);
        self.belt_rate = [self.belt_rate[1], belt_rate];
        let belt = self.belt[1] + belt_rate;
        self.belt = [self.belt[1], belt];
        if feeding {
            while self.next_load <= belt {
                self.loads.push(Load {
                    start: self.next_load,
                    variant: self.issued,
                });
                self.issued += 1;
                self.next_load += LOAD_SPACING;
            }
        } else {
            self.next_load = self.next_load.max(belt);
        }
        self.loads.retain(|load| belt - load.start < BELT_RUN);

        let burning = feeding || carrying;
        let heat = self.heat[1];
        self.heat = [
            heat,
            if burning {
                (heat + 1.0 / HEAT_UP_TICKS).min(1.0)
            } else {
                (heat - 1.0 / COOL_TICKS).max(0.0)
            },
        ];
    }

    /// How far the drum has lowered, eased, 0 stowed to 1 lowered.
    pub(crate) fn deploy(&self, alpha: f32) -> f32 {
        let linear = lerp(self.deploy, alpha);
        linear * linear * (3.0 - 2.0 * linear)
    }

    pub(crate) fn spin(&self, alpha: f32) -> f32 {
        lerp(self.spin, alpha)
    }

    /// How hard the drum grinds, 0 silent to 1 cutting.
    pub(crate) fn grind(&self, alpha: f32) -> f32 {
        lerp(self.grind, alpha)
    }

    pub(crate) fn belt(&self, alpha: f32) -> f32 {
        lerp(self.belt, alpha)
    }

    /// How fast the belt runs, 0 stopped to 1 at full speed.
    pub(crate) fn belt_speed(&self, alpha: f32) -> f32 {
        lerp(self.belt_rate, alpha) / BELT_SPEED
    }

    pub(crate) fn heat(&self, alpha: f32) -> f32 {
        lerp(self.heat, alpha)
    }

    /// How far each load has ridden from the hood, and its variant.
    pub(crate) fn loads(&self, alpha: f32) -> impl Iterator<Item = (f32, usize)> + '_ {
        let belt = self.belt(alpha);
        self.loads
            .iter()
            .map(move |load| (belt - load.start, load.variant))
            .filter(|(along, _)| (0.0..BELT_RUN).contains(along))
    }
}

fn spin_target(cutting: bool, feeding: bool) -> f32 {
    if cutting {
        CUT_SPIN
    } else if feeding {
        FEED_SPIN
    } else {
        0.0
    }
}

fn approach(value: f32, target: f32, step: f32) -> f32 {
    if value < target {
        (value + step).min(target)
    } else {
        (value - step).max(target)
    }
}

fn lerp(pair: [f32; 2], alpha: f32) -> f32 {
    pair[0] + (pair[1] - pair[0]) * alpha.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests;
