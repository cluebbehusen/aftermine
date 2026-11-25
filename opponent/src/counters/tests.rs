use super::*;

#[test]
fn the_chart_was_measured_with_the_current_unit_stats() {
    assert_eq!(
        chart::STATS,
        fingerprint(),
        "unit stats changed since the counter chart was measured: \
         run `cargo run --release -p aftermine-driver -- fight-lab --write` \
         from the repository root"
    );
}

#[test]
fn each_pair_reads_as_the_inverse_from_the_other_side() {
    for (a, kind) in MEASURED.iter().enumerate() {
        assert_eq!(chart::TRADES[a][a], 1_000);
        for (b, enemy) in MEASURED.iter().enumerate() {
            let product = u64::from(chart::TRADES[a][b]) * u64::from(chart::TRADES[b][a]);
            assert!(
                product.abs_diff(1_000_000) * 100 <= 1_000_000,
                "{kind:?} v {enemy:?}: {product}"
            );
        }
    }
}

#[test]
fn a_kind_the_chart_does_not_measure_counts_at_its_price() {
    assert_eq!(trade(UnitKind::Bombard, &[(UnitKind::Lancer, 110)]), 1_000);
    assert_eq!(trade(UnitKind::Lancer, &[(UnitKind::Mantle, 500)]), 1_000);
    assert_eq!(trade(UnitKind::Lancer, &[] as &[(UnitKind, u64)]), 1_000);
}

#[test]
fn a_mix_weighs_each_enemy_kind_by_its_value() {
    let lancer = |enemy| pair(UnitKind::Lancer, enemy);
    let mixed = trade(
        UnitKind::Lancer,
        &[(UnitKind::Sentinel, 300), (UnitKind::Warden, 100)],
    );
    assert_eq!(
        mixed,
        (lancer(UnitKind::Sentinel) * 300 + lancer(UnitKind::Warden) * 100) / 400
    );
    assert_eq!(weight(UnitKind::Lancer, &[(UnitKind::Lancer, 110)]), 1_000);
}

#[test]
fn a_gun_reads_as_the_inverse_of_what_its_attackers_trade_against_it() {
    for (a, kind) in MEASURED.iter().enumerate() {
        for (g, (gun, tier)) in GUNS.iter().enumerate() {
            let taken = u64::from(chart::AGAINST[a][g]);
            assert_eq!(pair(*kind, Piece::Gun(*gun, *tier)), taken);
            let product = pair(Piece::Gun(*gun, *tier), *kind) * taken;
            assert!(
                product.abs_diff(1_000_000) <= 10_000 || taken == 125 || taken == 8_000,
                "{gun:?} {tier} against {kind:?}: {product}"
            );
        }
    }
}

#[test]
fn a_gun_against_a_kind_the_chart_does_not_measure_is_even_if_it_can_fire_on_it() {
    let gun = |kind, tier| Piece::Gun(kind, tier);
    assert_eq!(pair(gun(BuildingKind::Turret, 0), UnitKind::Bombard), 1_000);
    assert_eq!(
        pair(gun(BuildingKind::FlakTurret, 1), UnitKind::Bombard),
        125,
        "flak cannot fire on the ground"
    );
    assert_eq!(
        pair(UnitKind::Bombard, gun(BuildingKind::FlakTurret, 0)),
        8_000,
        "and the Bombard destroys it for nothing"
    );
    assert_eq!(
        pair(gun(BuildingKind::Turret, 0), UnitKind::Kestrel),
        1_000,
        "neither can hurt the other"
    );
    assert_eq!(
        pair(gun(BuildingKind::Lattice, 0), UnitKind::Lancer),
        1_000,
        "an unmeasured structure is even"
    );
    assert_eq!(
        pair(gun(BuildingKind::Turret, 0), gun(BuildingKind::Bastion, 1)),
        1_000
    );
}

#[test]
fn two_sides_weighed_against_each_other_stand_in_their_measured_ratio() {
    let lancers = [(Piece::Unit(UnitKind::Lancer), 1_100)];
    let sentinels = [(Piece::Unit(UnitKind::Sentinel), 1_100)];
    let (own, theirs) = (weigh(&lancers, &sentinels), weigh(&sentinels, &lancers));
    let measured = pair(UnitKind::Lancer, UnitKind::Sentinel);
    assert!(
        (own * 1_000 / theirs).abs_diff(measured) * 100 <= measured * 2,
        "{own} v {theirs}: {measured}"
    );
    assert_eq!(weigh(&sentinels, &sentinels), 1_100, "a mirror is even");
}
