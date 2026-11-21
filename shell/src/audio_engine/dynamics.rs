//! Bus dynamics: a compressor that glues stacked clips and a lookahead
//! limiter that keeps the master under its ceiling. Both read the two
//! channels together, so neither moves a panned sound across the image.

use std::collections::VecDeque;
use std::time::Duration;

use kira::Frame;
use kira::effect::{Effect, EffectBuilder};
use kira::info::Info;

/// One-pole smoothing coefficient for `time` at `sample_rate`.
fn coefficient(time: Duration, sample_rate: u32) -> f32 {
    let samples = time.as_secs_f64() * f64::from(sample_rate);
    if samples <= 1.0 {
        0.0
    } else {
        crate::numeric::to_f32((-1.0 / samples).exp())
    }
}

fn level(frame: Frame) -> f32 {
    frame.left.abs().max(frame.right.abs())
}

/// A feed-forward compressor with a soft knee.
pub(super) struct CompressorBuilder {
    pub threshold_db: f32,
    pub ratio: f32,
    pub knee_db: f32,
    pub attack: Duration,
    pub release: Duration,
}

impl EffectBuilder for CompressorBuilder {
    type Handle = ();

    fn build(self) -> (Box<dyn Effect>, ()) {
        (Box::new(Compressor::new(self, 48_000)), ())
    }
}

pub(super) struct Compressor {
    settings: CompressorBuilder,
    attack: f32,
    release: f32,
    /// The gain change now applied, in decibels, never above zero.
    reduction_db: f32,
}

impl Compressor {
    fn new(settings: CompressorBuilder, sample_rate: u32) -> Self {
        let mut compressor = Self {
            settings,
            attack: 0.0,
            release: 0.0,
            reduction_db: 0.0,
        };
        compressor.set_sample_rate(sample_rate);
        compressor
    }

    fn set_sample_rate(&mut self, sample_rate: u32) {
        self.attack = coefficient(self.settings.attack, sample_rate);
        self.release = coefficient(self.settings.release, sample_rate);
    }

    /// The static gain change for an input level, in decibels.
    fn curve(&self, input_db: f32) -> f32 {
        let CompressorBuilder {
            threshold_db,
            ratio,
            knee_db,
            ..
        } = self.settings;
        let over = input_db - threshold_db;
        let slope = ratio.recip() - 1.0;
        if 2.0 * over <= -knee_db {
            0.0
        } else if 2.0 * over >= knee_db {
            slope * over
        } else {
            slope * (over + knee_db / 2.0).powi(2) / (2.0 * knee_db)
        }
    }

    fn process_frames(&mut self, frames: &mut [Frame]) {
        for frame in frames {
            let input_db = 20.0 * level(*frame).max(1.0e-6).log10();
            let target = self.curve(input_db);
            let coefficient = if target < self.reduction_db {
                self.attack
            } else {
                self.release
            };
            self.reduction_db = target + coefficient * (self.reduction_db - target);
            *frame *= 10.0_f32.powf(self.reduction_db / 20.0);
        }
    }
}

impl Effect for Compressor {
    fn init(&mut self, sample_rate: u32, _internal_buffer_size: usize) {
        self.set_sample_rate(sample_rate);
    }

    fn on_change_sample_rate(&mut self, sample_rate: u32) {
        self.set_sample_rate(sample_rate);
    }

    fn process(&mut self, input: &mut [Frame], _dt: f64, _info: &Info) {
        self.process_frames(input);
    }
}

/// A brickwall limiter that sees `lookahead` ahead and delays its output by
/// as much, so its gain is already down when a peak arrives.
pub(super) struct LimiterBuilder {
    /// Linear peak the output never exceeds.
    pub ceiling: f32,
    pub lookahead: Duration,
    pub release: Duration,
}

impl EffectBuilder for LimiterBuilder {
    type Handle = ();

    fn build(self) -> (Box<dyn Effect>, ()) {
        (Box::new(Limiter::new(self, 48_000)), ())
    }
}

/// Each input frame asks for the gain that brings it to the ceiling. The
/// least gain asked in the last `length + 1` frames is held, and the held
/// values are averaged over as many frames again. An output frame leaves the
/// delay `length` frames after it entered, so every held value in that
/// average covers it: the gain on a peak is never above what the peak asked,
/// and the gain ramps down linearly across the lookahead before it.
pub(super) struct Limiter {
    settings: LimiterBuilder,
    length: usize,
    delay: Vec<Frame>,
    /// Increasing gains asked, oldest first, with the frame that asked.
    least: VecDeque<(u64, f32)>,
    held: Vec<f32>,
    held_sum: f64,
    release: f32,
    gain: f32,
    index: u64,
    delay_slot: usize,
    held_slot: usize,
}

impl Limiter {
    fn new(settings: LimiterBuilder, sample_rate: u32) -> Self {
        let mut limiter = Self {
            settings,
            length: 1,
            delay: Vec::new(),
            least: VecDeque::new(),
            held: Vec::new(),
            held_sum: 0.0,
            release: 0.0,
            gain: 1.0,
            index: 0,
            delay_slot: 0,
            held_slot: 0,
        };
        limiter.set_sample_rate(sample_rate);
        limiter
    }

    fn set_sample_rate(&mut self, sample_rate: u32) {
        let frames = self.settings.lookahead.as_secs_f64() * f64::from(sample_rate);
        self.length = crate::numeric::to_usize(crate::numeric::to_f32(frames.round())).max(1);
        self.delay = vec![Frame::ZERO; self.length];
        self.least = VecDeque::with_capacity(self.length + 2);
        self.held = vec![1.0; self.length + 1];
        self.held_sum = (self.length + 1) as f64;
        self.release = coefficient(self.settings.release, sample_rate);
        self.gain = 1.0;
        self.index = 0;
        self.delay_slot = 0;
        self.held_slot = 0;
    }

    fn process_frames(&mut self, frames: &mut [Frame]) {
        let span = self.length as u64;
        let window = (self.length + 1) as f64;
        for frame in frames {
            let peak = level(*frame);
            let asked = if peak > self.settings.ceiling {
                self.settings.ceiling / peak
            } else {
                1.0
            };
            while self.least.back().is_some_and(|&(_, gain)| gain >= asked) {
                self.least.pop_back();
            }
            self.least.push_back((self.index, asked));
            while self
                .least
                .front()
                .is_some_and(|&(index, _)| index + span < self.index)
            {
                self.least.pop_front();
            }
            let hold = self.least.front().map_or(1.0, |&(_, gain)| gain);
            self.held_sum += f64::from(hold) - f64::from(self.held[self.held_slot]);
            self.held[self.held_slot] = hold;
            self.held_slot = (self.held_slot + 1) % self.held.len();
            let smooth = crate::numeric::to_f32(self.held_sum / window);
            self.gain = if smooth < self.gain {
                smooth
            } else {
                smooth + self.release * (self.gain - smooth)
            };
            let delayed = std::mem::replace(&mut self.delay[self.delay_slot], *frame);
            self.delay_slot = (self.delay_slot + 1) % self.length;
            *frame = delayed * self.gain;
            self.index += 1;
        }
    }
}

impl Effect for Limiter {
    fn init(&mut self, sample_rate: u32, _internal_buffer_size: usize) {
        self.set_sample_rate(sample_rate);
    }

    fn on_change_sample_rate(&mut self, sample_rate: u32) {
        self.set_sample_rate(sample_rate);
    }

    fn process(&mut self, input: &mut [Frame], _dt: f64, _info: &Info) {
        self.process_frames(input);
    }
}

#[cfg(test)]
mod tests;
