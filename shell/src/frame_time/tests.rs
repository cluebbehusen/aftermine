use super::*;

#[test]
fn frame_time_bounds_presentation_and_sanitizes_the_clock() {
    let ordinary = FrameTime::measure(0.016, None);
    assert_eq!(ordinary.presentation, 0.016);
    assert_eq!(ordinary.raw, 0.016);
    let suspended = FrameTime::measure(60.0, None);
    assert_eq!(suspended.presentation, MAX_PRESENTATION_DT);
    assert_eq!(
        suspended.raw, 60.0,
        "the sim clocks cap their own catch-up and still see the gap"
    );
    for garbage in [-1.0, f32::NAN, f32::INFINITY] {
        let time = FrameTime::measure(garbage, None);
        assert_eq!((time.presentation, time.raw), (0.0, 0.0));
    }
}

#[test]
fn catch_up_and_the_seek_budget_follow_the_paced_rate() {
    let paced = |hz: f64, n: u32| Some(pacing::Refresh::from_hz(hz).unwrap().every(n));
    let at = |hz, n| FrameTime::measure(1.0 / 60.0, paced(hz, n));
    assert_eq!(at(120.0, 4).catch_up, 48, "64x keeps up at 30 FPS");
    assert_eq!(at(144.0, 4).catch_up, 40);
    assert_eq!(at(40.0, 1).catch_up, 36);
    for (hz, n) in [(60.0, 1), (120.0, 2), (120.0, 1), (240.0, 1)] {
        assert_eq!(
            at(hz, n).catch_up,
            pacing::LEAST_CATCH_UP,
            "{hz} Hz every {n}"
        );
    }
    assert_eq!(
        FrameTime::measure(0.0, None).catch_up,
        pacing::LEAST_CATCH_UP
    );

    let half_frame = |time: FrameTime| time.seek_budget.as_secs_f64() * 2.0;
    for (hz, n, period) in [
        (120.0, 4, 1.0 / 30.0),
        (60.0, 1, 1.0 / 60.0),
        (120.0, 1, 1.0 / 120.0),
    ] {
        assert!(
            (half_frame(at(hz, n)) - period).abs() < 1e-6,
            "{hz} Hz every {n}"
        );
    }
    assert!(
        (half_frame(FrameTime::measure(0.0, None)) - 1.0 / 60.0).abs() < 1e-6,
        "an unknown rate budgets as 60 Hz"
    );
}
