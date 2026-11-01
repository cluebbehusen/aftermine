use super::*;
use macroquad::prelude::vec2;

fn drive(screen: &mut CodexScreen, key: Key) -> Out {
    let mut mouse = vec2(0.0, 0.0);
    let mut sounds = Vec::new();
    screen.update(
        &[RawEvent::KeyDown { key }, RawEvent::KeyUp { key }],
        &mut mouse,
        &mut sounds,
    )
}

/// Every row's page, in list order.
fn pages(screen: &CodexScreen) -> Vec<Entry> {
    screen
        .menu
        .lines()
        .iter()
        .filter_map(|line| match line {
            crate::menu::Line::Row(_, entry) => Some(*entry),
            crate::menu::Line::Header(_) => None,
        })
        .collect()
}

#[test]
fn every_kind_has_exactly_one_page() {
    let screen = CodexScreen::open();
    let mut units = 0;
    let mut buildings = 0;
    for entry in pages(&screen) {
        match entry {
            Entry::Unit(_) => units += 1,
            Entry::Building(_) => buildings += 1,
        }
    }
    assert_eq!(units, aftermine_sim::stats::UnitKind::ALL.len());
    assert_eq!(buildings, aftermine_sim::stats::BuildingKind::ALL.len());
    // No unit is listed under two factories.
    let mut seen = std::collections::BTreeSet::new();
    for entry in pages(&screen) {
        assert!(seen.insert(format!("{entry:?}")), "{entry:?} listed twice");
    }
}

#[test]
fn opens_on_a_page_and_escape_leaves() {
    let mut screen = CodexScreen::open();
    assert_eq!(screen.mode_name(), "codex");
    assert_eq!(
        screen.selected_entry(),
        Some(Entry::Unit(UnitKind::Harvester)),
        "the first real row is the first Foundry unit"
    );
    assert_eq!(drive(&mut screen, Key::Down), Out::Stay);
    assert_eq!(
        screen.selected_entry(),
        Some(Entry::Unit(UnitKind::Sentinel))
    );
    assert_eq!(drive(&mut screen, Key::Escape), Out::Leave);
}

#[test]
fn the_back_button_leaves_and_no_row_does() {
    let mut screen = CodexScreen::open();
    assert!(!screen.menu.view().items.iter().any(|item| item == "Back"));
    let last = screen.menu.view().items.len() - 1;
    screen.menu.select(last);
    assert!(screen.selected_entry().is_some(), "the last row is a page");
    assert_eq!(drive(&mut screen, Key::Enter), Out::Stay);
    for touch in [false, true] {
        let mut mouse = vec2(0.0, 0.0);
        let out = screen.update(
            &crate::button::press_back(touch),
            &mut mouse,
            &mut Vec::new(),
        );
        assert_eq!(out, Out::Leave);
    }
}

#[test]
fn activating_a_page_row_stays() {
    let mut screen = CodexScreen::open();
    assert_eq!(drive(&mut screen, Key::Enter), Out::Stay);
}

#[test]
fn notes_read_from_the_stats_table() {
    let harvester = unit_notes(UnitKind::Harvester);
    assert!(
        harvester
            .iter()
            .any(|l| l.starts_with("Carries 10 scrap per trip"))
    );
    let skyhook = unit_notes(UnitKind::Skyhook);
    assert!(
        skyhook
            .iter()
            .any(|l| l == "Carries up to 4 transport slots of ground units.")
    );
    // Each upgrade rung is two lines: how to get it, then what it has.
    let turret = building_notes(BuildingKind::Turret);
    assert_eq!(turret.len(), 4, "two upgrade rungs: {turret:?}");
    assert!(turret[0].starts_with("Upgrades to Heavy Turret"));
    assert!(turret[1].starts_with("Heavy Turret: "));
    assert!(
        turret[2].starts_with("Upgrades to Bulwark") && turret[2].ends_with("Needs a Crucible.")
    );

    let extractor = building_notes(BuildingKind::Extractor);
    assert!(
        extractor.iter().any(|line| line
            == "Makes 120 scrap/min, or 180 scrap/min with your Foundry within 8 tiles.")
    );
    assert!(extractor.iter().any(|line| line
        == "Upgrades to Deep Bore for 350 scrap in 30 s. Needs a Crucible and a Foundry within 8 tiles."));

    let foundry = building_notes(BuildingKind::Foundry);
    assert!(foundry.iter().any(|line| line
        == "Raises the output of your Extractors within 8 tiles from 120 to 180 scrap/min."));
    assert!(
        foundry
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
    let ticks = cradle
        .base_stats()
        .construction
        .expect("a cradle is built")
        .build_ticks;
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
fn the_last_section_is_structures() {
    let screen = CodexScreen::open();
    assert!(
        screen
            .menu
            .lines()
            .iter()
            .any(|line| matches!(line, crate::menu::Line::Header(title) if title == "STRUCTURES"))
    );
}
