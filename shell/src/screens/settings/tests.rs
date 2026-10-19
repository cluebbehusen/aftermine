use super::*;
use macroquad::prelude::vec2;

/// Settings opened on `tab`.
fn on(tab: Tab, config: &Config) -> SettingsScreen {
    let mut screen = SettingsScreen::open(config);
    screen.show(tab, config);
    screen
}

/// Where `item` sits in the open tab's menu.
fn index(screen: &SettingsScreen, item: Item) -> usize {
    screen
        .menu
        .lines()
        .iter()
        .position(|line| matches!(line, Line::Row(_, at) if *at == item))
        .unwrap_or_else(|| panic!("{item:?} is not on {:?}", screen.face))
}

/// The row `item` as automation reads it.
fn label(screen: &SettingsScreen, item: Item) -> String {
    screen.menu.view().items[index(screen, item)].clone()
}

/// Puts the cursor on `item`.
fn focus(screen: &mut SettingsScreen, item: Item) {
    let at = index(screen, item);
    screen.menu.select(at);
}

fn drive(s: &mut SettingsScreen, config: &mut Config, events: &[RawEvent], ctrl0: bool) -> Update {
    let mut mouse = vec2(0.0, 0.0);
    let mut sounds = Vec::new();
    s.update(events, &mut mouse, &mut sounds, config, ctrl0, false)
}

fn press(key: Key) -> Vec<RawEvent> {
    vec![RawEvent::KeyDown { key }, RawEvent::KeyUp { key }]
}

fn tap(at: Vec2) -> Vec<RawEvent> {
    vec![
        RawEvent::TouchDown {
            id: 1,
            x: at.x,
            y: at.y,
        },
        RawEvent::TouchUp {
            id: 1,
            x: at.x,
            y: at.y,
        },
    ]
}

fn click(at: Vec2) -> Vec<RawEvent> {
    use oxide_protocol::MouseButton;
    vec![
        RawEvent::MouseDown {
            button: MouseButton::Left,
            x: at.x,
            y: at.y,
        },
        RawEvent::MouseUp {
            button: MouseButton::Left,
            x: at.x,
            y: at.y,
        },
    ]
}

/// Activates the focused profile row, then confirms the dialog it
/// raises.
fn apply_focused_profile(s: &mut SettingsScreen, config: &mut Config) -> Update {
    drive(s, config, &press(Key::Enter), false);
    assert!(
        matches!(s.face, Face::Confirming(_)),
        "a profile row asks first"
    );
    drive(s, config, &press(Key::Down), false);
    drive(s, config, &press(Key::Enter), false)
}

#[test]
fn a_capture_prompt_is_information_and_key_help_is_coaching() {
    let config = Config::default();
    let mut screen = SettingsScreen::open(&config);
    assert_eq!(screen.status(), None);
    assert!(
        screen.coaching().is_some(),
        "the settings help waits until stuck"
    );
    screen.show(Tab::Controls, &config);
    assert!(screen.coaching().is_some(), "the controls key help too");
    screen.face = Face::Capturing(Action::PanUp);
    screen.notice = Some(Notice {
        text: "a delivered notice".to_string(),
        danger: false,
    });
    assert_eq!(
        screen.status(),
        Some((CAPTURE_PROMPT, false)),
        "an armed capture always says it is listening"
    );
    assert_eq!(screen.coaching(), None);
}

#[test]
fn the_settings_hint_names_only_what_the_hands_in_use_have() {
    for hands in crate::platform::ALL_HANDS {
        for tab_keys in [true, false] {
            let hint = settings_hint(hands, tab_keys);
            crate::platform::assert_copy_fits(hands, hint);
            assert_eq!(hint.contains("{confirm}"), hands.keys, "{hint}");
            assert_eq!(
                hint.contains("{prev_tab}"),
                hands.keys && tab_keys,
                "an unbound tab key is never named: {hint}"
            );
        }
    }
}

#[test]
fn every_row_lives_on_one_tab_and_touch_hides_only_what_it_cannot_use() {
    let offered = |touch_only| -> Vec<Row> {
        Tab::ALL
            .into_iter()
            .flat_map(|tab| rows(tab, touch_only))
            .collect()
    };
    assert_eq!(
        offered(false),
        Row::ALL.to_vec(),
        "desktop shows every row once, in tab order"
    );
    let touch = offered(true);
    assert!(!touch.contains(&Row::OpenDiagnostics), "no file manager");
    for kept in [Row::EdgePan, Row::LeftHandedPreset] {
        assert!(
            touch.contains(&kept),
            "{kept:?} serves an attached keyboard or trackpad"
        );
    }
    assert_eq!(touch.len(), Row::ALL.len() - 1);
    assert!(Tab::ALL.into_iter().all(|tab| !rows(tab, true).is_empty()));
}

#[test]
fn each_tab_reports_its_own_mode_and_only_controls_edits_bindings() {
    let config = Config::default();
    let mut screen = SettingsScreen::open(&config);
    let mut seen = Vec::new();
    for tab in Tab::ALL {
        screen.show(tab, &config);
        seen.push(screen.mode_name());
        assert_eq!(screen.edits_bindings(), tab == Tab::Controls, "{tab:?}");
    }
    assert_eq!(
        seen,
        ["settings", "settings_audio", "settings_display", "controls"]
    );
    screen.face = Face::Capturing(Action::PanUp);
    assert!(screen.edits_bindings(), "a capture reads raw keys");
    assert_eq!(screen.mode_name(), "controls");
}

