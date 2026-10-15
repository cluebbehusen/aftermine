use super::*;
use crate::UnitKind;
use crate::scenario::UnitSpec;
use crate::tick::spatial::UnitIndex;

/// West's Roc and two East Skyhooks exactly as far from it, listed in
/// `order`, half-turned when `mirrored`; returns the Skyhook the Roc's
/// drones would take, as a tile in the unmirrored frame.
fn pick(order: [(i32, i32); 2], mirrored: bool) -> TilePos {
    let (width, height) = (30, 22);
    let place = |(x, y): (i32, i32)| {
        if mirrored {
            (width - 1 - x, height - 1 - y)
        } else {
            (x, y)
        }
    };
    let mut scenario = crate::Scenario::skirmish();
    scenario.map = vec![".".repeat(30); 22];
    scenario.map[2].replace_range(2..3, "1");
    scenario.map[17].replace_range(25..26, "2");
    scenario.buildings.clear();
    let unit = |player, kind, at| {
        let (x, y) = place(at);
        UnitSpec { player, kind, x, y }
    };
    scenario.units = vec![
        unit(0, UnitKind::Roc, (8, 10)),
        unit(1, UnitKind::Skyhook, order[0]),
        unit(1, UnitKind::Skyhook, order[1]),
    ];
    let mut value = serde_json::to_value(scenario.build().unwrap()).unwrap();
    if mirrored {
        value["units"][0]["heading"] = serde_json::json!(128);
    }
    let state: State = serde_json::from_value(value).unwrap();
    let mut index = UnitIndex::new();
    index.rebuild(&state.units);
    index.survey(&state);
    let roc = &state.units[0];
    let (_, at) = aircraft_near(
        &state,
        &index,
        roc.player,
        roc.pos,
        crate::stats::LAUNCH_RADIUS,
        (roc.pos, roc.heading),
    )
    .expect("premise: both Skyhooks are near and seen");
    let tile = TilePos::containing(at);
    let (x, y) = place((tile.x, tile.y));
    TilePos::new(x, y)
}

#[test]
fn equally_near_aircraft_are_told_apart_by_where_they_are_not_their_ids() {
    let north_first = pick([(12, 8), (12, 12)], false);
    assert_eq!(
        north_first,
        pick([(12, 12), (12, 8)], false),
        "listing order"
    );
    assert_eq!(
        north_first,
        pick([(12, 8), (12, 12)], true),
        "a half-turned scene"
    );
}
