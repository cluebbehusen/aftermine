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
fn the_army_ledger_keeps_units_that_walked_off_and_drops_those_killed_in_sight() {
    use aftermine_sim::{PlayerId, Scenario, UnitId};
    let state = Scenario::skirmish().build().unwrap();
    let width = usize::try_from(state.map().width()).unwrap();
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
    let mut memory = Memory {
        horizon: 3_600,
        ..Memory::default()
    };
    memory.observe(&observation);
    // The next look: Sentinel 1's tile is in sight without it, so it fell
    // there; Sentinel 2 walked off out of sight.
    observation.tick = 6;
    observation.enemy_units.clear();
    observation.visible[10 * width + 10] = true;
    memory.observe(&observation);
    let ids = |list: &[SeenUnit]| -> Vec<UnitId> { list.iter().map(|unit| unit.id).collect() };
    assert_eq!(ids(memory.army()), [UnitId(2)]);
    let cost = u64::from(UnitKind::Sentinel.stats().cost);
    // Long after the short memory forgot it, the ledger counts it in full,
    // then fades it to nothing by the horizon.
    observation.tick = 1_200;
    memory.observe(&observation);
    assert!(memory.units().is_empty());
    assert_eq!(memory.army_value(&memory.army()[0], 1_200), cost);
    observation.tick = 2_400;
    memory.observe(&observation);
    assert_eq!(memory.army_value(&memory.army()[0], 2_400), cost / 2);
    observation.tick = 3_600;
    memory.observe(&observation);
    assert!(memory.army().is_empty());
}