#[test]
fn tabs_step_by_key_wrap_at_the_ends_and_answer_clicks_and_taps() {
    crate::render::set_viewport(1280.0, 800.0);
    crate::render::set_user_scale(1.0);
    let mut config = Config::default();
    let mut screen = SettingsScreen::open(&config);
    drive(&mut screen, &mut config, &press(Key::E), false);
    assert_eq!(screen.face, Face::Tab(Tab::Audio));
    drive(&mut screen, &mut config, &press(Key::Q), false);
    drive(&mut screen, &mut config, &press(Key::Q), false);
    assert_eq!(
        screen.face,
        Face::Tab(Tab::Controls),
        "Q wraps off the left"
    );
    drive(&mut screen, &mut config, &press(Key::E), false);
    assert_eq!(
        screen.face,
        Face::Tab(Tab::General),
        "E wraps off the right"
    );

    let strip = crate::tabs::rects(Tab::ALL.len());
    let up = drive(&mut screen, &mut config, &click(strip[2].center()), false);
    assert_eq!(screen.face, Face::Tab(Tab::Display));
    assert!(!up.dirty, "a tab switch edits nothing");
    drive(&mut screen, &mut config, &tap(strip[1].center()), false);
    assert_eq!(screen.face, Face::Tab(Tab::Audio));
    assert_eq!(
        screen.menu.selected,
        index(&screen, Item::Setting(Row::MasterVolume)),
        "a tab opens on its first row"
    );
}

#[test]
fn the_binding_editor_switches_tabs_on_q_e_and_the_players_own_tab_chords() {
    let mut config = Config::default();
    assert!(
        config
            .bindings
            .rebind_slot(Action::PrevTab, 0, Chord::bare(Key::Z))
    );
    let mut screen = on(Tab::Controls, &config);
    drive(&mut screen, &mut config, &press(Key::Z), false);
    assert_eq!(screen.face, Face::Tab(Tab::Display), "the player's chord");
    screen.show(Tab::Controls, &config);
    drive(&mut screen, &mut config, &press(Key::E), false);
    assert_eq!(
        screen.face,
        Face::Tab(Tab::General),
        "the classic key still works where keys are raw"
    );
}

#[test]
fn a_capture_takes_the_tab_keys_as_its_answer() {
    let mut config = Config::default();
    let mut screen = on(Tab::Controls, &config);
    focus(&mut screen, Item::Binding(Action::Patrol));
    drive(&mut screen, &mut config, &press(Key::Enter), false);
    assert_eq!(screen.face, Face::Capturing(Action::Patrol));
    drive(&mut screen, &mut config, &press(Key::E), false);
    assert_eq!(
        screen.face,
        Face::Tab(Tab::Controls),
        "the capture answered; the tab stayed"
    );
}

#[test]
fn left_and_right_stop_at_the_ends_while_enter_wraps() {
    let mut config = Config::default();
    let mut screen = on(Tab::Audio, &config);
    focus(&mut screen, Item::Setting(Row::MasterVolume));
    assert_eq!(config.volumes.master, 1.0);
    let up = drive(&mut screen, &mut config, &press(Key::Right), false);
    assert!(!up.dirty, "already at the top");
    assert_eq!(config.volumes.master, 1.0);
    let up = drive(&mut screen, &mut config, &press(Key::Left), false);
    assert!(up.dirty);
    assert_eq!(config.volumes.master, 0.75);
    assert_eq!(
        label(&screen, Item::Setting(Row::MasterVolume)),
        "Master volume: 75%"
    );
    for _ in 0..5 {
        drive(&mut screen, &mut config, &press(Key::Left), false);
    }
    assert_eq!(config.volumes.master, 0.0, "Left stops at silence");
    drive(&mut screen, &mut config, &press(Key::Enter), false);
    assert_eq!(config.volumes.master, 0.25, "Enter steps up");

    screen.show(Tab::Display, &config);
    focus(&mut screen, Item::Setting(Row::MarkerTiming));
    drive(&mut screen, &mut config, &press(Key::Left), false);
    assert_eq!(config.markers.timing_label(), "Later");
    drive(&mut screen, &mut config, &press(Key::Left), false);
    assert_eq!(config.markers.timing_label(), "Later");
    focus(&mut screen, Item::Setting(Row::ReducedMotion));
    drive(&mut screen, &mut config, &press(Key::Right), false);
    assert!(config.reduced_motion, "Right turns a toggle on");
    drive(&mut screen, &mut config, &press(Key::Right), false);
    assert!(config.reduced_motion, "and keeps it on");
    drive(&mut screen, &mut config, &press(Key::Left), false);
    assert!(!config.reduced_motion);
}

#[test]
fn a_value_between_the_steps_moves_to_its_neighbour() {
    let mut config = Config::default();
    for (step, landed) in [
        (Step::Back, 0.5),
        (Step::Forward, 0.75),
        (Step::Cycle, 0.75),
    ] {
        config.volumes.music = 0.6;
        assert!(step_setting(&mut config, Row::MusicVolume, step));
        assert_eq!(config.volumes.music, landed, "{step:?}");
    }
    config.ui_scale = 0.9;
    step_setting(&mut config, Row::UiScale, Step::Back);
    assert_eq!(config.ui_scale, 0.75, "never past the level below");
    crate::render::set_user_scale(1.0);
    config.markers.start = 33.0;
    assert_eq!(config.markers.timing_label(), "Custom");
    assert!(step_setting(&mut config, Row::MarkerTiming, Step::Cycle));
    assert_eq!(config.markers.timing_label(), "Standard");
    assert!(!step_setting(&mut config, Row::PlayerName, Step::Cycle));
}

