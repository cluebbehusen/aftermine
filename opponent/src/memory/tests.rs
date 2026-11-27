use super::*;

impl Memory {
    /// Remembers the enemy units in sight with none treated as stranded.
    pub(crate) fn observe(&mut self, observation: &ObservationData) {
        self.observe_stranded(observation, |_| false);
    }
}

#[test]
fn failures_expire_and_the_oldest_leaves_first() {
    let mut memory = Memory::default();
    let anchor = TilePos::new(4, 4);
    memory.fail(BuildingKind::Fabricator, anchor, 100);
    assert!(memory.failed(BuildingKind::Fabricator, anchor, 100 + FAILURE_TICKS - 1));
    assert!(!memory.failed(BuildingKind::Airworks, anchor, 100));
    assert!(!memory.failed(BuildingKind::Fabricator, anchor, 100 + FAILURE_TICKS));
    memory.forget(100 + FAILURE_TICKS);
    assert_eq!(memory, Memory::default());

    for x in 0..=i32::try_from(FAILURE_CAP).unwrap() {
        memory.fail(BuildingKind::Fabricator, TilePos::new(x, 0), 200);
    }
    assert_eq!(memory.failures.len(), FAILURE_CAP);
    assert!(!memory.failed(BuildingKind::Fabricator, TilePos::new(0, 0), 200));
    assert_eq!(memory.validate(200, 8, 8, 0), Ok(()));
    assert!(memory.validate(199, 8, 8, 0).is_err());
}

#[test]
fn raids_are_remembered_apart_from_given_up_targets() {
    let mut memory = Memory::default();
    let anchor = TilePos::new(4, 4);
    memory.raid(BuildingKind::Foundry, anchor, 100);
    assert!(memory.raided(BuildingKind::Foundry, anchor, 100));
    assert!(!memory.abandoned(BuildingKind::Foundry, anchor, 100));
    assert_eq!(memory.validate(100, 8, 8, 0), Ok(()));
    assert!(
        memory.validate(99, 8, 8, 0).is_err(),
        "a raid after the checkpoint was taken"
    );
    memory.forget(100 + FAILURE_TICKS);
    assert!(!memory.raided(BuildingKind::Foundry, anchor, 100 + FAILURE_TICKS));
}

#[test]
fn units_that_cannot_leave_their_ground_are_remembered_longer() {
    use aftermine_sim::{PlayerId, Scenario, UnitId};
    let state = Scenario::skirmish().build().unwrap();
    let mut observation = ObservationData::fog_honest(&state, PlayerId(0));
    observation
        .visible
        .iter_mut()
        .for_each(|visible| *visible = false);
    let template = observation.my_units[0].clone();
    observation.enemy_units = [(1, 10), (2, 20)]
        .into_iter()
        .map(|(id, x)| aftermine_sim::observation::UnitObs {
            id: UnitId(id),
            kind: UnitKind::Sentinel,
            tile: TilePos::new(x, 10),
            ..template.clone()
        })
        .collect();
    let mut memory = Memory::default();
    let stranded = |unit: &SeenUnit| unit.id == UnitId(2);
    memory.observe_stranded(&observation, stranded);
    observation.enemy_units.clear();
    let remembered =
        |memory: &Memory| -> Vec<UnitId> { memory.units().iter().map(|unit| unit.id).collect() };
    observation.tick = UNIT_TICKS;
    memory.observe_stranded(&observation, stranded);
    assert_eq!(remembered(&memory), [UnitId(2)]);
    observation.tick = STRANDED_TICKS - 1;
    memory.observe_stranded(&observation, stranded);
    assert_eq!(remembered(&memory), [UnitId(2)]);
    observation.tick = STRANDED_TICKS;
    memory.observe_stranded(&observation, stranded);
    assert!(remembered(&memory).is_empty());
}

#[test]
fn a_frame_whose_extractor_vanished_is_remembered_as_failed() {
    use aftermine_sim::{PlayerId, Scenario};
    let state = Scenario::skirmish().build().unwrap();
    let mut observation = ObservationData::fog_honest(&state, PlayerId(0));
    let mut extractor = observation.my_buildings[0].clone();
    extractor.kind = BuildingKind::Extractor;
    let (kept, lost) = (TilePos::new(3, 3), TilePos::new(9, 9));
    observation.my_buildings = [kept, lost]
        .map(|anchor| {
            let mut building = extractor.clone();
            building.anchor = anchor;
            building
        })
        .to_vec();
    observation.tick = 100;
    let mut memory = Memory::default();
    memory.lost_extractors(&observation);
    assert!(!memory.failed(BuildingKind::Extractor, lost, 100));

    observation
        .my_buildings
        .retain(|building| building.anchor == kept);
    observation.tick = 112;
    memory.lost_extractors(&observation);
    assert!(memory.failed(BuildingKind::Extractor, lost, 112));
    assert!(!memory.failed(BuildingKind::Extractor, kept, 112));
    assert_eq!(memory.validate(112, 16, 16, 0), Ok(()));

    memory.extractors = vec![lost, kept];
    assert!(
        memory.validate(112, 16, 16, 0).is_err(),
        "own Extractors out of order"
    );
    memory.extractors = vec![TilePos::new(20, 0)];
    assert!(
        memory.validate(112, 16, 16, 0).is_err(),
        "an own Extractor off the map"
    );
    // A seat may own more Extractors than failures are remembered.
    memory.extractors = (0..300).map(|x| TilePos::new(x * 2, 0)).collect();
    assert_eq!(memory.validate(112, 600, 2, 0), Ok(()));
}

