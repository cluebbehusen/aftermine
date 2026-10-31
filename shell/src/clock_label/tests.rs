use super::*;

#[test]
fn clock_grows_an_hour_field_only_when_needed() {
    assert_eq!(seconds(0), "0:00");
    assert_eq!(seconds(65), "1:05");
    assert_eq!(seconds(3599), "59:59");
    assert_eq!(seconds(3600), "1:00:00");
    assert_eq!(seconds(3600 * 2 + 61), "2:01:01");
}

#[test]
fn match_time_truncates_partial_seconds() {
    let per_second = u64::from(aftermine_sim::TICKS_PER_SECOND);
    assert_eq!(ticks(0), "0:00");
    assert_eq!(ticks(per_second - 1), "0:00");
    assert_eq!(ticks(per_second * 125), "2:05");
}