#[test]
fn marker_settings_apply_live_and_remain_touch_reachable_in_small_windows() {
    crate::render::set_viewport(640.0, 400.0);
    crate::render::set_user_scale(1.5);
    let mut config = Config::default();
    let mut screen = on(Tab::Display, &config);
    focus(&mut screen, Item::Setting(Row::MarkerTiming));
    for (name, midpoint) in [("Earlier", 26.0), ("Later", 14.0), ("Standard", 20.0)] {
        let update = drive(&mut screen, &mut config, &press(Key::Enter), false);
        assert!(update.dirty);
        assert!(label(&screen, Item::Setting(Row::MarkerTiming)).ends_with(name));
        assert_eq!(crate::strategic_markers::marker_alpha(midpoint), 0.5);
    }
    focus(&mut screen, Item::Setting(Row::MarkerSize));
    for size in [1.25, 1.5, 0.75, 1.0] {
        let rect = screen
            .menu
            .item_rect(index(&screen, Item::Setting(Row::MarkerSize)))
            .expect("selected size row visible");
        let update = drive(&mut screen, &mut config, &tap(rect.center()), false);
        assert!(update.dirty);
        assert_eq!(config.markers.scale, size);
        assert_eq!(crate::strategic_markers::prefs(), config.markers);
    }
    crate::render::set_viewport(1280.0, 800.0);
    crate::render::set_user_scale(1.0);
}

#[test]
fn the_back_button_cancels_a_capture_then_leaves() {
    let mut config = Config::default();
    let mut screen = on(Tab::Controls, &config);
    assert!(!screen.menu.view().items.iter().any(|item| item == "Back"));
    let back = crate::button::press_back(true);

    screen.face = Face::Capturing(Action::PanUp);
    let update = drive(&mut screen, &mut config, &back, false);
    assert_eq!(screen.face, Face::Tab(Tab::Controls), "cancels capture");
    assert!(!update.dirty, "a cancelled capture binds nothing");
    assert_eq!(config.bindings, BindingMap::classic());

    let update = drive(&mut screen, &mut config, &back, false);
    assert_eq!(update.out, Out::Leave);
}

#[test]
fn escape_leaves_from_any_tab_but_first_cancels_a_capture() {
    let mut config = Config::default();
    for tab in Tab::ALL {
        let mut screen = on(tab, &config);
        let update = drive(&mut screen, &mut config, &press(Key::Escape), false);
        assert_eq!(update.out, Out::Leave, "{tab:?}");
    }
    let mut screen = on(Tab::Controls, &config);
    focus(&mut screen, Item::Binding(Action::Patrol));
    drive(&mut screen, &mut config, &press(Key::Enter), false);
    let update = drive(&mut screen, &mut config, &press(Key::Escape), false);
    assert_eq!(update.out, Out::Stay);
    assert_eq!(screen.face, Face::Tab(Tab::Controls));
    assert_eq!(
        config.bindings.chord_for(Action::Patrol),
        BindingMap::classic().chord_for(Action::Patrol)
    );
}

#[test]
fn cycling_a_row_edits_the_config_and_reports_dirty() {
    let mut config = Config::default();
    let mut s = on(Tab::Display, &config);
    for _ in 0..index(&s, Item::Setting(Row::ReducedMotion)) {
        drive(&mut s, &mut config, &press(Key::Down), false);
    }
    let up = drive(&mut s, &mut config, &press(Key::Enter), false);
    assert!(config.reduced_motion);
    assert!(up.dirty, "the caller is told to persist");
    assert_eq!(
        s.menu.selected,
        index(&s, Item::Setting(Row::ReducedMotion)),
        "the cursor stays on the tuned row"
    );
}

#[test]
fn music_volume_is_a_live_persisted_settings_row() {
    let mut config = Config::default();
    let mut screen = on(Tab::Audio, &config);
    let music = index(&screen, Item::Setting(Row::MusicVolume));
    for _ in 0..music {
        drive(&mut screen, &mut config, &press(Key::Down), false);
    }
    let update = drive(&mut screen, &mut config, &press(Key::Enter), false);
    assert!(update.dirty);
    assert_eq!(config.volumes.music, 0.0);
    assert_eq!(screen.menu.selected, music);
    assert_eq!(screen.menu.view().items[music], "Music volume: 0%");
}

#[test]
fn music_volume_is_touch_reachable() {
    crate::render::set_viewport(1280.0, 800.0);
    crate::render::set_user_scale(1.0);
    let mut config = Config::default();
    let mut screen = on(Tab::Audio, &config);
    let music = index(&screen, Item::Setting(Row::MusicVolume));
    let row = screen.menu.item_rect(music).expect("music row is visible");
    // Another layout test must not move the row between measurement and input.
    std::thread::spawn(|| crate::render::set_user_scale(0.75))
        .join()
        .unwrap();
    let update = drive(&mut screen, &mut config, &tap(row.center()), false);
    assert!(update.dirty);
    assert_eq!(config.volumes.music, 0.0);
    assert_eq!(screen.menu.selected, music);
}

