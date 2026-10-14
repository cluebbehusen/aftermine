use super::*;

fn frame(seconds: f32, played: bool) -> Frame {
    Frame {
        played: if played { seconds } else { 0.0 },
        minimized: Some(false),
        unsaved: true,
    }
}

const NOW: Duration = Duration::from_secs(100);

#[test]
fn only_live_play_counts_toward_the_minute() {
    let mut schedule = Schedule::default();
    for _ in 0..1000 {
        schedule.observe(frame(0.25, false));
    }
    assert_eq!(schedule.due(false, true, NOW), None, "menus and pauses");
    for _ in 0..239 {
        schedule.observe(frame(0.25, true));
    }
    assert_eq!(schedule.due(false, true, NOW), None);
    schedule.observe(frame(0.25, true));
    assert_eq!(schedule.due(false, true, NOW), Some(Reason::Periodic));
    assert_eq!(
        schedule.due(false, false, NOW),
        None,
        "a decided or saved session owes no autosave"
    );
    schedule.started();
    assert_eq!(schedule.due(false, true, NOW), None);
}

#[test]
fn going_inactive_saves_at_once_unless_nothing_is_new() {
    let mut schedule = Schedule::default();
    schedule.observe(Frame {
        minimized: Some(true),
        ..frame(0.016, true)
    });
    assert_eq!(schedule.due(false, true, NOW), Some(Reason::Inactive));
    assert!(schedule.inactive());
    schedule.started();
    schedule.observe(Frame {
        minimized: Some(true),
        ..frame(0.016, false)
    });
    assert_eq!(
        schedule.due(false, true, NOW),
        None,
        "staying inactive is not a new edge"
    );
    schedule.observe(frame(0.016, false));
    schedule.observe(Frame {
        minimized: Some(true),
        unsaved: false,
        ..frame(0.016, false)
    });
    assert!(!schedule.inactive(), "a saved session has nothing to add");
    schedule.observe(Frame {
        minimized: None,
        ..frame(0.016, false)
    });
    schedule.observe(Frame {
        minimized: Some(true),
        ..frame(0.016, false)
    });
    assert_eq!(
        schedule.due(false, true, NOW),
        Some(Reason::Inactive),
        "an unknown state is not minimized"
    );
}

#[test]
fn a_decided_match_records_first() {
    let mut schedule = Schedule::default();
    schedule.observe(Frame {
        minimized: Some(true),
        ..frame(PERIOD, true)
    });
    assert_eq!(schedule.due(true, false, NOW), Some(Reason::Decided));
}

#[test]
fn a_failure_retries_after_a_delay_and_warns_once() {
    let mut schedule = Schedule::default();
    schedule.observe(frame(PERIOD, true));
    schedule.started();
    assert!(schedule.failed(NOW), "the first failure is reported");
    assert_eq!(schedule.due(true, true, NOW + RETRY / 2), None);
    assert_eq!(
        schedule.due(false, true, NOW + RETRY),
        Some(Reason::Periodic),
        "the failed autosave is owed again"
    );
    schedule.started();
    assert!(!schedule.failed(NOW + RETRY), "repeats stay quiet");
    schedule.succeeded();
    assert_eq!(
        schedule.due(true, false, NOW + RETRY),
        Some(Reason::Decided)
    );
    schedule.started();
    assert!(
        schedule.failed(NOW + RETRY * 2),
        "a new run of failures warns"
    );
}
