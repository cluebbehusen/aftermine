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
    Game::with_viewport(oxide_sim::Scenario::skirmish(), vec2(1280.0, 800.0)).unwrap()
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
        assert!(model.menu_button.w > 0.0 && model.pause_status.w > 0.0);
    }
}

#[test]
fn a_spectator_has_no_top_bar_controls_but_keeps_the_minimap() {
    let mut game = skirmish();
    game.presentation.spectate = true;
    let hud = layout(
        &game.view(),
        &InputState::new(),
        &BindingMap::classic(),
        env(1280.0, 800.0),
        None,
        &fixed,
    );
    assert_eq!(hud.layout.menu_button.w, 0.0);
    assert_eq!(hud.layout.pause_status.w, 0.0);
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
        .find(|unit| unit.player == human && unit.kind == oxide_sim::UnitKind::Harvester)
        .unwrap();
    let (id, home) = (
        harvester.id,
        chassis::grid::TilePos::containing(harvester.pos),
    );
    for (dx, queue) in [(4, false), (6, true)] {
        game.state.tick(&[oxide_sim::PlayerCommand {
            player: human,
            command: oxide_sim::Command::Run {
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
        .find(|unit| unit.player == human && unit.kind == oxide_sim::UnitKind::Harvester)
        .unwrap();
    let (id, home) = (
        harvester.id,
        chassis::grid::TilePos::containing(harvester.pos),
    );
    for (dx, queue) in [(4, false), (6, true)] {
        game.state.tick(&[oxide_sim::PlayerCommand {
            player: human,
            command: oxide_sim::Command::Run {
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
    input.arm_placement(oxide_sim::BuildingKind::Turret);
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
fn a_finger_with_no_ghost_down_leaves_the_ribbon_one_row_tall() {
    let mut game = skirmish();
    let human = game.presentation.human;
    let harvester = game
        .state
        .units()
        .iter()
        .find(|unit| unit.player == human && unit.kind == oxide_sim::UnitKind::Harvester)
        .unwrap()
        .id;
    game.presentation.selection.units.push(harvester);
    let mut input = InputState::new();
    input.build_menu = true;
    input.arm_placement(oxide_sim::BuildingKind::Turret);
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
        .find(|unit| unit.player == human && unit.kind == oxide_sim::UnitKind::Harvester)
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
    input.arm_placement(oxide_sim::BuildingKind::Barricade);
    let at = |dx: i32| {
        game.presentation.camera.to_screen(vec2(
            (foundry.x + dx) as f32 + 0.5,
            (foundry.y + 3) as f32 + 0.5,
        ))
    };
    let (a, b, c) = (at(3), at(4), at(5));
    let events = [
        oxide_protocol::RawEvent::MouseDown {
            button: oxide_protocol::MouseButton::Left,
            x: a.x,
            y: a.y,
        },
        oxide_protocol::RawEvent::MouseMove { x: b.x, y: b.y },
        oxide_protocol::RawEvent::MouseMove { x: c.x, y: c.y },
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
    let cost = oxide_sim::BuildingKind::Barricade
        .base_stats()
        .construction
        .unwrap()
        .cost;
    assert_eq!(ribbon.label, "Barricade \u{d7}3");
    assert_eq!(ribbon.cost, Some((3 * cost).to_string()));
    let status = ribbon.status.expect("the cursor rests on the row's end");
    assert_eq!(status.detail.as_deref(), Some("Queues as #3"));
}
