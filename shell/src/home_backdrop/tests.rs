use super::*;

fn open(scene: &str) -> (aftermine_kit::playback::Playback, u64) {
    let path = std::path::Path::new("..").join(scene);
    let recording =
        aftermine_kit::load_recording(&path).unwrap_or_else(|error| panic!("{scene}: {error}"));
    let checkpoint = recording.keyframes.ticks().next().expect(scene);
    (
        aftermine_kit::playback::Playback::open(recording).expect(scene),
        checkpoint,
    )
}

/// A checkpoint is a world encoded by this build's types, so a change to the
/// simulation's state can stop it restoring. Re-record with
/// `cargo run -p aftermine-driver -- home-replays` from the repository root.
#[test]
fn every_scene_restores_its_checkpoint_without_simulating() {
    for scene in SCENES {
        let (mut playback, checkpoint) = open(scene);
        assert!(
            playback.seek_step(checkpoint, 0),
            "{scene} has to simulate to its checkpoint, so its stored world no longer restores"
        );
        assert_eq!(playback.state.current_tick(), checkpoint, "{scene}");
    }
}

#[test]
fn every_scene_starts_in_an_undecided_fight() {
    for scene in SCENES {
        let (mut playback, checkpoint) = open(scene);
        playback.seek(checkpoint);
        assert!(
            aftermine_kit::fights::fiercest(&playback.state).is_some(),
            "{scene}"
        );
        assert!(playback.state.result().is_none(), "{scene} is already over");
    }
}

#[test]
fn automation_always_shows_the_first_scene() {
    assert_eq!(choose(true), 0);
    assert!(choose(false) < SCENES.len());
}

#[test]
fn a_scene_starts_over_at_its_end() {
    crate::render::set_viewport(1440.0, 900.0);
    let path = std::path::Path::new("..").join(SCENES[0]);
    let recording = aftermine_kit::load_recording(&path).expect("the first scene loads");
    let mut backdrop = HomeBackdrop::open(recording).expect("the first scene opens");
    let checkpoint = backdrop.checkpoint;
    backdrop.session.seek_to(u64::MAX);
    while backdrop.session.ticks_left() > 0 {
        backdrop.session.advance_frame(
            FrameTime {
                presentation: 0.0,
                raw: 0.0,
            },
            crate::render::viewport(),
        );
    }
    backdrop.age = 30.0;
    backdrop.session.presentation.camera.center += vec2(30.0, 30.0);
    backdrop.advance(0.016, false);
    assert!(backdrop.age < 1.0);
    assert!(backdrop.session.ticks_left() > 0);
    assert_eq!(backdrop.session.view().state.current_tick(), checkpoint);
    let center = backdrop.session.presentation.camera.center;
    assert!(center.distance(backdrop.anchor) < 1.0, "{center:?}");
}

#[test]
fn the_view_stays_near_the_fight_however_long_home_is_open() {
    for second in 0..7_200 {
        let offset = sway(second as f32);
        assert!(offset.x.abs() <= SWAY && offset.y.abs() <= SWAY);
    }
}

#[test]
fn reduced_motion_holds_the_match_and_the_view_still() {
    crate::render::set_viewport(1440.0, 900.0);
    let path = std::path::Path::new("..").join(SCENES[0]);
    let recording = aftermine_kit::load_recording(&path).expect("the first scene loads");
    let mut backdrop = HomeBackdrop::open(recording).expect("the first scene opens");
    let checkpoint = backdrop.checkpoint;
    for _ in 0..120 {
        backdrop.advance(0.05, true);
    }
    assert_eq!(backdrop.session.view().state.current_tick(), checkpoint);
    assert_eq!(backdrop.session.presentation.camera.center, backdrop.anchor);
}

#[test]
fn the_match_behind_home_never_queues_a_sound() {
    crate::render::set_viewport(1440.0, 900.0);
    let path = std::path::Path::new("..").join(SCENES[0]);
    let recording = aftermine_kit::load_recording(&path).expect("the first scene loads");
    let mut backdrop = HomeBackdrop::open(recording).expect("the first scene opens");
    for _ in 0..600 {
        backdrop.advance(0.05, false);
        assert!(backdrop.session.presentation.sounds_pending.is_empty());
    }
}

#[test]
fn a_refused_match_stays_refused() {
    let mut home = HomeMatch::Refused;
    assert!(home.playing(true).is_none());
    home.unload();
    assert!(matches!(home, HomeMatch::Refused));
}
