//! Motor loops driven by the currently presented missile state.

use crate::numeric;
use aftermine_sim::{ProjectileKind, Target};
use chassis::fx::Vec2Fx;
use macroquad::audio::{PlaySoundParams, Sound, play_sound, set_sound_volume, stop_sound};
use macroquad::prelude::Vec2;

use crate::audio_timeline::{missile_ejection_ticks, projectile_elapsed_ticks};
use crate::game::{Scene, SoundKind, world_vec};

pub(crate) const MOTOR_VOICES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RocketKey {
    shooter: Target,
    launch: Vec2Fx,
    impact: Vec2Fx,
    arrival: u64,
    occurrence: usize,
}

/// One loop the frame wants sounding, keyed by what it follows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Motor<K> {
    pub(crate) key: K,
    pub(crate) gain: f32,
}

fn motor_envelope(elapsed: f32, total: f32) -> f32 {
    let powered = elapsed - missile_ejection_ticks(total);
    if powered < 0.0 || elapsed >= total {
        return 0.0;
    }
    // A short fade at both ends prevents discontinuities at loop admission.
    (powered / 0.4).clamp(0.0, 1.0) * ((total - elapsed) / 0.4).clamp(0.0, 1.0)
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

fn audible_motors(game: &Scene<'_>) -> Vec<Motor<RocketKey>> {
    let now = game.state.current_tick() as f32 + game.presentation.tick_fraction();
    let shells = game.state.shells();
    let mut motors = Vec::new();
    for (index, shell) in shells.iter().enumerate() {
        if shell.kind != ProjectileKind::Missile {
            continue;
        }
        let launch = world_vec(shell.launch);
        let impact = world_vec(shell.impact);
        let total = shell.arrival.saturating_sub(shell.launched_at).max(1) as f32;
        let elapsed = projectile_elapsed_ticks(now, shell.arrival, total);
        let envelope = motor_envelope(elapsed, total);
        if envelope <= 0.0 {
            continue;
        }
        let from =
            crate::render::entities::shell_visual_origin(launch, impact, shell.shooter, shell.kind);
        let progress = crate::render::entities::missile_travel_progress(
            elapsed / total,
            total,
            from.distance(impact),
        );
        let at = from.lerp(impact, progress);
        if game.state.hostile(game.presentation.human, shell.player)
            && !game.presentation.all_seeing()
            && !game.my_vision().visible(numeric::tile_at(at))
        {
            continue;
        }
        let occurrence = shells[..index]
            .iter()
            .filter(|earlier| {
                earlier.kind == shell.kind
                    && earlier.shooter == shell.shooter
                    && earlier.launch == shell.launch
                    && earlier.impact == shell.impact
                    && earlier.arrival == shell.arrival
            })
            .count();
        let gain = camera_gain(game, SoundKind::RocketMotor, at) * envelope;
        if gain > 0.0 {
            motors.push(Motor {
                key: RocketKey {
                    shooter: shell.shooter,
                    launch: shell.launch,
                    impact: shell.impact,
                    arrival: shell.arrival,
                    occurrence,
                },
                gain,
            });
        }
    }
    densest(motors, MOTOR_VOICES)
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

/// The Avalanche motors.
pub(crate) type RocketLoops = LoopBank<RocketKey, MOTOR_VOICES>;

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

impl RocketLoops {
    pub(crate) fn update(&mut self, game: &Scene<'_>, running: bool, clips: &[Sound], volume: f32) {
        let desired = if running {
            audible_motors(game)
        } else {
            Vec::new()
        };
        self.play(&desired, clips, volume);
    }
}

#[cfg(test)]
mod tests;
