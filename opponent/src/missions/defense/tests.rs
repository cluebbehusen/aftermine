use super::*;
use aftermine_sim::PlayerId;

/// West's view of the strip field with `units` as owner, kind and tile, and
/// their ids in that order.
fn viewed(units: &[(u8, UnitKind, i32, i32)]) -> (ObservationData, Vec<UnitId>) {
    let scenario = crate::tests::match_on(crate::tests::strip(false), &["West", "East"], units);
    let state = scenario.build().unwrap();
    let ids = units
        .iter()
        .map(|&(_, _, x, y)| {
            state
                .units()
                .iter()
                .find(|unit| unit.tile() == TilePos::new(x, y))
                .unwrap()
                .id
        })
        .collect();
    (ObservationData::fog_honest(&state, PlayerId(0)), ids)
}

#[test]
fn a_gun_the_foundry_no_longer_faces_is_lost_but_focus_fire_is_kept() {
    let (observation, ids) = viewed(&[
        (0, UnitKind::Sentinel, 3, 4),
        (1, UnitKind::Bombard, 6, 4),
        (1, UnitKind::Sentinel, 6, 6),
    ]);
    let (gun, raider) = (ids[1], ids[2]);
    assert!(observation.enemy_units.len() == 2, "premise: both in sight");
    assert!(lost(&observation, (None, Some(gun)), (&[], Pursuit::Hunt)));
    assert!(!lost(
        &observation,
        (None, Some(raider)),
        (&[], Pursuit::Hunt)
    ));
    assert!(lost(
        &observation,
        (None, Some(UnitId(999))),
        (&[], Pursuit::Hunt)
    ));
    assert!(!lost(
        &observation,
        (Some(gun), Some(gun)),
        (&[], Pursuit::Hunt)
    ));
}

#[test]
fn defenders_refused_an_order_are_not_counted_as_sent() {
    let (observation, ids) = viewed(&[
        (0, UnitKind::Sentinel, 3, 4),
        (0, UnitKind::Flakhound, 3, 5),
        (1, UnitKind::Bombard, 6, 4),
    ]);
    let (sentinel, flakhound, gun) = (ids[0], ids[1], ids[2]);
    let goal = TilePos::new(6, 4);
    let sent = |allowance| {
        let mut ledger = Ledger::new(PlayerId(0), 0, allowance);
        dispatch(
            &observation,
            &mut ledger,
            vec![sentinel, flakhound],
            goal,
            Pursuit::Gun(gun),
        )
    };
    let mut both = vec![sentinel, flakhound];
    both.sort_unstable();
    assert_eq!(sent(2), both);
    assert_eq!(sent(1), [sentinel], "the anti-air hunt was refused");
    assert!(sent(0).is_empty());
}

#[test]
fn a_shell_from_beyond_a_bastions_reach_is_blamed_on_a_citadel() {
    let gun = |tier: u8| BuildingObs {
        provisional: false,
        id: aftermine_sim::BuildingId(0),
        player: PlayerId(1),
        kind: BuildingKind::Bastion,
        anchor: TilePos::new(2, 4),
        hp: BuildingKind::Bastion.tier_stats(tier).max_hp,
        built: true,
        seen: true,
        tier,
    };
    // Ten and a half tiles from the footprint's centre.
    let impact = TilePos::new(13, 4);
    assert!(!battery(&gun(0), impact), "past the Bastion's reach");
    assert!(battery(&gun(1), impact), "inside the Citadel's");
}

