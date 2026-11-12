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
        backdrop
            .session
            .advance_frame(FrameTime::measure(0.0, None), crate::render::viewport());
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

/// This scene's view rests at the map's edge and leaves it in the first second.
#[test]
fn nothing_hops_against_the_glide() {
    crate::render::set_viewport(1440.0, 900.0);
    let viewport = crate::render::viewport();
    let path = std::path::Path::new("..").join(SCENES[0]);
    let recording = aftermine_kit::load_recording(&path).expect("the first scene loads");
    let mut backdrop = HomeBackdrop::open(recording).expect("the first scene opens");
    let mut last: Option<Vec<Vec2>> = None;
    for _ in 0..300 {
        backdrop.advance(1.0 / 60.0, false);
        let camera = &backdrop.session.presentation.camera;
        let shown_center = camera.center + backdrop.slip / camera.zoom;
        let truth = viewport * 0.5 - shown_center * camera.zoom;
        let errors: Vec<Vec2> = (0..200)
            .map(|tile| camera.to_screen(vec2(tile as f32, tile as f32)).floor())
            .chain(std::iter::once(camera.to_screen(Vec2::ZERO).round()))
            .map(|snapped| snapped - backdrop.slip - truth)
            .collect();
        if let Some(last) = &last {
            for (now, then) in errors.iter().zip(last) {
                assert!(
                    (*now - *then).abs().max_element() < 0.01,
                    "at {}",
                    backdrop.age
                );
            }
        }
        last = Some(errors);
    }
}

#[test]
fn a_blast_never_jolts_the_view() {
    crate::render::set_viewport(1440.0, 900.0);
    let path = std::path::Path::new("..").join(SCENES[0]);
    let recording = aftermine_kit::load_recording(&path).expect("the first scene loads");
    let mut backdrop = HomeBackdrop::open(recording).expect("the first scene opens");
    backdrop.session.presentation.camera.kick(9.0);
    backdrop.advance(0.016, false);
    let camera = &backdrop.session.presentation.camera;
    assert_eq!(camera.to_screen(camera.center), vec2(720.0, 450.0));
}

#[test]
fn a_refused_match_stays_refused() {
    let mut home = HomeMatch::Refused;
    assert!(home.playing(true).is_none());
    home.unload();
    assert!(matches!(home, HomeMatch::Refused));
}

#[test]
fn a_center_splits_into_a_whole_pixel_center_and_what_is_left() {
    let zoom = 25.714_286;
    let viewport = vec2(1440.0, 900.0);
    for step in 0..2_000 {
        let center = vec2(40.0, 30.0) + vec2(0.0013, 0.0009) * step as f32;
        let (whole, slip) = whole_pixels(center, zoom, viewport);
        assert!((0.0..=1.0).contains(&slip.x) && (0.0..=1.0).contains(&slip.y));
        assert!(((whole - center) * zoom + slip).length() < 0.01);
    }
}

/// The glide relies on this: between whole-pixel centers, everything the world
/// snaps moves by the same amount, so the canvas never shimmers. That covers the
/// tiles, which floor, and the ground, which rounds where world `(0, 0)` lands.
/// Windows of odd sizes put the screen's center on a half pixel.
#[test]
fn whole_pixel_centers_move_snapped_tiles_rigidly() {
    for viewport in [
        vec2(1440.0, 900.0),
        vec2(1133.0, 744.0),
        vec2(2560.0, 1441.0),
    ] {
        let zoom = (viewport.x / TILES_ACROSS).max(LEAST_ZOOM);
        let snapped = |center: Vec2| -> Vec<Vec2> {
            let mut camera = crate::camera::Camera::new(vec2(100.0, 100.0), 200, 200, viewport);
            camera.show(whole_pixels(center, zoom, viewport).0, zoom);
            (0..200)
                .map(|tile| camera.to_screen(vec2(tile as f32, tile as f32)).floor())
                .chain(std::iter::once(camera.to_screen(Vec2::ZERO).round()))
                .collect()
        };
        let start = snapped(vec2(90.0, 70.0));
        for pixels in 1..400 {
            let moved = vec2(90.0, 70.0) + Vec2::splat(pixels as f32 / zoom);
            let shift = snapped(moved);
            let first = shift[0] - start[0];
            for (now, then) in shift.iter().zip(&start) {
                assert_eq!(*now - *then, first, "{viewport} after {pixels} pixels");
            }
        }
    }
}
