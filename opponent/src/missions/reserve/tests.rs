use super::Keep;
use crate::counters::Piece;
use aftermine_sim::{BuildingKind, UnitKind};

fn price(kind: UnitKind) -> u64 {
    u64::from(kind.stats().cost)
}

/// A local guard's keep against `threat` on the ground, at an even share.
fn guarding(threat: Vec<(Piece, u64)>, guns: Vec<(Piece, u64)>) -> Keep {
    Keep {
        open: [true; 2],
        floor: 0,
        share: 1_000,
        threat: [threat, Vec::new()],
        guns: [guns, Vec::new()],
        weights: Vec::new(),
        held: [0; 2],
        extra: [0; 2],
    }
}

/// What `count` units of `kind` at full health hold against the ground.
fn held(keep: &Keep, kind: UnitKind, count: u64) -> u64 {
    keep.force(kind, kind.stats().max_hp)[0] * count
}

#[test]
fn a_guard_holds_by_what_it_trades_against_the_threat_not_by_its_price() {
    let lancers = vec![(Piece::Unit(UnitKind::Lancer), 6 * price(UnitKind::Lancer))];
    let keep = guarding(lancers, Vec::new());
    let value = 6 * price(UnitKind::Lancer);
    // Each guard is worth at least the Lancers.
    let sentinels = value.div_ceil(price(UnitKind::Sentinel));
    let wardens = value.div_ceil(price(UnitKind::Warden));
    let against =
        |kind: UnitKind, count: u64| keep.need(&[(Piece::Unit(kind), count * price(kind))])[0];
    assert!(
        held(&keep, UnitKind::Sentinel, sentinels) < against(UnitKind::Sentinel, sentinels),
        "Lancers beat Sentinels, so their price leaves the guard short"
    );
    assert!(
        held(&keep, UnitKind::Warden, wardens) >= against(UnitKind::Warden, wardens),
        "Wardens beat Lancers, so as much of them holds"
    );
}

#[test]
fn covering_guns_count_by_what_they_trade_against_the_threat() {
    let turret = vec![(Piece::Gun(BuildingKind::Turret, 0), 600)];
    let value = 3_000;
    let need =
        |kind: UnitKind| guarding(vec![(Piece::Unit(kind), value)], turret.clone()).need(&[])[0];
    assert!(
        need(UnitKind::Sentinel) < need(UnitKind::Lancer),
        "a Turret covers more of Sentinels, which it beats, than of Lancers worth as much"
    );
}

#[test]
fn with_nothing_to_weigh_against_the_threat_counts_at_its_price() {
    let threat = vec![
        (Piece::Unit(UnitKind::Lancer), 600),
        (Piece::Unit(UnitKind::Sentinel), 400),
    ];
    let mut keep = guarding(threat, Vec::new());
    keep.share = 1_500;
    assert_eq!(keep.need(&[]), [1_500, 0]);
    keep.floor = 2_000;
    assert_eq!(
        keep.need(&[]),
        [2_000, 2_000],
        "the floor holds in each domain"
    );
    keep.open = [false, true];
    keep.add(0, 100);
    assert_eq!(
        keep.need(&[]),
        [100, 2_000],
        "a domain no enemy reaches keeps only what is kept beyond the need"
    );
}

#[test]
fn a_guard_s_need_against_each_domain_weighs_only_the_units_that_hit_it() {
    let sentinels = vec![(Piece::Unit(UnitKind::Sentinel), 900)];
    let keep = guarding(sentinels, Vec::new());
    let wardens = (Piece::Unit(UnitKind::Warden), 1_120);
    let talons = (Piece::Unit(UnitKind::Talon), 1_000);
    assert_eq!(
        keep.need(&[wardens, talons]),
        keep.need(&[wardens]),
        "Talons hold nothing against Sentinels, which they cannot hit"
    );
}

#[test]
fn an_escort_gap_is_priced_only_with_the_kinds_that_hit_its_domain() {
    let mut keep = guarding(Vec::new(), Vec::new());
    keep.threat[1] = vec![(Piece::Unit(UnitKind::Buzzard), 600)];
    let wardens = [(Piece::Unit(UnitKind::Warden), 1_000)];
    assert_eq!(
        keep.scrap([0, 300], &wardens),
        [0, 300],
        "Wardens cannot hit aircraft, so the air gap stands at its price"
    );
    let talons = [(Piece::Unit(UnitKind::Talon), 1_000)];
    assert!(
        keep.scrap([0, 300], &talons)[1] < 300,
        "Talons, which beat Buzzards, close it for less"
    );
}
