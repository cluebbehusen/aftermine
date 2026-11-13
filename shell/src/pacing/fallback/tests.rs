use super::*;

fn ms(millis: f64) -> Duration {
    Duration::from_secs_f64(millis / 1000.0)
}

fn display(hz: f64) -> Refresh {
    Refresh::from_hz(hz).expect("a real display's refresh")
}

/// `at`, less `by`.
fn less(at: Duration, by: Duration) -> Duration {
    at.checked_sub(by).expect("a time after the clock's start")
}

/// A frame of `took`, asked to span `interval` refreshes of a display at
/// `hz`, in a window on screen.
fn frame(took: Duration, interval: u32, hz: f64) -> Frame {
    Frame {
        took,
        interval,
        refresh: Some(display(hz)),
        hidden: false,
    }
}

/// Feeds `frame` until `over` of frame time has passed.
fn feed(fallback: &mut Fallback, frame: Frame, over: Duration) {
    let mut fed = Duration::ZERO;
    while fed < over {
        fallback.observe(frame);
        fed += frame.took;
    }
}

fn engaged(fallback: &Fallback) -> Option<Vsync> {
    match fallback.phase {
        Phase::Engaged { vsync, .. } => Some(vsync),
        Phase::Watching { .. } => None,
    }
}

/// A fallback engaged on a display at `hz`, judged from frames of `took`
/// asked to span two refreshes.
fn engaged_on(took: Duration, hz: f64) -> Fallback {
    let mut fallback = Fallback::default();
    feed(&mut fallback, frame(took, 2, hz), ms(2_000.0));
    assert!(engaged(&fallback).is_some());
    fallback
}

const LONG: f64 = 10_000.0;

#[test]
fn frames_as_far_apart_as_asked_never_engage_it() {
    let mut fallback = Fallback::default();
    for (hz, interval) in [(60.0, 2), (120.0, 4), (144.0, 3), (60.0, 1)] {
        let asked = display(hz).period() * interval;
        feed(&mut fallback, frame(asked, interval, hz), ms(LONG));
        assert_eq!(engaged(&fallback), None, "{hz} Hz, every {interval}");
    }
    // A hitch and the quick frames that catch up after it move no median.
    for _ in 0..100 {
        for took in [33.3, 33.3, 33.3, 33.3, 66.7, 1.0, 1.0, 1.0] {
            fallback.observe(frame(ms(took), 2, 60.0));
        }
    }
    assert_eq!(engaged(&fallback), None);
}

#[test]
fn a_driver_holding_frames_to_every_refresh_engages_it() {
    let mut fallback = Fallback::default();
    feed(&mut fallback, frame(ms(16.7), 2, 60.0), ms(LONG));
    assert_eq!(engaged(&fallback), Some(Vsync::EveryRefresh));
    let mut fallback = Fallback::default();
    feed(&mut fallback, frame(ms(7.0), 4, 120.0), ms(LONG));
    assert_eq!(
        engaged(&fallback),
        Some(Vsync::EveryRefresh),
        "a refresh measured short still counts as one"
    );
}

#[test]
fn a_driver_with_vsync_off_engages_it() {
    let mut fallback = Fallback::default();
    feed(&mut fallback, frame(ms(4.0), 2, 60.0), ms(LONG));
    assert_eq!(engaged(&fallback), Some(Vsync::Off));
    let mut fallback = Fallback::default();
    feed(&mut fallback, frame(ms(13.0), 2, 60.0), ms(LONG));
    assert_eq!(
        engaged(&fallback),
        Some(Vsync::Off),
        "under four fifths of a refresh"
    );
    let mut full = Fallback::default();
    feed(&mut full, frame(ms(4.0), 1, 60.0), ms(LONG));
    assert_eq!(
        engaged(&full),
        Some(Vsync::Off),
        "the full rate tells vsync off as well"
    );
}

#[test]
fn a_flood_of_frames_ends_its_check_early() {
    let mut fallback = Fallback::default();
    let flood = frame(ms(0.05), 2, 60.0);
    feed(&mut fallback, flood, SETTLE + flood.took * 2_100);
    assert_eq!(engaged(&fallback), Some(Vsync::Off));
}

#[test]
fn frames_slower_than_asked_never_engage_it() {
    let mut fallback = Fallback::default();
    // A heavy load at 40 ms against 33.3 asked, a limiter outside the game
    // at 30 FPS against 60 asked, and frames just inside four fifths.
    for (took, interval) in [(40.0, 2), (33.3, 1), (26.8, 2)] {
        feed(&mut fallback, frame(ms(took), interval, 60.0), ms(LONG));
        assert_eq!(engaged(&fallback), None, "{took} ms, every {interval}");
    }
}

