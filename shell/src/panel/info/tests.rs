use super::*;
use crate::game::Game;

fn selected(kind: UnitKind, foreign: bool) -> Panel {
    let mut scenario = aftermine_sim::Scenario::skirmish();
    let player = u8::from(foreign);
    // Clear of the Foundry by the widest hull's overhang.
    scenario.units = vec![aftermine_sim::scenario::UnitSpec {
        player,
        kind,
        x: 7,
        y: 7,
    }];
    let mut game = Game::with_viewport(scenario, macroquad::prelude::vec2(640.0, 400.0)).unwrap();
    game.presentation.selection.units = game.state.units().iter().map(|u| u.id).collect();
    build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap()
}

#[test]
fn every_unit_exposes_health_sight_and_its_weapon_reload() {
    // A scenario cannot start with a drone: one exists only with the carrier
    // it docks on.
    for kind in UnitKind::ALL
        .into_iter()
        .filter(|kind| !kind.stats().docks())
    {
        let panel = selected(kind, false);
        assert_eq!(
            panel.info.health.map(|health| (health.hp, health.max_hp)),
            Some((kind.stats().max_hp, kind.stats().max_hp))
        );
        assert!(
            panel
                .info
                .rows
                .iter()
                .any(|r| r.label == "Sight" && r.value == format!("{} tiles", kind.stats().vision))
        );
        let cycles: Vec<_> = panel
            .info
            .rows
            .iter()
            .filter(|r| r.label == "Reload")
            .map(|r| &r.value)
            .collect();
        // Neighbouring weapons that read the same share one block.
        let contact = kind.stats().contact_reach.is_some();
        let mut blocks: Vec<_> = kind
            .stats()
            .weapons
            .iter()
            .map(|weapon| (weapon_rows(weapon, contact), weapon.cooldown_ticks))
            .collect();
        blocks.dedup_by(|a, b| a.0 == b.0);
        assert_eq!(cycles.len(), blocks.len(), "{kind:?}");
        for (actual, (_, cooldown)) in cycles.into_iter().zip(blocks) {
            assert_eq!(*actual, tick_time_label(cooldown));
        }
    }
}

#[test]
fn a_roc_shows_its_twin_guns_once_and_counts_its_drones() {
    let stats = UnitKind::Roc.stats();
    let bay = stats.bay.unwrap();
    let panel = selected(UnitKind::Roc, false);
    let sections: Vec<_> = panel
        .info
        .rows
        .iter()
        .filter(|row| row.section)
        .map(|row| (row.label.as_str(), row.value.as_str()))
        .collect();
    let damage = format!("{} dmg/hit", stats.weapons[0].damage);
    assert_eq!(
        sections,
        [("Ground \u{d7}2", damage.as_str())],
        "two identical guns, one block"
    );
    let drones = panel
        .info
        .rows
        .iter()
        .find(|r| r.label == "Drones")
        .unwrap();
    assert_eq!(drones.value, format!("0/{}", bay.capacity));
    assert_eq!(
        drones.icon,
        Some(StatIcon::Capability(CapabilityIcon::Drones)),
        "not the Cargo row's crane"
    );
    let hostile = selected(UnitKind::Roc, true);
    let drones = hostile
        .info
        .rows
        .iter()
        .find(|r| r.label == "Drones")
        .unwrap();
    assert_eq!(drones.value, format!("up to {}", bay.capacity));
}

#[test]
fn role_facts_preserve_demolition_damage() {
    let sapper = selected(UnitKind::Sapper, false);
    assert!(
        sapper
            .info
            .rows
            .iter()
            .any(|r| r.label == "Structure hit" && r.value == "250 damage")
    );
}

#[test]
fn hostile_inspection_does_not_expose_current_load_or_orders() {
    for kind in [UnitKind::Skyhook, UnitKind::Harvester] {
        let panel = selected(kind, true);
        assert!(panel.cards.is_empty() && panel.queue.is_empty());
        let load = panel
            .info
            .rows
            .iter()
            .find(|r| matches!(r.label.as_str(), "Cargo" | "Scrap load"))
            .unwrap();
        assert!(!load.value.contains('/'));
    }
}

