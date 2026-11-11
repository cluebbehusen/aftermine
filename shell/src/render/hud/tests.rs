use super::*;
use crate::action::BindingMap;
use crate::game::Game;

/// Every glyph half as wide as the size: deterministic and headless.
fn fixed(_: Face, text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * size * 0.5
}

fn env(w: f32, h: f32) -> HudEnv {
    HudEnv {
        viewport: vec2(w, h),
        ui: 1.0,
    }
}

fn skirmish() -> Game {
    Game::with_viewport(aftermine_sim::Scenario::skirmish(), vec2(1280.0, 800.0)).unwrap()
}

#[test]
fn a_selected_foundry_lays_its_cards_inside_the_action_region() {
    let mut game = skirmish();
    let human = game.presentation.human;
    let foundry = game
        .state
        .buildings()
        .iter()
        .find(|building| building.player == human)
        .unwrap()
        .id;
    game.presentation.selection.buildings.push(foundry);
    // iPad mini, iPad Pro 11", and Steam Deck: the floor and its neighbours.
    for (w, h) in [(1133.0, 744.0), (1210.0, 834.0), (1280.0, 800.0)] {
        let hud = layout(
            &game.view(),
            &InputState::new(),
            &BindingMap::classic(),
            env(w, h),
            None,
            &fixed,
        );
        let panel = hud.panel.as_ref().expect("a selection has a panel");
        let model = hud.layout;
        assert!(model.card_count > 0);
        assert_eq!(model.card_count, panel.cards.len().min(16));
        let actions = model.panel_regions[1];
        let cards = &model.cards[..model.card_count];
        for (rect, _) in cards {
            assert!(
                rect.x >= actions.x
                    && rect.y >= actions.y
                    && rect.right() <= actions.right()
                    && rect.bottom() <= actions.bottom(),
                "{rect:?} outside {actions:?} at {w}x{h}"
            );
        }
        assert!(
            cards.iter().all(|(rect, _)| rect.y == cards[0].0.y),
            "rally controls and every product share one row at {w}x{h}"
        );
        assert_eq!(
            model.minimap,
            minimap_rect_scaled(
                game.state.map().width(),
                game.state.map().height(),
                vec2(w, h),
                1.0
            )
        );
        assert!(model.menu_button.w > 0.0 && model.pause_button.w > 0.0);
    }
}

#[test]
fn a_spectator_has_no_top_bar_controls_but_keeps_the_minimap() {
    let mut game = skirmish();
    game.presentation.stance = crate::game::Stance::Spectator;
    let hud = layout(
        &game.view(),
        &InputState::new(),
        &BindingMap::classic(),
        env(1280.0, 800.0),
        None,
        &fixed,
    );
    assert_eq!(hud.layout.menu_button.w, 0.0);
    assert_eq!(hud.layout.pause_button.w, 0.0);
    assert!(hud.geometry.top_bar.is_none());
    assert!(hud.layout.minimap.w > 0.0);
}

#[test]
fn a_resize_moves_the_minimap_before_the_next_click() {
    let game = skirmish();
    let input = InputState::new();
    let bindings = BindingMap::classic();
    let view = game.view();
    refresh(&view, &input, &bindings, env(1280.0, 800.0), None, &fixed);
    let small = game.presentation.layout.get().minimap;
    refresh(&view, &input, &bindings, env(1600.0, 1000.0), None, &fixed);
    let large = game.presentation.layout.get().minimap;
    assert_ne!(small, large);
    let center = large.center();
    assert!(
        !small.contains(center),
        "the new minimap's center must be off the old one"
    );
    assert!(crate::render::minimap_world_at(&view, center).is_some());
}