#[test]
fn the_name_row_opens_a_name_form_that_saves_a_valid_name() {
    crate::render::set_viewport(1280.0, 800.0);
    let mut config = Config::default();
    let mut s = SettingsScreen::open(&config);
    let name = Item::Setting(Row::PlayerName);
    assert_eq!(label(&s, name), "Name: not set");
    focus(&mut s, name);
    let up = drive(&mut s, &mut config, &press(Key::Enter), false);
    assert!(!up.dirty);
    assert_eq!(s.mode_name(), "player_name");
    assert!(!s.edits_bindings(), "the form edits text, not bindings");
    assert_eq!(s.keyboard(), Some(Keyboard::Text));
    assert_eq!(s.menu.view().items, vec!["_"]);

    let refused = drive(&mut s, &mut config, &press(Key::Enter), false);
    assert!(!refused.dirty, "a blank name keeps the form up");
    assert_eq!(s.mode_name(), "player_name");
    let typed: Vec<RawEvent> = " Connor ".chars().map(|ch| RawEvent::Text { ch }).collect();
    drive(&mut s, &mut config, &typed, false);
    let saved = drive(&mut s, &mut config, &press(Key::Enter), false);
    assert!(saved.dirty);
    assert_eq!(config.player_name.as_deref(), Some("Connor"));
    assert_eq!(s.mode_name(), "settings");
    assert_eq!(s.keyboard(), None);
    assert_eq!(label(&s, name), "Name: Connor");
    assert_eq!(s.menu.selected, index(&s, name), "back on the row");

    // Escape and the corner BACK both close the form without saving.
    drive(&mut s, &mut config, &press(Key::Enter), false);
    drive(&mut s, &mut config, &press(Key::Backspace), false);
    let up = drive(&mut s, &mut config, &press(Key::Escape), false);
    assert!(!up.dirty);
    assert_eq!(up.out, Out::Stay, "Escape leaves the form, not Settings");
    assert_eq!(config.player_name.as_deref(), Some("Connor"));
    drive(&mut s, &mut config, &press(Key::Enter), false);
    assert_eq!(s.menu.view().items, vec!["Connor_"], "prefilled");
    assert!(!s.take_keyboard_request());
    let back = crate::button::press_back(true);
    let up = drive(&mut s, &mut config, &back, false);
    assert_eq!(up.out, Out::Stay);
    assert_eq!(s.mode_name(), "settings");
}

#[test]
fn the_control_group_row_shows_and_hides_the_bar() {
    let mut config = Config::default();
    assert_eq!(
        Row::ControlGroups.label(&config).to_string(),
        "Control group bar: Shown"
    );
    step_setting(&mut config, Row::ControlGroups, Step::Cycle);
    assert!(!config.control_groups);
    assert_eq!(
        Row::ControlGroups.label(&config).to_string(),
        "Control group bar: Hidden"
    );
    assert!(!render::control_groups(), "the HUD follows at once");
    step_setting(&mut config, Row::ControlGroups, Step::Cycle);
    assert!(render::control_groups());
    assert!(rows(Tab::Display, true).contains(&Row::ControlGroups));
}

#[test]
fn performance_cycles_with_keyboard_mouse_and_touch_in_small_windows() {
    use crate::config::PerformanceDisplay;
    for scale in [0.75, 1.0, 1.25, 1.5] {
        crate::render::set_viewport(640.0, 400.0);
        crate::render::set_user_scale(scale);
        let mut config = Config::default();
        let mut screen = SettingsScreen::open(&config);
        let row = Item::Setting(Row::PerformanceDisplay);
        for _ in 0..index(&screen, row) {
            drive(&mut screen, &mut config, &press(Key::Down), false);
        }
        for mode in [
            PerformanceDisplay::Fps,
            PerformanceDisplay::Detailed,
            PerformanceDisplay::Off,
        ] {
            let rect = screen
                .menu
                .item_rect(index(&screen, row))
                .expect("selected row visible");
            let events = match mode {
                PerformanceDisplay::Fps => press(Key::Enter),
                PerformanceDisplay::Detailed => click(rect.center()),
                PerformanceDisplay::Off => tap(rect.center()),
            };
            let update = drive(&mut screen, &mut config, &events, false);
            assert!(update.dirty);
            assert_eq!(config.performance_display, mode);
            assert_eq!(screen.menu.selected, index(&screen, row));
            assert_eq!(
                label(&screen, row),
                format!("Performance readout: {}", mode.label())
            );
        }
    }
    crate::render::set_viewport(1280.0, 800.0);
    crate::render::set_user_scale(1.0);
}

#[test]
fn match_tips_start_on_and_switch_with_keyboard_mouse_and_touch() {
    let mut config = Config::default();
    assert!(config.match_tips);
    assert!(rows(Tab::General, true).contains(&Row::MatchTips));
    let mut screen = SettingsScreen::open(&config);
    let row = Item::Setting(Row::MatchTips);
    focus(&mut screen, row);
    for (on, how) in [(false, 0), (true, 1), (false, 2)] {
        let rect = screen
            .menu
            .item_rect(index(&screen, row))
            .expect("selected row visible");
        let events = match how {
            0 => press(Key::Enter),
            1 => click(rect.center()),
            _ => tap(rect.center()),
        };
        assert!(drive(&mut screen, &mut config, &events, false).dirty);
        assert_eq!(config.match_tips, on);
        assert_eq!(
            label(&screen, row),
            format!("Match tips: {}", if on { "On" } else { "Off" })
        );
    }
}

