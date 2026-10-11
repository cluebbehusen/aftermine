use super::*;
use chassis::grid::TilePos;
use oxide_sim::vision::PublicBuilding;
use oxide_sim::{BuildingKind, PlayerId, Scenario};

#[test]
fn a_public_building_joins_as_a_memory_and_a_remembered_one_takes_its_tier() {
    let state = Scenario::skirmish().build().unwrap();
    let mut observation = ObservationData::fog_honest(&state, PlayerId(0));
    observation.enemy_buildings.clear();
    let ghost = BuildingObs {
        provisional: false,
        id: BuildingId(u32::MAX),
        player: PlayerId(1),
        kind: BuildingKind::Extractor,
        anchor: TilePos::new(30, 18),
        hp: 300,
        built: true,
        seen: false,
        tier: 0,
    };
    observation.enemy_buildings.push(ghost.clone());
    observation.public_buildings = vec![
        PublicBuilding {
            owner: PlayerId(1),
            kind: BuildingKind::Extractor,
            tier: 1,
            anchor: TilePos::new(15, 9),
        },
        PublicBuilding {
            owner: PlayerId(1),
            kind: BuildingKind::Extractor,
            tier: 1,
            anchor: TilePos::new(30, 18),
        },
    ];
    merge(&mut observation);
    assert_eq!(
        observation.enemy_buildings,
        vec![
            BuildingObs {
                anchor: TilePos::new(15, 9),
                hp: 900,
                tier: 1,
                ..ghost.clone()
            },
            BuildingObs { tier: 1, ..ghost },
        ]
    );
}
