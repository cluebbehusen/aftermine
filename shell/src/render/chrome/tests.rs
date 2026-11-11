use super::*;

#[test]
fn hud_copy_names_keys_only_on_desktop() {
    use crate::game::AlertKind;
    assert_eq!(idle_badge_text(3, "F1", false), "3 idle [F1]");
    crate::platform::tests::assert_touch_copy(&idle_badge_text(3, "F1", true));
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
    crate::platform::tests::assert_touch_copy(&alert_badge_text(AlertKind::Attack, "Tab", true));
    crate::platform::tests::assert_touch_copy(&alert_badge_text(
        AlertKind::Assembly(aftermine_sim::UnitKind::Maw),
        "Tab",
        true,
    ));
    for hands in crate::platform::tests::ALL_HANDS {
        let hint = concede_hint(hands);
        assert_eq!(hint.contains("{back}"), hands.keys, "{hint}");
        if !hands.keys {
            crate::platform::tests::assert_keyless_copy(hint);
        }
        if hands.touch() {
            crate::platform::tests::assert_mouseless_copy(hint);
        }
    }
    // A controller names its own Menu button, since pad mode hides the
    // top bar's.
    for hands in crate::platform::tests::pad_hands() {
        let hint = concede_hint(hands);
        assert!(hint.contains("{menu}"), "{hint}");
        crate::platform::tests::assert_touch_copy(&hint.replace("{menu}", "Menu"));
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
    assert_eq!(input.chrome_hover_point(), Some(vec2(400.0, 300.0)));
    input.last_pointer = crate::input::Pointer::Touch;
    assert_eq!(input.chrome_hover_point(), None);
}

#[test]
fn the_paused_status_names_a_key_only_where_one_exists() {
    assert_eq!(paused_status("P", false), "PAUSED [P]");
    assert_eq!(paused_status("P", true), "PAUSED");
    assert_eq!(paused_status("", false), "PAUSED");
}

#[test]
fn the_top_bar_shows_income_before_anything_earns() {
    let game =
        crate::game::Game::with_viewport(aftermine_sim::Scenario::skirmish(), vec2(1280.0, 800.0))
            .unwrap();
    let view = game.view();
    assert_eq!(crate::panel::passive_income(&view), 0);
    let measure = |_: super::super::hud::Face, text: &str, size: f32| {
        text.chars().count() as f32 * size * 0.5
    };
    let top = top_bar_layout(
        &view,
        &crate::action::BindingMap::classic(),
        super::super::hud::HudEnv {
            viewport: vec2(1280.0, 800.0),
            ui: 1.0,
        },
        &measure,
        crate::input::HudHands::Pointer,
    );
    assert_eq!(top.passive, "+0/min income");
}

#[test]
fn in_pad_mode_the_running_clock_makes_room_for_view() {
    let game =
        crate::game::Game::with_viewport(aftermine_sim::Scenario::skirmish(), vec2(1280.0, 800.0))
            .unwrap();
    let view = game.view();
    assert!(!view.clock.paused);
    let measure = |_: super::super::hud::Face, text: &str, size: f32| {
        text.chars().count() as f32 * size * 0.5
    };
    let env = super::super::hud::HudEnv {
        viewport: vec2(1280.0, 800.0),
        ui: 1.0,
    };
    let bindings = crate::action::BindingMap::classic();
    let lay = |hud| top_bar_layout(&view, &bindings, env, &measure, hud);
    let mouse = lay(crate::input::HudHands::Pointer);
    let pad = lay(crate::input::HudHands::Pad {
        names: Some(aftermine_protocol::PadFamily::Xbox),
    });
    assert_eq!(mouse.status, pad.status, "the same clock");
    let icon = crate::hint_line::icon_width(crate::theme::Type::Small.at(1.0));
    assert!(
        mouse.bar.status_x - pad.bar.status_x >= icon,
        "the clock leaves room for View's icon, as PAUSED does"
    );
    let unnamed = lay(crate::input::HudHands::Pad { names: None });
    assert_eq!(
        unnamed.bar.status_x, mouse.bar.status_x,
        "with no controller to name, no icon needs room"
    );
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

#[test]
fn army_tooltips_say_what_a_press_selects() {
    use crate::layout::ArmyReach;
    assert_eq!(
        army_tooltip(ArmyReach::All, 18, "Ctrl+A", false),
        "Select all 18 army units   [Ctrl+A]"
    );
    assert_eq!(
        army_tooltip(ArmyReach::InView, 14, "Ctrl+S", false),
        "Select the 14 army units in view   [Ctrl+S]"
    );
    assert_eq!(
        army_tooltip(ArmyReach::All, 1, "", false),
        "Select your 1 army unit"
    );
    assert_eq!(
        army_tooltip(ArmyReach::InView, 1, "Ctrl+S", true),
        "Select the 1 army unit in view"
    );
    crate::platform::tests::assert_touch_copy(&army_tooltip(ArmyReach::InView, 3, "Ctrl+S", true));
    assert_eq!(army_tooltip(ArmyReach::All, 0, "", true), "No army units");
    assert_eq!(
        army_tooltip(ArmyReach::InView, 0, "", true),
        "No army units in view"
    );
}

#[test]
fn the_queue_flag_shows_while_orders_queue_for_own_units() {
    use crate::input::QueueToggle;
    use aftermine_protocol::{Key, PadButton, RawEvent};
    let mut game =
        crate::game::Game::with_viewport(aftermine_sim::Scenario::skirmish(), vec2(1280.0, 800.0))
            .unwrap();
    let bindings = crate::action::BindingMap::classic();
    let mut input = InputState::new();
    let measure = |_: super::super::hud::Face, text: &str, size: f32| {
        text.chars().count() as f32 * size * 0.5
    };
    let env = super::super::hud::HudEnv {
        viewport: vec2(1280.0, 800.0),
        ui: 1.0,
    };
    let flag = |game: &crate::game::Game, input: &InputState| {
        queue_flag_layout(&game.view(), input, env, Rect::default(), &measure)
    };
    let feed = |game: &mut crate::game::Game, input: &mut InputState, event: RawEvent| {
        crate::input::apply_events(game, input, &bindings, &[event], |_, _| {});
    };
    let human = game.presentation.human;
    let own = game
        .state
        .units()
        .iter()
        .find(|u| u.player == human)
        .expect("an own unit")
        .id;
    let foundry = game
        .state
        .buildings()
        .iter()
        .find(|b| b.player == human)
        .expect("an own Foundry")
        .id;

    game.presentation.selection.buildings = vec![foundry];
    feed(&mut game, &mut input, RawEvent::KeyDown { key: Key::Shift });
    assert_eq!(
        flag(&game, &input),
        None,
        "Shift with a structure is a hotkey or an added pick, not a queue"
    );
    game.presentation.selection.buildings.clear();
    game.presentation.selection.units = vec![own];
    let held = flag(&game, &input).expect("Shift queues a unit's orders");
    assert_eq!(held.span, QueueSpan::On);
    assert_eq!(
        held.rule,
        Rect::new(0.0, crate::layout::TOP_BAR_H, 1280.0, 3.0)
    );
    assert_eq!(
        held.tab.y,
        held.rule.y + held.rule.h,
        "the tab hangs from the rule"
    );
    assert!((held.tab.center().x - 640.0).abs() <= 1.0, "centred");
    feed(&mut game, &mut input, RawEvent::KeyUp { key: Key::Shift });
    assert_eq!(flag(&game, &input), None);

    feed(
        &mut game,
        &mut input,
        RawEvent::PadDown {
            pad: 0,
            button: PadButton::LeftTrigger,
        },
    );
    assert_eq!(flag(&game, &input).map(|f| f.span), Some(QueueSpan::On));
    feed(
        &mut game,
        &mut input,
        RawEvent::PadUp {
            pad: 0,
            button: PadButton::LeftTrigger,
        },
    );
    assert_eq!(flag(&game, &input), None);

    input.queue_toggle = QueueToggle::Latched { at: 0.0 };
    let once = flag(&game, &input).expect("a latched QUEUE");
    assert_eq!(once.span, QueueSpan::Once);
    assert_eq!(once.tab, held.tab, "the tab holds still as QUEUE locks");
    input.queue_toggle = QueueToggle::Locked;
    game.presentation.selection.units.clear();
    assert_eq!(
        flag(&game, &input).map(|f| f.span),
        Some(QueueSpan::On),
        "a lit QUEUE flags itself as long as its toggle shows"
    );

    game.presentation.net_notice = Some("Waiting for the host".to_string());
    let under = flag(&game, &input).expect("still queuing");
    assert!(
        under.tab.y >= crate::layout::TOP_BAR_H + NET_NOTICE_H,
        "the tab hangs below a LAN notice"
    );
    game.presentation.stance = crate::game::Stance::Spectator;
    assert_eq!(flag(&game, &input), None, "a spectator queues nothing");
}

#[test]
fn the_queue_tab_keeps_clear_of_a_lesson_or_tip_card() {
    use crate::render::tutorial_card::CornerCard;
    let game =
        crate::game::Game::with_viewport(aftermine_sim::Scenario::skirmish(), vec2(1133.0, 744.0))
            .unwrap();
    let mut input = InputState::new();
    input.queue_toggle = crate::input::QueueToggle::Locked;
    let measure = |_: super::super::hud::Face, text: &str, size: f32| {
        text.chars().count() as f32 * size * 0.5
    };
    // iPad mini at the default scale, and an 11-inch iPad at 125%.
    for (w, h, ui) in [(1133.0, 744.0, 1.0), (1194.0, 834.0, 1.25)] {
        let env = super::super::hud::HudEnv {
            viewport: vec2(w, h),
            ui,
        };
        let mut view = game.view();
        let open =
            queue_flag_layout(&view, &input, env, Rect::default(), &measure).expect("QUEUE is on");
        assert!((open.tab.center().x - w * 0.5).abs() <= 1.0, "centred");
        for card in [CornerCard::Lesson, CornerCard::Tip] {
            view.corner_card = Some(card);
            let tab = queue_flag_layout(&view, &input, env, Rect::default(), &measure)
                .expect("QUEUE is on")
                .tab;
            assert!(
                tab.x >= card.right(env) && tab.right() <= w,
                "{card:?} at {w}x{h} x{ui}: {tab:?}"
            );
        }
    }
}

#[test]
fn the_queue_tab_keeps_clear_of_the_performance_readout() {
    use crate::render::tutorial_card::CornerCard;
    let game =
        crate::game::Game::with_viewport(aftermine_sim::Scenario::skirmish(), vec2(1133.0, 744.0))
            .unwrap();
    let mut input = InputState::new();
    input.queue_toggle = crate::input::QueueToggle::Locked;
    let measure = |_: super::super::hud::Face, text: &str, size: f32| {
        text.chars().count() as f32 * size * 0.5
    };
    let mut view = game.view();
    // Room between the centre and a readout: the tab stays centred.
    let env = super::super::hud::HudEnv {
        viewport: vec2(1133.0, 744.0),
        ui: 1.0,
    };
    let readout = Rect::new(881.0, 46.0, 240.0, 120.0);
    let tab = queue_flag_layout(&view, &input, env, readout, &measure)
        .expect("QUEUE is on")
        .tab;
    assert!((tab.center().x - 566.5).abs() <= 1.0, "centred: {tab:?}");
    // A lesson card at 150% leaves no room beside the readout: the tab
    // hangs under it instead of hiding behind it.
    let env = super::super::hud::HudEnv {
        viewport: vec2(1133.0, 744.0),
        ui: 1.5,
    };
    view.corner_card = Some(CornerCard::Lesson);
    let readout = Rect::new(755.0, 69.0, 360.0, 180.0);
    let tab = queue_flag_layout(&view, &input, env, readout, &measure)
        .expect("QUEUE is on")
        .tab;
    assert!(
        tab.y >= readout.bottom() && tab.x >= readout.x && tab.right() <= readout.right(),
        "{tab:?} under {readout:?}"
    );
}
