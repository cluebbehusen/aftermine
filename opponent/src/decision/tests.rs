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

/// What a short defense keeps for three `attacker`s that only structures
/// could answer, with a Fabricator standing or not.
fn kept_against(attacker: UnitKind, fabricator: bool) -> Option<u32> {
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
    let cost = u64::from(attacker.stats().cost) * 3;
    let siege = Shortfall {
        ground,
        gap: [cost, 0],
        attackers: vec![(attacker, cost)],
        read: true,
    };
    awaited(&observation, &map, &producers, &[siege], 0, 400)
}

#[test]
fn a_bastion_the_seat_can_build_frees_savings_against_lancers() {
    // Lancers outrange a Turret, but not a Bastion: once a Fabricator stands,
    // the Bastion pays back and frees the scrap kept for the saving target.
    let kind = UnitKind::Lancer;
    assert_eq!(
        kept_against(kind, false),
        None,
        "no Bastion without a Fabricator"
    );
    assert_eq!(
        kept_against(kind, true),
        Some(0),
        "a Bastion frees the savings"
    );
}

#[test]
fn a_bastion_frees_no_savings_against_bombards_that_outrange_it() {
    // Bombards match a Bastion's range from its footprint's centre, so they
    // shell it from beyond its reach: the emergency puts none up, and the
    // scrap kept for the saving target stays kept.
    assert_eq!(kept_against(UnitKind::Bombard, true), None);
}

#[test]
fn against_sentinels_the_answer_is_the_lancer_that_outtrades_them() {
    // A Lancer one-shots a Sentinel; a Warden takes two shots and is slow to
    // reach them. Per scrap the Lancer destroys far more.
    let short = Shortfall {
        ground: None,
        gap: [450, 0],
        attackers: vec![(UnitKind::Sentinel, 450)],
        read: true,
    };
    let worth = |kind| answer(Fighter::Unit(kind), &short);
    assert!(
        worth(UnitKind::Lancer) > worth(UnitKind::Warden),
        "Lancer {} v Warden {}",
        worth(UnitKind::Lancer),
        worth(UnitKind::Warden)
    );
}

#[test]
fn fire_from_out_of_sight_is_no_builder_for_a_scuttler_to_meet() {
    // With a Fabricator standing, a shortfall that armed fire leaves is met
    // with what the army trains even when no attacker stands in sight; one
    // left by builders alone is still met with the cheapest unit.
    let mut scenario = crate::tests::arena(400);
    scenario
        .buildings
        .push(aftermine_sim::scenario::BuildingSpec {
            player: 0,
            kind: BuildingKind::Fabricator,
            x: 3,
            y: 8,
        });
    let state = scenario.build().unwrap();
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let map = MapModel::from_scenario(&scenario).unwrap();
    let frame = HomeFrame::of(&observation, &map).unwrap();
    // Producers with nothing queued are free to train now.
    let producers = producers(&observation, frame, 1);
    let ground = observation
        .my_buildings
        .first()
        .and_then(|foundry| map.component(foundry.anchor));
    let best = |read: bool| {
        let short = Shortfall {
            ground,
            gap: [200, 0],
            attackers: Vec::new(),
            read,
        };
        best_answer(&observation, &map, &producers, &short, 400, &[]).map(|(fighter, _)| fighter)
    };
    assert_eq!(best(false), Some(Fighter::Unit(UnitKind::Scuttler)));
    assert!(
        !matches!(best(true), Some(Fighter::Unit(UnitKind::Scuttler))),
        "{:?}",
        best(true)
    );
}
