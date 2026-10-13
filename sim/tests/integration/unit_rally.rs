//! Rallying onto a unit: every newborn escorts it, the rally rides along
//! while its unit is carried, and it clears the tick its unit leaves play.

use crate::common;
use common::{cmd, open_arena, run, run_until, unit};

use chassis::grid::TilePos;
use oxide_sim::command::RejectReason;
use oxide_sim::scenario::UnitSpec;
use oxide_sim::{BuildingId, Command, Event, Order, Rally, State, UnitId, UnitKind};
use serde_json::json;

fn build(units: Vec<UnitSpec>) -> State {
    let mut scenario = open_arena(40, 20, units);
    for player in &mut scenario.players {
        player.scrap = 1_000;
    }
    scenario.build().unwrap()
}

fn foundry(state: &State) -> BuildingId {
    state
        .buildings()
        .iter()
        .find(|b| b.player.0 == 0)
        .expect("the west Foundry")
        .id
}

fn rally(building: BuildingId, rally: Option<Rally>) -> Command {
    Command::SetRally { building, rally }
}

fn train(building: BuildingId, kind: UnitKind) -> Command {
    Command::Train { building, kind }
}

/// Trains `kind` and returns the newborn at the end of its birth tick.
fn newborn(state: &mut State, building: BuildingId, kind: UnitKind) -> UnitId {
    state.tick(&[cmd(0, train(building, kind))]);
    let events = run_until(state, 2_000, |_, events| {
        events
            .iter()
            .any(|e| matches!(e, Event::UnitTrained { .. }))
    });
    events
        .iter()
        .find_map(|e| match e {
            Event::UnitTrained { unit, .. } => Some(*unit),
            _ => None,
        })
        .unwrap()
}

fn rejected(events: &[Event]) -> Vec<RejectReason> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::CommandRejected { reason, .. } => Some(*reason),
            _ => None,
        })
        .collect()
}

#[test]
fn every_newborn_escorts_the_rally_unit() {
    let mut state = build(vec![unit(0, UnitKind::Sentinel, 12, 10)]);
    let (home, guard) = (foundry(&state), state.units()[0].id);
    let report = state.tick(&[cmd(0, rally(home, Some(Rally::Unit(guard))))]);
    assert!(rejected(&report.events).is_empty());
    assert_eq!(
        state.building(home).unwrap().rally,
        Some(Rally::Unit(guard))
    );
    for kind in [UnitKind::Harvester, UnitKind::Sentinel] {
        let born = newborn(&mut state, home, kind);
        assert_eq!(
            state.unit(born).unwrap().order,
            Order::Accompany {
                unit: guard,
                weld: None
            },
            "{kind:?} escorts the rally unit from birth"
        );
    }
}

#[test]
fn a_unit_rally_rides_with_its_unit_aboard_a_transport() {
    let mut state = build(vec![
        unit(0, UnitKind::Sentinel, 12, 10),
        unit(0, UnitKind::Skyhook, 14, 10),
    ]);
    let home = foundry(&state);
    let (guard, sling) = (state.units()[0].id, state.units()[1].id);
    state.tick(&[
        cmd(0, rally(home, Some(Rally::Unit(guard)))),
        cmd(
            0,
            Command::Load {
                units: vec![guard],
                transport: sling,
                queue: false,
            },
        ),
    ]);
    run_until(&mut state, 200, |s, _| s.unit(guard).is_none());
    assert_eq!(
        state.building(home).unwrap().rally,
        Some(Rally::Unit(guard)),
        "boarding keeps the unit in play"
    );
    state.tick(&[cmd(0, rally(home, None))]);
    let report = state.tick(&[cmd(0, rally(home, Some(Rally::Unit(guard))))]);
    assert!(
        rejected(&report.events).is_empty(),
        "a rider can be rallied onto"
    );
    assert_eq!(
        state.building(home).unwrap().rally,
        Some(Rally::Unit(guard))
    );
    let born = newborn(&mut state, home, UnitKind::Sentinel);
    assert!(matches!(
        state.unit(born).unwrap().order,
        Order::Accompany { unit, .. } if unit == guard
    ));
    run(&mut state, 300);
    let (escort, carrier) = (
        state.unit(born).unwrap().tile(),
        state.unit(sling).unwrap().tile(),
    );
    assert!(
        escort.chebyshev(carrier) <= 2,
        "the newborn found its way to the carrier: {escort:?} vs {carrier:?}"
    );
}

#[test]
fn a_unit_rally_clears_the_tick_its_unit_dies() {
    let mut state = build(vec![
        unit(0, UnitKind::Harvester, 20, 10),
        unit(1, UnitKind::Sentinel, 22, 10),
    ]);
    let home = foundry(&state);
    let doomed = state.units()[0].id;
    let mut data = serde_json::to_value(&state).unwrap();
    data["units"][0]["hp"] = json!(1);
    state = serde_json::from_value(data).unwrap();
    state.tick(&[cmd(0, rally(home, Some(Rally::Unit(doomed))))]);
    let events = run_until(&mut state, 200, |s, events| {
        s.validate_invariants().unwrap();
        events
            .iter()
            .any(|e| matches!(e, Event::UnitDied { unit, .. } if *unit == doomed))
    });
    assert!(!events.is_empty());
    assert_eq!(state.building(home).unwrap().rally, None);
    let born = newborn(&mut state, home, UnitKind::Harvester);
    assert_eq!(
        state.unit(born).unwrap().order,
        Order::Idle,
        "a cleared rally leaves the newborn at the doorstep"
    );
}

#[test]
fn a_unit_rally_refuses_foreign_and_absent_units() {
    let mut state = build(vec![
        unit(0, UnitKind::Sentinel, 12, 10),
        unit(1, UnitKind::Sentinel, 30, 15),
    ]);
    let home = foundry(&state);
    let enemy = state.units()[1].id;
    for target in [enemy, UnitId(999)] {
        let before = state.clone();
        let report = state.tick(&[cmd(0, rally(home, Some(Rally::Unit(target))))]);
        assert_eq!(rejected(&report.events), [RejectReason::InvalidTarget]);
        let mut idle = before;
        idle.tick(&[]);
        assert_eq!(state.hash(), idle.hash(), "a refusal changes nothing");
    }
    // A tile rally still behaves as before.
    let report = state.tick(&[cmd(0, rally(home, Some(Rally::Tile(TilePos::new(9, 9)))))]);
    assert!(rejected(&report.events).is_empty());
}