#[test]
fn an_enemy_producer_among_the_buildings_counts_its_whole_price() {
    let site = |kind: BuildingKind, hp: u32| BuildingObs {
        provisional: false,
        id: aftermine_sim::BuildingId(0),
        player: PlayerId(1),
        kind,
        anchor: TilePos::new(9, 7),
        hp,
        built: false,
        seen: true,
        tier: 0,
    };
    let unarmed = |building: &BuildingObs| {
        Siege {
            batteries: vec![(building, TilePos::new(8, 7))],
            ..Siege::default()
        }
        .unarmed()[0]
    };
    // A Fabricator site barely begun is still everything it will train.
    let fabricator = site(BuildingKind::Fabricator, 1);
    assert_eq!(
        unarmed(&fabricator),
        u64::from(BuildingKind::Fabricator.invested_cost(0))
    );
    // A structure that trains nothing counts by its health: half of it, half
    // its price.
    let max_hp = BuildingKind::Extractor.tier_stats(0).max_hp;
    let extractor = site(BuildingKind::Extractor, max_hp / 2);
    assert_eq!(
        unarmed(&extractor),
        u64::from(BuildingKind::Extractor.invested_cost(0)) / 2
    );
}

#[test]
fn only_a_siege_with_nothing_armed_in_it_is_unarmed() {
    let site = |kind: BuildingKind| BuildingObs {
        provisional: false,
        id: aftermine_sim::BuildingId(0),
        player: PlayerId(1),
        kind,
        anchor: TilePos::new(9, 7),
        hp: 1,
        built: false,
        seen: true,
        tier: 0,
    };
    let (fabricator, turret) = (site(BuildingKind::Fabricator), site(BuildingKind::Turret));
    let beside = TilePos::new(8, 7);
    let (observation, _) = viewed(&[(0, UnitKind::Sentinel, 3, 3)]);
    let enemy = |kind: UnitKind| aftermine_sim::observation::UnitObs {
        kind,
        player: PlayerId(1),
        ..observation.my_units[0].clone()
    };
    let (harvester, scuttler) = (enemy(UnitKind::Harvester), enemy(UnitKind::Scuttler));
    let builder = Siege {
        strays: vec![&harvester],
        batteries: vec![(&fabricator, beside)],
        ..Siege::default()
    };
    assert!(builder.unarmed_only(), "a builder and its producer");
    let raider = Siege {
        units: vec![&scuttler],
        ..Siege::default()
    };
    assert!(!raider.unarmed_only(), "an armed raider");
    let gun = Siege {
        batteries: vec![(&turret, beside)],
        ..Siege::default()
    };
    assert!(!gun.unarmed_only(), "a gun among the batteries");
    let unseen = Siege {
        unseen: Some(beside),
        ..Siege::default()
    };
    assert!(!unseen.unarmed_only(), "a gun out of sight");
    let charge = site(BuildingKind::ScuttleCharge);
    let mined = Siege {
        strays: vec![&harvester],
        batteries: vec![(&charge, beside)],
        ..Siege::default()
    };
    assert!(!mined.unarmed_only(), "a mine beside the builder");
    let left = Siege {
        strays: vec![&harvester],
        armed: true,
        ..Siege::default()
    };
    assert!(
        !left.unarmed_only(),
        "a gun left to production or shelling from out of sight"
    );
}

