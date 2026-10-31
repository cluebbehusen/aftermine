use super::*;
use aftermine_sim::{PlayerId, Scenario, UnitKind};

/// The strip field, with a chasm when `chasm` holds; West's `members` and one
/// East `enemy`, each as kind and tile.
fn field(chasm: bool, members: &[(UnitKind, i32, i32)], enemy: (UnitKind, i32, i32)) -> Scenario {
    let units: Vec<_> = members
        .iter()
        .map(|&(kind, x, y)| (0, kind, x, y))
        .chain([(1, enemy.0, enemy.1, enemy.2)])
        .collect();
    crate::tests::match_on(crate::tests::strip(chasm), &["West", "East"], &units)
}

/// How many of West's units `shooters` would focus on East's unit.
fn focused(scenario: &Scenario) -> usize {
    let state = scenario.build().unwrap();
    let map = MapModel::from_scenario(scenario).unwrap();
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let members: Vec<&UnitObs> = observation.my_units.iter().collect();
    let east = state
        .units()
        .iter()
        .find(|unit| unit.player == PlayerId(1))
        .unwrap();
    let enemy = UnitObs {
        id: east.id,
        player: east.player,
        kind: east.kind,
        tile: east.tile(),
        ..*members[0]
    };
    shooters(&map, &members, &enemy).len()
}

#[test]
fn a_diagonal_enemy_inside_the_old_square_but_out_of_straight_reach_is_not_focused() {
    let sentinel = |x, y| (UnitKind::Sentinel, x, y);
    let near = field(false, &[sentinel(4, 4)], sentinel(6, 5));
    assert_eq!(focused(&near), 1, "about 2.2 tiles away");
    let diagonal = field(false, &[sentinel(4, 4)], sentinel(7, 7));
    assert_eq!(focused(&diagonal), 0, "3 tiles each way is 4.2 in a line");
}

#[test]
fn ground_members_never_focus_an_enemy_across_ground_they_cannot_walk() {
    let sentinel = |x, y| (UnitKind::Sentinel, x, y);
    assert_eq!(focused(&field(false, &[sentinel(7, 4)], sentinel(9, 4))), 1);
    assert_eq!(
        focused(&field(true, &[sentinel(7, 4)], sentinel(9, 4))),
        0,
        "in range, but across the chasm"
    );
}

#[test]
fn nothing_focuses_an_enemy_behind_terrain_that_stops_its_shot() {
    let behind = |terrain: &str, member: (UnitKind, i32, i32), enemy| {
        let mut scenario = field(false, &[member], enemy);
        scenario.map[4].replace_range(10..11, terrain);
        focused(&scenario)
    };
    let sentinel = (UnitKind::Sentinel, 9, 4);
    let target = (UnitKind::Sentinel, 11, 4);
    assert_eq!(behind(".", sentinel, target), 1, "premise: in reach");
    assert_eq!(behind("#", sentinel, target), 0, "rock, ground round it");
    let gun = (UnitKind::Avalanche, 6, 4);
    let far = (UnitKind::Sentinel, 14, 4);
    assert_eq!(behind("#", gun, far), 1, "shells arc over rock");
    assert_eq!(behind("^", gun, far), 0, "but not over a peak");
}

#[test]
fn a_sapper_beside_a_ground_member_threatens_it() {
    let scenario = field(
        false,
        &[(UnitKind::Sentinel, 4, 4)],
        (UnitKind::Sapper, 5, 4),
    );
    let state = scenario.build().unwrap();
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let members: Vec<&UnitObs> = observation.my_units.iter().collect();
    let [sapper] = &observation.enemy_units[..] else {
        panic!("premise: the Sapper is in sight");
    };
    assert!(threatens(sapper, &members));
}

#[test]
fn a_gun_never_focuses_an_enemy_inside_its_minimum_range() {
    let enemy = (UnitKind::Sentinel, 14, 4);
    let close = field(false, &[(UnitKind::Avalanche, 12, 4)], enemy);
    assert_eq!(focused(&close), 0);
    let clear = field(false, &[(UnitKind::Avalanche, 6, 4)], enemy);
    assert_eq!(focused(&clear), 1);
}
