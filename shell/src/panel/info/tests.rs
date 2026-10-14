use super::*;
use crate::game::Game;

fn selected(kind: UnitKind, foreign: bool) -> Panel {
    let mut scenario = oxide_sim::Scenario::skirmish();
    let player = u8::from(foreign);
    scenario.units = vec![oxide_sim::scenario::UnitSpec {
        player,
        kind,
        x: 6,
        y: 5,
    }];
    let mut game = Game::with_viewport(scenario, macroquad::prelude::vec2(640.0, 400.0)).unwrap();
    game.presentation.selection.units = game.state.units().iter().map(|u| u.id).collect();
    build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap()
}

#[test]
fn every_unit_exposes_health_sight_and_its_weapon_reload() {
    for kind in UnitKind::ALL {
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
        assert_eq!(cycles.len(), kind.stats().weapons.len());
        for (actual, weapon) in cycles.into_iter().zip(kind.stats().weapons) {
            assert_eq!(*actual, tick_time_label(weapon.cooldown_ticks));
        }
    }
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
fn scouts_list_their_radar_and_a_surveyor_reports_it_unfolding() {
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

    let mut scenario = oxide_sim::Scenario::skirmish();
    scenario.units = vec![oxide_sim::scenario::UnitSpec {
        player: 0,
        kind: UnitKind::Surveyor,
        x: 6,
        y: 5,
    }];
    let mut game = Game::with_viewport(scenario, macroquad::prelude::vec2(640.0, 400.0)).unwrap();
    game.presentation.selection.units = game.state.units().iter().map(|u| u.id).collect();
    let status = |game: &Game| {
        build_for_palette(&game.view(), &BindingMap::classic(), false)
            .unwrap()
            .info
            .status
    };
    assert!(status(&game).is_empty(), "folded at the start");
    game.do_tick();
    assert_eq!(status(&game), ["Unfolding radar"]);
    let deploy = UnitKind::Surveyor.stats().radar.unwrap().deploy_ticks;
    for _ in 1..deploy {
        game.do_tick();
    }
    assert_eq!(status(&game), ["Radar up"]);
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
fn a_battery_counts_warheads_landing_in_its_ring_apart_from_those_still_tracked() {
    use chassis::fx::{Fx, Vec2Fx};
    use oxide_sim::strike::{KnownWarhead, WarheadLine};

    let at = |x: i32| Vec2Fx::new(Fx::from_num(x), Fx::from_num(0));
    let warhead = |landing: Option<i32>, stopped: bool| KnownWarhead {
        id: oxide_sim::WarheadId(0),
        player: None,
        hostile: true,
        position: at(0),
        line: landing.map(|x| WarheadLine {
            launch: at(0),
            impact: at(x),
            arrival: 100,
        }),
        exposure: None,
        interceptor: stopped.then_some(oxide_sim::BuildingId(7)),
    };
    let summary = |warheads: &[KnownWarhead]| incoming_warheads(warheads, at(0), Fx::from_num(15));

    assert_eq!(
        summary(&[warhead(None, false)]).as_deref(),
        Some("Tracking 1 warhead"),
        "a contact without a line is neither incoming nor unanswered"
    );
    assert_eq!(
        summary(&[warhead(Some(20), false)]),
        None,
        "outside the ring"
    );
    assert_eq!(
        summary(&[
            warhead(Some(5), true),
            warhead(Some(10), false),
            warhead(None, false),
            KnownWarhead {
                hostile: false,
                ..warhead(Some(5), false)
            },
        ])
        .as_deref(),
        Some("2 incoming warheads, 1 unanswered, tracking 1 more")
    );
}
