use super::*;
use aftermine_sim::PlayerId;

fn crusher((x, y): (i32, i32), heading: u8) -> UnitObs {
    let scenario = crate::tests::match_on(
        crate::tests::strip(false),
        &["West", "East"],
        &[(1, UnitKind::Maw, x, y)],
    );
    let state = scenario.build().unwrap();
    let mut maw = aftermine_sim::observation::ObservationData::fog_honest(&state, PlayerId(1))
        .my_units
        .into_iter()
        .find(|unit| unit.kind == UnitKind::Maw)
        .unwrap();
    maw.heading = heading;
    maw
}

#[test]
fn a_small_machine_ahead_of_a_crusher_is_in_its_path_and_one_beside_or_behind_is_not() {
    // Compass step 0 faces along +x.
    let maw = crusher((10, 6), 0);
    let interval = 12;
    assert!(in_path(
        &maw,
        UnitKind::Sentinel,
        TilePos::new(13, 6),
        interval
    ));
    assert!(in_path(
        &maw,
        UnitKind::Harvester,
        TilePos::new(12, 7),
        interval
    ));
    assert!(!in_path(
        &maw,
        UnitKind::Sentinel,
        TilePos::new(7, 6),
        interval
    ));
    assert!(!in_path(
        &maw,
        UnitKind::Sentinel,
        TilePos::new(11, 11),
        interval
    ));
    assert!(
        !in_path(&maw, UnitKind::Breaker, TilePos::new(13, 6), interval),
        "a Breaker is too broad to go under its front"
    );
    let turned = crusher((10, 6), 128);
    assert!(!in_path(
        &turned,
        UnitKind::Sentinel,
        TilePos::new(13, 6),
        interval
    ));
    assert!(in_path(
        &turned,
        UnitKind::Sentinel,
        TilePos::new(7, 6),
        interval
    ));
}
