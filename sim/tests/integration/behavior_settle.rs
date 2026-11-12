//! Crowd settling: measured displacement after units reach steady state.

use crate::common;

use aftermine_sim::UnitKind;
use chassis::grid::TilePos;

use common::*;

/// Sum of per-unit displacement magnitudes over one tick, in
/// thousandths of a tile (integer — the probe must not do float math
/// on sim values).
fn tick_displacement(state: &mut aftermine_sim::State) -> u64 {
    let before: Vec<_> = state.units().iter().map(|u| (u.id, u.pos)).collect();
    state.tick(&[]);
    let mut total = 0u64;
    for (id, old) in before {
        if let Some(u) = state.units().iter().find(|u| u.id == id) {
            let d = u.pos - old;
            let sq = d.length_sq();
            // Raw Q32.32 bits: zero means EXACTLY stationary, and the
            // probe only compares relative magnitudes.
            total += u64::try_from(sq.to_bits()).unwrap_or(0);
        }
    }
    total
}

#[test]
fn a_parked_crowd_goes_fully_stationary() {
    let units: Vec<_> = (0..10)
        .map(|i| unit(0, UnitKind::Harvester, 2 + (i % 5), 5 + (i / 5)))
        .collect();
    let mut state = arena(units).build().unwrap();
    let ids: Vec<_> = state.units().iter().map(|u| u.id).collect();
    state.tick(&[cmd(0, orders::run(ids, TilePos::new(9, 2)))]);
    // Ample time to arrive and settle (the arena is 16x9).
    for _ in 0..1_200 {
        state.tick(&[]);
    }
    let mut still = 0;
    let mut moving_ticks = Vec::new();
    for t in 0..200 {
        let d = tick_displacement(&mut state);
        if d == 0 {
            still += 1;
        } else {
            moving_ticks.push((t, d));
        }
    }
    assert!(
        state
            .units()
            .iter()
            .all(|u| matches!(u.order, aftermine_sim::Order::Idle)),
        "precondition: everyone arrived and idles"
    );
    assert_eq!(
        still,
        200,
        "a parked crowd still vibrates: {} moving ticks, first {:?}",
        moving_ticks.len(),
        moving_ticks.first()
    );
}