#[test]
fn a_busy_builder_keeps_its_orders_dock_over_the_construction_palette() {
    let mut game = skirmish();
    let human = game.presentation.human;
    let harvester = game
        .state
        .units()
        .iter()
        .find(|unit| unit.player == human && unit.kind == aftermine_sim::UnitKind::Harvester)
        .unwrap();
    let (id, home) = (
        harvester.id,
        chassis::grid::TilePos::containing(harvester.pos),
    );
    for (dx, queue) in [(4, false), (6, true)] {
        game.state.tick(&[aftermine_sim::PlayerCommand {
            player: human,
            command: aftermine_sim::Command::Run {
                units: vec![id],
                goal: chassis::grid::TilePos {
                    x: home.x + dx,
                    y: home.y,
                },
                queue,
            },
        }]);
    }
    game.presentation.selection.units.push(id);
    let mut input = InputState::new();
    input.build_menu = true;
    // The layout floor, the Steam Deck, and a desktop window.
    for (w, h) in [(1133.0, 744.0), (1280.0, 800.0), (1920.0, 1080.0)] {
        let hud = layout(
            &game.view(),
            &input,
            &BindingMap::classic(),
            env(w, h),
            None,
            &fixed,
        );
        assert!(matches!(
            hud.geometry.panel,
            Some(panel_draw::PanelLayout::Catalog { .. })
        ));
        let model = hud.layout;
        let band = model.panel_regions[1];
        assert_eq!(model.queue_count, 2, "both runs show at {w}x{h}");
        assert!(model.queue_stop.0.w > 0.0, "Stop shows at {w}x{h}");
        assert!(model.orders.bottom() <= band.y, "{w}x{h}");
        assert!(model.orders.y >= crate::layout::TOP_BAR_H, "{w}x{h}");
        for (rect, _) in &model.queue_slots[..model.queue_count] {
            assert!(rect.bottom() <= band.y && rect.y >= model.orders.y);
        }
    }
}

#[test]
fn the_placement_ribbon_says_what_a_click_would_do_and_holds_still() {
    let mut game = skirmish();
    let human = game.presentation.human;
    let harvester = game
        .state
        .units()
        .iter()
        .find(|unit| unit.player == human && unit.kind == aftermine_sim::UnitKind::Harvester)
        .unwrap();
    let (id, home) = (
        harvester.id,
        chassis::grid::TilePos::containing(harvester.pos),
    );
    for (dx, queue) in [(4, false), (6, true)] {
        game.state.tick(&[aftermine_sim::PlayerCommand {
            player: human,
            command: aftermine_sim::Command::Run {
                units: vec![id],
                goal: chassis::grid::TilePos {
                    x: home.x + dx,
                    y: home.y,
                },
                queue,
            },
        }]);
    }
    game.presentation.selection.units.push(id);
    let foundry = game
        .state
        .buildings()
        .iter()
        .find(|building| building.player == human)
        .unwrap()
        .anchor;
    let mut input = InputState::new();
    input.build_menu = true;
    input.arm_placement(aftermine_sim::BuildingKind::Turret);
    let ribbon = |game: &Game, input: &mut InputState, tile: chassis::grid::TilePos| {
        input.mouse = game
            .presentation
            .camera
            .to_screen(vec2(tile.x as f32 + 0.5, tile.y as f32 + 0.5));
        layout(
            &game.view(),
            input,
            &BindingMap::classic(),
            env(1133.0, 744.0),
            None,
            &fixed,
        )
        .geometry
        .ribbon
        .expect("placement shows its ribbon")
    };
    let open = chassis::grid::TilePos::new(foundry.x + 3, foundry.y + 3);

    let builds = ribbon(&game, &mut input, open);
    let status = builds.status.as_ref().expect("a spot under the cursor");
    assert_eq!(status.verdict, "Builds now");
    assert_eq!(status.detail.as_deref(), Some("Replaces 2 orders"));
    assert!(builds.rect.h > crate::theme::MIN_TOUCH_TARGET);

    let blocked = ribbon(&game, &mut input, foundry);
    let status = blocked.status.as_ref().expect("a spot under the cursor");
    assert_eq!(status.verdict, "Something already stands there");
    assert_eq!(
        status.detail, None,
        "a refused spot neither queues nor replaces"
    );
    assert_eq!(blocked.rect, builds.rect, "the ribbon holds still");

    input.queue_toggle = crate::input::QueueToggle::Locked;
    let queued = ribbon(&game, &mut input, open);
    let status = queued.status.as_ref().expect("a spot under the cursor");
    assert_eq!(status.detail.as_deref(), Some("Queues as #3"));
    assert_eq!(queued.rect, builds.rect);
}

