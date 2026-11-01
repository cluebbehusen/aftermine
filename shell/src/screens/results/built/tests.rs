use super::*;
use std::collections::BTreeMap;

#[test]
fn a_seat_lists_units_then_structures_most_built_first() {
    let player = PlayerStats {
        seat: 0,
        scrap: Vec::new(),
        army_value: Vec::new(),
        wealth: Vec::new(),
        scrap_earned: Vec::new(),
        value_lost: Vec::new(),
        kinds: Vec::new(),
        scrap_collected: 0,
        scrap_earned_total: 0,
        value_lost_total: 0,
        units_trained: 0,
        units_built: BTreeMap::from([
            (UnitKind::Lancer, 2),
            (UnitKind::Harvester, 9),
            (UnitKind::Scuttler, 2),
        ]),
        buildings_completed: 0,
        structures_built: BTreeMap::from([(BuildingKind::Turret, 1), (BuildingKind::Airworks, 3)]),
        units_lost: 0,
        buildings_lost: 0,
        buildings_salvaged: 0,
        units_retired: 0,
    };
    let (first, second) = if UnitKind::Scuttler < UnitKind::Lancer {
        (UnitKind::Scuttler, UnitKind::Lancer)
    } else {
        (UnitKind::Lancer, UnitKind::Scuttler)
    };
    assert_eq!(
        entries(&player),
        [
            BuiltEntry::Unit(UnitKind::Harvester, 9),
            BuiltEntry::Unit(first, 2),
            BuiltEntry::Unit(second, 2),
            BuiltEntry::Structure(BuildingKind::Airworks, 3),
            BuiltEntry::Structure(BuildingKind::Turret, 1),
        ]
    );
}

#[test]
fn a_roomy_report_wraps_and_a_tight_one_shrinks() {
    let roomy = row_shape(2, 600.0, 1.0).expect("plenty of room");
    assert_eq!((roomy.icon, roomy.lines), (LARGEST_ICON, 2));
    assert!(
        roomy.height * 2.0 < 600.0,
        "rows stack instead of spreading"
    );

    let tight = row_shape(8, 200.0, 1.0).expect("still legible");
    assert_eq!(tight.lines, 1);
    assert!(tight.height * 8.0 <= 200.0);
    assert!(tight.icon < tight.height);

    assert_eq!(row_shape(8, 100.0, 1.0), None, "too short to read");
}
