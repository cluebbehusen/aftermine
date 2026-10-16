use super::*;
use oxide_sim::Scenario;

/// Three islands in a row, West's start on the first, then Mid's and East's.
fn islands() -> Scenario {
    let map: Vec<String> = (0..9)
        .map(|row| {
            let mut tiles: Vec<char> = ".".repeat(30).chars().collect();
            for x in [9, 10, 19, 20] {
                tiles[x] = '~';
            }
            if row == 3 {
                tiles[2] = '1';
                tiles[13] = '2';
                tiles[24] = '3';
            }
            tiles.into_iter().collect()
        })
        .collect();
    crate::tests::match_on(map, &["West", "Mid", "East"], &[])
}

#[test]
fn lifts_go_after_the_rival_first() {
    let scenario = islands();
    let state = scenario.build().unwrap();
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let map = MapModel::from_scenario(&scenario).unwrap();
    let frame = HomeFrame::of(&observation, &map).unwrap();
    let memory = Memory::default();
    let start = |owner: u8| Objective {
        owner: PlayerId(owner),
        building: BuildingKind::Foundry,
        anchor: map.start(PlayerId(owner)).unwrap(),
    };
    let objectives = [start(1), start(2)];
    let first = |rival| {
        targets(&objectives, &observation, &map, frame, &memory, rival)
            .first()
            .map(|objective| objective.owner)
    };
    assert_eq!(first(None), Some(PlayerId(1)), "premise: the nearer start");
    assert_eq!(first(Some(PlayerId(2))), Some(PlayerId(2)));
}