#[test]
fn a_held_modifier_from_another_screen_rides_into_the_captured_chord() {
    // Ctrl went down on another tab, so no Ctrl edge appears in this
    // frame's events; only the baseline knows. The capture must still
    // record Ctrl+K.
    let mut config = Config::default();
    let mut s = on(Tab::Controls, &config);
    focus(&mut s, Item::Binding(Action::Patrol));
    drive(&mut s, &mut config, &press(Key::Enter), true);
    assert!(matches!(s.face, Face::Capturing(_)));
    let up = drive(&mut s, &mut config, &press(Key::K), true);
    assert!(up.dirty);
    assert_eq!(
        config.bindings.chord_for(Action::Patrol),
        Some(Chord {
            key: Key::K,
            ctrl: true,
            shift: false
        }),
        "the held Ctrl rides into the chord"
    );
}

#[test]
fn a_chord_released_within_the_frame_still_reads_its_modifiers() {
    // The whole chord in one batch: Ctrl down, K down, K up, Ctrl up.
    // Batch-final modifier state is false, so only reading the modifiers
    // at the key's press gets this right.
    let mut config = Config::default();
    let mut s = on(Tab::Controls, &config);
    focus(&mut s, Item::Binding(Action::Patrol));
    drive(&mut s, &mut config, &press(Key::Enter), false);
    let batch = vec![
        RawEvent::KeyDown { key: Key::Ctrl },
        RawEvent::KeyDown { key: Key::K },
        RawEvent::KeyUp { key: Key::K },
        RawEvent::KeyUp { key: Key::Ctrl },
    ];
    drive(&mut s, &mut config, &batch, false);
    assert_eq!(
        config.bindings.chord_for(Action::Patrol),
        Some(Chord {
            key: Key::K,
            ctrl: true,
            shift: false
        })
    );
}

#[test]
fn a_conflicting_chord_is_refused_and_the_notice_names_the_holder() {
    let mut config = Config::default();
    let before = config.bindings.chord_for(Action::Patrol);
    let mut s = on(Tab::Controls, &config);
    focus(&mut s, Item::Binding(Action::Patrol));
    drive(&mut s, &mut config, &press(Key::Enter), false);
    let up = drive(&mut s, &mut config, &press(Key::G), false);
    assert!(!up.dirty);
    let notice = s.notice.as_ref().expect("the refusal reports");
    assert_eq!(notice.text, "G is already bound to Run");
    assert!(notice.danger);
    assert_eq!(config.bindings.chord_for(Action::Patrol), before);
    // Navigation is not an action: the notice waits to be read.
    drive(&mut s, &mut config, &press(Key::Down), false);
    assert!(s.notice.is_some(), "arrow keys must not eat the notice");
    // The next action clears it.
    drive(&mut s, &mut config, &press(Key::Enter), false);
    assert!(s.notice.is_none(), "arming a row is the next action");
}

#[test]
fn unbinding_after_a_refusal_retires_the_notice() {
    let mut config = Config::default();
    let mut s = on(Tab::Controls, &config);
    focus(&mut s, Item::Binding(Action::Patrol));
    drive(&mut s, &mut config, &press(Key::Enter), false);
    drive(&mut s, &mut config, &press(Key::G), false);
    assert!(s.notice.is_some());
    let up = drive(&mut s, &mut config, &press(Key::X), false);
    assert!(up.dirty);
    assert_eq!(config.bindings.chord_for(Action::Patrol), None);
    assert!(s.notice.is_none(), "the refusal no longer applies");
}

#[test]
fn the_strip_names_the_classic_tab_keys_while_the_tab_actions_are_unbound() {
    let mut bindings = BindingMap::classic();
    bindings.unbind_slot(Action::PrevTab, 0);
    bindings.unbind_slot(Action::NextTab, 0);
    crate::menu::set_bindings(&bindings);
    assert_eq!(
        crate::tabs::keys(),
        Some(("Q".to_string(), "E".to_string()))
    );
    assert!(
        crate::menu::binding_hint(settings_hint(crate::platform::ALL_HANDS[0], true))
            .contains("Q/E switch tabs")
    );
    crate::menu::set_bindings(&BindingMap::classic());
}

#[test]
fn a_conflict_with_a_non_remappable_holder_is_still_named() {
    // Enter is Confirm: not on the remap screen, but a reachable
    // collision whose holder must still be named.
    let mut config = Config::default();
    let mut s = on(Tab::Controls, &config);
    focus(&mut s, Item::Binding(Action::Patrol));
    drive(&mut s, &mut config, &press(Key::Enter), false);
    drive(&mut s, &mut config, &press(Key::Num1), false);
    assert_eq!(
        s.notice.as_ref().map(|n| n.text.as_str()),
        Some("1 is already bound to Recall group 1")
    );
}

#[test]
fn switching_tabs_clears_the_notice() {
    let mut config = Config::default();
    let mut s = on(Tab::Controls, &config);
    focus(&mut s, Item::Binding(Action::Patrol));
    drive(&mut s, &mut config, &press(Key::Enter), false);
    drive(&mut s, &mut config, &press(Key::G), false);
    assert!(s.notice.is_some());
    drive(&mut s, &mut config, &press(Key::Q), false);
    assert!(
        s.face == Face::Tab(Tab::Display) && s.notice.is_none(),
        "a tab change retires the notice with its context"
    );
}

