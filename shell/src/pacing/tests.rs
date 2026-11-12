use super::*;

fn display(hz: f64) -> Refresh {
    Refresh::from_hz(hz).expect("a real display's refresh")
}

/// The choices at `hz`, as their labels' numbers.
fn offered(hz: f64) -> Vec<u32> {
    choices(display(hz)).into_iter().map(Rate::fps).collect()
}

#[test]
fn a_display_offers_its_refresh_and_whole_divisions_down_to_thirty() {
    assert_eq!(offered(120.0), [120, 60, 40, 30]);
    assert_eq!(offered(144.0), [144, 72, 48, 36]);
    assert_eq!(offered(165.0), [165, 83, 55, 41]);
    assert_eq!(offered(240.0), [240, 120, 80, 60]);
    assert_eq!(offered(60.0), [60, 30]);
    assert_eq!(offered(59.94), [60, 30], "29.97 still counts as thirty");
    assert_eq!(
        choices(display(59.94)),
        [Rate(59_940), Rate(29_970)],
        "the fraction survives"
    );
    assert_eq!(offered(50.0), [50]);
    assert_eq!(offered(40.0), [40]);
    assert_eq!(offered(24.0), [24], "a display under thirty offers itself");
}

#[test]
fn only_a_reading_a_display_could_give_is_a_refresh() {
    for nonsense in [f64::NAN, f64::INFINITY, -60.0, 0.0, 19.9, 1000.1] {
        assert_eq!(Refresh::from_hz(nonsense), None, "{nonsense}");
    }
    assert_eq!(display(20.0).every(1), Rate(20_000));
    assert_eq!(display(1000.0).every(1), Rate(1_000_000));
}

#[test]
fn a_stored_cap_follows_the_display_it_lands_on() {
    let capped = FrameRate::Capped(60);
    for (hz, rate) in [
        (120.0, 60),
        (144.0, 48),
        (240.0, 60),
        (60.0, 60),
        (40.0, 40),
    ] {
        let refresh = display(hz);
        assert_eq!(
            refresh.every(divisor(capped, refresh)).fps(),
            rate,
            "{hz} Hz"
        );
    }
    assert_eq!(
        divisor(FrameRate::Capped(30), display(144.0)),
        4,
        "no choice fits, so the slowest"
    );
    assert_eq!(divisor(FrameRate::Capped(1000), display(144.0)), 1);
    assert_eq!(divisor(FrameRate::Full, display(240.0)), 1);
}

#[test]
fn every_choice_stores_a_setting_that_picks_it_again() {
    for hz in [59.94, 60.0, 120.0, 144.0, 165.0, 240.0, 40.0, 50.0, 24.0] {
        let refresh = display(hz);
        for n in 1..=u32::try_from(choices(refresh).len()).unwrap() {
            let setting = setting_at(refresh, n);
            assert_eq!(setting == FrameRate::Full, n == 1, "{hz} Hz, every {n}");
            assert_eq!(divisor(setting, refresh), n, "{hz} Hz, every {n}");
        }
    }
    assert_eq!(setting_at(display(59.94), 2), FrameRate::Capped(30));
}

#[test]
fn a_setting_reads_as_the_rate_it_gives() {
    assert_eq!(label(FrameRate::Full, Some(display(120.0))), "120 FPS");
    assert_eq!(label(FrameRate::Full, Some(display(59.94))), "60 FPS");
    assert_eq!(label(FrameRate::Capped(30), Some(display(59.94))), "30 FPS");
    assert_eq!(label(FrameRate::Capped(60), Some(display(144.0))), "48 FPS");
    assert_eq!(
        label(FrameRate::Capped(60), None),
        "60 FPS",
        "with no display known, a cap reads as itself"
    );
    assert_eq!(label(FrameRate::Full, None), "Full");
}

#[test]
fn a_still_screen_idles_at_the_slowest_choice_and_harnesses_run_uncapped() {
    let refresh = Some(display(120.0));
    assert_eq!(pace(FrameRate::Full, refresh, false, false), 1);
    assert_eq!(pace(FrameRate::Capped(60), refresh, false, false), 2);
    assert_eq!(pace(FrameRate::Full, refresh, true, false), 4, "idle at 30");
    assert_eq!(pace(FrameRate::Capped(40), refresh, true, false), 4);
    assert_eq!(
        pace(FrameRate::Full, Some(display(40.0)), true, false),
        1,
        "nothing slower to drop to"
    );
    for still in [false, true] {
        assert_eq!(
            pace(FrameRate::Capped(30), refresh, still, true),
            1,
            "uncapped"
        );
        assert_eq!(
            pace(FrameRate::Capped(30), None, still, false),
            1,
            "unknown refresh"
        );
    }
}

#[test]
fn a_screen_turns_quiet_a_second_after_the_last_input() {
    let ms = std::time::Duration::from_millis;
    let mut quiet = Quiet::default();
    assert!(quiet.quiet(ms(0)), "nothing heard yet");
    quiet.hear(ms(500));
    assert!(!quiet.quiet(ms(500)));
    assert!(!quiet.quiet(ms(1_499)));
    assert!(quiet.quiet(ms(1_500)));
    quiet.hear(ms(1_600));
    assert!(!quiet.quiet(ms(2_000)), "new input starts the wait over");
    assert!(quiet.quiet(ms(2_600)));
}
