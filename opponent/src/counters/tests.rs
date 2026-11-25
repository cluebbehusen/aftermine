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
    assert_eq!(trade(UnitKind::Lancer, &[]), 1_000);
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
        for (g, gun) in GUNS.iter().enumerate() {
            let taken = u64::from(chart::AGAINST[a][g]);
            let domain = kind.stats().domain;
            let fires = gun
                .base_stats()
                .weapons
                .iter()
                .any(|weapon| weapon.targets.covers(domain));
            if !fires {
                continue;
            }
            let product = against(*gun, *kind) * taken;
            assert!(
                product.abs_diff(1_000_000) <= 10_000 || taken == 125 || taken == 8_000,
                "{gun:?} against {kind:?}: {product}"
            );
        }
    }
}

#[test]
fn a_gun_against_a_kind_the_chart_does_not_measure_is_even_if_it_can_fire_on_it() {
    assert_eq!(against(BuildingKind::Turret, UnitKind::Bombard), 1_000);
    assert_eq!(
        against(BuildingKind::FlakTurret, UnitKind::Bombard),
        125,
        "flak cannot fire on the ground"
    );
    assert_eq!(
        against(BuildingKind::Lattice, UnitKind::Lancer),
        1_000,
        "an unmeasured structure is even"
    );
}

#[test]
fn a_gun_destroys_none_of_an_attacker_it_cannot_fire_on() {
    assert_eq!(
        against(BuildingKind::FlakTurret, UnitKind::Flakhound),
        125,
        "though neither can hurt the other, flak only fires at the air"
    );
    assert_eq!(against(BuildingKind::Turret, UnitKind::Buzzard), 125);
}

#[test]
fn a_gun_destroys_none_of_a_kind_built_to_wreck_structures() {
    assert_eq!(against(BuildingKind::Turret, UnitKind::Maw), 125);
    assert_eq!(against(BuildingKind::Bastion, UnitKind::Maw), 125);
}
