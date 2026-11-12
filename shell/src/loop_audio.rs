//! Looping voices that follow what a frame shows: Avalanche motors,
//! warheads and interceptors in flight, and a Maw at work. Each bank keeps
//! a fixed number of voices, each its own clip instance, and admits the
//! loudest loops the frame wants.

use crate::game::{Scene, SoundKind};
use macroquad::audio::{PlaySoundParams, Sound, play_sound, set_sound_volume, stop_sound};
use macroquad::prelude::Vec2;

/// One loop the frame wants sounding, keyed by what it follows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Motor<K> {
    pub(crate) key: K,
    pub(crate) gain: f32,
}

/// A loop's gain at `at` for the current camera: the one-shot mix's weighting.
pub(crate) fn camera_gain(game: &Scene<'_>, kind: SoundKind, at: Vec2) -> f32 {
    crate::audio_mix::frame_mix(
        [(kind, Some(at))],
        [],
        game.presentation.camera.center,
        game.presentation.camera.viewport() / game.presentation.camera.zoom * 0.5,
        game.presentation.camera.zoom,
    )
    .first()
    .map_or(0.0, |sound| sound.gain)
}

/// The loudest `voices` of `motors`, scaled so loops started together,
/// which play the same clip in phase, do not sum past one coherent voice.
pub(crate) fn densest<K>(mut motors: Vec<Motor<K>>, voices: usize) -> Vec<Motor<K>> {
    motors.sort_by(|left, right| right.gain.total_cmp(&left.gain));
    motors.truncate(voices);
    let count = motors.len().max(1) as f32;
    let density = count.sqrt().recip().min(1.5 / count);
    for motor in &mut motors {
        motor.gain *= density;
    }
    motors
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Change {
    Start(usize, f32),
    Gain(usize, f32),
    Stop(usize),
}

/// `N` looping voices, each its own clip instance, kept in step with the
/// loops a frame wants: a loop keeps its voice while it lasts and frees it
/// when it ends.
pub(crate) struct LoopBank<K, const N: usize> {
    slots: [Option<K>; N],
}

impl<K: Copy, const N: usize> Default for LoopBank<K, N> {
    fn default() -> Self {
        Self { slots: [None; N] }
    }
}

impl<K: Copy + PartialEq, const N: usize> LoopBank<K, N> {
    fn reconcile(&mut self, desired: &[Motor<K>]) -> Vec<Change> {
        let mut changes = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.is_some_and(|key| !desired.iter().any(|motor| motor.key == key)) {
                *slot = None;
                changes.push(Change::Stop(index));
            }
        }
        for motor in desired {
            if let Some(index) = self.slots.iter().position(|slot| *slot == Some(motor.key)) {
                changes.push(Change::Gain(index, motor.gain));
            } else if let Some(index) = self.slots.iter().position(Option::is_none) {
                self.slots[index] = Some(motor.key);
                changes.push(Change::Start(index, motor.gain));
            }
        }
        changes
    }

    /// Starts, steers, and stops the bank's voices toward `desired`.
    pub(crate) fn play(&mut self, desired: &[Motor<K>], clips: &[Sound], volume: f32) {
        let desired = if clips.len() == N && volume > 0.0 {
            desired
        } else {
            &[]
        };
        for change in self.reconcile(desired) {
            match change {
                Change::Start(slot, gain) => play_sound(
                    &clips[slot],
                    PlaySoundParams {
                        looped: true,
                        volume: volume * gain,
                    },
                ),
                Change::Gain(slot, gain) => set_sound_volume(&clips[slot], volume * gain),
                Change::Stop(slot) => stop_sound(&clips[slot]),
            }
        }
    }
}

#[cfg(test)]
mod tests;
