//! Motor loops for warheads and interceptors in flight. A loop plays only
//! while its missile would be drawn: always for the viewer's own and
//! allied missiles, and for a hostile one over a tile the viewer sees now,
//! or in an omniscient view (see [`crate::render::sky::missile_seen`]).

use crate::game::{EffectKind, INTERCEPT_OVERLAP_TICKS, Scene, SoundKind, TICK_DT};
use crate::render::sky::{missile_seen, warhead_track};
use crate::rocket_audio::{LoopBank, Motor, camera_gain, densest};
use macroquad::audio::Sound;
use oxide_sim::WarheadId;

pub(crate) const WARHEAD_VOICES: usize = 8;
pub(crate) const INTERCEPTOR_VOICES: usize = 4;

/// Ticks over which a warhead's motor rises under the Spire launch's
/// receding roar, which already carries the first moments of flight.
const WARHEAD_MOTOR_RISE: f32 = 30.0;

/// The share of the flight where the motor dies away: the warhead stops
/// burning there and coasts through its dive, as its plume does on screen.
const BURNOUT: (f32, f32) = (0.8, 0.86);

/// The share of an interceptor's climb its motor spends fading in and out.
const INTERCEPTOR_EDGE: f32 = 0.1;

/// Whatever the warhead loops and the interceptor loops follow: the warhead
/// or the interception, by the warhead's id.
pub(crate) type StrikeLoops<const N: usize> = LoopBank<WarheadId, N>;

fn warhead_envelope(tick: f32, launched_at: f32, progress: f32) -> f32 {
    let rise = ((tick - launched_at) / WARHEAD_MOTOR_RISE).clamp(0.0, 1.0);
    let burn = ((BURNOUT.1 - progress) / (BURNOUT.1 - BURNOUT.0)).clamp(0.0, 1.0);
    rise * burn
}

fn interceptor_envelope(climbed: f32) -> f32 {
    if !(0.0..1.0).contains(&climbed) {
        return 0.0;
    }
    (climbed / INTERCEPTOR_EDGE).min(1.0) * ((1.0 - climbed) / INTERCEPTOR_EDGE).min(1.0)
}

/// The tick the sky is drawn at, as the renderer draws it.
fn sky_tick(game: &Scene<'_>) -> f32 {
    game.state.current_tick().saturating_sub(1) as f32 + game.presentation.tick_fraction()
}

/// Every warhead in flight this viewer may know of and sees now, and every
/// stopped one still flying on toward its interceptor, at its motor's gain.
pub(crate) fn warhead_motors(game: &Scene<'_>) -> Vec<Motor<WarheadId>> {
    let human = game.presentation.human;
    let tick = sky_tick(game);
    let warheads = if game.presentation.all_seeing() {
        game.state.omniscient_warheads(human)
    } else {
        game.state.known_warheads(human)
    };
    let mut motors = Vec::new();
    let mut push =
        |key: WarheadId, hostile: bool, line: Option<oxide_sim::strike::WarheadLine>, at, tick| {
            let (ground, envelope) = match line {
                Some(line) => {
                    let (ground, progress) = warhead_track(line, tick);
                    let flight = oxide_sim::stats::WARHEAD.flight_ticks(line.launch, line.impact);
                    let launched_at = line.arrival.saturating_sub(flight) as f32;
                    (ground, warhead_envelope(tick, launched_at, progress))
                }
                None => (at, 1.0),
            };
            if envelope > 0.0 && missile_seen(game, hostile, ground) {
                let gain = camera_gain(game, SoundKind::WarheadMotor, ground) * envelope;
                if gain > 0.0 {
                    motors.push(Motor { key, gain });
                }
            }
        };
    for warhead in warheads {
        push(
            warhead.id,
            warhead.hostile,
            warhead.line,
            crate::game::world_vec(warhead.position),
            tick,
        );
    }
    let completed = game.state.current_tick();
    let fraction = game.presentation.tick_fraction();
    for fx in &game.presentation.fx {
        if let EffectKind::Interception {
            line: Some(line),
            warhead_hostile,
            contact,
            contact_tick,
            seed,
            completed_tick,
            ..
        } = fx.kind
        {
            // A stopped warhead flies on, and is heard, until the burst.
            let tick =
                completed_tick.saturating_sub(1) as f32 + fx.age_at(completed, fraction) / TICK_DT;
            if tick < contact_tick + INTERCEPT_OVERLAP_TICKS {
                push(WarheadId(seed), warhead_hostile, Some(line), contact, tick);
            }
        }
    }
    densest(motors, WARHEAD_VOICES)
}

/// Every interceptor climbing from its pod that this viewer sees now:
/// those launched ahead of a forecast stop, and those that left as the
/// stop was reported.
pub(crate) fn interceptor_motors(game: &Scene<'_>) -> Vec<Motor<WarheadId>> {
    let completed = game.state.current_tick();
    let fraction = game.presentation.tick_fraction();
    let mut motors = Vec::new();
    let tick = sky_tick(game);
    let climbed = |path: &crate::render::sky::InterceptorPath, tick: f32| {
        (tick - path.launch) / (path.contact - path.launch).max(1.0)
    };
    for launch in &game.presentation.prelaunches {
        let climbed = climbed(&launch.path, tick);
        let envelope = interceptor_envelope(climbed);
        let ground = launch.path.at(tick);
        let hostile = game
            .state
            .hostile(game.presentation.human, launch.battery_player);
        if envelope > 0.0 && missile_seen(game, hostile, ground) {
            let gain = camera_gain(game, SoundKind::InterceptorMotor, ground) * envelope;
            if gain > 0.0 {
                motors.push(Motor {
                    key: launch.warhead,
                    gain,
                });
            }
        }
    }
    for fx in &game.presentation.fx {
        if let EffectKind::Interception {
            path: Some(path),
            battery_player,
            seed,
            completed_tick,
            ..
        } = fx.kind
        {
            let tick =
                completed_tick.saturating_sub(1) as f32 + fx.age_at(completed, fraction) / TICK_DT;
            let climbed = climbed(&path, tick);
            let envelope = interceptor_envelope(climbed);
            let ground = path.at(tick);
            let hostile = game.state.hostile(game.presentation.human, battery_player);
            if envelope > 0.0 && missile_seen(game, hostile, ground) {
                let gain = camera_gain(game, SoundKind::InterceptorMotor, ground) * envelope;
                if gain > 0.0 {
                    motors.push(Motor {
                        key: WarheadId(seed),
                        gain,
                    });
                }
            }
        }
    }
    densest(motors, INTERCEPTOR_VOICES)
}

/// Keeps both banks of strike loops in step with the frame.
pub(crate) fn update(
    warheads: &mut StrikeLoops<WARHEAD_VOICES>,
    interceptors: &mut StrikeLoops<INTERCEPTOR_VOICES>,
    game: &Scene<'_>,
    running: bool,
    clips: (&[Sound], &[Sound]),
    volumes: (f32, f32),
) {
    let (warhead_loops, interceptor_loops) = if running {
        (warhead_motors(game), interceptor_motors(game))
    } else {
        (Vec::new(), Vec::new())
    };
    warheads.play(&warhead_loops, clips.0, volumes.0);
    interceptors.play(&interceptor_loops, clips.1, volumes.1);
}

#[cfg(test)]
mod tests;