#[test]
fn every_armed_ribbon_ends_in_a_cross_clear_of_its_words() {
    let mut game = skirmish();
    let human = game.presentation.human;
    let harvester = game
        .state
        .units()
        .iter()
        .find(|unit| unit.player == human && unit.kind == aftermine_sim::UnitKind::Harvester)
        .unwrap()
        .id;
    game.presentation.selection.units.push(harvester);
    let arms: [fn(&mut InputState); 3] = [
        |input| input.click_verb = Some(crate::input::ClickVerb::Hunt),
        |input| {
            input.build_menu = true;
            input.arm_placement(aftermine_sim::BuildingKind::Turret);
        },
        |input| input.patrol_route = Some(Vec::new()),
    ];
    for arm in arms {
        let mut input = InputState::new();
        arm(&mut input);
        let hud = layout(
            &game.view(),
            &input,
            &BindingMap::classic(),
            env(1133.0, 744.0),
            None,
            &fixed,
        );
        let ribbon = hud.geometry.ribbon.expect("an armed mode shows its ribbon");
        let cancel = ribbon.cancel;
        assert_eq!(hud.layout.ribbon_cancel, cancel);
        assert!(
            ribbon.rect.contains(cancel.center()) && cancel.right() <= ribbon.rect.right(),
            "{cancel:?} in {:?}",
            ribbon.rect
        );
        let words = ribbon.rect.x
            + 10.0
            + fixed(Face::Body, &ribbon.label, crate::theme::Type::Label.at(1.0));
        assert!(
            words <= cancel.x,
            "{}: {words} runs into {cancel:?}",
            ribbon.label
        );
    }
}

#[test]
fn a_lesson_card_holds_a_full_dock_under_it_at_the_floor_size() {
    use crate::tutorial::{CoachLine, Lesson, Tutorial};
    let mut scenario = aftermine_sim::Scenario::skirmish();
    scenario.players[0].scrap = 5000;
    let mut game = Game::with_viewport(scenario, vec2(1133.0, 744.0)).unwrap();
    let human = game.presentation.human;
    let foundry = game
        .state
        .buildings()
        .iter()
        .find(|building| building.player == human)
        .unwrap()
        .id;
    let train = aftermine_sim::PlayerCommand {
        player: human,
        command: aftermine_sim::Command::Train {
            building: foundry,
            kind: aftermine_sim::UnitKind::Harvester,
        },
    };
    game.state
        .tick(&vec![train; aftermine_sim::stats::QUEUE_CAP]);
    game.presentation.selection.buildings.push(foundry);
    let env = env(1133.0, 744.0);
    let input = InputState::new();
    // The tallest card any step shows with its red coach line, and one
    // folded.
    let card = |coach: Option<&CoachLine>, folded: bool| {
        Lesson::ALL
            .into_iter()
            .flat_map(|lesson| {
                let tutorial = Tutorial::begin_at(lesson, vec2(1133.0, 744.0))
                    .unwrap()
                    .tutorial
                    .unwrap();
                (0..lesson.steps().len()).map(move |at| {
                    let mut tutorial = tutorial.clone().at_step(at);
                    if folded {
                        tutorial.toggle_collapsed();
                    }
                    crate::render::tutorial_card::layout(
                        &tutorial,
                        coach,
                        &BindingMap::classic(),
                        crate::platform::Hands::at_launch(crate::platform::Platform::Desktop),
                        env,
                        &fixed,
                        crate::input::HudHands::Pointer,
                    )
                    .rect
                })
            })
            .max_by(|a, b| a.bottom().total_cmp(&b.bottom()))
            .unwrap()
    };
    let tall = card(Some(&CoachLine::Recovery), false);
    let folded = card(None, true);
    let mut stops = Vec::new();
    for card in [tall, folded] {
        let scene = crate::game::Scene {
            corner_card: Some(card),
            ..game.view()
        };
        let hud = layout(&scene, &input, &BindingMap::classic(), env, None, &fixed);
        let (stop, _) = hud.layout.queue_stop;
        assert!(stop.w > 0.0, "a queue offers Stop");
        assert_eq!(hud.layout.queue_count, aftermine_sim::stats::QUEUE_CAP);
        for (rect, _) in std::iter::once(&hud.layout.queue_stop)
            .chain(&hud.layout.queue_slots[..hud.layout.queue_count])
        {
            assert!(rect.y >= card.bottom(), "{rect:?} under {card:?}");
        }
        stops.push(stop);
    }
    assert!(
        stops[1].y < stops[0].y,
        "a folded card gives the dock its room back"
    );
}

