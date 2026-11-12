use super::*;
use aftermine_protocol::Key;
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
            &crate::button::tests::press_back(touch),
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

/// A stand-in for the body font: wider than its average glyph, so a page
/// that fits here fits the real face.
fn measure(text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * size * 0.55
}

/// Every page set for `viewport` at the user scale `user`, with the room
/// it had.
fn set_pages(viewport: Vec2, user: f32) -> Vec<(Entry, PageBody, f32)> {
    crate::render::set_viewport(viewport.x, viewport.y);
    crate::render::set_user_scale(user);
    let s = crate::render::ui_scale();
    let frame = PageFrame::at(s);
    let pages = pages(&CodexScreen::open())
        .into_iter()
        .map(|entry| {
            let body = page_body(entry, frame.body_width(), frame.body_room(), s, &measure);
            (entry, body, frame.body_room())
        })
        .collect();
    crate::render::set_viewport(1280.0, 800.0);
    crate::render::set_user_scale(1.0);
    pages
}

#[test]
fn every_page_shows_all_its_lines_at_the_largest_scale_and_the_floor_size() {
    for (viewport, user) in [(vec2(1280.0, 800.0), 1.5), (vec2(1133.0, 744.0), 1.0)] {
        for (entry, body, room) in set_pages(viewport, user) {
            assert!(
                body.lines.len() as f32 * body.line_h <= room,
                "{entry:?} at {viewport} and {user}x runs past its box"
            );
            assert!(
                body.lines.iter().all(|(line, _)| !line.ends_with("...")),
                "{entry:?} at {viewport} and {user}x lost lines"
            );
        }
    }
}

#[test]
fn a_long_page_steps_down_the_type_scale_rather_than_dropping_lines() {
    let turret = set_pages(vec2(1280.0, 800.0), 1.5)
        .into_iter()
        .find(|(entry, _, _)| *entry == Entry::Building(BuildingKind::Turret))
        .expect("the Turret has a page");
    assert!(
        turret.1.size < crate::theme::Type::Body.at(1.5),
        "the Turret's page is too long for body type at 150%"
    );
}

#[test]
fn a_page_too_long_for_the_smallest_type_says_it_goes_on() {
    let body = page_body(
        Entry::Building(BuildingKind::Turret),
        300.0,
        60.0,
        1.0,
        &measure,
    );
    assert!(body.lines.len() as f32 * body.line_h <= 60.0);
    let (last, _) = body.lines.last().expect("a line fits");
    assert!(last.ends_with("..."), "{last}");
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
