use super::*;

fn running() -> (Scenario, State, LiveMatchStats) {
    let scenario = Scenario::skirmish();
    let mut state = scenario.build().unwrap();
    let mut stats = LiveMatchStats::new(&state);
    for _ in 0..30 {
        let report = state.tick(&[]);
        stats.observe(&state, &report.events);
    }
    (scenario, state, stats)
}

#[test]
fn a_snapshot_round_trips_and_validates() {
    let (scenario, state, stats) = running();
    let snapshot = WorldSnapshot::capture(&scenario, &state, &stats).unwrap();
    let bytes = serde_json::to_vec(&snapshot).unwrap();
    let back: WorldSnapshot = serde_json::from_slice(&bytes).unwrap();
    back.validate().unwrap();
    assert_eq!(back.state().hash(), state.hash());
    let (_, origin, _) = back.into_parts();
    assert_eq!(origin.state().current_tick(), 30);
}

#[test]
fn a_snapshot_bound_to_another_scenario_is_refused() {
    let (scenario, state, stats) = running();
    let snapshot = WorldSnapshot::capture(&scenario, &state, &stats).unwrap();
    let mut json: serde_json::Value = serde_json::to_value(&snapshot).unwrap();
    json["scenario"]["name"] = serde_json::Value::from("Elsewhere");
    let forged: WorldSnapshot = serde_json::from_value(json).unwrap();
    assert!(forged.validate().is_err());
}