#[test]
fn frames_while_hidden_are_ignored() {
    let mut fallback = Fallback::default();
    let hidden = Frame {
        hidden: true,
        ..frame(ms(1.0), 2, 60.0)
    };
    feed(&mut fallback, hidden, ms(LONG));
    assert_eq!(engaged(&fallback), None);
    // Hiding the window between fast frames starts the check over.
    let fast = frame(ms(4.0), 2, 60.0);
    feed(&mut fallback, fast, ms(1_400.0));
    fallback.observe(hidden);
    feed(&mut fallback, fast, ms(1_400.0));
    assert_eq!(engaged(&fallback), None);
    feed(&mut fallback, fast, ms(200.0));
    assert_eq!(engaged(&fallback), Some(Vsync::Off));
}

#[test]
fn a_check_waits_for_frames_to_settle_after_a_change() {
    let mut fallback = Fallback::default();
    feed(&mut fallback, frame(ms(33.3), 2, 60.0), ms(LONG));
    // The new interval takes a few frames to reach the screen, which run
    // at the old rate.
    feed(&mut fallback, frame(ms(33.3), 4, 60.0), ms(450.0));
    feed(&mut fallback, frame(ms(66.7), 4, 60.0), ms(LONG));
    assert_eq!(engaged(&fallback), None);
    // The same after a change of refresh.
    feed(&mut fallback, frame(ms(16.7), 4, 120.0), ms(450.0));
    feed(&mut fallback, frame(ms(33.3), 4, 120.0), ms(LONG));
    assert_eq!(engaged(&fallback), None);
}

#[test]
fn a_check_takes_a_second_of_settled_frames() {
    let mut fallback = Fallback::default();
    let fast = frame(ms(4.0), 2, 60.0);
    feed(&mut fallback, fast, ms(1_480.0));
    assert_eq!(engaged(&fallback), None);
    feed(&mut fallback, fast, ms(40.0));
    assert_eq!(engaged(&fallback), Some(Vsync::Off));
}

#[test]
fn once_engaged_it_stays_engaged_on_the_same_refresh() {
    let mut fallback = engaged_on(ms(16.7), 60.0);
    feed(&mut fallback, frame(ms(33.3), 2, 60.0), ms(LONG));
    feed(&mut fallback, frame(ms(66.7), 4, 60.0), ms(LONG));
    let hidden = Frame {
        hidden: true,
        ..frame(ms(1.0), 1, 60.0)
    };
    feed(&mut fallback, hidden, ms(LONG));
    assert_eq!(engaged(&fallback), Some(Vsync::EveryRefresh));
}

#[test]
fn a_change_of_refresh_checks_again() {
    let mut fallback = engaged_on(ms(16.7), 60.0);
    fallback.observe(frame(ms(16.7), 2, 144.0));
    assert_eq!(engaged(&fallback), None, "it stops pacing at once");
    feed(&mut fallback, frame(ms(13.9), 2, 144.0), ms(LONG));
    assert_eq!(engaged(&fallback), None, "the new display honors it");
    let mut fallback = engaged_on(ms(16.7), 60.0);
    feed(&mut fallback, frame(ms(4.0), 2, 144.0), ms(LONG));
    assert_eq!(engaged(&fallback), Some(Vsync::Off));
}

#[test]
fn an_unknown_refresh_never_engages_it() {
    let mut fallback = Fallback::default();
    let unknown = Frame {
        refresh: None,
        ..frame(ms(1.0), 2, 60.0)
    };
    feed(&mut fallback, unknown, ms(LONG));
    assert_eq!(engaged(&fallback), None);
    let mut fallback = engaged_on(ms(4.0), 60.0);
    fallback.observe(unknown);
    assert_eq!(
        fallback.wake(2, Some(display(60.0)), ms(0.0), ms(1.0)),
        None
    );
}

#[test]
fn nothing_sleeps_before_it_engages_or_at_the_full_rate_with_vsync() {
    let mut fallback = Fallback::default();
    feed(&mut fallback, frame(ms(33.3), 2, 60.0), ms(LONG));
    assert_eq!(
        fallback.wake(2, Some(display(60.0)), ms(0.0), ms(1.0)),
        None
    );
    let mut fallback = engaged_on(ms(16.7), 60.0);
    assert_eq!(
        fallback.wake(1, Some(display(60.0)), ms(0.0), ms(1.0)),
        None,
        "vsync already holds the full rate to the refresh"
    );
}

