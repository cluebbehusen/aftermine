use super::*;

fn run(motion: &mut MawMotion, from: u64, ticks: u64, cutting: bool, feeding: bool) -> u64 {
    for tick in from..from + ticks {
        motion.observe(tick, cutting, feeding);
    }
    from + ticks
}

#[test]
fn an_idle_maw_stays_stowed_and_still() {
    let mut motion = MawMotion::new(0);
    run(&mut motion, 1, 200, false, false);
    assert_eq!(motion.deploy(1.0), 0.0);
    assert_eq!(motion.spin(1.0), 0.0);
    assert_eq!(motion.belt(1.0), 0.0);
    assert_eq!(motion.heat(1.0), 0.0);
    assert_eq!(motion.loads(1.0).count(), 0);
}

#[test]
fn cutting_lowers_and_spins_the_drum_but_leaves_the_belt_and_kiln() {
    let mut motion = MawMotion::new(0);
    run(&mut motion, 1, 40, true, false);
    assert_eq!(motion.deploy(1.0), 1.0);
    assert!(motion.spin(1.0) > 0.0);
    assert_eq!(motion.belt(1.0), 0.0);
    assert_eq!(motion.heat(1.0), 0.0);
    assert_eq!(motion.loads(1.0).count(), 0);
}

#[test]
fn feeding_carries_scrap_from_the_hood_into_a_burning_kiln() {
    let mut motion = MawMotion::new(0);
    run(&mut motion, 1, 120, false, true);
    assert_eq!(motion.deploy(1.0), 1.0);
    assert_eq!(motion.heat(1.0), 1.0);
    let rows: Vec<f32> = motion.loads(1.0).map(|(row, _)| row).collect();
    assert!(rows.len() > 2, "loads ride the belt: {rows:?}");
    assert!(rows.iter().all(|along| (0.0..BELT_RUN).contains(along)));
    let variants: std::collections::BTreeSet<usize> = motion.loads(1.0).map(|(_, v)| v).collect();
    assert_eq!(variants.len(), rows.len(), "each load is its own clump");
}

#[test]
fn the_belt_runs_until_its_last_load_is_in_the_kiln_then_everything_settles() {
    let mut motion = MawMotion::new(0);
    let tick = run(&mut motion, 1, 60, false, true);
    let tick = run(&mut motion, tick, 1, false, false);
    assert!(
        motion.loads(1.0).count() > 0,
        "loads already on the belt keep going"
    );
    let carried = motion.belt(1.0);
    let tick = run(&mut motion, tick, 20, false, false);
    assert!(
        motion.belt(1.0) > carried,
        "the belt still runs while it carries"
    );
    let tick = run(&mut motion, tick, 200, false, false);
    assert_eq!(motion.loads(1.0).count(), 0);
    let stopped = motion.belt(1.0);
    run(&mut motion, tick, 10, false, false);
    assert_eq!(motion.belt(1.0), stopped, "an empty belt stops");
    assert_eq!(
        motion.heat(1.0),
        0.0,
        "the kiln cools once it has nothing to burn"
    );
    assert_eq!(motion.deploy(1.0), 0.0, "the drum stows");
}

#[test]
fn a_repeated_tick_changes_nothing_and_alpha_interpolates_within_it() {
    let mut motion = MawMotion::new(0);
    run(&mut motion, 1, 10, true, true);
    let spin = motion.spin(1.0);
    motion.observe(10, true, true);
    assert_eq!(motion.spin(1.0), spin);
    let before = motion.spin(0.0);
    let between = motion.spin(0.5);
    assert!(before < between && between < spin);
}

#[test]
fn a_maw_first_seen_at_work_shows_that_work_at_once() {
    // A seek lands on a feeding Maw: its first frame is already the steady
    // state a while of feeding reaches, scrap spaced along the whole belt.
    let rows = |motion: &MawMotion| -> Vec<f32> {
        let mut rows: Vec<f32> = motion.loads(1.0).map(|(along, _)| along).collect();
        rows.sort_by(f32::total_cmp);
        rows
    };
    let spaced = |rows: &[f32]| {
        rows.windows(2)
            .all(|pair| (pair[1] - pair[0] - LOAD_SPACING).abs() < 1e-3)
    };
    let mut fed = MawMotion::settled(0, false, true);
    assert_eq!(fed.deploy(1.0), 1.0);
    assert_eq!(fed.heat(1.0), 1.0);
    let full = rows(&fed);
    assert!(
        spaced(&full) && full[0] < LOAD_SPACING && full[full.len() - 1] + LOAD_SPACING >= BELT_RUN,
        "a full belt: {full:?}"
    );
    run(&mut fed, 1, 30, false, true);
    let later = rows(&fed);
    assert!(
        spaced(&later),
        "new loads keep the belt's spacing: {later:?}"
    );

    let cutting = MawMotion::settled(0, true, false);
    assert_eq!(cutting.deploy(1.0), 1.0);
    assert_eq!(cutting.heat(1.0), 0.0);
    assert_eq!(cutting.loads(1.0).count(), 0);

    let idle = MawMotion::settled(0, false, false);
    assert_eq!(idle.deploy(1.0), 0.0);
    assert_eq!(idle.loads(1.0).count(), 0);
}

#[test]
fn the_drum_grinds_from_its_first_hit_and_falls_quiet_once_it_stops_cutting() {
    let mut motion = MawMotion::new(0);
    let tick = run(&mut motion, 1, 1, true, false);
    assert_eq!(motion.grind(0.0), 0.0);
    assert_eq!(motion.grind(1.0), 0.5, "it swells over two ticks");
    let tick = run(&mut motion, tick, 1, true, false);
    assert_eq!(motion.grind(1.0), 1.0);
    let tick = run(&mut motion, tick, 30, true, true);
    assert_eq!(motion.grind(1.0), 1.0, "it holds while the drum cuts");
    let tick = run(&mut motion, tick, 1, false, true);
    let fading = motion.grind(1.0);
    assert!(fading > 0.0 && fading < 1.0, "it dies away: {fading}");
    run(&mut motion, tick, 5, false, true);
    assert_eq!(
        motion.grind(1.0),
        0.0,
        "silent once it stops cutting, though the kiln still feeds"
    );
    assert_eq!(MawMotion::settled(0, true, false).grind(0.0), 1.0);
    assert_eq!(MawMotion::settled(0, false, true).grind(0.0), 0.0);
}

#[test]
fn the_belt_speed_eases_up_and_runs_on_until_its_last_load_is_in() {
    let mut motion = MawMotion::new(0);
    assert_eq!(motion.belt_speed(1.0), 0.0);
    let tick = run(&mut motion, 1, 1, false, true);
    let starting = motion.belt_speed(1.0);
    assert!(starting > 0.0 && starting < 1.0, "it eases up: {starting}");
    assert!(motion.belt_speed(0.5) < starting, "and between ticks");
    let tick = run(&mut motion, tick, 40, false, true);
    assert_eq!(motion.belt_speed(1.0), 1.0);
    let tick = run(&mut motion, tick, 20, false, false);
    assert_eq!(motion.belt_speed(1.0), 1.0, "it runs on while it carries");
    run(&mut motion, tick, 60, false, false);
    assert_eq!(motion.belt_speed(1.0), 0.0);
    assert_eq!(MawMotion::settled(0, false, true).belt_speed(0.0), 1.0);
}