#[test]
fn the_left_handed_preset_moves_the_verbs_and_reset_walks_home() {
    let mut config = Config::default();
    let mut s = on(Tab::Controls, &config);
    for _ in 0..index(&s, Item::Setting(Row::LeftHandedPreset)) {
        drive(&mut s, &mut config, &press(Key::Down), false);
    }
    let up = apply_focused_profile(&mut s, &mut config);
    assert!(up.dirty);
    assert!(
        s.notice.as_ref().is_some_and(|n| !n.danger),
        "the applied preset confirms on the screen's own notice line"
    );
    assert_eq!(
        config.bindings.chord_for(Action::TrainSlot(0)),
        Some(Chord::bare(Key::O)),
        "training moved to the right hand"
    );
    assert_eq!(
        config.bindings.chord_at(Action::PanLeft, 1),
        Some(Chord::bare(Key::Left)),
        "pans stay on the arrows"
    );
    assert_eq!(
        (
            config.bindings.chord_for(Action::PrevTab),
            config.bindings.chord_for(Action::NextTab)
        ),
        (Some(Chord::bare(Key::U)), Some(Chord::bare(Key::O))),
        "Previous tab stays the left key"
    );
    assert!(
        config.bindings.conflicts().is_empty(),
        "the preset must be conflict-free"
    );
    assert_eq!(
        config.bindings.chord_for(Action::Patrol),
        Some(Chord::bare(Key::Y))
    );
    focus(&mut s, Item::ResetBindings);
    apply_focused_profile(&mut s, &mut config);
    assert_eq!(config.bindings, BindingMap::classic());
}

#[test]
fn x_unbinds_and_reset_restores_the_classic_map() {
    let mut config = Config::default();
    let mut s = on(Tab::Controls, &config);
    focus(&mut s, Item::Binding(Action::Patrol));
    let up = drive(&mut s, &mut config, &press(Key::X), false);
    assert!(up.dirty);
    assert_eq!(config.bindings.chord_for(Action::Patrol), None);
    focus(&mut s, Item::ResetBindings);
    apply_focused_profile(&mut s, &mut config);
    assert_eq!(
        config.bindings.chord_for(Action::Patrol),
        BindingMap::classic().chord_for(Action::Patrol)
    );
}

#[test]
fn a_whole_profile_asks_first_and_every_way_out_keeps_the_rebinds() {
    crate::render::set_viewport(1280.0, 800.0);
    crate::render::set_user_scale(1.0);
    for profile in [Profile::Classic, Profile::LeftHanded] {
        let mut config = Config::default();
        assert!(config.bindings.rebind_slot(
            Action::Patrol,
            1,
            Chord {
                key: Key::K,
                ctrl: true,
                shift: true
            }
        ));
        let rebound = config.bindings.clone();
        let mut s = on(Tab::Controls, &config);
        for (way, events) in [
            ("Enter on the preselected Cancel", press(Key::Enter)),
            ("Escape", press(Key::Escape)),
            ("the BACK button", crate::button::press_back(true)),
        ] {
            focus(&mut s, profile.item());
            let up = drive(&mut s, &mut config, &press(Key::Enter), false);
            assert_eq!(s.face, Face::Confirming(profile));
            assert_eq!(s.mode_name(), "confirm_bindings");
            assert!(
                s.edits_bindings(),
                "the dialog answers the classic keys a broken map may lack"
            );
            assert_eq!(s.menu.value(), Some(&Item::Cancel), "Cancel is preselected");
            assert!(
                !up.dirty && config.bindings == rebound,
                "asking changes nothing"
            );
            let up = drive(&mut s, &mut config, &events, false);
            assert_eq!(s.face, Face::Tab(Tab::Controls), "{way} closes the dialog");
            assert_eq!(up.out, Out::Stay, "{way} stays in Settings");
            assert!(
                !up.dirty && config.bindings == rebound,
                "{way} keeps the rebinds"
            );
            assert_eq!(
                s.menu.value(),
                Some(&profile.item()),
                "{way} returns to the row that asked"
            );
        }
        let up = apply_focused_profile(&mut s, &mut config);
        assert!(up.dirty);
        assert_eq!(config.bindings, profile.bindings());
        assert_eq!(s.face, Face::Tab(Tab::Controls));
        assert_eq!(s.menu.value(), Some(&profile.item()));
        assert_eq!(
            s.notice.as_ref().map(|notice| notice.text.as_str()),
            Some(profile.applied()),
            "the notice line says the profile is in place"
        );
    }
}

#[test]
fn a_second_tap_on_a_profile_row_never_applies_it() {
    for (width, height) in [(1180.0, 820.0), (640.0, 400.0)] {
        crate::render::set_viewport(width, height);
        crate::render::set_user_scale(1.0);
        for profile in [Profile::Classic, Profile::LeftHanded] {
            let mut config = Config::default();
            assert!(config.bindings.rebind_slot(
                Action::Patrol,
                1,
                Chord {
                    key: Key::K,
                    ctrl: true,
                    shift: true
                }
            ));
            let rebound = config.bindings.clone();
            let mut s = on(Tab::Controls, &config);
            let row = s
                .menu
                .item_rect(index(&s, profile.item()))
                .expect("the profile rows open in view")
                .center();
            drive(&mut s, &mut config, &tap(row), false);
            assert_eq!(s.face, Face::Confirming(profile));
            let up = drive(&mut s, &mut config, &tap(row), false);
            assert!(
                !up.dirty && config.bindings == rebound,
                "the dialog's verb never sits under the row that raised it"
            );
        }
    }
}