#[test]
fn the_patrol_ribbon_holds_still_and_lights_start_once_it_has_a_point() {
    let game = skirmish();
    let mut input = InputState::new();
    let ribbon = |input: &InputState| {
        let hud = layout(
            &game.view(),
            input,
            &BindingMap::classic(),
            env(1133.0, 744.0),
            None,
            &fixed,
        );
        let ribbon = hud.geometry.ribbon.expect("patrol shows its ribbon");
        assert_eq!(hud.layout.mode_ribbon, ribbon.rect);
        let buttons = ribbon.patrol.expect("patrol buttons");
        assert_eq!(hud.layout.patrol_start, buttons.start);
        assert_eq!(hud.layout.ribbon_cancel, ribbon.cancel);
        (ribbon.rect, buttons, ribbon.cancel)
    };
    input.patrol_route = Some(Vec::new());
    let (empty, waiting, cancel) = ribbon(&input);
    assert!(!waiting.ready, "Start waits for a point");
    for button in [waiting.start, cancel] {
        assert!(empty.contains(button.center()), "{button:?} in {empty:?}");
        assert!(button.h <= empty.h);
    }
    assert!(waiting.start.right() < cancel.x);

    input.patrol_route = Some(vec![chassis::grid::TilePos::new(3, 3)]);
    let (one, ready, _) = ribbon(&input);
    assert!(ready.ready);
    input.patrol_route = Some(vec![
        chassis::grid::TilePos::new(3, 3);
        aftermine_sim::stats::ORDER_QUEUE_CAP
    ]);
    let (full, _, _) = ribbon(&input);
    assert_eq!(one, empty, "the ribbon holds still");
    assert_eq!(full, empty);
}

#[test]
fn a_finger_with_no_ghost_down_leaves_the_ribbon_one_row_tall() {
    let mut game = skirmish();
    let human = game.presentation.human;
    let harvester = game
        .state
        .units()
        .iter()
        .find(|unit| unit.player == human && unit.kind == aftermine_sim::UnitKind::Harvester)
        .unwrap()
        .id;
    game.presentation.selection.units.push(harvester);
    let mut input = InputState::new();
    input.build_menu = true;
    input.arm_placement(aftermine_sim::BuildingKind::Turret);
    input.last_pointer = crate::input::Pointer::Touch;
    let ribbon = layout(
        &game.view(),
        &input,
        &BindingMap::classic(),
        env(1133.0, 744.0),
        None,
        &fixed,
    )
    .geometry
    .ribbon
    .expect("placement shows its ribbon");
    assert!(ribbon.status.is_none(), "no spot to judge yet");
    assert_eq!(ribbon.rect.h, crate::theme::MIN_TOUCH_TARGET);
}

#[test]
fn a_planned_row_counts_itself_in_the_ribbon() {
    let mut game = skirmish();
    let human = game.presentation.human;
    let harvester = game
        .state
        .units()
        .iter()
        .find(|unit| unit.player == human && unit.kind == aftermine_sim::UnitKind::Harvester)
        .unwrap()
        .id;
    game.presentation.selection.units.push(harvester);
    let foundry = game
        .state
        .buildings()
        .iter()
        .find(|building| building.player == human)
        .unwrap()
        .anchor;
    let mut input = InputState::new();
    input.build_menu = true;
    input.arm_placement(aftermine_sim::BuildingKind::Barricade);
    let at = |dx: i32| {
        game.presentation.camera.to_screen(vec2(
            (foundry.x + dx) as f32 + 0.5,
            (foundry.y + 3) as f32 + 0.5,
        ))
    };
    let (a, b, c) = (at(3), at(4), at(5));
    let events = [
        aftermine_protocol::RawEvent::MouseDown {
            button: aftermine_protocol::MouseButton::Left,
            x: a.x,
            y: a.y,
        },
        aftermine_protocol::RawEvent::MouseMove { x: b.x, y: b.y },
        aftermine_protocol::RawEvent::MouseMove { x: c.x, y: c.y },
    ];
    crate::input::apply_events(
        &mut game,
        &mut input,
        &BindingMap::classic(),
        &events,
        |_, _| {},
    );
    assert_eq!(input.planned_stamps().len(), 3, "premise: a row of three");
    let ribbon = layout(
        &game.view(),
        &input,
        &BindingMap::classic(),
        env(1280.0, 800.0),
        None,
        &fixed,
    )
    .geometry
    .ribbon
    .expect("placement shows its ribbon");
    let cost = aftermine_sim::BuildingKind::Barricade
        .base_stats()
        .construction
        .cost;
    assert_eq!(ribbon.label, "Barricade \u{d7}3");
    assert_eq!(ribbon.cost, Some((3 * cost).to_string()));
    let status = ribbon.status.expect("the cursor rests on the row's end");
    assert_eq!(status.detail.as_deref(), Some("Queues as #3"));
}

