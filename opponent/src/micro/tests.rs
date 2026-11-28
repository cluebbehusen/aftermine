use super::*;

#[test]
fn the_widest_blast_is_a_shell_s() {
    let blast = widest_blast();
    assert!(blast > Fx::ZERO);
    assert!(
        UnitKind::ALL
            .iter()
            .flat_map(|kind| kind.stats().weapons)
            .filter(|weapon| weapon.projectile.is_some())
            .all(|weapon| weapon.splash.is_none_or(|splash| splash <= blast))
    );
}

#[test]
fn a_kind_reaches_a_domain_only_with_a_weapon_that_hits_it() {
    assert!(range_against(UnitKind::Lancer, Domain::Air).is_none());
    let (slot, range) = range_against(UnitKind::Lancer, Domain::Ground).unwrap();
    assert_eq!(slot, 0);
    assert_eq!(range, UnitKind::Lancer.stats().weapons[0].range);
}
