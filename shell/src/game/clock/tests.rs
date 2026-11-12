use super::*;

#[test]
fn a_paused_picture_shows_the_last_tick_whole() {
    let mut clock = Clock {
        accum: TICK_DT * 0.25,
        ..Clock::default()
    };
    assert!((clock.render_alpha() - 0.25).abs() < 1e-6);
    clock.paused = true;
    assert!((clock.render_alpha() - 1.0).abs() < f32::EPSILON);
    assert!(
        (clock.tick_fraction() - 0.25).abs() < 1e-6,
        "the debt waits"
    );
}

#[test]
fn owed_ticks_follow_speed_and_stop_at_a_frames_cap() {
    let mut clock = Clock {
        speed: 2.0,
        ..Clock::default()
    };
    assert_eq!(clock.due_ticks(TICK_DT * 1.5, 24), 3);
    assert!(clock.accum < TICK_DT, "only the fraction stays owed");
    let mut hitch = Clock::default();
    assert_eq!(hitch.due_ticks(TICK_DT * 100.0, 24), 24);
    assert!(hitch.accum < TICK_DT, "debt past the cap is dropped");
}

#[test]
fn the_top_speed_keeps_up_at_thirty_frames_a_second() {
    let thirty = crate::pacing::Refresh::from_hz(120.0).unwrap().every(4);
    let frame = crate::frame_time::FrameTime::measure(1.0 / 30.0, Some(thirty));
    // Half a tick owed first, so float rounding cannot leave a frame short.
    let mut clock = Clock {
        speed: 64.0,
        accum: TICK_DT * 0.5,
        ..Clock::default()
    };
    let ran: u64 = (0..30)
        .map(|_| clock.due_ticks(frame.raw, frame.catch_up))
        .sum();
    assert_eq!(ran, 1280, "a second's ticks at 64x");
}
