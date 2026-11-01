use super::*;

#[test]
fn hud_copy_names_keys_only_on_desktop() {
    use crate::game::AlertKind;
    assert_eq!(idle_badge_text(3, "F1", false), "3 idle [F1]");
    crate::platform::assert_touch_copy(&idle_badge_text(3, "F1", true));
    assert_eq!(
        alert_badge_text(AlertKind::Attack, "Tab", false),
        "under attack [Tab]"
    );
    assert_eq!(
        alert_badge_text(AlertKind::Attack, "", false),
        "under attack"
    );
    assert_eq!(
        alert_badge_text(
            AlertKind::Assembly(aftermine_sim::UnitKind::Maw),
            "Tab",
            false
        ),
        "enemy Maw assembling [Tab]"
    );
    crate::platform::assert_touch_copy(&alert_badge_text(AlertKind::Attack, "Tab", true));
    crate::platform::assert_touch_copy(&alert_badge_text(
        AlertKind::Assembly(aftermine_sim::UnitKind::Maw),
        "Tab",
        true,
    ));
    for hands in crate::platform::ALL_HANDS {
        let hint = concede_hint(hands);
        assert_eq!(hint.contains("{back}"), hands.keys, "{hint}");
        if !hands.keys {
            crate::platform::assert_keyless_copy(hint);
        }
        if hands.touch() {
            crate::platform::assert_mouseless_copy(hint);
        }
    }
}

#[test]
fn the_ribbon_stays_on_screen_above_the_panel() {
    for (viewport, scale) in [
        (vec2(1024.0, 768.0), 1.5),
        (vec2(1194.0, 834.0), 1.0),
        (vec2(1366.0, 1024.0), 1.25),
        (vec2(640.0, 400.0), 1.0),
    ] {
        for panel_top in [f32::INFINITY, viewport.y - 120.0 * scale] {
            for label in [60.0, 180.0, 320.0] {
                let w = ribbon_width(viewport, scale, label * scale);
                let ribbon = ribbon_geometry(
                    viewport,
                    scale,
                    panel_top,
                    (w, ribbon_height(scale, false)),
                    Rect::default(),
                );
                assert!(ribbon.x >= 0.0, "{viewport} @{scale}");
                assert!(ribbon.x + ribbon.w <= viewport.x);
                assert!(ribbon.y >= crate::layout::TOP_BAR_H * scale);
                assert!(ribbon.y + ribbon.h <= panel_top.min(viewport.y));
            }
        }
    }
}

#[test]
fn the_queue_toggle_sits_over_the_panel_corner_under_the_dock() {
    let viewport = vec2(1194.0, 834.0);
    for scale in [1.0, 1.25] {
        let info = Rect::new(0.0, 600.0, 300.0 * scale, 234.0);
        let actions = Rect::new(320.0 * scale, 650.0, 600.0, 184.0);
        let toggle = queue_toggle_rect(viewport, scale, &[info, actions]);
        assert!(toggle.x >= 0.0);
        assert!(toggle.y + toggle.h <= info.y, "above the panel it rests on");
        let dock_bottom = info.y - queue_dock_lift(scale);
        assert!(
            dock_bottom <= toggle.y - 8.0 * scale + 0.001,
            "the lifted dock clears it"
        );
        assert_eq!(
            toggle,
            queue_toggle_rect(viewport, scale, &[info, Rect::default()]),
            "the actions band never moves it"
        );
    }
}

#[test]
fn queue_is_offered_only_to_units_or_while_on() {
    let mut game =
        crate::game::Game::with_viewport(aftermine_sim::Scenario::skirmish(), vec2(1280.0, 800.0))
            .expect("skirmish builds");
    let mut input = InputState::new();
    let shown = |game: &crate::game::Game, input: &InputState| {
        queue_toggle_shown(&game.view(), input, true)
    };
    assert!(!shown(&game, &input), "nothing selected");
    let foundry = game
        .state
        .buildings()
        .iter()
        .find(|b| b.player == game.presentation.human)
        .expect("an own Foundry")
        .id;
    game.presentation.selection.buildings = vec![foundry];
    assert!(!shown(&game, &input), "buildings queue nothing");
    game.presentation.selection.buildings.clear();
    let own = game
        .state
        .units()
        .iter()
        .find(|u| u.player == game.presentation.human)
        .expect("an own unit")
        .id;
    game.presentation.selection.units = vec![own];
    assert!(shown(&game, &input), "an own unit");
    assert!(
        !queue_toggle_shown(&game.view(), &input, false),
        "desktop has Shift"
    );
    game.presentation.selection.units.clear();
    input.queue_toggle = crate::input::QueueToggle::Locked;
    assert!(shown(&game, &input), "QUEUE on can always be turned off");
    game.presentation.stance = crate::game::Stance::Spectator;
    assert!(!shown(&game, &input), "never while spectating");
}

