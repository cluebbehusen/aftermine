use super::*;

fn motor(key: u32) -> Motor<u32> {
    Motor { key, gain: 0.7 }
}

#[test]
fn overlapping_loops_stop_independently_and_do_not_retrigger_each_frame() {
    let mut loops = LoopBank::<u32, 16>::default();
    let (a, b) = (motor(1), motor(2));
    assert_eq!(
        loops.reconcile(&[a, b]),
        vec![Change::Start(0, 0.7), Change::Start(1, 0.7)]
    );
    assert_eq!(
        loops.reconcile(&[b]),
        vec![Change::Stop(0), Change::Gain(1, 0.7)]
    );
    assert_eq!(loops.reconcile(&[b]), vec![Change::Gain(1, 0.7)]);
    assert_eq!(loops.reconcile(&[]), vec![Change::Stop(1)]);
    assert!(loops.reconcile(&[]).is_empty());
}

#[test]
fn pause_or_seek_clears_loops_and_resuming_reconstructs_only_live_ones() {
    let mut loops = LoopBank::<u32, 16>::default();
    loops.reconcile(&[motor(1)]);
    assert_eq!(loops.reconcile(&[]), vec![Change::Stop(0)]);
    assert_eq!(loops.reconcile(&[motor(2)]), vec![Change::Start(0, 0.7)]);
}