#[test]
fn a_seen_enemy_counts_wherever_it_went_until_it_is_seen_to_die() {
    use aftermine_sim::observation::UnitObs;
    use aftermine_sim::{PlayerId, Scenario, UnitId};
    let state = Scenario::skirmish().build().unwrap();
    let mut observation = ObservationData::fog_honest(&state, PlayerId(0));
    let template = observation.my_units[0].clone();
    observation.enemy_units = [
        (1, UnitKind::Lancer),
        (2, UnitKind::Sentinel),
        (3, UnitKind::Drone),
    ]
    .into_iter()
    .map(|(id, kind)| UnitObs {
        id: UnitId(id),
        kind,
        tile: TilePos::new(10 + i32::try_from(id).unwrap(), 10),
        ..template.clone()
    })
    .collect();
    let mut memory = Memory::default();
    memory.observe(&observation);
    let counted = |memory: &Memory| memory.census().collect::<Vec<_>>();
    let map = MapModel::from_scenario(&Scenario::skirmish()).unwrap();
    let grounds: Vec<Option<u32>> = memory.census_on(&map).map(|(_, _, on)| on).collect();
    let seen_on = |x: i32| map.component(TilePos::new(x, 10));
    assert_eq!(
        grounds,
        [seen_on(11), seen_on(12)],
        "each on the ground it was seen on"
    );
    assert_eq!(
        counted(&memory),
        [UnitKind::Lancer, UnitKind::Sentinel],
        "a drone docks and launches again under a new id"
    );

    // Gone from where they stood while it was in sight: where they are is
    // forgotten at once, that they exist is not.
    observation.enemy_units.clear();
    observation
        .visible
        .iter_mut()
        .for_each(|visible| *visible = true);
    observation.tick = 12;
    memory.observe(&observation);
    assert!(memory.units().is_empty());
    assert_eq!(counted(&memory), [UnitKind::Lancer, UnitKind::Sentinel]);

    memory.died(&[OwnEvent::EnemyDied { unit: UnitId(1) }]);
    assert_eq!(counted(&memory), [UnitKind::Sentinel]);

    observation.tick = CENSUS_TICKS - 1;
    memory.observe(&observation);
    assert_eq!(counted(&memory), [UnitKind::Sentinel]);
    observation.tick = CENSUS_TICKS;
    memory.observe(&observation);
    assert!(counted(&memory).is_empty(), "a death it could not see");

    memory.assembled(
        &observation,
        &[OwnEvent::MachineAssembled {
            unit: UnitId(9),
            kind: UnitKind::Maw,
            player: PlayerId(1),
            pos: chassis::fx::Vec2Fx::default(),
        }],
        |_| true,
    );
    assert_eq!(counted(&memory), [UnitKind::Maw]);
}

#[test]
fn checkpoints_reject_a_malformed_census() {
    let entry = |id: u32, seen: u64| serde_json::json!({"id": id, "kind": "lancer", "player": 1, "tile": {"x": 1, "y": 1}, "seen": seen});
    let restored = |census: Vec<serde_json::Value>| -> Memory {
        serde_json::from_value(serde_json::json!({
            "units": [],
            "census": census,
            "failures": [],
            "abandoned": [],
            "raided": [],
            "scouted": [],
            "stuck": [],
            "extractors": [],
        }))
        .unwrap()
    };
    let valid = restored(vec![entry(1, 50), entry(2, 100)]);
    assert_eq!(valid.validate(100, 8, 8, 0), Ok(()));
    let json = serde_json::to_value(&valid).unwrap();
    assert_eq!(serde_json::from_value::<Memory>(json).unwrap(), valid);
    let malformed = [
        ("out of order", vec![entry(2, 50), entry(1, 50)]),
        ("counted twice", vec![entry(1, 50), entry(1, 60)]),
        ("seen after the checkpoint", vec![entry(1, 101)]),
        (
            "off the map",
            vec![
                serde_json::json!({"id": 1, "kind": "lancer", "player": 1, "tile": {"x": 9, "y": 1}, "seen": 50}),
            ],
        ),
        (
            "over the cap",
            (0..=u32::try_from(UNIT_CAP).unwrap())
                .map(|id| entry(id, 0))
                .collect(),
        ),
    ];
    for (why, census) in malformed {
        assert!(restored(census).validate(100, 8, 8, 0).is_err(), "{why}");
    }
}
