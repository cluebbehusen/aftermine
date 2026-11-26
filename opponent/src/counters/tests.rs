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
            let domain = kind.stats().domain;
            let fires = gun
                .tier_stats(*tier)
                .weapons
                .iter()
                .any(|weapon| weapon.targets.covers(domain));
            if !fires {
                continue;
            }
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
    assert_eq!(pair(gun(BuildingKind::Turret, 0), UnitKind::Mantle), 1_000);
    assert_eq!(
        pair(gun(BuildingKind::FlakTurret, 1), UnitKind::Mantle),
        125,
        "flak cannot fire on the ground"
    );
    assert_eq!(
        pair(gun(BuildingKind::Turret, 0), UnitKind::Kestrel),
        125,
        "a gun destroys none of what it cannot fire on"
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

#[test]
fn a_gun_destroys_none_of_an_attacker_it_cannot_fire_on() {
    assert_eq!(
        pair(Piece::Gun(BuildingKind::FlakTurret, 0), UnitKind::Flakhound),
        125,
        "though neither can hurt the other, flak only fires at the air"
    );
    assert_eq!(
        pair(Piece::Gun(BuildingKind::Turret, 0), UnitKind::Buzzard),
        125
    );
}

#[test]
fn a_gun_destroys_none_of_a_kind_built_to_wreck_structures() {
    assert_eq!(
        pair(Piece::Gun(BuildingKind::Turret, 0), UnitKind::Maw),
        125
    );
    assert_eq!(
        pair(Piece::Gun(BuildingKind::Bastion, 1), UnitKind::Maw),
        125
    );
}

#[test]
fn a_force_bought_to_the_price_to_beat_meets_its_margin() {
    // Production sized this way stops only once a launch would go: the
    // force at that price stands at the margin over the enemy, each weighed
    // against the other.
    let cases = [
        (
            UnitKind::Condor,
            vec![(Piece::Gun(BuildingKind::FlakTurret, 0), 270)],
        ),
        (
            UnitKind::Buzzard,
            vec![(Piece::Unit(UnitKind::Flakhound), 360)],
        ),
        (
            UnitKind::Scuttler,
            vec![(Piece::Unit(UnitKind::Warden), 280)],
        ),
        (
            UnitKind::Breaker,
            vec![(Piece::Gun(BuildingKind::Bastion, 1), 600)],
        ),
    ];
    for (kind, enemy) in cases {
        let price = price_to_beat(&enemy, &[(Piece::Unit(kind), 1)], 2_000);
        let force = [(Piece::Unit(kind), price)];
        let (held, met) = (weigh(&force, &enemy), weigh(&enemy, &force));
        assert!(
            held + 1 >= met * 2,
            "{kind:?}: {held} against {met} at {price}"
        );
        let short = [(Piece::Unit(kind), price * 9 / 10)];
        assert!(
            weigh(&short, &enemy) < met * 2,
            "{kind:?}: a tenth less falls short"
        );
    }
    let nothing: [(Piece, u64); 0] = [];
    assert_eq!(
        price_to_beat(&[(Piece::Unit(UnitKind::Lancer), 330)], &nothing, 2_000),
        660,
        "with nothing to weigh, the enemy's price by the margin"
    );
}

#[test]
fn spotted_artillery_against_a_gun_reads_the_siege_table_and_the_gun_its_inverse() {
    let citadel = Piece::Gun(BuildingKind::Bastion, 1);
    let bastion = Piece::Gun(BuildingKind::Bastion, 0);
    assert!(
        pair(UnitKind::Bombard, citadel) < pair(UnitKind::Bombard, bastion),
        "the Citadel outranges Bombards the Bastion only matches"
    );
    let product = pair(citadel, UnitKind::Bombard) * pair(UnitKind::Bombard, citadel);
    assert!(product.abs_diff(1_000_000) <= 10_000, "{product}");
    assert_eq!(
        pair(Piece::Gun(BuildingKind::FlakTurret, 0), UnitKind::Bombard),
        125,
        "flak cannot fire on the ground"
    );
}
