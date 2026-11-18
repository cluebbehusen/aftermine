use super::*;
use aftermine_sim::stats::{BuildingKind, UnitKind};

#[test]
fn the_sheet_has_a_page_for_every_kind() {
    let sheet = sheet();
    for kind in UnitKind::ALL {
        let heading = format!("## {}\n", aftermine_kit::roster::entity_name(kind.name()));
        assert!(sheet.contains(&heading), "{heading}");
    }
    for kind in BuildingKind::ALL {
        let heading = format!("## {}\n", aftermine_kit::roster::entity_name(kind.name()));
        assert!(sheet.contains(&heading), "{heading}");
    }
    assert!(sheet.starts_with("# FOUNDRY\n"));
    assert!(sheet.contains(&aftermine_kit::roster::unit_description(UnitKind::Sapper)));
}

#[test]
fn every_armed_page_says_how_often_it_fires() {
    let sheet = sheet();
    let page = |name: &str| {
        let start = sheet
            .find(&format!("## {name}\n"))
            .unwrap_or_else(|| panic!("no page for {name}"));
        let rest = &sheet[start + 3..];
        &rest[..rest.find("\n## ").unwrap_or(rest.len())]
    };
    let lancer = UnitKind::Lancer.stats().weapons;
    assert_eq!(lancer.len(), 1);
    assert!(
        page("Lancer").contains(&format!(
            "Fires once every {}.",
            aftermine_kit::roster::tick_time_label(lancer[0].cooldown_ticks)
        )),
        "{}",
        page("Lancer")
    );
    assert!(
        page("Sentinel").contains("Fires its weapons, in the order listed, once every "),
        "{}",
        page("Sentinel")
    );
    assert!(!page("Harvester").contains("Fires"));
    assert!(page("Turret").contains("Heavy Turret fires once every"));
}
