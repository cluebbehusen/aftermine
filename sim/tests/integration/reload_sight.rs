//! The reload a gun shows: every seat that sees a machine reads how long each
//! of its weapons still waits, and a weapon that reads zero fires next.

use crate::common;
use common::{building_id, cmd, open_arena_with, orders, run_until, unit};

use aftermine_sim::observation::ObservationData;
use aftermine_sim::scenario::BuildingSpec;
use aftermine_sim::stats::BuildingKind;
use aftermine_sim::{Event, PlayerId, Target, UnitKind};

#[test]
fn a_weapon_shown_ready_fires_on_the_next_tick_and_hostiles_in_sight_read_it_too() {
    let mut scenario = open_arena_with(
        24,
        12,
        vec![
            unit(1, UnitKind::Lancer, 8, 5),
            unit(0, UnitKind::Harvester, 9, 8),
        ],
        |_| {},
    );
    scenario.buildings = vec![BuildingSpec {
        player: 0,
        kind: BuildingKind::Barricade,
        x: 4,
        y: 5,
    }];
    let mut state = scenario.build().unwrap();
    let lancer = state.units()[0].id;
    let wall = building_id(&state, |b| b.kind == BuildingKind::Barricade);
    state.tick(&[cmd(1, orders::attack([lancer], Target::Building(wall)))]);
    run_until(&mut state, 400, |_, events| {
        events
            .iter()
            .any(|event| matches!(event, Event::AttackHit { .. }))
    });
    let reload = |state: &aftermine_sim::State, seat: u8| {
        let obs = ObservationData::fog_honest(state, PlayerId(seat));
        obs.my_units
            .iter()
            .chain(&obs.enemy_units)
            .find(|u| u.id == lancer)
            .map(|u| u.reload[0])
    };
    let own = reload(&state, 1).expect("its owner sees it");
    assert_eq!(
        own,
        UnitKind::Lancer.stats().weapons[0].cooldown_ticks - 1,
        "a gun that just fired waits its whole reload but the tick it fires"
    );
    assert_eq!(
        reload(&state, 0),
        Some(own),
        "a hostile in sight reads it too"
    );
    while reload(&state, 1) != Some(0) {
        state.tick(&[]);
    }
    let report = state.tick(&[]);
    assert!(
        report
            .events
            .iter()
            .any(|event| matches!(event, Event::AttackHit { attacker, .. } if *attacker == lancer)),
        "a weapon shown ready fires on the next tick"
    );
}
