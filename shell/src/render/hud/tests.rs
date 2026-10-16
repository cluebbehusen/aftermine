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
