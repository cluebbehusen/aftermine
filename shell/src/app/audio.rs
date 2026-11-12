//! The frame's sound: whichever session is visible feeds the mixer, and the
//! score follows the screen.

use super::{App, Screen};
use crate::game::SoundKind;
use macroquad::prelude::Vec2;

/// Mixes and plays this frame's queued sounds and steers the score.
pub(super) fn frame(app: &mut App, screen: &mut Screen, dt: f32) {
    // The mixer serves whichever session is visible; playback owns a
    // separate presentation game and therefore a separate sound queue.
    let presentation = screen.visible_presentation(&mut app.game);
    let mut queued: Vec<(SoundKind, Option<Vec2>)> = std::mem::take(&mut app.ui_sounds);
    queued.append(&mut presentation.sounds_pending);
    let fogged_blasts = std::mem::take(&mut presentation.fogged_blasts);
    let camera = &presentation.camera;
    let (cam_center, cam_half_extents, cam_zoom) = (
        camera.center,
        camera.viewport() / camera.zoom * 0.5,
        camera.zoom,
    );
    let motor_running = screen.motors_run(
        &app.game,
        super::screen_flow::held_for_controller(&app.game, &app.input),
    );
    let motor_game = screen.visible(&app.game).view();
    app.mixer.rocket_loops.update(
        &motor_game,
        motor_running,
        &app.sounds.rocket_motors,
        app.config.volumes.master
            * app.config.volumes.effects
            * crate::mixer::Mixer::base_volume(SoundKind::RocketMotor),
    );
    let effects = app.config.volumes.master * app.config.volumes.effects;
    crate::strike_audio::update(
        &mut app.mixer.warhead_loops,
        &mut app.mixer.interceptor_loops,
        &motor_game,
        motor_running,
        (&app.sounds.warhead_motors, &app.sounds.interceptor_motors),
        (
            effects * crate::mixer::Mixer::base_volume(SoundKind::WarheadMotor),
            effects * crate::mixer::Mixer::base_volume(SoundKind::InterceptorMotor),
        ),
    );
    let maw_combat = crate::maw_audio::update(
        &mut app.mixer.maw_cut_loops,
        &mut app.mixer.maw_kiln_loops,
        &motor_game,
        motor_running,
        (&app.sounds.maw_cuts, &app.sounds.maw_kilns),
        (
            effects * crate::mixer::Mixer::base_volume(SoundKind::MawCut),
            effects * crate::mixer::Mixer::base_volume(SoundKind::MawKiln),
        ),
    );
    let mixed = crate::audio_mix::frame_mix(
        queued,
        fogged_blasts,
        cam_center,
        cam_half_extents,
        cam_zoom,
    );
    let combat_impulse = maw_combat || mixed.iter().any(|event| event.combat);
    for event in mixed {
        app.mixer
            .play(&app.sounds, event.kind, &app.config.volumes, event.gain);
    }
    if screen.kind().scored()
        && let Some(soundtrack) = &mut app.soundtrack
    {
        soundtrack.update(
            screen.soundtrack_scene(&app.game),
            combat_impulse,
            dt,
            app.config.volumes,
        );
        soundtrack.apply(&app.sounds);
    }
}
