use super::*;

#[test]
fn drones_fill_the_rear_pair_first_and_leave_from_the_front() {
    let mut docks = DockMotion::settled(6);
    assert!((0..6).all(|dock| docks.resting(dock, 0.0) && docks.spread(dock, 0.0) == 0.0));

    let left = docks.observe(10, 2, &[40, 41, 42, 43], &[]);
    assert_eq!(left, vec![(40, 5), (41, 0), (42, 4), (43, 1)]);
    for dock in [2, 3] {
        assert!(docks.resting(dock, 12.0), "the rear pair stays docked");
    }
    for dock in [0, 1, 4, 5] {
        assert!(!docks.resting(dock, 10.0));
        assert_eq!(docks.spread(dock, 10.0), 0.0, "clamps start shut");
        assert_eq!(docks.spread(dock, 14.0), 1.0, "and swing open");
    }
}

#[test]
fn a_landing_drone_glides_in_before_its_clamps_close() {
    let mut docks = DockMotion::settled(0);
    assert_eq!(docks.spread(2, 0.0), 1.0, "empty docks stand open");
    let from = Vec2::new(3.0, 4.0);
    assert!(docks.observe(20, 1, &[], &[from]).is_empty());
    let landing = docks
        .landing(2, 20.0)
        .expect("the first drone lands in the rear");
    assert_eq!(landing.from, from);
    assert!(!docks.resting(2, 20.0));
    assert!(docks.landing(2, 20.0 + LAND_TICKS).is_none());
    assert!(docks.resting(2, 20.0 + LAND_TICKS));
    assert_eq!(docks.spread(2, 24.0), 1.0, "clamps wait for it to settle");
    assert_eq!(docks.spread(2, 30.0), 0.0, "then close");
    assert!(
        (0..6)
            .filter(|&dock| dock != 2)
            .all(|dock| docks.landing(dock, 21.0).is_none())
    );
}

#[test]
fn a_launched_drone_rises_clear_of_its_dock() {
    assert_eq!(launch_progress(0.0), 0.0);
    assert!(launch_progress(LAUNCH_TICKS * 0.5) > 0.0);
    assert_eq!(launch_progress(LAUNCH_TICKS), 1.0);
}