#[test]
fn a_defense_given_up_ends_only_once_every_member_falls_back() {
    // One defender is beside home's post and one far out: they need two
    // orders. With room for one, the mission stays to order the other later
    // rather than leave it on its last order.
    let units = [
        (0, UnitKind::Sentinel, 5, 2),
        (0, UnitKind::Sentinel, 14, 6),
    ];
    let scenario = crate::tests::match_on(crate::tests::strip(false), &["West", "East"], &units);
    let state = scenario.build().unwrap();
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let map = MapModel::from_scenario(&scenario).unwrap();
    let frame = crate::frame::HomeFrame::of(&observation, &map).unwrap();
    let foundry = observation.my_buildings[0].id;
    let mut members: Vec<UnitId> = observation.my_units.iter().map(|unit| unit.id).collect();
    members.sort_unstable();
    let goal = TilePos::new(16, 4);
    let post = home_post(&observation, &map, frame, goal).unwrap();
    assert!(
        observation
            .my_units
            .iter()
            .any(|unit| unit.tile.chebyshev(post) <= RESERVE_TILES)
            && observation
                .my_units
                .iter()
                .any(|unit| unit.tile.chebyshev(post) > RESERVE_TILES),
        "premise: one at the post and one away from it"
    );
    let missions: Missions = serde_json::from_value(serde_json::json!({
        "next": 1,
        "list": [{
            "id": 0,
            "task": {"task": "defend", "asset": foundry.0, "phase": {"engage": {"focus": null}}},
            "since": 0,
            "units": members,
            "goal": {"x": goal.x, "y": goal.y},
        }],
    }))
    .unwrap();
    let given_up = |allowance| {
        let mut missions = missions.clone();
        let mut ledger = Ledger::new(PlayerId(0), 0, allowance);
        missions.give_up(Some(0), &observation, &map, frame, goal, &mut ledger);
        missions.list.len()
    };
    assert_eq!(given_up(1), 1, "one member still has its last order");
    assert_eq!(given_up(2), 0);
}

#[test]
fn a_defender_far_off_chasing_is_in_neither_fight() {
    // One defender stands at the fight and another has chased far off: both
    // are out from the post, but only the one at the fight is worth anything
    // there, so the post's reserve does not wait on the chaser.
    let (observation, ids) = viewed(&[
        (0, UnitKind::Sentinel, 8, 4),
        (0, UnitKind::Sentinel, 18, 8),
    ]);
    let (at_fight, chaser) = (ids[0], ids[1]);
    let (out, worth) = front(
        &observation,
        &[at_fight, chaser],
        Some(TilePos::new(4, 1)),
        TilePos::new(6, 3),
    );
    assert!(out.contains(&at_fight) && out.contains(&chaser), "{out:?}");
    let alone = value(mine(&observation, at_fight).unwrap());
    assert_eq!(worth, alone);
}

#[test]
fn only_the_site_owner_s_builder_beside_it_comes_first() {
    // A Harvester beside an enemy site raises it only if it is the site's:
    // another seat's passing by cannot, so the site itself comes first.
    let (observation, _) = viewed(&[(1, UnitKind::Harvester, 9, 4)]);
    let builder = observation.enemy_units[0].clone();
    let passer = UnitObs {
        player: PlayerId(2),
        ..builder.clone()
    };
    let site = BuildingObs {
        provisional: false,
        id: aftermine_sim::BuildingId(7),
        player: PlayerId(1),
        kind: BuildingKind::Fabricator,
        anchor: TilePos::new(10, 4),
        hp: 50,
        built: false,
        seen: true,
        tier: 0,
    };
    let beside = TilePos::new(9, 5);
    let quarry = |stray: &UnitObs| {
        Siege {
            strays: vec![stray],
            batteries: vec![(&site, beside)],
            ..Siege::default()
        }
        .quarry(|tile| tile.x + tile.y)
        .map(|(target, _)| target)
    };
    assert_eq!(quarry(&builder), Some(AttackTarget::Unit(builder.id)));
    assert_eq!(quarry(&passer), Some(AttackTarget::Building(site.id)));
}