#[test]
fn the_status_row_goes_blank_over_chrome_and_keeps_its_height() {
    let mut game = skirmish();
    let human = game.presentation.human;
    let harvester = game
        .state
        .units()
        .iter()
        .find(|unit| unit.player == human && unit.kind == aftermine_sim::UnitKind::Harvester)
        .unwrap()
        .id;
    game.presentation.selection.units.push(harvester);
    let foundry = game
        .state
        .buildings()
        .iter()
        .find(|building| building.player == human)
        .unwrap()
        .anchor;
    let mut input = InputState::new();
    input.build_menu = true;
    input.arm_placement(aftermine_sim::BuildingKind::Turret);
    input.mouse = game.presentation.camera.to_screen(vec2(
        (foundry.x + 3) as f32 + 0.5,
        (foundry.y + 3) as f32 + 0.5,
    ));
    let lay_out = |game: &Game, input: &InputState| {
        let hud = layout(
            &game.view(),
            input,
            &BindingMap::classic(),
            env(1280.0, 800.0),
            None,
            &fixed,
        );
        let ribbon = hud.geometry.ribbon.expect("placement shows its ribbon");
        game.presentation.layout.set(hud.layout);
        ribbon
    };
    let over_ground = lay_out(&game, &input);
    assert_eq!(
        over_ground.status.as_ref().unwrap().verdict,
        "Builds now",
        "premise"
    );
    input.mouse = over_ground.rect.center();
    let over_ribbon = lay_out(&game, &input);
    let status = over_ribbon.status.expect("the row stays");
    assert!(status.verdict.is_empty() && status.detail.is_none());
    assert_eq!(over_ribbon.rect, over_ground.rect, "the ribbon holds still");
}

#[test]
fn the_select_all_button_sits_in_the_info_box_beside_the_title() {
    let mut game =
        crate::building_actions::tests::fixture(aftermine_sim::BuildingKind::Turret, &[0, 0, 0], 0);
    game.presentation.selection.buildings.truncate(1);
    for (w, h) in [(1133.0, 744.0), (1280.0, 800.0)] {
        let hud = layout(
            &game.view(),
            &InputState::new(),
            &BindingMap::classic(),
            env(w, h),
            None,
            &fixed,
        );
        let (rect, action) = hud.layout.select_all;
        assert_eq!(
            action,
            crate::panel::CardAction::SelectAllBuildings(aftermine_sim::BuildingKind::Turret)
        );
        let info = hud.layout.panel_regions[0];
        assert_eq!(
            rect.right(),
            info.right() - 12.0,
            "flush with the stat values"
        );
        assert!(
            rect.y >= info.y + 5.0 && rect.x > info.x + 52.0,
            "{rect:?} clears the edge and the portrait in {info:?} at {w}x{h}"
        );
    }
    game.presentation.selection.buildings = game
        .state
        .buildings()
        .iter()
        .filter(|b| b.kind == aftermine_sim::BuildingKind::Turret)
        .map(|b| b.id)
        .collect();
    let hud = layout(
        &game.view(),
        &InputState::new(),
        &BindingMap::classic(),
        env(1280.0, 800.0),
        None,
        &fixed,
    );
    assert_eq!(
        hud.layout.select_all.1,
        crate::panel::CardAction::None,
        "nothing left to gather"
    );
}