#[test]
fn the_ribbon_row_slides_clear_of_the_minimap() {
    let viewport = vec2(1024.0, 768.0);
    let scale = 1.5;
    let minimap = Rect::new(676.0, 520.0, 330.0, 230.0);
    let w = ribbon_width(viewport, scale, 200.0);
    let ribbon = ribbon_geometry(
        viewport,
        scale,
        f32::INFINITY,
        (w, ribbon_height(scale, false)),
        minimap,
    );
    assert!(
        ribbon.x + ribbon.w <= minimap.x,
        "the row stops short of the minimap"
    );
}

#[test]
fn a_touch_device_never_reads_at_a_stale_mouse_point() {
    let mut input = InputState::new();
    input.mouse = vec2(400.0, 300.0);
    input.last_pointer = crate::input::Pointer::Mouse;
    assert_eq!(hover_point(&input), Some(vec2(400.0, 300.0)));
    input.last_pointer = crate::input::Pointer::Touch;
    assert_eq!(hover_point(&input), None);
}

#[test]
fn the_paused_status_names_a_key_only_where_one_exists() {
    assert_eq!(paused_status("P", false), "PAUSED [P]");
    assert_eq!(paused_status("P", true), "PAUSED");
    assert_eq!(paused_status("", false), "PAUSED");
}

#[test]
fn toasts_clear_the_panel_and_its_orders_dock() {
    let viewport = vec2(640.0, 400.0);
    let panel_top = 128.0;
    let orders = Rect::new(0.0, 52.0, 400.0, 76.0);
    for index in 0..3 {
        let origin = toast_origin(viewport, 1.0, panel_top, orders, Rect::default(), index);
        assert!(origin.x > orders.x + orders.w);
        assert!(origin.y < panel_top);
        assert!(origin.y >= crate::layout::TOP_BAR_H + 18.0);
    }
}

#[test]
fn a_long_toast_wraps_short_of_the_minimap_at_the_layout_floor() {
    let viewport = vec2(1133.0, 744.0);
    let minimap = minimap_rect_scaled(40, 24, viewport, 1.0);
    let orders = Rect::new(0.0, 400.0, 400.0, 260.0);
    let origin = toast_origin(viewport, 1.0, 660.0, orders, Rect::default(), 0);
    let right = toast_right(
        viewport.x,
        1.0,
        origin.y - 94.0,
        origin.y + 6.0,
        &[minimap, Rect::default()],
    );
    assert_eq!(right, minimap.x - 12.0);
    let measure = |text: &str, size: f32| text.chars().count() as f32 * size * 0.5;
    let text = "A toast this long wraps onto more lines instead of running under the minimap";
    let (size, lines) = toast_lines(text, right - origin.x, 1.0, measure);
    assert_eq!(size, crate::theme::Type::Body.at(1.0));
    assert!(lines.len() > 1);
    assert_eq!(lines.join(" "), text);
    assert!(
        lines
            .iter()
            .all(|line| measure(line, size) <= right - origin.x)
    );
    assert_eq!(
        toast_lines("Queue is full", right - origin.x, 1.0, measure),
        (
            crate::theme::Type::Label.at(1.0),
            vec!["Queue is full".to_string()]
        )
    );
}

#[test]
fn chrome_clear_of_the_toast_rows_leaves_the_window_edge_free() {
    let above = Rect::new(1000.0, 60.0, 100.0, 200.0);
    assert_eq!(toast_right(1133.0, 1.0, 560.0, 660.0, &[above]), 1121.0);
}

#[test]
fn toasts_stack_above_the_ribbon_row() {
    let viewport = vec2(1280.0, 800.0);
    let panel_top = 640.0;
    let ribbon = ribbon_geometry(
        viewport,
        1.0,
        panel_top,
        (240.0, ribbon_height(1.0, true)),
        Rect::default(),
    );
    for index in 0..3 {
        let origin = toast_origin(viewport, 1.0, panel_top, Rect::default(), ribbon, index);
        assert!(origin.y < ribbon.y, "toast {index} clears the row");
    }
}
