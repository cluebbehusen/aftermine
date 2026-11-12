use super::*;
use crate::game::Game;

#[test]
fn every_dock_chip_that_cancels_work_discards_it_and_nothing_else_does() {
    let site = aftermine_sim::BuildingId(3);
    let tile = chassis::grid::TilePos::new(4, 5);
    for action in [
        CardAction::CancelOrder {
            unit: aftermine_sim::UnitId(1),
            key: aftermine_sim::OrderKey::Walk { tile },
            from_end: 0,
        },
        CardAction::CancelSite(site),
        CardAction::CancelFound(BuildingKind::Turret, tile),
        CardAction::CancelQueue(site, 0),
        CardAction::CancelRound(site, 0),
        CardAction::CancelDrone(aftermine_sim::UnitId(2), 0),
        CardAction::CancelProduction(UnitKind::Harvester),
    ] {
        assert!(action.discards_work(), "{action:?}");
    }
    for action in [
        CardAction::Dispatch(Action::Hunt),
        CardAction::ArmRally,
        CardAction::Unload,
        CardAction::FilterKind(UnitKind::Harvester),
        CardAction::None,
        CardAction::Refused,
    ] {
        assert!(!action.discards_work(), "{action:?}");
    }
}

#[test]
fn card_copy_names_only_what_the_hands_in_use_have() {
    use crate::platform::tests::{ALL_HANDS, assert_copy_fits};
    for hands in ALL_HANDS {
        let roster = roster_filter_desc(hands);
        assert_eq!(roster.len(), 2);
        for line in &roster {
            assert_copy_fits(hands, line);
        }
        assert_copy_fits(hands, &patrol_desc(hands));
        assert_copy_fits(hands, &patrolling_desc(hands));
        assert_copy_fits(hands, unload_queue_desc(hands));
        assert_copy_fits(hands, transport_load_desc(hands.touch()));
    }
    // A controller marks points with its order button and starts the loop
    // from the ring, never the ribbon's Start.
    for hands in crate::platform::tests::pad_hands() {
        let patrol = patrol_desc(hands);
        assert_copy_fits(hands, &patrol);
        let mark = crate::pad::button_name(hands.pad.unwrap(), aftermine_protocol::PadButton::West);
        assert!(
            patrol.contains(mark) && !patrol.contains("Start"),
            "{patrol}"
        );
    }
    let patrols: std::collections::BTreeSet<_> = ALL_HANDS.map(patrol_desc).into();
    assert_eq!(
        patrols.len(),
        ALL_HANDS.len(),
        "each pairing has its own patrol copy"
    );
}

#[test]
fn stripping_hotkeys_clears_every_card_row() {
    let mut game = game();
    game.presentation
        .selection
        .set_buildings(vec![human_foundry(&game)]);
    let mut panel =
        build_for_palette(&game.view(), &BindingMap::classic(), false).expect("hq panel");
    assert!(panel.cards.iter().any(|card| !card.hotkey.is_empty()));
    strip_hotkeys(&mut panel);
    for card in panel
        .roster
        .iter()
        .map(|chip| &chip.card)
        .chain(&panel.cards)
        .chain(&panel.queue)
    {
        assert!(
            card.hotkey.is_empty(),
            "{} keeps {}",
            card.title,
            card.hotkey
        );
    }

    let harvester = own_unit(&game, UnitKind::Harvester);
    game.state.tick(&[PlayerCommand {
        player: game.presentation.human,
        command: Command::Hunt {
            units: vec![harvester],
            goal: chassis::grid::TilePos::new(8, 8),
            queue: false,
        },
    }]);
    game.presentation.selection.clear();
    game.presentation.selection.set_units(vec![harvester]);
    let mut panel =
        build_for_palette(&game.view(), &BindingMap::classic(), false).expect("unit panel");
    assert_eq!(
        panel.stop.as_ref().map(|stop| stop.hotkey.as_str()),
        Some("X")
    );
    strip_hotkeys(&mut panel);
    assert_eq!(panel.stop.map(|stop| stop.hotkey), Some(String::new()));
}

#[test]
fn the_all_button_names_its_key_only_while_a_keyboard_is_in_use() {
    let mut game = game();
    let harvester = own_unit(&game, UnitKind::Harvester);
    game.presentation.selection.set_units(vec![harvester]);
    let input = crate::input::InputState::new();
    for hands in crate::platform::tests::ALL_HANDS
        .into_iter()
        .chain(crate::platform::tests::pad_hands())
    {
        crate::platform::set_hands(hands);
        let panel = build_for_input(&game.view(), &BindingMap::classic(), &input).expect("a panel");
        let all = panel.select_all.expect("more Harvesters to gather");
        assert_eq!(
            all.card.hotkey,
            if hands.keys { "I" } else { "" },
            "{hands:?}"
        );
    }
    crate::platform::set_hands(crate::platform::Hands::BUILD);
}

fn stat<'a>(panel: &'a Panel, label: &str) -> &'a info::StatRow {
    panel
        .info
        .rows
        .iter()
        .find(|row| row.label == label)
        .unwrap()
}
use crate::tests::support::{issue_tick, own_building, own_unit, sandbox};
use aftermine_sim::{Command, PlayerCommand, Scenario};
use macroquad::prelude::vec2;

/// Skirmish with a Sentinel beside West's start, since the opening fields
/// no fighter.
fn game() -> Game {
    let mut scenario = Scenario::skirmish();
    scenario.units.push(aftermine_sim::scenario::UnitSpec {
        player: 0,
        kind: UnitKind::Sentinel,
        x: 10,
        y: 7,
    });
    Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("skirmish builds")
}

fn human_foundry(game: &Game) -> aftermine_sim::BuildingId {
    game.state
        .buildings()
        .iter()
        .find(|b| b.player == game.presentation.human)
        .expect("human foundry")
        .id
}

fn extractor_panel_game() -> Game {
    let mut scenario = Scenario::skirmish();
    let frames: Vec<_> = scenario
        .map
        .iter()
        .enumerate()
        .flat_map(|(y, row)| {
            row.char_indices()
                .filter(|(_, tile)| *tile == 'E')
                .map(move |(x, _)| (x.fit::<i32>(), y.fit::<i32>()))
        })
        .collect();
    assert!(frames.len() >= 3, "fixture needs home and remote frames");
    let last = frames.len() - 1;
    scenario
        .buildings
        .extend(frames.into_iter().enumerate().map(|(index, (x, y))| {
            aftermine_sim::scenario::BuildingSpec {
                player: u8::from(index == last),
                kind: BuildingKind::Extractor,
                x,
                y,
            }
        }));
    Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("Extractor fixture builds")
}

#[test]
fn nothing_selected_builds_no_panel() {
    let game = game();
    assert!(build_for_palette(&game.view(), &BindingMap::classic(), false).is_none());
}

#[test]
fn own_extractors_name_current_income_without_exposing_foreign_support() {
    let mut game = extractor_panel_game();
    let own_extractors: Vec<_> = game
        .state
        .buildings()
        .iter()
        .filter(|building| {
            building.player == game.presentation.human && building.kind == BuildingKind::Extractor
        })
        .map(|building| building.id)
        .collect();
    let supported = own_extractors
        .iter()
        .copied()
        .find(|id| {
            game.state
                .extractor_income(*id)
                .map(aftermine_sim::ExtractorIncome::support)
                == Some(aftermine_sim::ExtractorSupport::Supported)
        })
        .expect("a home Extractor is supported");
    let remote = own_extractors
        .iter()
        .copied()
        .find(|id| {
            game.state
                .extractor_income(*id)
                .map(aftermine_sim::ExtractorIncome::support)
                == Some(aftermine_sim::ExtractorSupport::Remote)
        })
        .expect("a distant Extractor is remote");

    let income = |support| {
        format!(
            "{} scrap/min",
            aftermine_sim::ExtractorIncome::new(0, support).scrap_per_minute()
        )
    };
    game.presentation.selection.set_buildings(vec![supported]);
    let panel =
        build_for_palette(&game.view(), &BindingMap::classic(), false).expect("supported panel");
    assert_eq!(
        stat(&panel, "Income").value,
        income(aftermine_sim::ExtractorSupport::Supported)
    );
    assert_eq!(stat(&panel, "Support").value, "Foundry");

    game.presentation.selection.set_buildings(vec![remote]);
    let panel =
        build_for_palette(&game.view(), &BindingMap::classic(), false).expect("remote panel");
    assert_eq!(
        stat(&panel, "Income").value,
        income(aftermine_sim::ExtractorSupport::Remote)
    );
    assert_eq!(stat(&panel, "Support").value, "Remote");

    let foreign = game
        .state
        .buildings()
        .iter()
        .find(|building| {
            building.player != game.presentation.human && building.kind == BuildingKind::Extractor
        })
        .expect("foreign Extractor")
        .id;
    assert_eq!(
        game.state
            .extractor_income(foreign)
            .map(aftermine_sim::ExtractorIncome::support),
        Some(aftermine_sim::ExtractorSupport::Supported),
        "the fixture needs private dynamic support to hide"
    );
    game.presentation.selection.set_buildings(vec![foreign]);
    let panel =
        build_for_palette(&game.view(), &BindingMap::classic(), false).expect("foreign panel");
    assert!(
        panel
            .info
            .rows
            .iter()
            .all(|row| !matches!(row.label.as_str(), "Income" | "Support"))
    );
    assert_eq!(
        building_income(&game.view(), game.state.building(foreign).unwrap()),
        0
    );
    assert_eq!(stat(&panel, "Sight").value, "4 tiles");
}