#[test]
fn a_roc_s_drone_column_stands_beside_its_orders_dock() {
    let mut scenario = aftermine_sim::Scenario::skirmish();
    scenario.units.push(aftermine_sim::scenario::UnitSpec {
        player: 0,
        kind: aftermine_sim::UnitKind::Roc,
        x: 12,
        y: 10,
    });
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
    let roc = game
        .state
        .units()
        .iter()
        .find(|unit| unit.kind == aftermine_sim::UnitKind::Roc)
        .unwrap();
    let (id, home) = (roc.id, chassis::grid::TilePos::containing(roc.pos));
    // The bay autofills its first drone.
    game.do_tick();
    game.presentation.selection.units = vec![id];
    let model = |game: &Game| {
        layout(
            &game.view(),
            &InputState::new(),
            &BindingMap::classic(),
            env(1210.0, 834.0),
            None,
            &fixed,
        )
        .layout
    };
    let idle = model(&game);
    let band = idle.panel_regions[0].y;
    assert_eq!(
        idle.orders.w, 0.0,
        "premise: an idle Roc has no orders dock"
    );
    assert_eq!(idle.drone_count, 1);
    assert_eq!(idle.drones.x, 0.0, "alone, the column takes the corner");
    assert_eq!(idle.drones.bottom(), band, "it stands on the band");

    game.state.tick(&[aftermine_sim::PlayerCommand {
        player: game.presentation.human,
        command: aftermine_sim::Command::Run {
            units: vec![id],
            goal: chassis::grid::TilePos {
                x: home.x + 6,
                y: home.y,
            },
            queue: false,
        },
    }]);
    let busy = model(&game);
    assert!(
        busy.orders.w > 0.0,
        "premise: the run fills the orders dock"
    );
    assert!(
        busy.drones.x >= busy.orders.right(),
        "{:?} overlaps {:?}",
        busy.drones,
        busy.orders
    );
    assert_eq!(busy.drones.bottom(), band);
    let (chip, _) = busy.drone_slots[0];
    assert!(busy.drones.contains(chip.center()));
    assert_eq!(
        crate::layout::card_under(&busy, chip.center(), None).map(|hit| (hit.row, hit.action)),
        Some((
            crate::layout::CardRow::Drones,
            crate::panel::CardAction::CancelDrone(id, 0)
        ))
    );
}

#[test]
fn a_selection_of_every_kind_gets_a_roster_chip_for_each() {
    use aftermine_sim::UnitKind;
    let mut scenario = aftermine_sim::Scenario::skirmish();
    scenario.units.clear();
    let kinds: Vec<UnitKind> = UnitKind::ALL
        .into_iter()
        .filter(|&kind| kind != UnitKind::Drone)
        .collect();
    for (index, &kind) in kinds.iter().enumerate() {
        scenario.units.push(aftermine_sim::scenario::UnitSpec {
            player: 0,
            kind,
            x: 6 + 3 * crate::numeric::Fit::fit::<i32>(index % 8),
            y: 5 + 3 * crate::numeric::Fit::fit::<i32>(index / 8),
        });
    }
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
    game.presentation.selection.units = game.state.units().iter().map(|u| u.id).collect();
    for (w, h) in [(1133.0, 744.0), (1210.0, 834.0), (1280.0, 800.0)] {
        let hud = layout(
            &game.view(),
            &InputState::new(),
            &BindingMap::classic(),
            env(w, h),
            None,
            &fixed,
        );
        let model = hud.layout;
        assert_eq!(
            model.roster_count,
            kinds.len(),
            "one chip per kind at {w}x{h}"
        );
        let info = model.panel_regions[0];
        let chips = &model.roster_slots[..model.roster_count];
        for (index, (rect, action)) in chips.iter().enumerate() {
            assert!(
                matches!(action, crate::panel::CardAction::FilterKind(_)),
                "{action:?}"
            );
            assert!(
                rect.x >= info.x
                    && rect.y >= info.y
                    && rect.right() <= info.right()
                    && rect.bottom() <= info.bottom(),
                "{rect:?} outside {info:?} at {w}x{h}"
            );
            assert!(rect.y >= crate::layout::TOP_BAR_H, "{rect:?} at {w}x{h}");
            assert!(
                chips[..index]
                    .iter()
                    .all(|(other, _)| !rect.overlaps(other)),
                "{rect:?} overlaps at {w}x{h}"
            );
        }
    }
}

