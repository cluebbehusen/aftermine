//! A Maw's working loop: its drum grinding while it cuts. It follows the
//! machine's drawn motion, so it is heard while that work shows and only
//! while the viewer sees the machine.

use crate::audio_engine::Sound;
use crate::game::{Scene, SoundKind};
use crate::loop_audio::{LoopBank, Motor, camera_gain, densest};
use aftermine_sim::{UnitId, UnitKind};

pub(crate) const MAW_VOICES: usize = 4;

/// The drums' grind, by machine.
pub(crate) type MawLoops = LoopBank<UnitId, MAW_VOICES>;

/// Every seen Maw's grind, at its gain.
pub(crate) fn maw_motors(game: &Scene<'_>) -> Vec<Motor<UnitId>> {
    let alpha = game.clock.render_alpha();
    let mut cuts = Vec::new();
    for unit in game.state.units() {
        if unit.kind != UnitKind::Maw || !game.sees_unit(unit) {
            continue;
        }
        let Some(motion) = game.presentation.maw_motion.get(&unit.id.0) else {
            continue;
        };
        let at = game.presentation.draw_pos(unit.id, unit.pos, alpha);
        let gain = camera_gain(game, SoundKind::MawCut, at) * motion.grind(alpha);
        if gain > 0.0 {
            cuts.push(Motor { key: unit.id, gain });
        }
    }
    densest(cuts, MAW_VOICES)
}

/// Keeps the bank in step with the frame.
pub(crate) fn update(
    cuts: &mut MawLoops,
    game: &Scene<'_>,
    running: bool,
    clips: &[Sound],
    volume: f32,
) {
    let cut = if running {
        maw_motors(game)
    } else {
        Vec::new()
    };
    cuts.play(&cut, clips, volume);
}

#[cfg(test)]
mod tests;
