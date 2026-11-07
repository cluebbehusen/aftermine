use super::*;

#[test]
fn the_report_runs_to_its_end_without_a_press() {
    let mut screen = ColdOpenScreen::default();
    assert!(!screen.update(false, END - 0.1));
    assert!(screen.update(false, 0.1));
}

#[test]
fn any_press_skips_the_report() {
    let mut screen = ColdOpenScreen::default();
    assert!(screen.update(true, 0.0));
}

#[test]
fn every_row_appears_in_order_before_the_cut() {
    let mut last = HEADER_AT;
    for (_, _, at) in ROWS {
        assert!(at > last);
        last = at;
    }
    assert!(last < END);
}

#[test]
fn a_controller_button_skips_the_report_and_a_stick_nudge_does_not() {
    // The frame loop skips on any press `hints::is_press` counts.
    use aftermine_protocol::{PadButton, RawEvent, Stick};
    for button in [PadButton::South, PadButton::East, PadButton::Start] {
        assert!(crate::hints::is_press(&RawEvent::PadDown {
            pad: 0,
            button
        }));
    }
    assert!(!crate::hints::is_press(&RawEvent::PadStick {
        pad: 0,
        stick: Stick::Left,
        x: 0.0,
        y: 1.0,
    }));
}