#[test]
fn escape_outranks_a_confirm_picked_in_the_same_frame() {
    let escape = RawEvent::KeyDown { key: Key::Escape };
    let enter = RawEvent::KeyDown { key: Key::Enter };
    for events in [[escape, enter], [enter, escape]] {
        let mut config = Config::default();
        assert!(config.bindings.rebind_slot(
            Action::Patrol,
            1,
            Chord {
                key: Key::K,
                ctrl: true,
                shift: true
            }
        ));
        let rebound = config.bindings.clone();
        let mut s = on(Tab::Controls, &config);
        focus(&mut s, Item::ResetBindings);
        drive(&mut s, &mut config, &press(Key::Enter), false);
        drive(&mut s, &mut config, &press(Key::Down), false);
        assert_eq!(s.menu.value(), Some(&Item::Confirm));
        let up = drive(&mut s, &mut config, &events, false);
        assert_eq!(s.face, Face::Tab(Tab::Controls), "{events:?}");
        assert!(
            !up.dirty && config.bindings == rebound,
            "{events:?} keeps the rebinds"
        );
    }
}

#[test]
fn a_tab_held_while_settings_leaves_its_tabs_never_locks_the_strip() {
    crate::render::set_viewport(1180.0, 820.0);
    crate::render::set_user_scale(1.0);
    let strip = crate::tabs::rects(Tab::ALL.len());
    let held = strip[0].center();
    for item in [Item::ResetBindings, Item::Binding(Action::PanUp)] {
        let mut config = Config::default();
        let mut s = on(Tab::Controls, &config);
        let row = s
            .menu
            .item_rect(index(&s, item))
            .expect("the row opens in view")
            .center();
        let finger = |id: u64, at: Vec2, down: bool| {
            if down {
                RawEvent::TouchDown {
                    id,
                    x: at.x,
                    y: at.y,
                }
            } else {
                RawEvent::TouchUp {
                    id,
                    x: at.x,
                    y: at.y,
                }
            }
        };
        drive(&mut s, &mut config, &[finger(1, held, true)], false);
        drive(
            &mut s,
            &mut config,
            &[finger(2, row, true), finger(2, row, false)],
            false,
        );
        assert_ne!(s.face, Face::Tab(Tab::Controls), "{item:?} left the tabs");
        // The held finger lifts while the tabs are not listening.
        drive(&mut s, &mut config, &[finger(1, held, false)], false);
        drive(&mut s, &mut config, &press(Key::Escape), false);
        assert_eq!(s.face, Face::Tab(Tab::Controls));
        drive(&mut s, &mut config, &tap(strip[1].center()), false);
        assert_eq!(
            s.face,
            Face::Tab(Tab::Audio),
            "a later finger still switches tabs after {item:?}"
        );
    }
}

#[test]
fn the_page_heading_stacks_title_strip_and_list_and_shows_more_rows() {
    for (width, height) in [(640.0, 400.0), (1180.0, 820.0), (1920.0, 1080.0)] {
        for scale in SCALES {
            crate::render::set_viewport(width, height);
            crate::render::set_user_scale(scale);
            let s = crate::render::ui_scale();
            let page = on(Tab::Controls, &Config::default());
            let strip = crate::tabs::rects(Tab::ALL.len());
            let back = crate::button::corner_slot(0, s);
            let first = page.menu.item_rect(0).expect("the list opens on its top");
            let at = format!("{width}x{height} at {scale}");
            assert!(
                strip[0].y > crate::menu::page_title_baseline(s),
                "the strip hangs under the title, {at}"
            );
            assert!(
                strip.iter().all(|tab| !tab.overlaps(&back)),
                "the strip stays clear of BACK, {at}"
            );
            assert!(
                first.y >= strip[0].bottom(),
                "the list starts under the strip, {at}"
            );
            let mut marquee = tab_menu(Tab::Controls, &Config::default(), 0);
            marquee.heading = Heading::Marquee;
            assert!(
                page.menu.visible_range()[1] > marquee.visible_range()[1],
                "a page title leaves the list more rows than a marquee, {at}"
            );
        }
    }
    crate::render::set_user_scale(1.0);
}

#[test]
fn every_default_action_is_editable_and_secondary_edit_does_not_replace_primary() {
    let mut config = Config::default();
    let mut screen = on(Tab::Controls, &config);
    for binding in config.bindings.bindings() {
        index(&screen, Item::Binding(binding.action));
    }
    focus(&mut screen, Item::Binding(Action::PanUp));
    drive(&mut screen, &mut config, &press(Key::Right), false);
    drive(&mut screen, &mut config, &press(Key::Enter), false);
    assert!(drive(&mut screen, &mut config, &press(Key::I), false).dirty);
    assert_eq!(
        config.bindings.chord_at(Action::PanUp, 0),
        Some(Chord::bare(Key::W))
    );
    assert_eq!(
        config.bindings.chord_at(Action::PanUp, 1),
        Some(Chord::bare(Key::I))
    );
    drive(&mut screen, &mut config, &press(Key::X), false);
    assert_eq!(config.bindings.chord_at(Action::PanUp, 1), None);
    assert_eq!(
        config.bindings.chord_at(Action::PanUp, 0),
        Some(Chord::bare(Key::W))
    );
}

#[test]
fn left_and_right_on_the_controls_tab_step_a_setting_or_the_chord_column() {
    let mut config = Config::default();
    let mut screen = on(Tab::Controls, &config);
    focus(&mut screen, Item::Setting(Row::EdgePan));
    let edge_pan = config.camera.edge_pan;
    drive(
        &mut screen,
        &mut config,
        &press(if edge_pan { Key::Left } else { Key::Right }),
        false,
    );
    assert_eq!(config.camera.edge_pan, !edge_pan, "a setting row steps");
    assert_eq!(screen.binding_slot, 0, "and leaves the column alone");
    focus(&mut screen, Item::Binding(Action::PanUp));
    drive(&mut screen, &mut config, &press(Key::Right), false);
    assert_eq!(screen.binding_slot, 1, "a binding row picks the column");
}