#[test]
fn a_ring_fits_between_the_top_bar_and_the_panel_wherever_it_opens() {
    let mut game = skirmish();
    let human = game.presentation.human;
    let foundry = game
        .state
        .buildings()
        .iter()
        .find(|building| building.player == human)
        .unwrap()
        .id;
    // A producer's panel is the tallest band a ring opens over.
    game.presentation.selection.buildings.push(foundry);
    for (w, h, ui) in [
        (1133.0, 744.0, 1.0),
        (1280.0, 800.0, 1.0),
        (1280.0, 800.0, 1.5),
        (1133.0, 744.0, 1.5),
    ] {
        let viewport = vec2(w, h);
        let model = layout(
            &game.view(),
            &InputState::new(),
            &BindingMap::classic(),
            HudEnv { viewport, ui },
            None,
            &fixed,
        )
        .layout;
        for anchor in [viewport * 0.5, Vec2::ZERO, viewport] {
            let ring = crate::radial::Geometry::fit(
                anchor,
                viewport,
                ui,
                model.top_bar_h,
                model.panel_top,
            );
            let at = format!("{w}x{h} at {ui}x from {anchor}");
            assert!(ring.center.y - ring.reach() >= model.top_bar_h, "{at}");
            assert!(ring.legend_baseline() <= model.panel_top, "{at}");
            assert!(ring.center.x - ring.reach() >= 0.0, "{at}");
            assert!(ring.center.x + ring.reach() <= w, "{at}");
            assert!(ring.scale <= ui, "{at}");
        }
        // The window's center leaves room for the whole ring at UI scale 1.
        if ui == 1.0 {
            let ring = crate::radial::Geometry::fit(
                viewport * 0.5,
                viewport,
                ui,
                model.top_bar_h,
                model.panel_top,
            );
            assert_eq!(ring.scale, 1.0, "{w}x{h}");
        }
    }
}

/// The skirmish with the human's first structure, a Foundry, selected.
fn foundry_selected() -> (Game, aftermine_sim::BuildingId) {
    let mut game = skirmish();
    let human = game.presentation.human;
    let foundry = game
        .state
        .buildings()
        .iter()
        .find(|building| building.player == human)
        .unwrap()
        .id;
    game.presentation.selection.buildings.push(foundry);
    (game, foundry)
}

#[test]
fn pad_mode_leaves_out_what_buttons_do_and_moves_nothing_that_stays() {
    let (game, _) = foundry_selected();
    let mut pad = InputState::new();
    pad.hud_pad = true;
    let zero = Rect::new(0.0, 0.0, 0.0, 0.0);
    for (w, h) in [(1133.0, 744.0), (1280.0, 800.0)] {
        let lay = |input: &InputState| {
            layout(
                &game.view(),
                input,
                &BindingMap::classic(),
                env(w, h),
                None,
                &fixed,
            )
        };
        let mouse = lay(&InputState::new()).layout;
        let shown = lay(&pad);
        let padded = shown.layout;
        assert!(
            mouse.card_count > 0
                && mouse.army_strip.w > 0.0
                && mouse.group_column.w > 0.0
                && mouse.menu_button.w > 0.0,
            "premise at {w}x{h}"
        );
        // Left out: the rings, the bumpers and Menu stand in for them.
        assert_eq!(padded.card_count, 0);
        assert_eq!(padded.panel_regions[1], zero);
        assert_eq!(padded.army_strip, zero);
        assert_eq!(padded.army_buttons, [None; 2]);
        assert_eq!(padded.group_column, zero);
        assert_eq!(padded.group_slots, [None; crate::action::CONTROL_GROUPS]);
        assert_eq!(padded.menu_button, zero);
        assert_eq!(padded.queue_toggle, zero);
        assert!(shown.geometry.army_strip.is_none());
        assert!(shown.geometry.group_column.is_none());
        // Everything that stays keeps its place.
        assert_eq!(padded.panel_regions[0], mouse.panel_regions[0]);
        assert_eq!(padded.panel_top, mouse.panel_top);
        assert_eq!(padded.orders, mouse.orders);
        assert_eq!(padded.queue_slots, mouse.queue_slots);
        assert_eq!(padded.select_all, mouse.select_all);
        assert_eq!(padded.roster_slots, mouse.roster_slots);
        assert_eq!(padded.minimap, mouse.minimap);
        assert_eq!(padded.pause_button, mouse.pause_button);
        assert_eq!(padded.mode_ribbon, mouse.mode_ribbon);
        // What stays, such as the toasts, places itself by the layout
        // before anything was left out.
        assert_eq!(shown.geometry.laid_out.panel_regions, mouse.panel_regions);
        assert_eq!(shown.geometry.laid_out.army_strip, mouse.army_strip);
        assert_eq!(shown.geometry.laid_out.group_column, mouse.group_column);
    }
}

