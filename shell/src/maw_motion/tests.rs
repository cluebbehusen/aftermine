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
