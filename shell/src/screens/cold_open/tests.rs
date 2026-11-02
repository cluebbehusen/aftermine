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
