use super::*;
use aftermine_sim::Scenario;
use aftermine_sim::UnitKind::{Harvester, Sentinel};

/// Open ground holding `units`, each `(seat, kind, x, y)`.
fn field(units: &[(u8, aftermine_sim::UnitKind, i32, i32)]) -> State {
    let units: Vec<_> = units
        .iter()
        .map(|(player, kind, x, y)| {
            serde_json::json!({ "player": player, "kind": kind, "x": x, "y": y })
        })
        .collect();
    let scenario: Scenario = serde_json::from_value(serde_json::json!({
        "name": "Fight",
        "mode": "sandbox",
        "map": vec![".".repeat(80); 50],
        "players": [
            { "name": "One", "scrap": 0, "bot": false },
            { "name": "Two", "scrap": 0, "bot": false },
        ],
        "units": units,
    }))
    .expect("the staged field parses");
    scenario.build().expect("the staged field builds")
}

#[test]
fn one_side_alone_is_not_a_fight() {
    let state = field(&[(0, Sentinel, 20, 12), (0, Sentinel, 22, 12)]);
    assert_eq!(fiercest(&state), None);
}

#[test]
fn workers_are_not_a_fight() {
    let state = field(&[(0, Sentinel, 20, 12), (1, Harvester, 22, 12)]);
    assert_eq!(fiercest(&state), None);
}

#[test]
fn sides_out_of_one_view_are_not_a_fight() {
    let state = field(&[(0, Sentinel, 2, 2), (1, Sentinel, 76, 46)]);
    assert_eq!(fiercest(&state), None);
}

#[test]
fn the_view_with_the_most_opposed_units_wins() {
    let state = field(&[
        (0, Sentinel, 8, 8),
        (1, Sentinel, 10, 8),
        (0, Sentinel, 60, 40),
        (0, Sentinel, 62, 40),
        (1, Sentinel, 64, 40),
        (1, Sentinel, 66, 40),
    ]);
    let fight = fiercest(&state).expect("two armed sides share a view");
    assert_eq!(fight.opposed, 2);
    assert!((fight.center.0 - 63).abs() <= VIEW.0 / 2, "{fight:?}");
    assert!((fight.center.1 - 40).abs() <= VIEW.1 / 2, "{fight:?}");
}