#[test]
fn clicking_the_secondary_column_selects_it_even_when_release_is_a_later_frame() {
    use oxide_protocol::MouseButton;
    let mut config = Config::default();
    let mut screen = on(Tab::Controls, &config);
    focus(&mut screen, Item::Binding(Action::PanUp));
    let (x, y) = pan_up_columns(&screen).secondary.center().into();
    drive(
        &mut screen,
        &mut config,
        &[RawEvent::MouseDown {
            button: MouseButton::Left,
            x,
            y,
        }],
        false,
    );
    assert_eq!(screen.face, Face::Tab(Tab::Controls));
    drive(
        &mut screen,
        &mut config,
        &[RawEvent::MouseUp {
            button: MouseButton::Left,
            x,
            y,
        }],
        false,
    );
    assert_eq!(screen.binding_slot, 1);
    assert!(drive(&mut screen, &mut config, &press(Key::I), false).dirty);
    assert_eq!(
        config.bindings.chord_at(Action::PanUp, 0),
        Some(Chord::bare(Key::W))
    );
    assert_eq!(
        config.bindings.chord_at(Action::PanUp, 1),
        Some(Chord::bare(Key::I))
    );
}

#[test]
fn chord_columns_wrap_and_tab_steps_right() {
    let mut config = Config::default();
    let mut screen = on(Tab::Controls, &config);
    focus(&mut screen, Item::Binding(Action::PanUp));
    assert_eq!(screen.binding_slot, 0);
    drive(&mut screen, &mut config, &press(Key::Left), false);
    assert_eq!(
        screen.binding_slot, 1,
        "left from primary wraps to secondary"
    );
    drive(&mut screen, &mut config, &press(Key::Right), false);
    assert_eq!(
        screen.binding_slot, 0,
        "right from secondary wraps to primary"
    );
    drive(&mut screen, &mut config, &press(Key::Tab), false);
    assert_eq!(screen.binding_slot, 1);
    drive(&mut screen, &mut config, &press(Key::Tab), false);
    assert_eq!(screen.binding_slot, 0);
}

/// The Pan up row's columns, as the Controls tab draws them.
fn pan_up_columns(screen: &SettingsScreen) -> crate::menu::BindingColumns {
    let rect = screen
        .menu
        .item_rect(index(screen, Item::Binding(Action::PanUp)))
        .unwrap();
    crate::menu::BindingColumns::of(rect, crate::render::ui_scale())
}

/// Opens Controls on the Pan up row with `slot` chosen, clicks the row
/// at `x`, and returns the chosen slot and the row's label.
fn click_pan_up_at(
    slot: usize,
    x: impl Fn(&crate::menu::BindingColumns) -> f32,
) -> (usize, String) {
    let mut config = Config::default();
    let mut screen = SettingsScreen::open(&config);
    screen.binding_slot = slot;
    screen.show(Tab::Controls, &config);
    focus(&mut screen, Item::Binding(Action::PanUp));
    let columns = pan_up_columns(&screen);
    let at = vec2(x(&columns), columns.primary.center().y);
    drive(&mut screen, &mut config, &click(at), false);
    assert_eq!(screen.face, Face::Capturing(Action::PanUp));
    (
        screen.binding_slot,
        label(&screen, Item::Binding(Action::PanUp)),
    )
}

#[test]
fn a_click_picks_the_chord_column_it_lands_in() {
    assert_eq!(click_pan_up_at(0, |c| c.secondary.x + 1.0).0, 1);
    assert_eq!(click_pan_up_at(1, |c| c.secondary.x - 1.0).0, 0);
    assert_eq!(click_pan_up_at(1, |c| c.primary.x + 1.0).0, 0);
}

#[test]
fn a_click_on_the_action_name_keeps_the_chosen_column() {
    assert_eq!(click_pan_up_at(1, |c| c.name.center().x).0, 1);
    assert_eq!(click_pan_up_at(0, |c| c.name.center().x).0, 0);
}

#[test]
fn the_brackets_follow_the_clicked_column_while_capturing() {
    let (_, label) = click_pan_up_at(0, |c| c.secondary.center().x);
    let (_, keys) = label.rsplit_once(": ").unwrap();
    let (primary, secondary) = keys.split_once(" | ").unwrap();
    assert!(!primary.starts_with('['), "{label}");
    assert!(
        secondary.starts_with('[') && secondary.ends_with(']'),
        "{label}"
    );
}

#[test]
fn diagnostics_rows_survive_without_a_capture_toggle() {
    let config = Config::default();
    let screen = SettingsScreen::open(&config);
    assert!(
        !screen
            .menu
            .view()
            .items
            .iter()
            .any(|item| item.starts_with("Diagnostics:"))
    );
    if !crate::platform::TOUCH_ONLY {
        assert_eq!(
            label(&screen, Item::Setting(Row::OpenDiagnostics)),
            "Open diagnostics folder"
        );
    }
    assert_eq!(
        label(&screen, Item::Setting(Row::ExportDiagnostics)),
        "Export diagnostic report"
    );
    let mut old = serde_json::to_value(&config).unwrap();
    old.as_object_mut()
        .unwrap()
        .insert("diagnostics".into(), true.into());
    assert!(
        serde_json::from_value::<Config>(old).is_ok(),
        "a config saved with the old toggle still loads"
    );
}
