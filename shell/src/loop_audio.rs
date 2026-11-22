//! Looping voices that follow what a frame shows: Avalanche motors,
//! warheads and interceptors in flight, and a Maw at work. Each bank keeps
//! a fixed number of voices, each its own clip instance, and admits the
//! loudest loops the frame wants.

use crate::audio_engine::{
    Bus, PlaySoundParams, Sound, distance_level, play_sound, set_sound_panning, set_sound_volume,
    stop_sound,
};
use crate::game::{Scene, SoundKind};
use macroquad::prelude::Vec2;

/// How a loop sounds from the camera, placed as the one-shot mix places a
/// sound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Voice {
    pub(crate) gain: f32,
    /// From -1, left, to 1, right.
    pub(crate) pan: f32,
    /// From 0, beside the listener, to 1, as far off as the camera hears.
    pub(crate) remoteness: f32,
}

impl Voice {
    pub(crate) fn scaled(self, level: f32) -> Self {
        Self {
            gain: self.gain * level,
            ..self
        }
    }
}

/// One loop the frame wants sounding, keyed by what it follows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Motor<K> {
    pub(crate) key: K,
    pub(crate) voice: Voice,
}

/// A loop at `at` for the current camera, weighted and placed as the one-shot
/// mix would play it.
pub(crate) fn camera_voice(game: &Scene<'_>, kind: SoundKind, at: Vec2) -> Voice {
    crate::audio_mix::frame_mix(
        [(kind, Some(at))],
        [],
        game.presentation.camera.center,
        game.presentation.camera.viewport() / game.presentation.camera.zoom * 0.5,
        game.presentation.camera.zoom,
    )
    .first()
    .map_or(
        Voice {
            gain: 0.0,
            pan: 0.0,
            remoteness: 1.0,
        },
        |sound| Voice {
            gain: sound.gain,
            pan: sound.pan,
            remoteness: sound.remoteness.unwrap_or(0.0),
        },
    )
}

/// The gain that keeps a playing loop at the level of `voice`'s distance
/// band while it stays on the band it started on, whose level is `band`.
fn steered_gain(voice: Voice, band: f32) -> f32 {
    voice.gain * distance_level(voice.remoteness) / band.max(f32::EPSILON)
}

/// The loudest `voices` of `motors`, scaled so loops started together,
/// which play the same clip in phase, do not sum past one coherent voice.
pub(crate) fn densest<K>(mut motors: Vec<Motor<K>>, voices: usize) -> Vec<Motor<K>> {
    motors.sort_by(|left, right| right.voice.gain.total_cmp(&left.voice.gain));
    motors.truncate(voices);
    let count = motors.len().max(1) as f32;
    let density = count.sqrt().recip().min(1.5 / count);
    for motor in &mut motors {
        motor.voice.gain *= density;
    }
    motors
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Change {
    Start(usize, Voice),
    Steer(usize, Voice),
    Stop(usize),
}

/// `N` looping voices, each its own clip instance, kept in step with the
/// loops a frame wants: a loop keeps its voice while it lasts and frees it
/// when it ends. A loop plays on the distance band it started on, which
/// fixes its tone and room; its level and pan follow the camera.
pub(crate) struct LoopBank<K, const N: usize> {
    slots: [Option<K>; N],
    /// The level of the distance band each slot's loop started on.
    bands: [f32; N],
}

impl<K: Copy, const N: usize> Default for LoopBank<K, N> {
    fn default() -> Self {
        Self {
            slots: [None; N],
            bands: [1.0; N],
        }
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
                changes.push(Change::Steer(index, motor.voice));
            } else if let Some(index) = self.slots.iter().position(Option::is_none) {
                self.slots[index] = Some(motor.key);
                changes.push(Change::Start(index, motor.voice));
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
                Change::Start(slot, voice) => {
                    self.bands[slot] = distance_level(voice.remoteness);
                    play_sound(
                        &clips[slot],
                        PlaySoundParams {
                            pan: voice.pan,
                            remoteness: Some(voice.remoteness),
                            ..PlaySoundParams::new(Bus::Effects, true, volume * voice.gain)
                        },
                    );
                }
                Change::Steer(slot, voice) => {
                    set_sound_volume(&clips[slot], volume * steered_gain(voice, self.bands[slot]));
                    set_sound_panning(&clips[slot], voice.pan);
                }
                Change::Stop(slot) => stop_sound(&clips[slot]),
            }
        }
    }
}

#[cfg(test)]
mod tests;
