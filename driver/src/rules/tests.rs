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
    assert!(
        sheet.contains("\n# FOUNDRY\n"),
        "the Roster follows the mechanics"
    );
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

#[test]
fn a_gun_that_plants_says_how_long_planting_takes() {
    let sheet = sheet();
    let page = |name: &str| {
        let start = sheet
            .find(&format!("## {name}\n"))
            .unwrap_or_else(|| panic!("no page for {name}"));
        let rest = &sheet[start + 3..];
        rest[..rest.find("\n## ").unwrap_or(rest.len())].to_string()
    };
    let label = aftermine_kit::roster::tick_time_label;
    let piledriver = UnitKind::Piledriver.stats().brace.unwrap();
    let text = page("Piledriver");
    assert!(
        text.contains(&format!(
            "Plants its spades in {} once stopped and turned to its target",
            label(u32::from(piledriver.deploy_ticks))
        )),
        "{text}"
    );
    assert!(
        text.contains("up to 22.5° either way"),
        "the arc is 16 of 256 compass steps: {text}"
    );
    assert!(
        text.contains(&format!(
            "it stows them in {}, starting no sooner than {} after a shot",
            label(u32::from(
                piledriver
                    .deploy_ticks
                    .div_ceil(piledriver.retract_per_tick)
            )),
            label(piledriver.recoil_ticks)
        )),
        "{text}"
    );
    let bombard = page("Bombard");
    assert!(
        bombard.contains("To turn or drive, it stows them"),
        "{bombard}"
    );
    assert!(!page("Lancer").contains("Plants its spades"));
}

#[test]
fn the_sheet_opens_with_the_rules_a_player_cannot_watch() {
    let sheet = sheet();
    assert!(sheet.starts_with("# How the game works"), "{sheet}");
    assert!(
        sheet.contains("pays 180 scrap a minute instead of 120"),
        "the support rates come from the stats table"
    );
    assert!(sheet.contains("within 8 tiles"), "{sheet}");
    assert!(sheet.contains("costs 85% of the price"), "{sheet}");
}

#[test]
fn the_sheet_says_how_far_units_chase_and_why_workers_hold() {
    let sheet = sheet();
    let label = aftermine_kit::roster::tick_time_label;
    assert!(
        sheet.contains(&format!(
            "stood still for {} holds that spot: from there it chases freely within {} tiles",
            label(u32::from(stats::LEASH_STATION_TICKS)),
            stats::LEASH_RADIUS.to_num::<i32>()
        )),
        "{sheet}"
    );
    assert!(
        sheet.contains("the advance reports DangerHold once every"),
        "{sheet}"
    );
    assert!(
        sheet.contains(r#"{"type":"set_idle_weld","units":[3,4],"enabled":false}"#),
        "{sheet}"
    );
    let lancer = UnitKind::Lancer.stats().aggro_range;
    assert!(
        sheet.contains(&format!("Aggro range {lancer} tiles")),
        "{sheet}"
    );
}
