//! A Maw's working loops: its drum grinding while it cuts and its kiln
//! humming while the belt carries wreck in. Each follows the machine's
//! drawn motion, so it is heard while that work shows and only while the
//! viewer sees the machine.

use crate::game::{Scene, SoundKind};
use crate::rocket_audio::{LoopBank, Motor, camera_gain, densest};
use macroquad::audio::Sound;
use oxide_sim::{UnitId, UnitKind};

pub(crate) const MAW_VOICES: usize = 4;

/// The drums' grind, or the kilns' hum, by machine.
pub(crate) type MawLoops = LoopBank<UnitId, MAW_VOICES>;

/// Every seen Maw's grind and hum, at their gains.
pub(crate) fn maw_motors(game: &Scene<'_>) -> (Vec<Motor<UnitId>>, Vec<Motor<UnitId>>) {
    let alpha = game.clock.render_alpha();
    let (mut cuts, mut kilns) = (Vec::new(), Vec::new());
    for unit in game.state.units() {
        if unit.kind != UnitKind::Maw || !game.sees_unit(unit) {
            continue;
        }
        let Some(motion) = game.presentation.maw_motion.get(&unit.id.0) else {
            continue;
        };
        let at = game.presentation.draw_pos(unit.id, unit.pos, alpha);
        for (motors, kind, level) in [
            (&mut cuts, SoundKind::MawCut, motion.grind(alpha)),
            (&mut kilns, SoundKind::MawKiln, motion.belt_speed(alpha)),
        ] {
            let gain = camera_gain(game, kind, at) * level;
            if gain > 0.0 {
                motors.push(Motor { key: unit.id, gain });
            }
        }
    }
    (densest(cuts, MAW_VOICES), densest(kilns, MAW_VOICES))
}

/// Keeps both banks in step with the frame. Returns whether a loop that
/// raises the score sounds, as a weapon report does.
pub(crate) fn update(
    cuts: &mut MawLoops,
    kilns: &mut MawLoops,
    game: &Scene<'_>,
    running: bool,
    clips: (&[Sound], &[Sound]),
    volumes: (f32, f32),
) -> bool {
    let (cut, kiln) = if running {
        maw_motors(game)
    } else {
        (Vec::new(), Vec::new())
    };
    cuts.play(&cut, clips.0, volumes.0);
    kilns.play(&kiln, clips.1, volumes.1);
    let raises = |motors: &[Motor<UnitId>], volume: f32, kind: SoundKind| {
        !motors.is_empty() && volume > 0.0 && crate::mixer::spec(kind).combat
    };
    raises(&cut, volumes.0, SoundKind::MawCut) || raises(&kiln, volumes.1, SoundKind::MawKiln)
}

#[cfg(test)]
mod tests;
