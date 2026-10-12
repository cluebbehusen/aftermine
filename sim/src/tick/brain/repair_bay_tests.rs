use super::*;

#[test]
fn the_fast_meter_adds_up_exactly_on_any_clock() {
    // 3% of 650 hp per second is 7.8 hp a pulse, so any five pulses heal 39.
    let total = |from: u64| {
        (from..from + 5)
            .map(|pulse| fast_step(650, pulse))
            .sum::<u32>()
    };
    assert_eq!(total(0), 39);
    assert_eq!(
        total(u64::MAX - 5),
        39,
        "a late clock neither overflows nor drifts"
    );
    assert_eq!(
        fast_step(40, 0),
        crate::stats::REPAIR_BAY_STEP,
        "a light body never heals slower when calm"
    );
}
