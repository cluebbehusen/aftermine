use super::*;
use crate::missions::Shortfall;
use aftermine_sim::Scenario;

#[test]
fn one_shortfall_s_answer_frees_no_savings_while_another_has_none() {
    // A Turret pays back against a Sentinel but not against Lancers, which
    // outrange it. With both Foundries short, the Sentinel's answer must not
    // free the savings for an emergency against the Lancers.
    let state = Scenario::skirmish().build().unwrap();
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let map = MapModel::from_scenario(&Scenario::skirmish()).unwrap();
    let frame = HomeFrame::of(&observation, &map).unwrap();
    let producers = producers(&observation, frame, 0);
    let ground = observation
        .my_buildings
        .first()
        .and_then(|foundry| map.component(foundry.anchor));
    let short = |kind: UnitKind, count: u64| Shortfall {
        ground,
        gap: [u64::from(kind.stats().cost) * count, 0],
        attackers: vec![(kind, u64::from(kind.stats().cost) * count)],
        read: true,
    };
    let (raid, siege) = (short(UnitKind::Sentinel, 1), short(UnitKind::Lancer, 2));
    let awaited =
        |shortfalls: &[Shortfall]| awaited(&observation, &map, &producers, shortfalls, 0, 400);
    assert!(
        awaited(std::slice::from_ref(&raid)).is_some(),
        "premise: an answer pays back against the Sentinel"
    );
    assert_eq!(
        awaited(std::slice::from_ref(&siege)),
        None,
        "premise: none pays back against the Lancers"
    );
    assert_eq!(awaited(&[raid, siege]), None);
}

#[test]
fn a_bastion_the_seat_can_build_frees_savings_against_lancers() {
    // Lancers outrange a Turret, but not a Bastion: once a Fabricator stands,
    // the Bastion pays back and frees the scrap kept for the saving target.
    let answered = |fabricator: bool| {
        let mut scenario = crate::tests::arena(400);
        if fabricator {
            scenario
                .buildings
                .push(aftermine_sim::scenario::BuildingSpec {
                    player: 0,
                    kind: BuildingKind::Fabricator,
                    x: 3,
                    y: 8,
                });
        }
        let state = scenario.build().unwrap();
        let observation = ObservationData::fog_honest(&state, PlayerId(0));
        let map = MapModel::from_scenario(&scenario).unwrap();
        let frame = HomeFrame::of(&observation, &map).unwrap();
        // The Fabricator trains nothing here: only structures can answer.
        let producers: Vec<_> = producers(&observation, frame, 0)
            .into_iter()
            .filter(|producer| producer.building.kind != BuildingKind::Fabricator)
            .collect();
        let ground = observation
            .my_buildings
            .first()
            .and_then(|foundry| map.component(foundry.anchor));
        let cost = u64::from(UnitKind::Lancer.stats().cost) * 3;
        let siege = Shortfall {
            ground,
            gap: [cost, 0],
            attackers: vec![(UnitKind::Lancer, cost)],
            read: true,
        };
        awaited(&observation, &map, &producers, &[siege], 0, 400)
    };
    assert_eq!(answered(false), None, "no Bastion without a Fabricator");
    assert_eq!(answered(true), Some(0), "a Bastion frees the savings");
}
