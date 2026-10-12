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

/// Skirmish at its first pulse with an own built Repair Bay and a calm
/// Breaker beside it missing `missing` hp.
fn bay_and_breaker(missing: u32) -> (State, UnitId) {
    use crate::{BuildingKind, PlayerId, UnitKind};
    use chassis::fx::{Fx, Vec2Fx};
    use chassis::grid::TilePos;
    let mut state = crate::Scenario::skirmish().build().unwrap();
    state.place_building(PlayerId(0), BuildingKind::RepairBay, TilePos::new(12, 8));
    let beside = Vec2Fx::new(
        Fx::from_num(14) + Fx::lit("0.5"),
        Fx::from_num(8) + Fx::lit("0.5"),
    );
    let breaker = state.spawn_unit(PlayerId(0), UnitKind::Breaker, beside);
    let unit = state.unit_mut(breaker).unwrap();
    unit.hp = UnitKind::Breaker.stats().max_hp - missing;
    assert!(
        state
            .current_tick()
            .is_multiple_of(crate::stats::REPAIR_BAY_PERIOD)
    );
    (state, breaker)
}

/// The steps the aura queues on `state` behind `queued`, given `hits`.
fn bay_steps(state: &State, hits: &[PendingHit], queued: Vec<PendingUnitHeal>) -> Vec<u32> {
    let mut state = state.clone();
    let skip = queued.len();
    let mut heals = queued;
    repair_bay_aura(&mut state, hits, &mut heals, &mut Vec::new());
    heals[skip..].iter().map(|heal| heal.step).collect()
}

#[test]
fn a_bay_prices_its_step_after_the_heals_already_queued() {
    let (state, breaker) = bay_and_breaker(5);
    assert_eq!(bay_steps(&state, &[], Vec::new()), [5], "premise");
    let weld = PendingUnitHeal {
        unit: breaker,
        step: 2,
        player: crate::PlayerId(0),
        paid: 0,
        source: crate::event::UnitRepairSource::FieldWelder { unit: breaker },
    };
    assert_eq!(
        bay_steps(&state, &[], vec![weld]),
        [3],
        "the Bay pays only for the room the weld leaves"
    );
}

#[test]
fn a_patient_hit_on_the_pulse_takes_the_slow_step() {
    let (state, breaker) = bay_and_breaker(400);
    let at = state.unit(breaker).unwrap().pos;
    let hit = PendingHit {
        attacker: Target::Unit(breaker),
        victim: Target::Unit(breaker),
        damage: 10,
        impact: at,
        approach: chassis::fx::Vec2Fx::ZERO,
        origin: at,
    };
    assert!(
        bay_steps(&state, &[], Vec::new())[0] > crate::stats::REPAIR_BAY_STEP,
        "premise: a calm Breaker heals fast"
    );
    assert_eq!(
        bay_steps(&state, &[hit], Vec::new()),
        [crate::stats::REPAIR_BAY_STEP]
    );
}