#[test]
fn an_aircraft_defender_is_judged_by_what_can_hit_its_attackers() {
    // A West Talon faces two East Talons. Two Buzzards beside it cannot hit
    // aircraft, so they do not bring the fight level; a Flak Turret ten
    // tiles off cannot reach it, so it does not weigh in either.
    let (observation, ids) = viewed(&[
        (0, UnitKind::Talon, 4, 4),
        (0, UnitKind::Buzzard, 4, 5),
        (0, UnitKind::Buzzard, 5, 4),
        (1, UnitKind::Talon, 6, 4),
        (1, UnitKind::Talon, 6, 5),
    ]);
    let memory = Memory::default();
    let talon = mine(&observation, ids[0]).unwrap();
    assert!(outweighed(
        (&observation, &memory),
        &[ids[0], ids[1], ids[2]],
        talon
    ));
    let (observation, ids) = viewed(&[(0, UnitKind::Talon, 3, 4)]);
    let mut observation = observation;
    observation.enemy_buildings.push(BuildingObs {
        provisional: false,
        id: aftermine_sim::BuildingId(77),
        player: PlayerId(1),
        kind: BuildingKind::FlakTurret,
        anchor: TilePos::new(14, 4),
        hp: BuildingKind::FlakTurret.base_stats().max_hp,
        built: true,
        seen: true,
        tier: 0,
    });
    let talon = mine(&observation, ids[0]).unwrap();
    assert!(!outweighed((&observation, &memory), &[ids[0]], talon));
}

#[test]
fn a_search_goes_from_a_foundry_to_its_nearest_hit_structure_on_its_ground() {
    // Two West Turrets are hit by fire nobody sees, and a third across a
    // chasm. The Foundry looks over the nearer on its own ground; the one
    // its ground units cannot reach draws no search.
    let mut scenario = crate::tests::match_on(crate::tests::strip(true), &["West", "East"], &[]);
    for (x, y) in [(4, 4), (6, 7), (10, 4)] {
        scenario
            .buildings
            .push(aftermine_sim::scenario::BuildingSpec {
                player: 0,
                kind: BuildingKind::Turret,
                x,
                y,
            });
    }
    let state = scenario.build().unwrap();
    let mut observation = ObservationData::fog_honest(&state, PlayerId(0));
    for x in [4, 6, 10] {
        let y = if x == 6 { 7 } else { 4 };
        observation
            .salvage_incidents
            .push(crate::tests::fire_mark(TilePos::new(x, y), 0));
    }
    observation
        .salvage_incidents
        .sort_by_key(|mark| (mark.tile.y, mark.tile.x));
    let observation = observation;
    let map = MapModel::from_scenario(&scenario).unwrap();
    let frame = crate::frame::HomeFrame::of(&observation, &map).unwrap();
    let turret = |x: i32| {
        observation
            .my_buildings
            .iter()
            .find(|b| b.kind == BuildingKind::Turret && b.anchor.x == x)
            .unwrap()
    };
    let searched = [turret(6), turret(4), turret(10)];
    let found = searches(&observation, &map, frame, &searched);
    assert_eq!(found.len(), 1, "one search for the one Foundry");
    let (_, (beside, need)) = found[0];
    let near = turret(4);
    assert!(
        crate::frame::ring(near.anchor, near.kind.size()).any(|tile| tile == beside),
        "{beside:?}"
    );
    assert_eq!(need, u64::from(BuildingKind::Turret.invested_cost(0)));
    assert!(searches(&observation, &map, frame, &[turret(10)]).is_empty());
}

#[test]
fn a_search_looks_out_from_the_side_the_fire_came_from() {
    // One West Turret hit from out of sight: defenders go to the tile beside
    // it facing where the fire came from, whichever side the enemy starts.
    let mut scenario = crate::tests::match_on(crate::tests::strip(false), &["West", "East"], &[]);
    let anchor = TilePos::new(9, 4);
    scenario
        .buildings
        .push(aftermine_sim::scenario::BuildingSpec {
            player: 0,
            kind: BuildingKind::Turret,
            x: anchor.x,
            y: anchor.y,
        });
    let state = scenario.build().unwrap();
    let map = MapModel::from_scenario(&scenario).unwrap();
    let beside = |from: u8| {
        let mut observation = ObservationData::fog_honest(&state, PlayerId(0));
        observation.salvage_incidents = vec![crate::tests::fire_mark(anchor, from)];
        let frame = crate::frame::HomeFrame::of(&observation, &map).unwrap();
        let turret = observation
            .my_buildings
            .iter()
            .find(|b| b.kind == BuildingKind::Turret)
            .unwrap();
        let found = searches(&observation, &map, frame, &[turret]);
        let [(_, (beside, _))] = found[..] else {
            panic!("{found:?}");
        };
        beside
    };
    assert_eq!(beside(8), anchor.offset(-1, 0), "fire from the west");
    assert_eq!(beside(4), anchor.offset(0, 1), "fire from the south");
    assert_eq!(beside(12), anchor.offset(0, -1), "fire from the north");
    assert_eq!(beside(0), anchor.offset(1, 0), "fire from the east");
    assert_eq!(beside(2), anchor.offset(1, 1), "fire from the south-east");
}

