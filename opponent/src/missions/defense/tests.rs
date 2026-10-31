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
    assert!(lost(&observation, None, Some(gun)));
    assert!(!lost(&observation, None, Some(raider)));
    assert!(lost(&observation, None, Some(UnitId(999))));
    assert!(!lost(&observation, Some(gun), Some(gun)));
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
