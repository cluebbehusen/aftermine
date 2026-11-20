use super::*;

#[test]
fn every_kind_has_exactly_one_page() {
    let entries: Vec<Entry> = sections()
        .into_iter()
        .flat_map(|(_, entries)| entries)
        .collect();
    let units = entries
        .iter()
        .filter(|entry| matches!(entry, Entry::Unit(_)))
        .count();
    assert_eq!(units, UnitKind::ALL.len());
    assert_eq!(entries.len() - units, BuildingKind::ALL.len());
    let mut seen = std::collections::BTreeSet::new();
    for entry in &entries {
        assert!(seen.insert(format!("{entry:?}")), "{entry:?} listed twice");
    }
}

#[test]
fn a_page_opens_with_its_description_and_figures() {
    let page = page(Entry::Unit(UnitKind::Lancer));
    assert_eq!(
        page[0],
        Piece::Paragraph(unit_description(UnitKind::Lancer), Tone::Body)
    );
    assert_eq!(
        page[2],
        Piece::Line(unit_stat_line(UnitKind::Lancer), Tone::Primary)
    );
    assert_eq!(
        Entry::Building(BuildingKind::RepairBay).name(),
        "Repair Bay"
    );
    assert_eq!(
        Entry::Unit(UnitKind::Lancer).header(),
        format!("ground | {} scrap", UnitKind::Lancer.stats().cost)
    );
}

#[test]
fn names_read_as_players_write_them() {
    assert_eq!(plural_name("Foundry"), "Foundries");
    assert_eq!(plural_name("Repair Bay"), "Repair Bays");
    assert_eq!(plural_name("Airworks"), "Airworks");
    assert_eq!(series(&[]), "");
    assert_eq!(
        series(&["x".to_string(), "y".to_string(), "z".to_string()]),
        "x, y, and z"
    );
}

#[test]
fn tick_times_round_up_to_a_tenth() {
    let per_second = aftermine_sim::TICKS_PER_SECOND;
    assert_eq!(tick_time_label(4 * per_second), "4s");
    assert_eq!(tick_time_label(per_second / 2), "0.5s");
    assert_eq!(tick_time_label(1), "0.1s");
}

#[test]
fn notes_read_from_the_stats_table() {
    let harvest = UnitKind::Harvester.stats().harvest.unwrap();
    let carries = format!("Carries {} scrap per trip", harvest.capacity);
    assert!(
        unit_notes(UnitKind::Harvester)
            .iter()
            .any(|l| l.starts_with(&carries))
    );
    let slots = UnitKind::Skyhook.stats().transport_capacity;
    assert!(unit_notes(UnitKind::Skyhook).contains(&format!(
        "Carries up to {slots} transport slots of ground units."
    )));
    // Each upgrade rung is two lines: how to get it, then what it has.
    let turret = building_notes(BuildingKind::Turret);
    assert_eq!(
        turret.len(),
        2 * (BuildingKind::Turret.tiers().len() - 1),
        "{turret:?}"
    );
    assert!(turret[0].starts_with("Upgrades to Heavy Turret"));
    assert!(turret[1].starts_with("Heavy Turret: "));
    assert!(
        turret[2].starts_with("Upgrades to Bulwark") && turret[2].ends_with("Needs a Crucible.")
    );

    // The economy lines the build cards show lead the page.
    for kind in [BuildingKind::Extractor, BuildingKind::Foundry] {
        let economy = building_economy_lines(kind);
        assert!(!economy.is_empty(), "{kind:?}");
        assert!(building_notes(kind).starts_with(&economy), "{kind:?}");
    }
    let bore = BuildingKind::Extractor.tier_stats(1).construction;
    let seconds = bore.build_ticks as f32 / aftermine_sim::TICKS_PER_SECOND as f32;
    assert!(
        building_notes(BuildingKind::Extractor).contains(&format!(
            "Upgrades to Deep Bore for {} scrap in {seconds:.0} s. Needs a Crucible and a Foundry within {} tiles.",
            bore.cost,
            aftermine_sim::stats::EXTRACTOR_SUPPORT_RADIUS
        ))
    );
    assert!(
        building_notes(BuildingKind::Foundry)
            .iter()
            .any(|line| line == "Building another Foundry needs a Fabricator.")
    );
}

#[test]
fn requirements_read_as_a_list() {
    assert_eq!(
        building_notes(BuildingKind::RocCradle)
            .last()
            .map(String::as_str),
        Some("Needs a Crucible, a Deep Bore, and an Airworks.")
    );
    assert_eq!(series(&["x".to_string()]), "x");
    assert_eq!(series(&["x".to_string(), "y".to_string()]), "x and y");
    assert_eq!(article("airworks"), "an Airworks");
    assert_eq!(article("deep bore"), "a Deep Bore");
}

#[test]
fn a_cradle_page_gives_crew_times_not_one_worker_s_time() {
    let cradle = BuildingKind::MawCradle;
    assert!(!building_stat_line(cradle).contains("build"));
    assert!(!unit_stat_line(UnitKind::Maw).contains("build"));
    let ticks = cradle.base_stats().construction.build_ticks;
    let crew = aftermine_sim::stats::ASSEMBLY_CREW_CAP;
    let seconds = |worker: UnitKind| {
        ticks as usize
            / crew
            / worker.stats().build_rate as usize
            / aftermine_sim::TICKS_PER_SECOND as usize
    };
    assert!(
        building_notes(cradle).contains(&format!(
            "Takes {} s with {crew} Harvesters, or {} s with {crew} Excavators.",
            seconds(UnitKind::Harvester),
            seconds(UnitKind::Excavator)
        )),
        "{:?}",
        building_notes(cradle)
    );
}

#[test]
fn every_charge_finder_is_named() {
    assert_eq!(
        charge_finders(),
        ["Surveyors", "Kestrels", "Arrays"].map(str::to_string)
    );
}

#[test]
fn an_array_reveals_charges_only_where_its_owner_also_has_sight() {
    let notes = building_notes(BuildingKind::Array).join("\n");
    for line in [
        format!(
            "It reveals Scuttle Charges within {} tiles where you also have sight.",
            aftermine_sim::stats::CHARGE_BASE_ARRAY_DETECT_RADIUS
        ),
        format!(
            "reveals Scuttle Charges within {} tiles where you also have sight",
            aftermine_sim::stats::CHARGE_ARRAY_DETECT_RADIUS
        ),
    ] {
        assert!(notes.contains(&line), "{notes}");
    }
}

#[test]
fn weapon_lines_read_from_the_stats_table() {
    let sentinel = weapon_lines(UnitKind::Sentinel);
    assert_eq!(sentinel.len(), 2, "main gun and the anti-air poke");
    assert!(sentinel[0].contains("dmg"));
    assert!(sentinel[0].contains("tiles"));
    assert!(sentinel[0].contains("ground"));
    assert!(sentinel[1].contains("air"));
    let bombard = weapon_lines(UnitKind::Bombard);
    assert!(bombard[0].contains("| shell"));
    assert!(bombard[0].contains("splash"));
    let roc = weapon_lines(UnitKind::Roc);
    assert_eq!(roc.len(), 1, "identical guns share a line: {roc:?}");
    assert!(roc[0].starts_with("2 guns | "));
}