#[test]
fn recurring_income_tracks_real_output_upgrade_downtime_and_foundry_warmup() {
    let mut scenario = Scenario::skirmish();
    scenario.players[0].scrap = 10_000;
    scenario.buildings.extend([
        aftermine_sim::scenario::BuildingSpec {
            player: 0,
            kind: BuildingKind::Reclaimer,
            x: 9,
            y: 3,
        },
        aftermine_sim::scenario::BuildingSpec {
            player: 0,
            kind: BuildingKind::Fabricator,
            x: 12,
            y: 3,
        },
    ]);
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
    let reclaimer = own_building(&game, BuildingKind::Reclaimer);
    let foundry = human_foundry(&game);
    let rate = |game: &Game, id| building_income(&game.view(), game.state.building(id).unwrap());
    assert_eq!(rate(&game, reclaimer), 30);
    assert_eq!(rate(&game, foundry), 0);
    let initial = game.state.player(game.presentation.human).scrap;
    for _ in 0..1_200 {
        game.state.tick(&[]);
    }
    assert_eq!(
        game.state.player(game.presentation.human).scrap - initial,
        rate(&game, reclaimer)
    );
    game.state.tick(&[PlayerCommand {
        player: game.presentation.human,
        command: Command::UpgradeBuilding {
            building: reclaimer,
        },
    }]);
    assert!(!game.state.building(reclaimer).unwrap().built());
    assert_eq!(rate(&game, reclaimer), 0);
    for _ in 0..600 {
        game.state.tick(&[]);
    }
    assert_eq!(game.state.building(reclaimer).unwrap().tier, 1);
    assert_eq!(rate(&game, reclaimer), 120);
    game.presentation.selection.set_buildings(vec![reclaimer]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    assert_eq!(panel.title, "Refinery");
    assert_eq!(
        panel.portrait,
        CardIcon::Building(BuildingKind::Reclaimer, 1)
    );
    while game.state.current_tick() < aftermine_sim::stats::FOUNDRY_DRIP_START_TICK - 1 {
        game.state.tick(&[]);
    }
    assert_eq!(rate(&game, foundry), 0);
    game.state.tick(&[]);
    assert_eq!(rate(&game, foundry), 20);
    let initial = game.state.player(game.presentation.human).scrap;
    for _ in 0..1_200 {
        game.state.tick(&[]);
    }
    assert_eq!(
        game.state.player(game.presentation.human).scrap - initial,
        rate(&game, foundry) + rate(&game, reclaimer)
    );
}

#[test]
fn extractor_build_copy_names_both_rates_and_the_foundry_rule() {
    use aftermine_sim::{ExtractorIncome, ExtractorSupport};
    let remote = ExtractorIncome::new(0, ExtractorSupport::Remote).scrap_per_minute();
    let supported = ExtractorIncome::new(0, ExtractorSupport::Supported).scrap_per_minute();
    let radius = aftermine_sim::stats::EXTRACTOR_SUPPORT_RADIUS;
    let extractor = building_economy_lines(BuildingKind::Extractor);
    assert_eq!(
        extractor,
        [format!(
            "Makes {remote} scrap/min, or {supported} scrap/min with your Foundry within {radius} tiles."
        )]
    );

    let foundry = building_economy_lines(BuildingKind::Foundry);
    assert_eq!(
        foundry,
        [
            format!(
                "Raises the output of your Extractors within {radius} tiles from {remote} to {supported} scrap/min."
            ),
            "After the first 2 minutes, each Foundry makes 20 scrap/min.".to_string(),
        ]
    );

    let mut game = game();
    let harvester = own_unit(&game, UnitKind::Harvester);
    game.presentation.selection.set_units(vec![harvester]);
    let panel =
        build_for_palette(&game.view(), &BindingMap::classic(), true).expect("advanced builds");
    let extractor_card = panel
        .cards
        .iter()
        .find(|card| card.title == "Extractor")
        .expect("advanced palette contains Extractor");
    for rate in [remote, supported] {
        assert!(
            extractor_card
                .desc
                .iter()
                .any(|line| line.contains(&format!("{rate} scrap/min"))),
            "{:?}",
            extractor_card.desc
        );
    }
}

#[test]
fn scripted_opponents_show_their_difficulty_and_stance() {
    let mut game = game();
    game.scenario.players[1].bot_config = Some(aftermine_sim::scenario::BotConfig::new(
        aftermine_sim::scenario::BotDifficulty::Prime,
        aftermine_sim::scenario::BotStance::Aggressive,
        91,
    ));
    assert_eq!(
        bot_controller_label(&game.view(), aftermine_sim::PlayerId(1)),
        Some("Prime / Aggressive bot".to_string())
    );
}

#[test]
fn verb_cards_wear_their_atlas_icons() {
    use aftermine_sim::UnitKind;
    use chassis::grid::TilePos;
    let mut game = game();
    let sentinel = own_unit(&game, UnitKind::Sentinel);
    game.state.tick(&[PlayerCommand {
        player: game.presentation.human,
        command: Command::Hunt {
            units: vec![sentinel],
            goal: TilePos::new(20, 12),
            queue: false,
        },
    }]);
    game.presentation.selection.set_units(vec![sentinel]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    let patrol = panel
        .cards
        .iter()
        .find(|c| c.title == "Patrol")
        .expect("patrol card");
    assert_eq!(patrol.icon, CardIcon::Verb(VerbIcon::Patrol));
    assert_eq!(patrol.hotkey, "R", "the tooltip chord stays live");
    let hunt = panel
        .cards
        .iter()
        .find(|c| c.title == "Hunt")
        .expect("hunt card");
    assert_eq!(hunt.action, CardAction::Dispatch(Action::Hunt));
    assert_eq!(hunt.hotkey, "F");
    let chip = &panel.queue[0];
    assert!(chip.title.starts_with("Hunt"), "{}", chip.title);
    assert_eq!(
        chip.icon,
        CardIcon::Verb(VerbIcon::Hunt),
        "chips wear pictograms, not letters that shadow chords"
    );
}

#[test]
fn the_foundry_panel_speaks_its_roster() {
    let mut game = game();
    let foundry = human_foundry(&game);
    game.presentation.selection.set_buildings(vec![foundry]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    assert_eq!(panel.title, "Foundry");
    assert_eq!(panel.cards.len(), 7, "five units plus two rally controls");
    assert_eq!(panel.cards[0].title, "Set rally");
    assert_eq!(panel.cards[0].action, CardAction::ArmRally);
    assert_eq!(panel.cards[2].hotkey, "Q");
    assert_eq!(panel.cards[2].cost, Some(50));
    assert_eq!(unit_train_time_label(UnitKind::Harvester), "5s");
    assert_eq!(unit_train_time_label(UnitKind::Sentinel), "7.5s");
    assert!(panel.cards[2].enabled, "150 scrap affords a harvester");
    assert_eq!(
        panel.cards[2].action,
        CardAction::Dispatch(Action::TrainSlot(0)),
        "the card IS its hotkey"
    );
    assert!(panel.queue.is_empty(), "nothing queued yet");
    // The harvester's card carries no weapon line; the sentinel's
    // carries both of its guns.
    assert!(!panel.cards[2].desc.iter().any(|l| l.contains("dmg")));
    assert!(panel.cards[3].desc.iter().any(|l| l.contains("dmg")));

    game.state.tick(&[PlayerCommand {
        player: game.presentation.human,
        command: Command::Train {
            building: foundry,
            kind: UnitKind::Harvester,
        },
    }]);
    let panel =
        build_for_palette(&game.view(), &BindingMap::classic(), false).expect("queued panel");
    assert_eq!(panel.queue_label, "5s");
    game.state.tick(&[]);
    let panel =
        build_for_palette(&game.view(), &BindingMap::classic(), false).expect("progressing panel");
    assert_eq!(panel.queue_label, "4.9s");
}

#[test]
fn a_producer_always_exposes_set_reset_and_clear_rally_actions() {
    let mut game = game();
    let foundry = human_foundry(&game);
    game.presentation.selection.set_buildings(vec![foundry]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    assert!(
        panel
            .cards
            .iter()
            .any(|card| { card.title == "Set rally" && card.action == CardAction::ArmRally })
    );
    assert!(
        panel
            .cards
            .iter()
            .any(|card| card.title == "Clear rally" && !card.enabled)
    );

    game.state.tick(&[PlayerCommand {
        player: game.presentation.human,
        command: Command::SetRally {
            building: foundry,
            rally: Some(aftermine_sim::Rally::Tile(chassis::grid::TilePos::new(
                12, 8,
            ))),
        },
    }]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    assert!(
        panel
            .cards
            .iter()
            .any(|card| { card.title == "Reset rally" && card.action == CardAction::ArmRally })
    );
    assert!(panel.cards.iter().any(|card| {
        card.title == "Clear rally" && card.action == CardAction::ClearRally && card.enabled
    }));
}

#[test]
fn a_multi_producer_panel_puts_shared_rally_before_production() {
    let mut scenario = Scenario::skirmish();
    scenario
        .buildings
        .push(aftermine_sim::scenario::BuildingSpec {
            player: 0,
            kind: BuildingKind::Fabricator,
            x: 9,
            y: 3,
        });
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("fixture builds");
    game.presentation.selection.set_buildings(
        game.state
            .buildings()
            .iter()
            .filter(|building| building.player == game.presentation.human)
            .map(|building| building.id)
            .collect(),
    );

    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false)
        .expect("multi-building panel");
    assert_eq!(panel.title, "2 STRUCTURES");
    assert_eq!(panel.cards.len(), 2);
    assert_eq!(panel.cards[0].title, "Set rallies");
    assert_eq!(panel.cards[0].action, CardAction::ArmRally);
}

#[test]
fn a_non_producer_never_offers_a_rally_action() {
    let mut scenario = Scenario::skirmish();
    scenario
        .buildings
        .push(aftermine_sim::scenario::BuildingSpec {
            player: 0,
            kind: BuildingKind::Turret,
            x: 9,
            y: 3,
        });
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("fixture builds");
    let turret = own_building(&game, BuildingKind::Turret);
    game.presentation.selection.set_buildings(vec![turret]);

    let panel =
        build_for_palette(&game.view(), &BindingMap::classic(), false).expect("Turret panel");
    assert_eq!(panel.cards.len(), 1, "the turret offers its tier upgrade");
    assert!(
        panel.stop.is_none(),
        "a turret without a target has nothing to stop"
    );
    assert!(
        matches!(panel.cards[0].action, CardAction::Upgrade),
        "the turret's card lifts its tier"
    );
    assert!(
        panel
            .cards
            .iter()
            .all(|card| !matches!(card.action, CardAction::ArmRally | CardAction::ClearRally)),
        "a defense cannot rally units it never produces"
    );
}

#[test]
fn an_upgrade_needs_no_harvester_and_explains_its_downtime() {
    let mut scenario = Scenario::skirmish();
    scenario.players[0].scrap = 500;
    scenario
        .units
        .retain(|unit| unit.player != 0 || unit.kind != aftermine_sim::UnitKind::Harvester);
    scenario.buildings.extend([
        aftermine_sim::scenario::BuildingSpec {
            player: 0,
            kind: BuildingKind::Fabricator,
            x: 9,
            y: 3,
        },
        aftermine_sim::scenario::BuildingSpec {
            player: 0,
            kind: BuildingKind::Turret,
            x: 12,
            y: 3,
        },
    ]);
    let mut game =
        Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("upgrade fixture builds");
    let turret = own_building(&game, BuildingKind::Turret);
    game.presentation.selection.set_buildings(vec![turret]);

    let panel =
        build_for_palette(&game.view(), &BindingMap::classic(), false).expect("turret panel");
    let upgrade = panel
        .cards
        .iter()
        .find(|card| matches!(card.action, CardAction::Upgrade))
        .expect("turret offers its upgrade");
    assert!(upgrade.enabled, "automatic upgrades need no crew");
    assert!(
        upgrade.desc.iter().any(|line| line.contains("Offline")),
        "the card explains the downtime: {:?}",
        upgrade.desc
    );

    game.state.tick(&[PlayerCommand {
        player: game.presentation.human,
        command: Command::UpgradeBuilding { building: turret },
    }]);
    let panel =
        build_for_palette(&game.view(), &BindingMap::classic(), false).expect("upgrade panel");
    assert_eq!(panel.cards[0].title, "Upgrading");
    assert!(panel.cards[0].progress.is_some());
    assert!(
        panel.cards[0]
            .desc
            .iter()
            .all(|line| !line.contains("crew"))
    );
}

#[test]
fn a_locked_card_names_an_upgrade_by_the_structure_it_upgrades() {
    let mut game = game();
    let harvester = own_unit(&game, UnitKind::Harvester);
    game.presentation.selection.set_units(vec![harvester]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), true).expect("panel");
    let why = |title: &str| {
        panel
            .cards
            .iter()
            .find(|card| card.title == title)
            .and_then(|card| card.why.clone())
    };
    assert_eq!(
        why("Spire").as_deref(),
        Some("needs a standing Crucible and Deep Bore (Extractor upgrade)")
    );
    assert_eq!(
        why("Roc Cradle").as_deref(),
        Some("needs a standing Crucible, Deep Bore (Extractor upgrade), and Airworks")
    );
}

#[test]
fn poverty_and_capacity_disable_cards_with_reasons() {
    let mut scenario = Scenario::skirmish();
    // The bank must outlast the queue cap or poverty masks it.
    scenario.players[0].scrap = 500;
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("skirmish builds");
    let foundry = human_foundry(&game);
    game.presentation.selection.set_buildings(vec![foundry]);
    // Queue harvesters until 50 scrap remains: the sentinel card
    // (75) must dim with the price named.
    for _ in 0..3 {
        game.state.tick(&[PlayerCommand {
            player: game.presentation.human,
            command: Command::Train {
                building: foundry,
                kind: aftermine_sim::UnitKind::Harvester,
            },
        }]);
    }
    // 500 - 3x50 = 350: still rich, cards enabled, ghosts armed.
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    assert!(panel.cards[2].enabled);
    assert_eq!(panel.queue.len(), 3);
    assert_eq!(panel.queue[1].action, CardAction::CancelQueue(foundry, 1));
    // Fill to the sim's cap: every production card refuses.
    for _ in 0..aftermine_sim::stats::QUEUE_CAP {
        game.state.tick(&[PlayerCommand {
            player: game.presentation.human,
            command: Command::Train {
                building: foundry,
                kind: aftermine_sim::UnitKind::Harvester,
            },
        }]);
    }
    let queued = game.state.building(foundry).unwrap().queue.len();
    assert_eq!(queued, aftermine_sim::stats::QUEUE_CAP, "the sim capped it");
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    assert_eq!(
        panel.queue.len(),
        aftermine_sim::stats::QUEUE_CAP,
        "every paid queue slot remains inspectable and cancelable"
    );
    assert!(
        panel
            .cards
            .iter()
            .filter(|card| card.cost.is_some())
            .all(|card| !card.enabled)
    );
    assert!(
        panel
            .cards
            .iter()
            .filter(|card| card.cost.is_some())
            .all(|card| card.why.as_deref() == Some("queue is full")),
        "the reason names the cap, not the bank"
    );
}

#[test]
fn the_patrol_card_says_what_each_kind_of_unit_does_on_the_way() {
    use aftermine_sim::UnitKind;
    let mut scenario = Scenario::skirmish();
    for kind in [UnitKind::Sentinel, UnitKind::Skyhook] {
        scenario.units.push(aftermine_sim::scenario::UnitSpec {
            player: 0,
            kind,
            x: 10,
            y: 7,
        });
    }
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("skirmish builds");
    let own = |kind| own_unit(&game, kind);
    let (sentinel, harvester, skyhook) = (
        own(UnitKind::Sentinel),
        own(UnitKind::Harvester),
        own(UnitKind::Skyhook),
    );
    for (selection, promise) in [
        (
            vec![sentinel],
            "Units engage whatever they meet along the way.",
        ),
        (
            vec![harvester],
            "Workers repair damaged units and structures they pass.",
        ),
        (
            vec![skyhook],
            "Unarmed units walk the route without engaging.",
        ),
        (
            vec![harvester, skyhook],
            "Workers repair damaged units and structures they pass; others walk on.",
        ),
        (
            vec![sentinel, skyhook],
            "Armed units engage what they meet; the rest walk on.",
        ),
        (
            vec![sentinel, harvester],
            "Armed units engage what they meet; workers repair what is damaged.",
        ),
    ] {
        game.presentation.selection.set_units(selection);
        let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
        let patrol = panel
            .cards
            .iter()
            .find(|c| c.title == "Patrol")
            .expect("patrol card");
        assert_eq!(patrol.desc.last().map(String::as_str), Some(promise));
    }
}

#[test]
fn a_patrolling_selection_lights_patrol_and_its_dock_says_patrol() {
    let mut game = game();
    let sentinel = own_unit(&game, UnitKind::Sentinel);
    game.presentation.selection.set_units(vec![sentinel]);
    let patrol_card = |panel: &Panel| {
        panel
            .cards
            .iter()
            .find(|c| c.title == "Patrol")
            .expect("patrol card")
            .desc[0]
            .clone()
    };
    let idle = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    assert!(!idle.patrolling);
    assert_eq!(patrol_card(&idle), patrol_desc(crate::platform::hands()));

    let tile = chassis::grid::TilePos::new;
    game.issue(aftermine_sim::Command::Patrol {
        units: vec![sentinel],
        waypoints: vec![tile(14, 7), tile(14, 11)],
    });
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    assert!(panel.patrolling, "a staged patrol lights the card");
    assert_eq!(
        patrol_card(&panel),
        patrolling_desc(crate::platform::hands())
    );
    let chips: Vec<(&str, &CardIcon)> = panel
        .queue
        .iter()
        .map(|chip| (chip.title.as_str(), &chip.icon))
        .collect();
    let looped = CardIcon::Verb(VerbIcon::Patrol);
    assert_eq!(chips, [("Patrol (now)", &looped), ("Patrol", &looped)]);
    assert_eq!(
        panel.queue[1].desc,
        [
            "Fights what it meets on the way.",
            "Loops back to the first point after the last."
        ]
    );
}

#[test]
fn the_harvester_panel_is_the_same_grammar() {
    let mut game = game();
    let harvester = own_unit(&game, UnitKind::Harvester);
    game.presentation.selection.set_units(vec![harvester]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    assert_eq!(panel.title, "Harvester");
    // Unarmed, it has no Run or Hunt, and an empty hopper has
    // no cargo to return.
    let titles: Vec<&str> = panel.cards.iter().map(|card| card.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Patrol",
            "Accompany",
            "Salvage",
            "Repair",
            "Retire",
            "Build"
        ]
    );
    // Auto-repair rides on the Repair card as its autocast.
    let repair = panel
        .cards
        .iter()
        .find(|card| card.title == "Repair")
        .unwrap();
    assert_eq!(
        repair.autocast,
        Some(Autocast {
            state: AutocastState::On,
            toggle: Action::ToggleIdleWeld,
        })
    );
    assert!(
        repair
            .desc
            .iter()
            .any(|line| line.starts_with("Auto-repair on")),
        "{:?}",
        repair.desc
    );
    assert!(
        panel
            .cards
            .iter()
            .filter(|card| card.title != "Repair")
            .all(|card| card.autocast.is_none())
    );
    assert!(
        !panel
            .info
            .rows
            .iter()
            .any(|row| matches!(row.label.as_str(), "Ground" | "Air"))
    );
    assert_eq!(
        panel.info.health.map(|health| (health.hp, health.max_hp)),
        Some((60, 60))
    );
    assert_eq!(stat(&panel, "Speed").value, "2.5 tiles/s");
    assert!(panel.cards.iter().any(|card| card.title == "Build"));
    assert!(
        !panel
            .cards
            .iter()
            .any(|card| matches!(card.action, CardAction::ArmBuild(_)))
    );
    let construction = build_for_palette(&game.view(), &BindingMap::classic(), true).unwrap();
    assert_eq!(construction.cards.len(), MAX_CARDS);
    let (back, buildings) = construction.cards.split_last().expect("cards");
    assert!(
        buildings
            .iter()
            .all(|card| matches!(card.action, CardAction::ArmBuild(_)))
    );
    assert_eq!(back.action, CardAction::ClosePalette, "Back comes last");
    // An idle unit with nothing queued shows no order chips at all —
    // the dock only exists when there is a program to show.
    assert!(panel.queue.is_empty(), "idle shows no dock");
    assert!(panel.stop.is_none(), "an idle unit has nothing to stop");
    // Give it a program: the strip appears, and its chip removes the
    // order it shows.
    let goal = chassis::grid::TilePos::new(8, 8);
    game.state.tick(&[aftermine_sim::PlayerCommand {
        player: game.presentation.human,
        command: aftermine_sim::Command::Hunt {
            units: vec![harvester],
            goal,
            queue: false,
        },
    }]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    assert_eq!(panel.queue.len(), 1);
    assert_eq!(
        panel.queue[0].action,
        CardAction::CancelOrder {
            unit: harvester,
            key: aftermine_sim::OrderKey::Walk { tile: goal },
            from_end: 0,
        }
    );
    let stop = panel.stop.as_ref().expect("a busy unit can stop");
    assert_eq!(stop.action, CardAction::Dispatch(Action::StopOrScrap));
    assert_eq!(stop.hotkey, "X");
}

/// Places `kind` on the first tile the sim accepts near the
/// harvester, returns the site.
fn place(
    game: &mut Game,
    builder: aftermine_sim::UnitId,
    kind: BuildingKind,
    queue: bool,
) -> BuildingId {
    use chassis::grid::TilePos;
    let here = game.state.unit(builder).expect("builder").tile();
    let before: Vec<BuildingId> = game.state.buildings().iter().map(|b| b.id).collect();
    for dy in -6i32..=6 {
        for dx in -6i32..=6 {
            let (x, y) = (here.x + dx, here.y + dy);
            if x < 0 || y < 0 {
                continue;
            }
            let anchor = TilePos::new(x, y);
            if !game.state.can_place(game.presentation.human, kind, anchor) {
                continue;
            }
            game.state.tick(&[PlayerCommand {
                player: game.presentation.human,
                command: Command::Build {
                    units: vec![builder],
                    kind,
                    anchor,
                    queue,
                    defer: false,
                },
            }]);
            if let Some(b) = game
                .state
                .buildings()
                .iter()
                .find(|b| !before.contains(&b.id) && b.kind == kind)
            {
                return b.id;
            }
        }
    }
    panic!("no ground accepted a {}", kind.name());
}

fn builder_game() -> (Game, aftermine_sim::UnitId) {
    builder_game_with(&[])
}

/// A builder's skirmish with `buildings` already standing.
fn builder_game_with(
    buildings: &[aftermine_sim::scenario::BuildingSpec],
) -> (Game, aftermine_sim::UnitId) {
    let mut scenario = Scenario::skirmish();
    scenario.players[0].scrap = 5000;
    scenario.buildings.extend_from_slice(buildings);
    let game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("skirmish builds");
    let harvester = own_unit(&game, UnitKind::Harvester);
    (game, harvester)
}

#[test]
fn build_chips_wear_the_works_they_are_raising() {
    let (mut game, harvester) = builder_game();
    let turret = place(&mut game, harvester, BuildingKind::Turret, false);
    let array = place(&mut game, harvester, BuildingKind::Array, true);
    game.presentation.selection.set_units(vec![harvester]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    assert_eq!(panel.queue.len(), 2, "two legs of one program");
    // Two Build chips look different: each carries its own works,
    // ghosted while the site is still rising.
    assert!(matches!(
        panel.queue[0].icon,
        CardIcon::Order {
            subject: OrderSubject::Building(BuildingKind::Turret, _, _),
            verb: VerbIcon::Build,
            ghost: true,
        }
    ));
    assert!(matches!(
        panel.queue[1].icon,
        CardIcon::Order {
            subject: OrderSubject::Building(BuildingKind::Array, _, _),
            verb: VerbIcon::Build,
            ghost: true,
        }
    ));
    assert!(
        panel.queue[0].title.starts_with("Build - Turret"),
        "{}",
        panel.queue[0].title
    );
    assert!(
        panel.queue[1].title.starts_with("Build - Array"),
        "{}",
        panel.queue[1].title
    );
    assert!(panel.queue[0].desc.iter().any(|l| l.contains("% raised")));
    assert!(
        panel.queue[0].progress.is_some(),
        "a site chip meters its rise"
    );
    assert_eq!(
        panel.queue[0].action,
        CardAction::CancelSite(turret),
        "the active site can be abandoned from its order chip"
    );
    assert_eq!(
        panel.queue[1].action,
        CardAction::CancelSite(array),
        "a queued paid site targets its own works"
    );
}

#[test]
fn the_orders_dock_offers_every_order_past_its_visible_slots() {
    let (mut game, harvester) = builder_game();
    game.presentation.selection.set_units(vec![harvester]);
    let here = game.state.unit(harvester).expect("harvester").tile();
    let goals: Vec<_> = (0..12)
        .map(|step| chassis::grid::TilePos::new(here.x + 2 + step % 2, here.y - 3 + step / 2))
        .collect();
    game.state.tick(
        &goals
            .iter()
            .map(|&goal| PlayerCommand {
                player: game.presentation.human,
                command: Command::Hunt {
                    units: vec![harvester],
                    goal,
                    queue: true,
                },
            })
            .collect::<Vec<_>>(),
    );
    let orders = game
        .view()
        .projection()
        .program(harvester)
        .expect("a program")
        .orders
        .len();
    assert_eq!(orders, 12, "every queued walk stands");
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    assert_eq!(
        panel.queue.len(),
        orders,
        "the dock shows what fits and counts the rest"
    );
    assert!(matches!(
        panel.queue.last().map(|card| card.action),
        Some(CardAction::CancelOrder { unit, from_end: 0, .. }) if unit == harvester
    ));
}

#[test]
fn deferred_build_chips_cancel_their_logical_sites() {
    use chassis::grid::TilePos;

    let (game, harvester) = builder_game();
    let program = Program {
        orders: vec![Order::Found {
            kind: BuildingKind::Bastion,
            anchor: TilePos::new(11, 7),
        }],
        looping: false,
    };
    let card = own_order_card(&game.view(), harvester, &program, 0, &Projection::default());
    assert_eq!(
        card.action,
        CardAction::CancelFound(BuildingKind::Bastion, TilePos::new(11, 7))
    );
    assert!(card.desc.iter().any(|line| line.contains("planned site")));
}

#[test]
fn a_site_chip_takes_the_selection_off_a_site_other_workers_hold() {
    use aftermine_sim::{OrderKey, UnitId};
    use chassis::grid::TilePos;

    let mut scenario = Scenario::skirmish();
    scenario.players[0].scrap = 5000;
    scenario.units.push(aftermine_sim::scenario::UnitSpec {
        player: 0,
        kind: UnitKind::Harvester,
        x: 8,
        y: 6,
    });
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("skirmish builds");
    let human = game.presentation.human;
    let crew: Vec<UnitId> = game
        .state
        .units()
        .iter()
        .filter(|u| u.player == human && u.kind == UnitKind::Harvester)
        .map(|u| u.id)
        .collect();
    assert_eq!(crew.len(), 4, "premise: four workers");
    let (first, last) = crew.split_at(2);
    let mut anchors: Vec<TilePos> = Vec::new();
    for y in 1..12 {
        for x in 1..24 {
            let anchor = TilePos::new(x, y);
            if anchors.iter().all(|other| other.chebyshev(anchor) >= 3)
                && game.state.can_place(human, BuildingKind::Turret, anchor)
            {
                anchors.push(anchor);
            }
        }
    }
    let &[built, planned, staged, ..] = anchors.as_slice() else {
        panic!("open ground for three Turrets: {anchors:?}");
    };
    let build = |anchor, defer| Command::Build {
        units: crew.clone(),
        kind: BuildingKind::Turret,
        anchor,
        queue: false,
        defer,
    };
    let chip = |game: &mut Game, selected: &[UnitId]| {
        game.presentation.selection.set_units(selected.to_vec());
        build_for_palette(&game.view(), &BindingMap::classic(), false)
            .expect("panel")
            .queue
            .swap_remove(0)
    };
    let takes_off = |card: &Card| {
        card.desc
            .iter()
            .any(|line| line.contains("take the selection off this site"))
    };
    // In play only a tick drains the staged batch, and the cached projection
    // and staged command phase key on its length, so dropping the batch here
    // drops both caches too.
    let unstage = |game: &mut Game| {
        game.pending.clear();
        game.presentation.projection.take();
        game.presentation.staged.get_mut().take();
    };

    game.state.tick(&[PlayerCommand {
        player: human,
        command: build(built, false),
    }]);
    let site = game
        .state
        .buildings()
        .iter()
        .find(|b| b.anchor == built)
        .expect("the site stands")
        .id;
    for part in [first, last] {
        let card = chip(&mut game, part);
        assert_eq!(
            card.action,
            CardAction::CancelOrder {
                unit: part[0],
                key: OrderKey::Build { site },
                from_end: 0,
            },
            "the rest of the crew keeps the site"
        );
        assert!(takes_off(&card), "{:?}", card.desc);
    }
    let card = chip(&mut game, &crew);
    assert_eq!(card.action, CardAction::CancelSite(site));
    assert!(!takes_off(&card), "{:?}", card.desc);
    game.issue(Command::Stop {
        units: last.to_vec(),
    });
    assert_eq!(
        chip(&mut game, first).action,
        CardAction::CancelSite(site),
        "a staged Stop leaves the selection the whole crew"
    );
    unstage(&mut game);

    // Ground in sight is verified within the tick, so the plan is staged.
    game.issue(build(planned, true));
    let card = chip(&mut game, first);
    assert_eq!(
        game.view()
            .projection()
            .program(first[0])
            .map(|p| p.orders[0]),
        Some(Order::Found {
            kind: BuildingKind::Turret,
            anchor: planned,
        }),
        "premise: the planned site awaits its ground"
    );
    assert_eq!(
        card.action,
        CardAction::CancelOrder {
            unit: first[0],
            key: OrderKey::Found {
                kind: BuildingKind::Turret,
                anchor: planned,
            },
            from_end: 0,
        }
    );
    assert!(takes_off(&card), "{:?}", card.desc);
    assert_eq!(
        chip(&mut game, &crew).action,
        CardAction::CancelFound(BuildingKind::Turret, planned)
    );

    unstage(&mut game);
    game.issue(build(staged, false));
    assert_eq!(
        chip(&mut game, first).action,
        CardAction::CancelFound(BuildingKind::Turret, staged),
        "a site only the staged commands place has no id a removal could name"
    );
}

/// Contiguous index per [`Order`] variant: a new order stops this
/// compiling until its chip is covered below.
fn order_kind(order: &Order) -> usize {
    match order {
        Order::Idle => 0,
        Order::Run { .. } => 1,
        Order::Harvest { .. } => 2,
        Order::Attack { .. } => 3,
        Order::Build { .. } => 4,
        Order::Repair { .. } => 5,
        Order::Hunt { .. } => 6,
        Order::Salvage { .. } => 7,
        Order::Found { .. } => 8,
        Order::RepairUnit { .. } => 9,
        Order::Advance { .. } => 10,
        Order::Board { .. } => 11,
        Order::Unload { .. } => 12,
        Order::Land { .. } => 13,
        Order::ReturnCargo { .. } => 14,
        Order::Accompany { .. } => 15,
        Order::Retire { .. } => 16,
        Order::Scout { .. } => 17,
        Order::Tend { .. } => 18,
    }
}

#[test]
fn every_own_chip_removes_its_order_but_a_site_held_by_the_selection_alone_cancels() {
    use aftermine_sim::{AttackTarget, ContactId, Goal, OrderKey};
    use chassis::grid::TilePos;

    let (mut game, harvester) = builder_game();
    let site = place(&mut game, harvester, BuildingKind::Turret, false);
    let foundry = human_foundry(&game);
    let tile = TilePos::new(9, 9);
    let goal = Goal::at(tile);
    let contact = AttackTarget::Contact(ContactId(7));
    let planned = TilePos::new(11, 7);
    let program = Program {
        orders: vec![
            Order::Idle,
            Order::Run { goal },
            Order::Hunt { goal },
            Order::Advance { goal },
            Order::Attack {
                target: contact,
                pursue: false,
                resume: Some(aftermine_sim::Resume::Hunt(goal)),
            },
            Order::Land {
                goal: TilePos::new(3, 3),
                from: Some(tile),
            },
            Order::Land {
                goal: tile,
                from: None,
            },
            Order::Attack {
                target: contact,
                pursue: true,
                resume: None,
            },
            Order::Unload {
                at: goal,
                reverse: false,
            },
            Order::Harvest {
                node: TilePos::new(4, 4),
                anchor: tile,
                withdrawing: false,
            },
            Order::ReturnCargo {
                foundry,
                repair: false,
            },
            Order::Build { site },
            Order::Build { site: foundry },
            Order::Found {
                kind: BuildingKind::Bastion,
                anchor: planned,
            },
            Order::Repair { building: foundry },
            Order::Salvage { building: foundry },
            Order::RepairUnit { unit: harvester },
            Order::Board {
                transport: harvester,
            },
            Order::Accompany {
                unit: harvester,
                weld: None,
            },
            Order::Retire {
                crucible: foundry,
                pad: None,
            },
            Order::Scout { tile },
            Order::Tend { goal, weld: None },
        ],
        looping: false,
    };
    let mut kinds: Vec<usize> = program.orders.iter().map(order_kind).collect();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(kinds, (0..19).collect::<Vec<_>>(), "every order has a chip");

    let view = game.view();
    let actions: Vec<CardAction> = (0..program.orders.len())
        .map(|index| {
            own_order_card(&view, harvester, &program, index, &Projection::default()).action
        })
        .collect();
    let remove = |key, from_end| CardAction::CancelOrder {
        unit: harvester,
        key,
        from_end,
    };
    let walk = OrderKey::Walk { tile };
    assert_eq!(
        actions,
        [
            CardAction::None,
            remove(walk, 6),
            remove(walk, 5),
            remove(walk, 4),
            remove(walk, 3),
            remove(walk, 2),
            remove(OrderKey::Land { pad: tile }, 0),
            remove(OrderKey::Attack { objective: contact }, 0),
            remove(OrderKey::Unload { tile }, 0),
            remove(OrderKey::Harvest { anchor: tile }, 0),
            remove(OrderKey::ReturnCargo, 0),
            CardAction::CancelSite(site),
            remove(OrderKey::Build { site: foundry }, 0),
            CardAction::CancelFound(BuildingKind::Bastion, planned),
            remove(OrderKey::Repair { building: foundry }, 0),
            remove(OrderKey::Salvage { building: foundry }, 0),
            remove(OrderKey::RepairUnit { unit: harvester }, 0),
            remove(
                OrderKey::Board {
                    transport: harvester
                },
                0
            ),
            remove(OrderKey::Accompany { unit: harvester }, 0),
            remove(OrderKey::Retire { crucible: foundry }, 0),
            remove(walk, 1),
            remove(walk, 0),
        ]
    );
}

#[test]
fn a_tending_chip_names_the_building_it_repairs() {
    let (game, _) = builder_game();
    let foundry = human_foundry(&game);
    let order = Order::Tend {
        goal: aftermine_sim::Goal::at(chassis::grid::TilePos::new(9, 9)),
        weld: Some(aftermine_sim::Target::Building(foundry)),
    };
    let card = order_card(&game.view(), &order, true, true, None);
    assert!(
        matches!(
            card.icon,
            CardIcon::Order {
                subject: OrderSubject::Building(BuildingKind::Foundry, _, _),
                verb: VerbIcon::Repair,
                ghost: false,
            }
        ),
        "{:?}",
        card.icon
    );
    assert!(card.title.starts_with("Repair - Foundry"), "{}", card.title);
    assert!(
        card.desc.iter().any(|line| line.ends_with(" hp")),
        "{:?}",
        card.desc
    );
    assert!(
        card.progress.is_some(),
        "the chip meters the building's hull"
    );
}

#[test]
fn an_attack_chip_on_an_unseen_deep_bore_wears_its_public_tier() {
    let mut scenario = Scenario::skirmish();
    let mut rows = vec![vec!['.'; 40]; 30];
    rows[1][1] = '1';
    rows[27][37] = '2';
    rows[24][30] = 'E';
    scenario.map = rows.into_iter().map(|r| r.into_iter().collect()).collect();
    scenario.players[1].scrap = 1_000;
    // The Crucible unlocks the Deep Bore.
    let spec = |kind, x, y| aftermine_sim::scenario::BuildingSpec {
        player: 1,
        kind,
        x,
        y,
    };
    scenario.buildings = vec![
        spec(BuildingKind::Extractor, 30, 24),
        spec(BuildingKind::Crucible, 34, 22),
    ];
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("fixture builds");
    let bore = game
        .state
        .buildings()
        .iter()
        .find(|building| building.kind == BuildingKind::Extractor)
        .expect("fixture has an Extractor")
        .id;
    game.state.tick(&[PlayerCommand {
        player: aftermine_sim::PlayerId(1),
        command: Command::UpgradeBuilding { building: bore },
    }]);
    assert_eq!(game.state.building(bore).unwrap().tier, 1, "premise");
    assert!(game.my_vision().ghosts().is_empty(), "premise: never seen");
    let target =
        aftermine_sim::AttackTarget::RememberedBuilding(aftermine_sim::RememberedBuilding {
            owner: aftermine_sim::PlayerId(1),
            building_kind: BuildingKind::Extractor,
            anchor: chassis::grid::TilePos::new(30, 24),
        });
    let order = Order::Attack {
        target,
        pursue: false,
        resume: None,
    };
    let card = order_card(&game.view(), &order, true, true, None);
    assert!(
        matches!(
            card.icon,
            CardIcon::Order {
                subject: OrderSubject::Building(BuildingKind::Extractor, 1, _),
                ..
            }
        ),
        "{:?}",
        card.icon
    );
    assert!(card.title.contains("Deep Bore"), "{}", card.title);
}

#[test]
fn a_repair_chip_wears_the_patients_tier() {
    let spec = |kind, x| aftermine_sim::scenario::BuildingSpec {
        player: 0,
        kind,
        x,
        y: 3,
    };
    let (mut game, _) = builder_game_with(&[
        spec(BuildingKind::Fabricator, 9),
        spec(BuildingKind::Turret, 12),
    ]);
    let turret = own_building(&game, BuildingKind::Turret);
    game.state.tick(&[PlayerCommand {
        player: game.presentation.human,
        command: Command::UpgradeBuilding { building: turret },
    }]);
    let card = order_card(
        &game.view(),
        &Order::Repair { building: turret },
        true,
        true,
        None,
    );
    assert!(
        matches!(
            card.icon,
            CardIcon::Order {
                subject: OrderSubject::Building(BuildingKind::Turret, 1, _),
                verb: VerbIcon::Repair,
                ..
            }
        ),
        "{:?}",
        card.icon
    );
    assert!(
        card.title.starts_with("Repair - Heavy Turret"),
        "{}",
        card.title
    );
}

#[test]
fn a_chip_whose_subject_is_gone_falls_back_to_the_bare_verb() {
    // Orders outlive their subjects by a tick — the panel names
    // what it can find and never invents a silhouette.
    let (game, _) = builder_game();
    let dangling = Order::Repair {
        building: BuildingId(9999),
    };
    let card = order_card(&game.view(), &dangling, true, true, None);
    assert_eq!(card.icon, CardIcon::Verb(VerbIcon::Repair));
    assert_eq!(card.title, "Repair (now)");
    assert!(card.progress.is_none());
    assert_eq!(card.desc.len(), 1, "no detail line it cannot back up");
}

#[test]
fn a_foreign_program_is_never_enriched() {
    // A teammate's dock shows the verb and nothing about what it acts
    // on, rather than relying on what team sight shares.
    let (mut game, harvester) = builder_game();
    place(&mut game, harvester, BuildingKind::Turret, false);
    game.presentation.selection.set_units(vec![harvester]);
    let own = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    assert!(matches!(own.queue[0].icon, CardIcon::Order { .. }));
    let order = game.state.unit(harvester).expect("builder").order;
    let bare = order_card(&game.view(), &order, true, false, None);
    assert_eq!(bare.icon, CardIcon::Verb(VerbIcon::Build));
    assert_eq!(bare.title, "Build (now)");
    assert!(bare.progress.is_none());
}

#[test]
fn weapon_lines_read_from_the_stats_table() {
    let sentinel = weapon_lines(aftermine_sim::UnitKind::Sentinel);
    assert_eq!(sentinel.len(), 2, "main gun and the anti-air poke");
    assert!(sentinel[0].contains("dmg"));
    assert!(sentinel[0].contains("tiles"));
    assert!(sentinel[0].contains("ground"));
    assert!(sentinel[1].contains("air"));
    let bombard = weapon_lines(aftermine_sim::UnitKind::Bombard);
    assert!(bombard[0].contains("| shell"));
    assert!(bombard[0].contains("splash"));
    let roc = weapon_lines(aftermine_sim::UnitKind::Roc);
    assert_eq!(roc.len(), 1, "identical guns share a line: {roc:?}");
    assert!(roc[0].starts_with("2 guns | "));
}

#[test]
fn panel_copy_uses_only_supported_font_glyphs() {
    let units = [
        UnitKind::Harvester,
        UnitKind::Sentinel,
        UnitKind::Scuttler,
        UnitKind::Lancer,
        UnitKind::Bombard,
        UnitKind::Flakhound,
        UnitKind::Buzzard,
        UnitKind::Talon,
    ];
    let buildings = [
        BuildingKind::Foundry,
        BuildingKind::Fabricator,
        BuildingKind::Turret,
        BuildingKind::FlakTurret,
        BuildingKind::Bastion,
        BuildingKind::Array,
        BuildingKind::Reclaimer,
        BuildingKind::RepairBay,
        BuildingKind::Extractor,
    ];
    // ChakraPetch also carries the en dash an idle Extractor's rows read.
    let supported = |text: &str| text.chars().all(|c| c.is_ascii() || c == '\u{2013}');
    let assert_card = |card: &Card| {
        assert!(supported(&card.title), "card title: {}", card.title);
        assert!(supported(&card.hotkey), "card hotkey: {}", card.hotkey);
        if let Some(why) = &card.why {
            assert!(supported(why), "card refusal: {why}");
        }
        for line in &card.desc {
            assert!(supported(line), "card description: {line}");
        }
    };
    let assert_panel = |panel: &Panel| {
        assert!(supported(&panel.title), "panel title: {}", panel.title);
        assert!(
            supported(&panel.summary),
            "panel subtitle: {}",
            panel.summary
        );
        assert!(
            supported(&panel.queue_label),
            "panel queue label: {}",
            panel.queue_label
        );
        for row in &panel.info.rows {
            assert!(
                supported(&row.label) && supported(&row.value),
                "panel stat: {row:?}"
            );
        }
        for status in &panel.info.status {
            assert!(supported(status), "panel status: {status}");
        }
        if let Some(health) = panel.info.health {
            assert!(supported(health.label), "health label: {}", health.label);
        }
        if let Some(text) = panel.info.reserved.as_ref().and_then(|l| l.text.as_ref()) {
            assert!(supported(text), "panel status: {text}");
        }
        for card in panel
            .roster
            .iter()
            .map(|chip| &chip.card)
            .chain(&panel.cards)
            .chain(&panel.queue)
        {
            assert_card(card);
        }
    };
    for kind in units {
        assert!(
            supported(&crate::description::unit_description(kind)),
            "{} description",
            kind.name()
        );
        for line in weapon_lines(kind) {
            assert!(supported(&line), "{} weapon: {line}", kind.name());
        }
    }
    for kind in buildings {
        assert!(
            supported(&crate::description::building_description(kind)),
            "{} description",
            kind.name()
        );
    }

    let mut foundry_game = game();
    foundry_game
        .presentation
        .selection
        .set_buildings(vec![human_foundry(&foundry_game)]);
    assert_panel(
        &build_for_palette(&foundry_game.view(), &BindingMap::classic(), false)
            .expect("Foundry panel"),
    );

    let (mut builder_game, harvester) = builder_game();
    let site = place(&mut builder_game, harvester, BuildingKind::Array, false);
    builder_game
        .presentation
        .selection
        .set_units(vec![harvester]);
    assert_panel(
        &build_for_palette(&builder_game.view(), &BindingMap::classic(), false)
            .expect("Harvester panel"),
    );
    let extractor = place(&mut builder_game, harvester, BuildingKind::Extractor, true);
    builder_game.presentation.selection.clear();
    for site in [site, extractor] {
        builder_game
            .presentation
            .selection
            .set_buildings(vec![site]);
        assert_panel(
            &build_for_palette(&builder_game.view(), &BindingMap::classic(), false)
                .expect("site panel"),
        );
    }
}

#[test]
fn a_one_tile_sight_reads_in_the_singular() {
    let (mut game, harvester) = builder_game_with(&[aftermine_sim::scenario::BuildingSpec {
        player: 0,
        kind: BuildingKind::Fabricator,
        x: 12,
        y: 3,
    }]);
    let barricade = place(&mut game, harvester, BuildingKind::Barricade, false);
    game.presentation.selection.clear();
    game.presentation.selection.set_buildings(vec![barricade]);
    let panel =
        build_for_palette(&game.view(), &BindingMap::classic(), false).expect("Barricade panel");
    assert_eq!(stat(&panel, "Sight").value, "1 tile");
}

#[test]
fn a_selected_bastion_shows_its_minimum_and_maximum_range() {
    let (mut game, harvester) = builder_game_with(&[aftermine_sim::scenario::BuildingSpec {
        player: 0,
        kind: BuildingKind::Fabricator,
        x: 12,
        y: 3,
    }]);
    let bastion = place(&mut game, harvester, BuildingKind::Bastion, false);
    game.presentation.selection.clear();
    game.presentation.selection.set_buildings(vec![bastion]);

    let panel =
        build_for_palette(&game.view(), &BindingMap::classic(), false).expect("Bastion panel");
    assert_eq!(panel.title, "Bastion");
    assert_eq!(stat(&panel, "Range").value, "2.5-9.5 tiles");
    assert_eq!(stat(&panel, "Sight").value, "6 tiles");
    assert_eq!(
        stat(&panel, "Ground").icon,
        Some(info::StatIcon::Capability(CapabilityIcon::Weapon))
    );
}

#[test]
fn a_single_unit_exposes_static_combat_facts_but_a_group_does_not() {
    let mut game = game();
    let sentinel = own_unit(&game, UnitKind::Sentinel);
    game.presentation.selection.set_units(vec![sentinel]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    assert_eq!(stat(&panel, "Ground").value, "10 dmg/hit");
    assert_eq!(
        stat(&panel, "Ground").icon,
        Some(info::StatIcon::Capability(CapabilityIcon::Weapon))
    );
    assert_eq!(stat(&panel, "Air").value, "4 dmg/hit");
    assert_eq!(
        stat(&panel, "Air").icon,
        Some(info::StatIcon::Capability(CapabilityIcon::AirWeapon))
    );
    assert_eq!(stat(&panel, "Speed").value, "2.2 tiles/s");

    let harvester = own_unit(&game, UnitKind::Harvester);
    game.presentation
        .selection
        .set_units(vec![sentinel, harvester]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).expect("panel");
    assert!(
        panel.info.rows.is_empty(),
        "mixed selections keep combat detail out of the command band"
    );
    assert_eq!(
        panel.roster.len(),
        2,
        "each selected kind gets one roster chip"
    );
    assert!(
        panel.summary.is_empty(),
        "the counted roster tiles replace the redundant kind list"
    );
    assert!(
        panel
            .cards
            .iter()
            .all(|card| !matches!(card.action, CardAction::FilterKind(_))),
        "roster filters cannot consume command-card capacity"
    );
}

#[test]
fn a_walled_in_producer_says_so_on_its_queue() {
    for gap in [false, true] {
        let mut map = vec![".........."; 10];
        map[4..8].copy_from_slice(&[
            "....####..",
            "....#..#..",
            if gap { "....#....." } else { "....#..#.." },
            "....####..",
        ]);
        let scenario = serde_json::from_value(serde_json::json!({
            "name": "Walled in", "mode": "sandbox", "map": map,
            "players": [
                {"name": "You", "scrap": 1000, "bot": false},
                {"name": "Target", "scrap": 0, "bot": true}
            ],
            "units": [],
            "buildings": [{"player": 0, "kind": "foundry", "x": 5, "y": 5}]
        }))
        .unwrap();
        let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
        let foundry = human_foundry(&game);
        game.presentation.selection.set_buildings(vec![foundry]);
        let panel = |game: &Game| {
            build_for_palette(&game.view(), &BindingMap::classic(), false).expect("a panel")
        };
        let idle = panel(&game);
        assert_ne!(idle.queue_label, "Walled in", "nothing queued");
        game.state.tick(&[PlayerCommand {
            player: game.presentation.human,
            command: Command::Train {
                building: foundry,
                kind: UnitKind::Harvester,
            },
        }]);
        let queued = panel(&game);
        assert_eq!(
            queued.queue_label == "Walled in",
            !gap,
            "said before the job finishes: {}",
            queued.queue_label
        );
        assert_eq!(
            queued.queue[0]
                .desc
                .iter()
                .any(|line| line == WALLED_IN_DESC),
            !gap,
            "the held chip says why"
        );
        assert_eq!(
            queued.info.status, idle.info.status,
            "the info box keeps its lines"
        );
        for _ in 0..=UnitKind::Harvester.stats().train_ticks {
            game.state.tick(&[]);
        }
        let held = !game.state.building(foundry).unwrap().queue.is_empty();
        assert_eq!(held, !gap, "the simulation agrees");
        if held {
            assert_eq!(panel(&game).queue_label, "Walled in");
        }
    }
}

#[test]
fn production_queue_time_counts_partial_head_and_marks_a_blocked_spawn_ready() {
    let queue = std::collections::VecDeque::from([UnitKind::Harvester, UnitKind::Sentinel]);
    assert_eq!(
        production_queue_label(&queue, 25).as_deref(),
        Some("11.3s"),
        "75 head ticks plus 150 queued ticks"
    );
    assert_eq!(
        production_queue_label(&queue, UnitKind::Harvester.stats().train_ticks).as_deref(),
        Some("Ready"),
        "nothing behind a blocked head is counting down"
    );
    assert_eq!(
        production_queue_label(
            &std::collections::VecDeque::from([UnitKind::Harvester]),
            UnitKind::Harvester.stats().train_ticks,
        )
        .as_deref(),
        Some("Ready")
    );
    assert!(production_queue_label(&std::collections::VecDeque::new(), 0).is_none());
}

#[test]
fn a_magazine_docks_its_ordered_rounds_and_keeps_its_facts_short() {
    let mut game = crate::building_actions::tests::fixture(BuildingKind::Lattice, &[0], 2000);
    let lattice = game.presentation.selection.buildings()[0];
    let stats = BuildingKind::Lattice.magazine().unwrap();
    // The battery autofills its first interceptor; a second is bought.
    game.do_tick();
    issue_tick(&mut game, Command::OrderRound { building: lattice });
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    let chips: Vec<_> = panel
        .queue
        .iter()
        .map(|card| (card.action, card.progress.is_some()))
        .collect();
    assert_eq!(
        chips,
        [
            (CardAction::CancelRound(lattice, 0), true),
            (CardAction::CancelRound(lattice, 1), false),
        ],
        "one chip per paid round, the head wearing its meter"
    );
    assert_eq!(panel.queue[0].title, "Interceptor");
    assert_eq!(panel.queue[0].icon, CardIcon::Round(BuildingKind::Lattice));
    assert_eq!(
        panel.queue_label,
        tick_time_label(2 * stats.round_ticks - 2),
        "the dock heading counts down both rounds"
    );
    for gone in ["Assembling", "Queued", "Autofill", "Round"] {
        assert!(
            panel.info.rows.iter().all(|row| row.label != gone),
            "{gone} belongs to the dock or the cards"
        );
    }
    assert_eq!(stat(&panel, "Interceptors").value, "0/2");
    assert_eq!(stat(&panel, "Intercept reach").value, "15 tiles");
}

#[test]
fn an_allied_magazine_docks_its_rounds_read_only() {
    let mut scenario = sandbox(16, 8, 2, &[], &[(1, BuildingKind::Lattice, 6, 3)]);
    scenario.players[1].scrap = 1000;
    for player in &mut scenario.players {
        player.team = Some(0);
    }
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
    let lattice = game.state.buildings()[0].id;
    game.presentation.selection.set_buildings(vec![lattice]);
    // The ally's battery autofills its first interceptor.
    game.do_tick();
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    assert_eq!(panel.cards[0].title, "Ally structure");
    let chips: Vec<_> = panel
        .queue
        .iter()
        .map(|card| (card.action, card.progress.is_some()))
        .collect();
    assert_eq!(
        chips,
        [(CardAction::None, true)],
        "a teammate sees the order and its meter but cannot cancel it"
    );
}

#[test]
fn the_top_bar_income_follows_the_tick_it_was_summed_at() {
    let mut scenario = Scenario::skirmish();
    scenario.players[0].scrap = 10_000;
    scenario.buildings.extend([
        aftermine_sim::scenario::BuildingSpec {
            player: 0,
            kind: BuildingKind::Reclaimer,
            x: 9,
            y: 3,
        },
        aftermine_sim::scenario::BuildingSpec {
            player: 0,
            kind: BuildingKind::Fabricator,
            x: 12,
            y: 3,
        },
    ]);
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
    let reclaimer = own_building(&game, BuildingKind::Reclaimer);
    let summed = |game: &Game| -> u32 {
        game.state
            .buildings()
            .iter()
            .map(|building| building_income(&game.view(), building))
            .sum()
    };
    assert_eq!(passive_income(&game.view()), 30);
    assert_eq!(passive_income(&game.view()), summed(&game));
    game.state.tick(&[PlayerCommand {
        player: game.presentation.human,
        command: Command::UpgradeBuilding {
            building: reclaimer,
        },
    }]);
    assert_eq!(
        passive_income(&game.view()),
        0,
        "an upgrading Reclaimer pays nothing"
    );
    game.presentation.human = aftermine_sim::PlayerId(1);
    assert_eq!(
        passive_income(&game.view()),
        summed(&game),
        "another seat's sum"
    );
}

#[test]
fn a_building_group_counts_what_stands_built_and_what_does_not() {
    let game = crate::building_actions::tests::fixture(BuildingKind::Turret, &[0, 0, 1, 0], 0);
    let mut turrets: Vec<_> = game
        .state
        .buildings()
        .iter()
        .filter(|b| game.presentation.selection.buildings().contains(&b.id))
        .cloned()
        .collect();
    assert_eq!(building_summary(&turrets), "4 built");
    turrets[0].phase = aftermine_sim::BuildingPhase::Site { progress: 0 };
    turrets[2].phase = aftermine_sim::BuildingPhase::Upgrading { progress: 0 };
    turrets[3].focus = Some(aftermine_sim::AttackTarget::Building(turrets[1].id));
    assert_eq!(
        building_summary(&turrets),
        "2 built \u{b7} 1 building \u{b7} 1 upgrading \u{b7} 1 targeting"
    );
}

#[test]
fn a_spire_group_shows_its_warheads_under_its_buildings() {
    let mut game = crate::building_actions::tests::fixture(BuildingKind::Spire, &[0, 0, 0], 2000);
    let ids = game.presentation.selection.buildings().to_vec();
    let mut json = serde_json::to_value(&*game.state).unwrap();
    for b in json["buildings"].as_array_mut().unwrap() {
        if b["id"] == serde_json::json!(ids[0]) {
            b["magazine"]["loaded"] = 1.into();
        }
    }
    *game.state = serde_json::from_value(json).unwrap();
    game.issue(aftermine_sim::Command::OrderRound { building: ids[1] });
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    assert_eq!(
        panel.info.status,
        vec![
            "3 built".to_string(),
            "Warheads: 1 loaded \u{b7} 1 building \u{b7} 1 empty".to_string()
        ]
    );
    assert!(
        panel
            .cards
            .iter()
            .any(|card| card.title == "Launch (1 loaded)"),
        "{:?}",
        panel
            .cards
            .iter()
            .map(|card| &card.title)
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_mixed_building_selection_offers_one_counted_chip_per_kind() {
    let mut game =
        crate::building_actions::tests::fixture(BuildingKind::Turret, &[2, 1, 2, 2, 1], 0);
    let human = game.presentation.human;
    game.presentation.selection.set_buildings(
        game.state
            .buildings()
            .iter()
            .filter(|b| b.player == human && b.kind != BuildingKind::Foundry)
            .map(|b| b.id)
            .collect(),
    );
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    assert_eq!(panel.title, "7 STRUCTURES");
    let chips: Vec<(&str, usize, CardAction)> = panel
        .roster
        .iter()
        .map(|chip| (chip.card.title.as_str(), chip.count, chip.card.action))
        .collect();
    assert_eq!(
        chips,
        [
            (
                "Crucible x1",
                1,
                CardAction::FilterBuilding(BuildingKind::Crucible)
            ),
            (
                "Fabricator x1",
                1,
                CardAction::FilterBuilding(BuildingKind::Fabricator)
            ),
            (
                "Turret x5",
                5,
                CardAction::FilterBuilding(BuildingKind::Turret)
            ),
        ]
    );
    let turrets = &panel.roster[2].card;
    assert_eq!(turrets.icon, CardIcon::Building(BuildingKind::Turret, 2));
    assert_eq!(turrets.desc[0], "3 Bulwark, 2 Heavy Turret");
    assert_eq!(
        panel.roster[0].card.desc,
        roster_filter_desc(crate::platform::hands()),
        "one tier needs no breakdown"
    );
}

#[test]
fn a_single_kind_offers_every_one_you_own_until_all_are_selected() {
    let mut game = crate::building_actions::tests::fixture(BuildingKind::Spire, &[0, 0, 0], 0);
    let ids = game.presentation.selection.buildings().to_vec();
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    assert!(
        panel.select_all.is_none(),
        "every Spire is already selected"
    );

    game.presentation.selection.set_buildings(vec![ids[1]]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    assert!(panel.roster.is_empty(), "one kind needs no roster strip");
    let all = panel
        .select_all
        .as_ref()
        .expect("two Spires sit unselected");
    assert_eq!(all.owned, 3);
    assert_eq!(all.label(), "All 3");
    assert_eq!(all.card.title, "Select all 3 Spires");
    assert!(all.card.desc.is_empty(), "the heading says it all");
    assert_eq!(
        all.card.action,
        CardAction::SelectAllBuildings(BuildingKind::Spire)
    );
    assert_eq!(
        panel
            .card(crate::layout::CardRow::SelectAll, 0)
            .map(|card| card.title.as_str()),
        Some("Select all 3 Spires"),
        "the button's hover text comes from its card"
    );
    assert!(panel.card(crate::layout::CardRow::SelectAll, 1).is_none());
}

#[test]
fn a_single_unit_kind_offers_every_one_you_own_until_all_are_selected() {
    let scenario = Scenario::from_json(
        r#"{
        "name":"Select all units",
        "map":["................", "..1.........2...", "................", "................"],
        "players":[{"name":"You","scrap":0,"bot":false},
                   {"name":"Target","scrap":0,"bot":true}],
        "units":[
          {"player":0,"kind":"sentinel","x":3,"y":3},
          {"player":0,"kind":"sentinel","x":4,"y":3},
          {"player":0,"kind":"sentinel","x":5,"y":3},
          {"player":0,"kind":"lancer","x":6,"y":3},
          {"player":1,"kind":"sentinel","x":12,"y":3}],
        "buildings":[]
    }"#,
    )
    .unwrap();
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
    let human = game.presentation.human;
    let sentinels: Vec<_> = game
        .state
        .units()
        .iter()
        .filter(|u| u.player == human && u.kind == UnitKind::Sentinel)
        .map(|u| u.id)
        .collect();
    game.presentation.selection.set_units(vec![sentinels[0]]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    let all = panel
        .select_all
        .as_ref()
        .expect("two Sentinels sit unselected");
    assert_eq!(
        all.card.title, "Select all 3 Sentinels",
        "the Lancer and the enemy's Sentinel do not count"
    );
    assert_eq!(
        all.card.action,
        CardAction::SelectAllUnits(UnitKind::Sentinel)
    );

    let lancer = own_unit(&game, UnitKind::Lancer);
    game.presentation
        .selection
        .set_units([game.presentation.selection.units(), &[lancer]].concat());
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    assert!(
        panel.select_all.is_none(),
        "a mixed selection has its strip"
    );

    game.presentation.selection.set_units(sentinels);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    assert!(
        panel.select_all.is_none(),
        "every Sentinel is already selected"
    );
}

/// A skirmish with a Roc for each seat, every bay refilling.
fn rival_rocs() -> (Game, aftermine_sim::UnitId, aftermine_sim::UnitId) {
    let mut scenario = Scenario::skirmish();
    for seat in &mut scenario.players {
        seat.scrap = seat.scrap.max(1000);
    }
    for (player, x) in [(0, 12), (1, 20)] {
        scenario.units.push(aftermine_sim::scenario::UnitSpec {
            player,
            kind: UnitKind::Roc,
            x,
            y: 10,
        });
    }
    let game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
    let roc = |player: u8| {
        game.state
            .units()
            .iter()
            .find(|unit| unit.kind == UnitKind::Roc && unit.player.0 == player)
            .unwrap()
            .id
    };
    let (own, enemy) = (roc(0), roc(1));
    (game, own, enemy)
}

#[test]
fn a_roc_docks_its_drones_on_order_beside_its_program() {
    let (mut game, roc, enemy) = rival_rocs();
    let bay = UnitKind::Roc.stats().bay.unwrap();
    // The bay autofills its first drone; a second is bought.
    game.do_tick();
    issue_tick(&mut game, Command::OrderDrone { unit: roc });
    game.presentation.selection.set_units(vec![roc]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    let column = panel.drones.as_ref().expect("drones on order get a column");
    let chips: Vec<_> = column
        .chips
        .iter()
        .map(|card| (card.action, card.progress.is_some()))
        .collect();
    assert_eq!(
        chips,
        [
            (CardAction::CancelDrone(roc, 0), true),
            (CardAction::CancelDrone(roc, 1), false),
        ],
        "one chip per paid drone, the one building wearing its meter"
    );
    assert_eq!(column.chips[0].title, "Drone");
    assert_eq!(column.chips[0].icon, CardIcon::Unit(bay.drone));
    assert_eq!(column.chips[0].desc, refund_chip_desc());
    assert_eq!(
        column.label,
        tick_time_label(2 * bay.build_ticks - 2),
        "the heading counts down both drones"
    );
    assert!(
        panel
            .queue
            .iter()
            .all(|card| !matches!(card.action, CardAction::CancelDrone(..))),
        "the orders dock keeps the program alone"
    );
    assert_eq!(stat(&panel, "Drones").value, format!("0/{}", bay.capacity));

    game.presentation.selection.set_units(vec![enemy]);
    assert!(
        game.state.unit(enemy).unwrap().bay.unwrap().ordered > 0,
        "premise: the enemy bay is building too"
    );
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    assert!(
        panel.drones.is_none(),
        "an enemy's drones on order are intent"
    );
    assert_eq!(
        stat(&panel, "Drones").value,
        format!("up to {}", bay.capacity)
    );
}

#[test]
fn the_panel_types_its_rally_verb_and_what_its_dock_holds() {
    let mut game = game();
    let human = game.presentation.human;
    let foundry = human_foundry(&game);
    game.presentation.selection.set_buildings(vec![foundry]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    assert_eq!(panel.rally, Some(RallyVerb::Set));
    assert_eq!(panel.cards[0].title, "Set rally");
    assert_eq!(panel.dock, DockKind::Work);

    game.state.tick(&[PlayerCommand {
        player: human,
        command: Command::SetRally {
            building: foundry,
            rally: Some(aftermine_sim::Rally::Tile(chassis::grid::TilePos::new(
                8, 8,
            ))),
        },
    }]);
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    assert_eq!(panel.rally, Some(RallyVerb::Reset));
    assert_eq!(panel.cards[0].title, "Reset rally");

    game.presentation.selection.clear();
    game.presentation.selection.set_units(
        game.state
            .units()
            .iter()
            .filter(|unit| unit.player == human)
            .map(|unit| unit.id)
            .take(1)
            .collect(),
    );
    let panel = build_for_palette(&game.view(), &BindingMap::classic(), false).unwrap();
    assert_eq!(panel.dock, DockKind::Orders);
    assert_eq!(panel.rally, None);
}