#[test]
fn the_click_that_ends_pad_mode_lands_on_the_hud_pad_mode_drew() {
    use aftermine_protocol::{MouseButton, RawEvent};
    let (mut game, foundry) = foundry_selected();
    let mouse = layout(
        &game.view(),
        &InputState::new(),
        &BindingMap::classic(),
        env(1280.0, 800.0),
        None,
        &fixed,
    )
    .layout;
    let train = mouse.cards[..mouse.card_count]
        .iter()
        .find(|(_, action)| {
            matches!(
                action,
                crate::panel::CardAction::Dispatch(crate::action::Action::TrainSlot(_))
            )
        })
        .expect("the Foundry trains")
        .0
        .center();
    let relayout = |game: &Game, input: &InputState| {
        refresh(
            &game.view(),
            input,
            &BindingMap::classic(),
            env(1280.0, 800.0),
            None,
            &fixed,
        );
    };
    let click = [
        RawEvent::MouseDown {
            button: MouseButton::Left,
            x: train.x,
            y: train.y,
        },
        RawEvent::MouseUp {
            button: MouseButton::Left,
            x: train.x,
            y: train.y,
        },
    ];
    let mut input = InputState::new();
    input.hud_pad = true;
    relayout(&game, &input);
    crate::input::apply_events(
        &mut game,
        &mut input,
        &BindingMap::classic(),
        &click,
        relayout,
    );
    assert!(game.pending.is_empty(), "no hidden card took the click");
    input.settle_hud();
    assert!(!input.hud_pad, "the click ended pad mode");
    game.presentation.selection.buildings = vec![foundry];
    relayout(&game, &input);
    crate::input::apply_events(
        &mut game,
        &mut input,
        &BindingMap::classic(),
        &click,
        relayout,
    );
    assert!(
        game.pending
            .iter()
            .any(|pending| matches!(pending.command, aftermine_sim::Command::Train { .. })),
        "the next click finds the card the mouse HUD draws"
    );
}

#[test]
fn the_salvage_chip_is_part_of_the_hud_and_stays_off_its_chrome() {
    let game = skirmish();
    let view = game.view();
    let map = game.state.map();
    let tile = (0..map.height())
        .flat_map(|y| (0..map.width()).map(move |x| chassis::grid::TilePos::new(x, y)))
        .find(|tile| {
            matches!(
                view.known_salvage(*tile),
                Some(crate::game::Salvage::Scrap(_))
            ) && {
                let at = game
                    .presentation
                    .camera
                    .to_screen(vec2(tile.x as f32 + 0.5, tile.y as f32 + 0.5));
                at.x > 100.0 && at.x < 900.0 && at.y > 100.0 && at.y < 500.0
            }
        })
        .expect("the opening view shows a scrap node");
    let Some(crate::game::Salvage::Scrap(amount)) = view.known_salvage(tile) else {
        unreachable!()
    };
    let mut input = InputState::new();
    input.mouse = game
        .presentation
        .camera
        .to_screen(vec2(tile.x as f32 + 0.5, tile.y as f32 + 0.5));
    let lay_out = |input: &InputState| {
        layout(
            &game.view(),
            input,
            &BindingMap::classic(),
            env(1280.0, 800.0),
            None,
            &fixed,
        )
    };
    let hud = lay_out(&input);
    let chip = hud
        .geometry
        .salvage_chip
        .expect("hovered scrap names itself");
    assert_eq!(chip.text, format!("scrap {amount}"));
    assert_eq!(chip.at, input.mouse + vec2(14.0, -10.0));

    let mut covered = hud.layout;
    covered.panel_regions[0] = Rect::new(input.mouse.x - 8.0, input.mouse.y - 8.0, 16.0, 16.0);
    assert_eq!(
        salvage_chip(&game.view(), &covered, Some(input.mouse), 1.0, &fixed),
        None,
        "nothing shows through chrome over the scrap"
    );
    assert_eq!(
        salvage_chip(&game.view(), &hud.layout, None, 1.0, &fixed),
        None,
        "nor without a pointer"
    );
}