#[test]
fn scouts_list_their_radar_and_a_surveyor_unfolds_it_without_a_status_line() {
    let radar = |panel: &Panel| {
        panel
            .info
            .rows
            .iter()
            .find(|r| r.label == "Radar")
            .map(|r| r.value.clone())
    };
    assert_eq!(
        radar(&selected(UnitKind::Kestrel, false)),
        Some("14 tiles".into())
    );
    assert_eq!(
        radar(&selected(UnitKind::Surveyor, false)),
        Some("12 tiles (parked)".into())
    );
    assert_eq!(radar(&selected(UnitKind::Sentinel, false)), None);

    let mut scenario = aftermine_sim::Scenario::skirmish();
    scenario.units = vec![aftermine_sim::scenario::UnitSpec {
        player: 0,
        kind: UnitKind::Surveyor,
        x: 6,
        y: 5,
    }];
    let mut game = Game::with_viewport(scenario, macroquad::prelude::vec2(640.0, 400.0)).unwrap();
    game.presentation.selection.units = game.state.units().iter().map(|u| u.id).collect();
    // The unfolding rig and the radar ring say what its state is.
    let quiet = |game: &Game| {
        let info = build_for_palette(&game.view(), &BindingMap::classic(), false)
            .unwrap()
            .info;
        info.status.is_empty() && info.reserved.is_none()
    };
    assert!(quiet(&game), "folded at the start");
    game.do_tick();
    assert!(game.state.units()[0].radar_deploy > 0, "premise: unfolding");
    assert!(quiet(&game), "unfolding");
    let deploy = UnitKind::Surveyor.stats().radar.unwrap().deploy_ticks;
    for _ in 1..deploy {
        game.do_tick();
    }
    assert!(game.state.units()[0].radar().is_some(), "premise: up");
    assert!(quiet(&game), "up");
}

#[test]
fn point_defense_charges_read_the_same_for_owner_and_enemy() {
    for foreign in [false, true] {
        let panel = selected(UnitKind::Mantle, foreign);
        assert!(
            panel
                .info
                .rows
                .iter()
                .any(|r| r.label == "Charges" && r.value == "3/3"),
            "foreign: {foreign}"
        );
    }
}

#[test]
fn point_defense_reach_carries_the_icon_its_range_ring_draws() {
    let panel = selected(UnitKind::Mantle, false);
    let reach = panel
        .info
        .rows
        .iter()
        .find(|r| r.label == "Intercept reach")
        .unwrap();
    assert_eq!(reach.value, "4.5 tiles");
    assert_eq!(
        reach.icon,
        Some(StatIcon::Capability(CapabilityIcon::Intercept))
    );
}

#[test]
fn a_lattice_counts_warheads_landing_in_its_ring_and_those_no_interceptor_meets() {
    use aftermine_sim::strike::{KnownWarhead, WarheadLine};
    use chassis::fx::{Fx, Vec2Fx};

    let at = |x: i32| Vec2Fx::new(Fx::from_num(x), Fx::from_num(0));
    let warhead = |landing: Option<i32>, stopped: bool| KnownWarhead {
        id: aftermine_sim::WarheadId(0),
        player: None,
        hostile: true,
        position: at(0),
        line: landing.map(|x| WarheadLine {
            launch: at(0),
            impact: at(x),
            arrival: 100,
        }),
        exposure: None,
        interceptor: stopped.then_some(aftermine_sim::BuildingId(7)),
    };
    let line = |warheads: &[KnownWarhead]| incoming_line(warheads, at(0), Fx::from_num(15));

    assert_eq!(
        line(&[warhead(None, false)]),
        None,
        "a contact without a line is not incoming"
    );
    assert_eq!(line(&[warhead(Some(20), false)]), None, "outside the ring");
    assert_eq!(
        line(&[warhead(Some(5), true), warhead(Some(10), true)]).as_deref(),
        Some("2 incoming"),
        "every one answered"
    );
    assert_eq!(
        line(&[
            warhead(Some(5), true),
            warhead(Some(10), false),
            warhead(None, false),
            KnownWarhead {
                hostile: false,
                ..warhead(Some(5), false)
            },
        ])
        .as_deref(),
        Some("2 incoming, 1 unanswered")
    );
}

#[test]
fn only_a_lattice_keeps_a_status_line_while_it_is_blank() {
    for (kind, reserved) in [
        (BuildingKind::Lattice, true),
        (BuildingKind::Spire, false),
        (BuildingKind::Turret, false),
    ] {
        let game = crate::building_actions::tests::fixture(kind, &[0], 0);
        let info = build_for_palette(&game.view(), &BindingMap::classic(), false)
            .unwrap()
            .info;
        assert_eq!(
            info.reserved,
            reserved.then(ReservedLine::default),
            "{kind:?}"
        );
        assert!(info.status.is_empty(), "{kind:?}: {:?}", info.status);
    }
}

#[test]
fn rows_speak_the_rosters_words() {
    let row = |panel: &Panel, label: &str| {
        panel
            .info
            .rows
            .iter()
            .find(|r| r.label == label)
            .map(|r| r.value.clone())
    };
    let skyhook = UnitKind::Skyhook.stats().transport_capacity;
    assert_eq!(
        row(&selected(UnitKind::Skyhook, false), "Cargo"),
        Some(format!("0/{skyhook} slots"))
    );
    assert_eq!(
        row(&selected(UnitKind::Skyhook, true), "Cargo"),
        Some(format!("{skyhook} slots"))
    );
    assert!(row(&selected(UnitKind::Surveyor, false), "Charge detection").is_some());
}