#[test]
fn the_full_rate_without_vsync_is_held_to_the_refresh() {
    let mut fallback = Fallback::default();
    feed(&mut fallback, frame(ms(4.0), 1, 60.0), ms(2_000.0));
    assert_eq!(engaged(&fallback), Some(Vsync::Off));
    let one = display(60.0).period();
    let start = ms(1_000.0);
    assert_eq!(
        fallback.wake(1, Some(display(60.0)), start, start + ms(3.0)),
        Some(start + one)
    );
    let began = start + one + ms(1.0);
    assert_eq!(
        fallback.wake(1, Some(display(60.0)), began, began + ms(3.0)),
        Some(start + one * 2)
    );
}

#[test]
fn a_frame_held_to_every_refresh_wakes_half_a_refresh_before_its_target() {
    let mut fallback = engaged_on(ms(16.7), 60.0);
    let period = display(60.0).period();
    let start = ms(1_000.0);
    let first = fallback.wake(2, Some(display(60.0)), start, start + ms(3.0));
    assert_eq!(first, Some(less(start + period * 2, period / 2)));
    // The swap waited for its refresh, so the next frame counts from when
    // it began.
    let waited = start + period * 2 + ms(0.2);
    assert_eq!(
        fallback.wake(2, Some(display(60.0)), waited, waited + ms(3.0)),
        Some(less(waited + period * 2, period / 2))
    );
    // A driver that queues frames ahead returns at once; the next frame
    // still aims two refreshes past the last aim.
    let last_aim = waited + period * 2;
    let queued = less(last_aim, period / 2) + ms(0.5);
    assert_eq!(
        fallback.wake(2, Some(display(60.0)), queued, queued + ms(3.0)),
        Some(less(last_aim + period * 2, period / 2))
    );
    // A frame whose work runs past its wake presents at once, and the
    // next counts from when it began.
    let aim = last_aim + period * 2;
    assert_eq!(
        fallback.wake(4, Some(display(60.0)), aim, aim + period * 4),
        None
    );
    let late = aim + period * 5;
    assert_eq!(
        fallback.wake(4, Some(display(60.0)), late, late + ms(1.0)),
        Some(less(late + period * 4, period / 2))
    );
}

#[test]
fn a_frame_without_vsync_wakes_on_an_anchored_deadline() {
    let mut fallback = engaged_on(ms(4.0), 60.0);
    let two = display(60.0).period() * 2;
    let start = ms(1_000.0);
    let first = start + two;
    assert_eq!(
        fallback.wake(2, Some(display(60.0)), start, start + ms(3.0)),
        Some(first)
    );
    // The next deadline counts from the last, not from when the frame
    // began after presenting.
    let began = first + ms(2.0);
    assert_eq!(
        fallback.wake(2, Some(display(60.0)), began, began + ms(3.0)),
        Some(first + two)
    );
    // A frame late by less than a whole frame presents at once and the
    // deadlines keep their beat.
    let behind = first + two * 2 + ms(10.0);
    assert_eq!(
        fallback.wake(2, Some(display(60.0)), less(behind, ms(5.0)), behind),
        None
    );
    assert_eq!(
        fallback.wake(2, Some(display(60.0)), behind, behind + ms(1.0)),
        Some(first + two * 3)
    );
    // A whole frame behind, the deadlines start over from now.
    let lost = first + two * 5;
    assert_eq!(
        fallback.wake(2, Some(display(60.0)), less(lost, ms(5.0)), lost),
        None
    );
    assert_eq!(
        fallback.wake(2, Some(display(60.0)), lost + ms(1.0), lost + ms(2.0)),
        Some(lost + two)
    );
}

#[test]
fn a_change_of_interval_keeps_the_deadlines_beat() {
    let mut fallback = engaged_on(ms(4.0), 60.0);
    let period = display(60.0).period();
    let start = ms(1_000.0);
    let first = start + period * 2;
    assert_eq!(
        fallback.wake(2, Some(display(60.0)), start, start),
        Some(first)
    );
    // Each frame begins a little after its deadline; the next deadline
    // still counts from the last, whatever interval it asks for.
    let began = first + ms(2.0);
    let second = first + period * 4;
    assert_eq!(
        fallback.wake(4, Some(display(60.0)), began, began + ms(1.0)),
        Some(second)
    );
    let began = second + ms(2.0);
    assert_eq!(
        fallback.wake(1, Some(display(60.0)), began, began + ms(1.0)),
        Some(second + period),
        "the full rate keeps the beat too"
    );
}

#[test]
fn a_frame_on_a_display_with_another_refresh_never_sleeps() {
    let mut fallback = engaged_on(ms(4.0), 60.0);
    let start = ms(1_000.0);
    assert_eq!(fallback.wake(2, Some(display(144.0)), start, start), None);
    assert_eq!(fallback.wake(2, None, start, start), None);
    assert!(
        fallback
            .wake(2, Some(display(60.0)), start, start)
            .is_some()
    );
}