#[test]
fn two_foundries_look_over_two_hit_structures_not_one_twice() {
    // One hit Turret stands nearest the first Foundry, another nearest the
    // second; the second Foundry also stands near the first Turret. Each
    // Foundry looks over its own, so both are searched.
    let mut scenario = crate::tests::match_on(crate::tests::strip(false), &["West", "East"], &[]);
    scenario
        .buildings
        .push(aftermine_sim::scenario::BuildingSpec {
            player: 0,
            kind: BuildingKind::Foundry,
            x: 2,
            y: 6,
        });
    for (x, y) in [(5, 2), (8, 7)] {
        scenario
            .buildings
            .push(aftermine_sim::scenario::BuildingSpec {
                player: 0,
                kind: BuildingKind::Turret,
                x,
                y,
            });
    }
    let state = scenario.build().unwrap();
    let mut observation = ObservationData::fog_honest(&state, PlayerId(0));
    observation.salvage_incidents = vec![
        crate::tests::fire_mark(TilePos::new(5, 2), 0),
        crate::tests::fire_mark(TilePos::new(8, 7), 0),
    ];
    let observation = observation;
    let map = MapModel::from_scenario(&scenario).unwrap();
    let frame = crate::frame::HomeFrame::of(&observation, &map).unwrap();
    let searched: Vec<&BuildingObs> = observation
        .my_buildings
        .iter()
        .filter(|b| b.kind == BuildingKind::Turret)
        .collect();
    let found = searches(&observation, &map, frame, &searched);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_ne!(found[0].1.0, found[1].1.0, "{found:?}");
}

#[test]
fn a_sapper_is_no_unarmed_intruder() {
    let (observation, ids) = viewed(&[
        (0, UnitKind::Sentinel, 3, 4),
        (1, UnitKind::Sapper, 5, 4),
        (1, UnitKind::Surveyor, 5, 6),
    ]);
    assert!(!intruder(&observation, Some(ids[1])));
    assert!(intruder(&observation, Some(ids[2])));
}

#[test]
fn only_a_member_that_can_hit_a_site_ends_its_attack_by_standing_idle() {
    let (mut observation, ids) = viewed(&[
        (0, UnitKind::Sentinel, 3, 4),
        (0, UnitKind::Flakhound, 3, 6),
    ]);
    let site = Pursuit::Quarry(AttackTarget::Building(aftermine_sim::BuildingId(77)));
    for unit in &mut observation.my_units {
        unit.idle = unit.kind == UnitKind::Flakhound;
    }
    assert!(!idle_at_site(&observation, &ids, site));
    for unit in &mut observation.my_units {
        unit.idle = true;
    }
    assert!(idle_at_site(&observation, &ids, site));
}

#[test]
fn a_ground_defender_bombed_by_aircraft_it_cannot_hit_is_outweighed() {
    let (observation, ids) = viewed(&[
        (0, UnitKind::Scuttler, 4, 4),
        (0, UnitKind::Scuttler, 4, 5),
        (1, UnitKind::Buzzard, 6, 4),
    ]);
    let memory = Memory::default();
    let scuttler = mine(&observation, ids[0]).unwrap();
    assert!(outweighed(
        (&observation, &memory),
        &[ids[0], ids[1]],
        scuttler
    ));
}
