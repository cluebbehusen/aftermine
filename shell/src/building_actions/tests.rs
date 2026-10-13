use super::*;
use crate::numeric::Fit;
use macroquad::prelude::vec2;
use oxide_sim::Scenario;

pub(crate) fn fixture(kind: BuildingKind, tiers: &[u8], scrap: u32) -> Game {
    let mut scenario = Scenario::from_json(r#"{
        "name":"Building actions", "map":["........................................", "........................................",
          "..1.................................2...", "........................................",
          "........................................", "........................................",
          "........................................", "........................................",
          "........................................", "........................................",
          "........................................", "........................................"],
        "players":[{"name":"You","scrap":0,"bot":false},
                   {"name":"Target","scrap":0,"bot":true}],
        "units":[], "buildings":[]
    }"#).unwrap();
    scenario.players[0].scrap = scrap;
    for (i, _) in tiers.iter().enumerate() {
        scenario.buildings.push(oxide_sim::scenario::BuildingSpec {
            player: 0,
            kind,
            x: 8 + 3 * i.fit::<i32>(),
            y: 5,
        });
    }
    for (i, kind) in [BuildingKind::Fabricator, BuildingKind::Crucible]
        .into_iter()
        .enumerate()
    {
        scenario.buildings.push(oxide_sim::scenario::BuildingSpec {
            player: 0,
            kind,
            x: 3 + 4 * i.fit::<i32>(),
            y: 8,
        });
    }
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
    game.presentation.selection.buildings = game
        .state
        .buildings()
        .iter()
        .filter(|b| b.player == game.presentation.human && b.kind == kind && b.anchor.y == 5)
        .map(|b| b.id)
        .collect();
    let mut json = serde_json::to_value(&*game.state).unwrap();
    for (id, &tier) in game.presentation.selection.buildings.iter().zip(tiers) {
        let b = json["buildings"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|b| b["id"] == serde_json::json!(id))
            .unwrap();
        b["tier"] = tier.into();
        b["hp"] = kind.tier_stats(tier).max_hp.into();
    }
    *game.state = serde_json::from_value(json).unwrap();
    game.state.validate_invariants().unwrap();
    game
}

fn assert_accepted(game: &mut Game) {
    let events = game.do_tick().events;
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, oxide_sim::Event::CommandRejected { .. })),
        "{events:?}"
    );
    game.state.validate_invariants().unwrap();
}

#[test]
fn mixed_tiers_advance_once_and_skip_max_tier_with_pending_input() {
    for (kind, tiers, cost) in [
        (BuildingKind::Reclaimer, vec![0, 0, 0, 1, 1, 1], 900),
        (BuildingKind::Turret, vec![0, 1, 2], 450),
        (BuildingKind::FlakTurret, vec![0, 1], 120),
        (BuildingKind::Array, vec![0, 1], 150),
    ] {
        let mut game = fixture(kind, &tiers, 5000);
        let before = game.state.hash();
        let ids = game.presentation.selection.buildings.clone();
        let batch = SelectedBuildings::inspect(&game.view())
            .upgrade_batch()
            .unwrap();
        assert_eq!(batch.cost, cost);
        let expected: Vec<_> = ids
            .iter()
            .zip(&tiers)
            .filter(|(_, tier)| kind.upgrade_from(**tier).is_some())
            .map(|(&id, _)| id)
            .collect();
        assert_eq!(batch.recipients, expected);
        upgrade(&mut game);
        upgrade(&mut game);
        assert_eq!(game.pending.len(), expected.len());
        assert_eq!(
            game.state.hash(),
            before,
            "previews cannot advance the match"
        );
        assert_accepted(&mut game);
        for (id, tier) in ids.iter().zip(tiers) {
            let b = game.state.building(*id).unwrap();
            assert_eq!(b.tier, (tier + 1).min(kind.tiers().len().fit::<u8>() - 1));
            assert_eq!(b.built(), !expected.contains(id));
        }
    }
}

