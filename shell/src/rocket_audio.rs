//! Motor loops driven by the currently presented missile state.

use crate::numeric;
use aftermine_sim::{ProjectileKind, Target};
use chassis::fx::Vec2Fx;
use macroquad::audio::Sound;

use crate::game::{Scene, SoundKind, world_vec};
use crate::game::{missile_ejection_ticks, projectile_elapsed_ticks};
use crate::loop_audio::{LoopBank, Motor, camera_gain, densest};

pub(crate) const MOTOR_VOICES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RocketKey {
    shooter: Target,
    launch: Vec2Fx,
    impact: Vec2Fx,
    arrival: u64,
    occurrence: usize,
}

fn motor_envelope(elapsed: f32, total: f32) -> f32 {
    let powered = elapsed - missile_ejection_ticks(total);
    if powered < 0.0 || elapsed >= total {
        return 0.0;
    }
    // A short fade at both ends prevents discontinuities at loop admission.
    (powered / 0.4).clamp(0.0, 1.0) * ((total - elapsed) / 0.4).clamp(0.0, 1.0)
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
            && !game.sees_tile(numeric::tile_at(at))
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

/// The Avalanche motors.
pub(crate) type RocketLoops = LoopBank<RocketKey, MOTOR_VOICES>;

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
