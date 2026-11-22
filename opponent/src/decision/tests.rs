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