#[test]
fn funding_and_prerequisites_are_per_recipient_in_canonical_order() {
    let mut game = fixture(BuildingKind::Turret, &[1, 0, 0], 200);
    let ids = game.presentation.selection.buildings.clone();
    let foreign = game
        .state
        .buildings()
        .iter()
        .find(|b| b.player != game.presentation.human)
        .unwrap()
        .id;
    game.presentation.selection.buildings = vec![
        ids[2],
        foreign,
        ids[1],
        ids[0],
        ids[1],
        BuildingId(u32::MAX),
    ];
    let batch = SelectedBuildings::inspect(&game.view())
        .upgrade_batch()
        .unwrap();
    assert_eq!(
        batch.recipients,
        vec![ids[1]],
        "unaffordable earlier tier must not block a cheaper upgrade"
    );
    upgrade(&mut game);
    assert_eq!(game.pending.len(), 1);
    assert_accepted(&mut game);

    let mut game = fixture(BuildingKind::Turret, &[0, 1], 1000);
    let crucible = game
        .state
        .buildings()
        .iter()
        .find(|b| b.kind == BuildingKind::Crucible)
        .unwrap()
        .id;
    let mut json = serde_json::to_value(&*game.state).unwrap();
    json["buildings"]
        .as_array_mut()
        .unwrap()
        .retain(|b| b["id"] != serde_json::json!(crucible));
    *game.state = serde_json::from_value(json).unwrap();
    game.state.validate_invariants().unwrap();
    let batch = SelectedBuildings::inspect(&game.view())
        .upgrade_batch()
        .unwrap();
    assert_eq!(
        batch.recipients,
        vec![game.presentation.selection.buildings[0]]
    );
    assert!(batch.reason.unwrap().contains("Crucible"));
    upgrade(&mut game);
    assert_accepted(&mut game);
}

#[test]
fn pending_purchases_refunds_invalid_commands_and_surrender_share_one_preview() {
    let mut game = fixture(BuildingKind::Reclaimer, &[0, 0], 600);
    let home = game.home_foundry().unwrap().id;
    game.issue(Command::Train {
        building: home,
        kind: oxide_sim::UnitKind::Harvester,
    });
    assert_eq!(
        SelectedBuildings::inspect(&game.view())
            .upgrade_batch()
            .unwrap()
            .recipients
            .len(),
        1
    );
    game.issue(Command::CancelTrain {
        building: home,
        index: 0,
    });
    game.issue(Command::UpgradeBuilding {
        building: BuildingId(u32::MAX),
    });
    assert_eq!(
        SelectedBuildings::inspect(&game.view())
            .upgrade_batch()
            .unwrap()
            .recipients
            .len(),
        2
    );
    game.pending.pop();
    upgrade(&mut game);
    assert_accepted(&mut game);

    let mut game = fixture(BuildingKind::Turret, &[0], 1000);
    game.issue(Command::Surrender);
    upgrade(&mut game);
    stop_or_scrap(&mut game);
    assert_eq!(game.pending.len(), 1);
}

#[test]
fn pending_upgrades_and_cancellations_leave_only_live_defense_recipients() {
    let mut game = fixture(BuildingKind::Turret, &[0, 0, 2], 1000);
    let ids = game.presentation.selection.buildings.clone();
    game.issue(Command::UpgradeBuilding { building: ids[0] });
    stop_or_scrap(&mut game);
    assert!(
        matches!(&game.pending.last().unwrap().command, Command::ClearFocus { buildings } if buildings == &ids[1..])
    );
    assert_accepted(&mut game);
}

#[test]
fn group_site_cancellation_skips_completed_and_committed_upgrades() {
    let mut game = fixture(BuildingKind::Turret, &[0, 0, 0], 1000);
    let ids = game.presentation.selection.buildings.clone();
    let mut json = serde_json::to_value(&*game.state).unwrap();
    let site = json["buildings"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|b| b["id"] == serde_json::json!(ids[1]))
        .unwrap();
    site["phase"] = serde_json::json!({"phase": "site", "progress": 1});
    site["hp"] = (BuildingKind::Turret.base_stats().max_hp / 5).into();
    *game.state = serde_json::from_value(json).unwrap();
    game.state.validate_invariants().unwrap();
    game.issue(Command::UpgradeBuilding { building: ids[0] });
    stop_or_scrap(&mut game);
    assert!(
        matches!(&game.pending.last().unwrap().command, Command::ClearFocus { buildings } if buildings == &[ids[2]])
    );
    scrap_sites(&mut game);
    scrap_sites(&mut game);
    assert_eq!(
        game.pending
            .iter()
            .filter(|pc| matches!(pc.command, Command::Cancel { .. }))
            .count(),
        1
    );
    assert_eq!(game.presentation.selection.buildings, vec![ids[0], ids[2]]);
    assert_accepted(&mut game);
    assert!(game.state.building(ids[1]).is_none());
    assert!(!game.state.building(ids[0]).unwrap().built());
}

