use super::*;
use aftermine_sim::vision::PublicBuilding;
use aftermine_sim::{BuildingKind, PlayerId, Scenario};
use chassis::grid::TilePos;

#[test]
fn a_public_building_joins_as_a_memory_and_a_remembered_one_is_brought_up_to_date() {
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
        built: false,
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
            progress: None,
        },
        PublicBuilding {
            owner: PlayerId(1),
            kind: BuildingKind::Extractor,
            tier: 1,
            anchor: TilePos::new(30, 18),
            progress: None,
        },
    ];
    merge(&mut observation);
    assert_eq!(
        observation.enemy_buildings,
        vec![
            BuildingObs {
                anchor: TilePos::new(15, 9),
                hp: 900,
                built: true,
                tier: 1,
                ..ghost.clone()
            },
            BuildingObs {
                hp: 900,
                built: true,
                tier: 1,
                ..ghost
            },
        ]
    );
}