#[test]
fn every_building_inherits_single_and_group_actions_from_capabilities() {
    let game = fixture(BuildingKind::Turret, &[0, 0], 10000);
    let original = SelectedBuildings::inspect(&game.view());
    for kind in BuildingKind::ALL {
        for count in [1, 2] {
            let mut selected = original.clone();
            selected.buildings.truncate(count);
            selected.tech = BuildingKind::ALL.iter().map(|kind| (*kind, 0)).collect();
            for b in &mut selected.buildings {
                b.kind = kind;
            }
            let cards = selected.cards(&BindingMap::classic());
            let has = |action| cards.iter().any(|c| c.action == action && c.enabled);
            assert_eq!(
                has(CardAction::Upgrade),
                kind.upgrade_from(0).is_some(),
                "{kind:?} x {count}"
            );
            assert!(
                !has(CardAction::Dispatch(Action::StopOrScrap)),
                "Stop lives on the dock"
            );
            assert_eq!(
                has(CardAction::ArmRally),
                !kind.base_stats().produces.is_empty()
            );
            let production = crate::production::Production::from_selected(selected.clone());
            assert_eq!(
                production.homogeneous(),
                !kind.base_stats().produces.is_empty()
            );
            if let Some(batch) = production.batch(0) {
                assert_eq!(batch.recipients.len(), count);
            }
            for b in &mut selected.buildings {
                b.phase = oxide_sim::BuildingPhase::Site { progress: 0 };
            }
            assert!(
                selected
                    .cards(&BindingMap::classic())
                    .iter()
                    .any(|c| c.action == CardAction::ScrapSites)
            );
        }
    }
}

#[test]
fn a_deep_bore_waits_for_a_foundry_in_reach() {
    let game = fixture(BuildingKind::Extractor, &[0], 1_000);
    let mut selected = SelectedBuildings::inspect(&game.view());
    assert_eq!(
        selected.upgrade_batch().unwrap().recipients.len(),
        1,
        "the home Foundry supports the Extractor"
    );
    selected.foundries.clear();
    let batch = selected.upgrade_batch().unwrap();
    assert!(batch.recipients.is_empty());
    assert_eq!(
        batch.reason.as_deref(),
        Some("needs a Foundry within 8 tiles")
    );
}

/// Loads every selected magazine to capacity, as finished assembly does.
fn load_selected(game: &mut Game) {
    let mut json = serde_json::to_value(&*game.state).unwrap();
    for id in &game.presentation.selection.buildings {
        let b = json["buildings"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|b| b["id"] == serde_json::json!(id))
            .unwrap();
        let kind: BuildingKind = serde_json::from_value(b["kind"].clone()).unwrap();
        b["magazine"]["loaded"] = kind.magazine().unwrap().capacity.into();
    }
    *game.state = serde_json::from_value(json).unwrap();
}

#[test]
fn a_spire_offers_its_strike_only_while_loaded_and_a_full_magazine_refuses_an_order() {
    let mut game = fixture(BuildingKind::Spire, &[0], 2000);
    let bindings = BindingMap::classic();
    let card = |game: &Game, title: &str| {
        SelectedBuildings::inspect(&game.view())
            .cards(&bindings)
            .into_iter()
            .find(|card| card.title == title)
            .unwrap_or_else(|| panic!("no {title} card"))
    };
    assert!(!card(&game, "Launch").enabled, "nothing to launch yet");
    assert!(card(&game, "Order warhead").enabled);
    assert!(!card(&game, "Cancel warhead").enabled);
    assert_eq!(
        card(&game, "Autofill: off").action,
        CardAction::Dispatch(Action::ToggleAutofill)
    );
    load_selected(&mut game);
    assert!(card(&game, "Launch").enabled);
    assert!(
        !card(&game, "Order warhead").enabled,
        "one warhead fills a Spire"
    );
    let selected = SelectedBuildings::inspect(&game.view());
    assert_eq!(
        selected.loaded_silos(),
        game.presentation.selection.buildings
    );
    assert!(matches!(
        selected.autofill_toggle(),
        Some(Command::SetAutofill { enabled: true, .. })
    ));
}

#[test]
fn a_lattice_orders_into_the_emptiest_battery_and_cancels_the_busiest() {
    let mut game = fixture(BuildingKind::Lattice, &[0, 0], 2000);
    // Both start with autofill on.
    let selected = SelectedBuildings::inspect(&game.view());
    assert!(matches!(
        selected.autofill_toggle(),
        Some(Command::SetAutofill { enabled: false, .. })
    ));
    let ids = game.presentation.selection.buildings.clone();
    assert_eq!(selected.round_recipient(), Some(ids[0]));
    assert_eq!(selected.round_to_cancel(), None);
    load_selected(&mut game);
    assert_eq!(
        SelectedBuildings::inspect(&game.view()).round_recipient(),
        None,
        "two full batteries take no order"
    );
}
