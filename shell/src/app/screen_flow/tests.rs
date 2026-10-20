use super::*;

/// A finished lesson's card, laid out headless at 1280x800.
fn finished_card() -> CardLayout {
    let game = Tutorial::begin_at(Lesson::Command, vec2(1280.0, 800.0)).unwrap();
    let tutorial = game
        .tutorial
        .expect("begin stages the lesson")
        .at_step(Lesson::Command.steps().len());
    tutorial_card::layout(
        &tutorial,
        None,
        &crate::action::BindingMap::classic(),
        crate::platform::Hands {
            pointer: crate::input::Pointer::Mouse,
            keys: true,
        },
        render::hud::HudEnv {
            viewport: vec2(1280.0, 800.0),
            ui: 1.0,
        },
        &|_, text, size| text.chars().count() as f32 * size * 0.5,
    )
}

#[test]
fn the_lesson_card_keeps_its_presses_and_its_buttons_act() {
    let card = finished_card();
    let button = |wanted| {
        card.buttons
            .iter()
            .find(|(_, button)| *button == wanted)
            .map(|(rect, _)| *rect)
            .unwrap()
    };
    let on_card = vec2(card.rect.x + 2.0, card.rect.y + 2.0);
    let world = vec2(640.0, 600.0);
    assert!(!card.rect.contains(world));

    let mut events = vec![
        RawEvent::TouchDown {
            id: 1,
            x: on_card.x,
            y: on_card.y,
        },
        RawEvent::TouchDown {
            id: 2,
            x: world.x,
            y: world.y,
        },
        RawEvent::TouchUp {
            id: 2,
            x: world.x,
            y: world.y,
        },
    ];
    let filtered = filter_card_pointer(&mut events, &card, 1.0);
    assert_eq!(filtered, CardPointer::default());
    assert_eq!(
        events,
        vec![
            RawEvent::TouchDown {
                id: 2,
                x: world.x,
                y: world.y,
            },
            RawEvent::TouchUp {
                id: 2,
                x: world.x,
                y: world.y,
            },
        ],
        "the card eats its finger and leaves the world's alone"
    );

    // A fingertip just below the drawn button still reaches it; the
    // mouse pointer there only meets the card.
    let next = button(CardButton::Next);
    let near = vec2(next.center().x, next.bottom() + 3.0);
    let mut events = vec![
        RawEvent::TouchDown {
            id: 3,
            x: near.x,
            y: near.y,
        },
        RawEvent::MouseDown {
            button: MouseButton::Left,
            x: near.x,
            y: near.y,
        },
    ];
    let filtered = filter_card_pointer(&mut events, &card, 1.0);
    assert_eq!(filtered.pressed, Some(CardButton::Next));
    assert!(events.is_empty());
    let mut events = vec![RawEvent::MouseDown {
        button: MouseButton::Left,
        x: near.x,
        y: near.y,
    }];
    assert_eq!(
        filter_card_pointer(&mut events, &card, 1.0),
        CardPointer::default()
    );
    assert!(events.is_empty());

    let fold = button(CardButton::Fold).center();
    let lessons = button(CardButton::Lessons).center();
    let mut events = vec![
        RawEvent::MouseDown {
            button: MouseButton::Right,
            x: lessons.x,
            y: lessons.y,
        },
        RawEvent::MouseDown {
            button: MouseButton::Left,
            x: fold.x,
            y: fold.y,
        },
        RawEvent::MouseUp {
            button: MouseButton::Left,
            x: on_card.x,
            y: on_card.y,
        },
    ];
    let filtered = filter_card_pointer(&mut events, &card, 1.0);
    assert_eq!(
        filtered,
        CardPointer {
            pressed: Some(CardButton::Fold),
            swallowed_release: true,
        },
        "only the left button presses a card button"
    );
    assert!(events.is_empty());
}

#[test]
fn the_tip_card_closes_by_mouse_and_by_finger_and_keeps_its_other_presses() {
    let card = tutorial_card::tip_layout(
        crate::tips::Tip::IdleHarvesters,
        1.0,
        &crate::action::BindingMap::classic(),
        crate::platform::Hands {
            pointer: crate::input::Pointer::Mouse,
            keys: true,
        },
        render::hud::HudEnv {
            viewport: vec2(1280.0, 800.0),
            ui: 1.0,
        },
        &|_, text, size| text.chars().count() as f32 * size * 0.5,
    );
    let close = card.buttons[0].0;
    let mouse = |at: Vec2| {
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
    };
    let mut events = mouse(close.center());
    let filtered = filter_card_pointer(&mut events, &card, 1.0);
    assert_eq!(filtered.pressed, Some(CardButton::Dismiss));
    assert!(events.is_empty());

    let near = vec2(close.center().x, close.bottom() + 3.0);
    let mut events = vec![RawEvent::TouchDown {
        id: 1,
        x: near.x,
        y: near.y,
    }];
    let filtered = filter_card_pointer(&mut events, &card, 1.0);
    assert_eq!(
        filtered.pressed,
        Some(CardButton::Dismiss),
        "a fingertip just below the cross still closes the tip"
    );
    assert!(events.is_empty());

    let body = vec2(card.rect.x + 20.0, card.rect.bottom() - 4.0);
    let mut events = mouse(body);
    let filtered = filter_card_pointer(&mut events, &card, 1.0);
    assert_eq!(filtered.pressed, None, "the tip's text is no button");
    assert!(events.is_empty(), "and the world never sees the press");
}

fn configured_new_match_draft() -> NewMatchDraft {
    let mut draft = NewMatchDraft::default();
    draft.set_scenario(Scenario::skirmish(), None);
    draft.seats[1].difficulty = oxide_sim::scenario::BotDifficulty::Prime;
    draft.seats[1].stance = oxide_sim::scenario::BotStance::Aggressive;
    draft
}

#[test]
fn escape_clears_a_selection_before_pausing_except_for_terminal_overlays() {
    assert!(!playing_escape_opens_pause(false, false, false, false));
    assert!(playing_escape_opens_pause(true, false, false, false));
    assert!(!playing_escape_opens_pause(true, true, false, false));
    assert!(playing_escape_opens_pause(true, true, true, false));
    assert!(playing_escape_opens_pause(true, true, false, true));
}

#[test]
fn opening_pause_freezes_play_and_ends_the_concede_banner() {
    let mut game =
        Game::with_viewport(oxide_sim::Scenario::skirmish(), vec2(1280.0, 800.0)).unwrap();
    game.presentation.conceded_banner = true;
    let screen = open_pause(&game, PauseCause::Player);
    assert!(matches!(screen, Screen::Pause(_)));
    enter(&mut game, &screen);
    assert!(game.clock.paused);
    assert!(!game.presentation.conceded_banner);
}

#[test]
fn a_lessons_pause_menu_offers_the_lesson_list() {
    let game = Tutorial::begin_at(Lesson::Scrap, vec2(1280.0, 800.0)).unwrap();
    let Screen::Pause(pause) = open_pause(&game, PauseCause::Player) else {
        panic!("the player opens the pause menu");
    };
    let items = pause.menu.view().items;
    assert!(items.iter().any(|item| item == "Lessons"));
    assert!(!items.iter().any(|item| item == "Save Game"));
}

#[test]
fn a_suspension_gap_pauses_only_a_running_live_match() {
    // (raw_dt, live_streak, running, exempt) -> pauses
    let cases = [
        ((SUSPENSION_GAP_SECS, 2, true, false), true),
        ((60.0, 9, true, false), true),
        ((SUSPENSION_GAP_SECS - 0.01, 9, true, false), false),
        ((60.0, 1, true, false), false),
        ((60.0, 0, true, false), false),
        ((60.0, 9, false, false), false),
        ((60.0, 9, true, true), false),
    ];
    for ((dt, streak, running, exempt), pauses) in cases {
        assert_eq!(
            gap_opens_pause(dt, streak, running, exempt),
            pauses,
            "dt {dt}, streak {streak}, running {running}, exempt {exempt}"
        );
    }
}

#[test]
fn a_suspension_pause_explains_itself() {
    let mut game =
        Game::with_viewport(oxide_sim::Scenario::skirmish(), vec2(1280.0, 800.0)).unwrap();
    let screen = open_pause(&game, PauseCause::Suspension);
    enter(&mut game, &screen);
    let Screen::Pause(pause) = screen else {
        panic!("a suspension opens the pause menu");
    };
    assert!(game.clock.paused);
    assert_eq!(pause.subtitle("Skirmish"), "paused after an interruption");
}

#[test]
fn new_match_seed_window_advances_once_only_after_a_successful_launch() {
    let mut personality_seeds = PersonalitySeedSource::from_seed(41);
    let first_base = personality_seeds.match_base();
    let draft = configured_new_match_draft();

    assert!(resolve_new_match(WizardOut::Stay, &draft, &mut personality_seeds).is_none());
    assert_eq!(personality_seeds.match_base(), first_base);
    assert!(resolve_new_match(WizardOut::Home, &draft, &mut personality_seeds).is_none());
    assert_eq!(
        personality_seeds.match_base(),
        first_base,
        "backing out cannot consume an opponent identity"
    );

    let mut invalid = configured_new_match_draft();
    invalid.seats.pop();
    let failed = resolve_new_match(WizardOut::Launch, &invalid, &mut personality_seeds)
        .expect("launch outcome has a result");
    assert!(failed.is_err());
    assert_eq!(
        personality_seeds.match_base(),
        first_base,
        "a launch refusal must leave the same seed window available for retry"
    );

    let launched = resolve_new_match(WizardOut::Launch, &draft, &mut personality_seeds)
        .expect("launch outcome has a result");
    let Ok(game) = launched else {
        panic!("a valid draft launches locally");
    };
    assert_eq!(
        game.scenario.players[1]
            .bot_config
            .expect("opponent is configured")
            .personality_seed,
        first_base + 1
    );
    assert_eq!(
        personality_seeds.match_base(),
        first_base.wrapping_add(crate::screens::wizard::launch::BOT_PERSONALITY_WINDOW),
        "one successful match consumes exactly one complete roster window"
    );
}

#[test]
fn the_first_match_launches_like_a_new_match_and_counts_once_it_ends() {
    let mut personality_seeds = PersonalitySeedSource::from_seed(41);
    let base = personality_seeds.match_base();
    let mut config = config::Config {
        player_name: Some("Rook".into()),
        ..config::Config::default()
    };
    let mut game = first_match_game(&config, &mut personality_seeds).unwrap();
    assert_eq!(game.scenario.players[0].name, "Rook");
    assert_eq!(
        game.scenario.players[1]
            .bot_config
            .map(|c| c.personality_seed),
        Some(base + 1)
    );
    assert_eq!(
        personality_seeds.match_base(),
        base.wrapping_add(crate::screens::wizard::launch::BOT_PERSONALITY_WINDOW),
        "the launch spends one roster window, like New Match"
    );
    assert!(game.tutorial.is_none(), "no lesson card and no steps");

    game.advance_ticks(5);
    assert!(!mark_first_match(&mut config, &game), "still being played");
    let mut resumed = game
        .capture_save()
        .checkpoint()
        .unwrap()
        .restore()
        .unwrap()
        .install();
    resumed.issue(oxide_sim::Command::Surrender);
    resumed.do_tick();
    assert!(resumed.state.result().is_some());
    assert!(
        mark_first_match(&mut config, &resumed),
        "a resumed first match counts when it ends, even as a loss"
    );
    assert!(config.first_match_done);
    assert!(!mark_first_match(&mut config, &resumed), "it counts once");

    let mut other = Game::new(Scenario::skirmish()).unwrap();
    other.issue(oxide_sim::Command::Surrender);
    other.do_tick();
    let mut fresh = config::Config::default();
    assert!(
        !mark_first_match(&mut fresh, &other),
        "another match ending is not the first match"
    );
}

#[test]
fn tips_show_in_the_first_match_and_elsewhere_only_when_turned_on() {
    let mut config = config::Config::default();
    let first = first_match_game(&config, &mut PersonalitySeedSource::from_seed(41)).unwrap();
    let other = Game::new(Scenario::skirmish()).unwrap();
    assert!(tips_on(&config, &first));
    assert!(!tips_on(&config, &other), "off by default");
    config.match_tips = true;
    assert!(tips_on(&config, &other));
}

#[test]
fn restart_and_rematch_rebuild_the_exact_opponents_without_rerolling() {
    let mut personality_seeds = PersonalitySeedSource::from_seed(41);
    let draft = configured_new_match_draft();
    let launched = resolve_new_match(WizardOut::Launch, &draft, &mut personality_seeds)
        .expect("launch outcome has a result");
    let Ok(game) = launched else {
        panic!("a valid draft launches locally");
    };
    let next_base = personality_seeds.match_base();
    let expected = game.scenario.clone();

    let restarted = rebuild_match(&game).expect("Restart rebuilds the match");
    let rematched = rebuild_match(&game).expect("Rematch rebuilds the match");
    for rebuilt in [&restarted, &rematched] {
        assert_eq!(rebuilt.scenario, expected);
        assert_eq!(rebuilt.recorder.setup, expected);
        assert_eq!(rebuilt.hash_hex(), game.hash_hex());
    }
    assert_eq!(
        personality_seeds.match_base(),
        next_base,
        "rebuilding an existing scenario never consumes New Match entropy"
    );
}

#[test]
fn a_viewer_restores_the_exact_screen_that_opened_it() {
    let game = Game::new(Scenario::skirmish()).expect("game");
    let mut pause = PauseScreen::open(false, true);
    pause.begin_naming("Skirmish");
    let viewer = open_playback(live_playback(&game).expect("viewer"), Screen::Pause(pause));
    let Screen::Playback { session, back } = viewer else {
        panic!("a viewer opens");
    };
    assert!(!session.clock.paused);
    assert!(session.seeking.is_none());
    assert_eq!(
        back.mode(),
        "save_name",
        "leaving lands on the same pause menu, mid-edit, not a fresh one"
    );
}

#[test]
fn every_way_out_of_the_final_map_ends_inspection() {
    let mut game =
        Game::with_viewport(oxide_sim::Scenario::skirmish(), vec2(1280.0, 800.0)).unwrap();
    game.presentation.selection.units.push(oxide_sim::UnitId(1));
    let final_map = Screen::FinalMap(FinalMapScreen::open());
    enter(&mut game, &final_map);
    assert!(game.clock.paused);
    assert!(game.presentation.spectating());
    assert!(game.presentation.selection.units.is_empty());
    // The step keys on the screen left, so a quit-save or a debug load
    // ends inspection as surely as the Back button.
    exit(&mut game, ScreenKind::FinalMap);
    assert!(!game.presentation.spectating());
}

#[test]
fn a_notice_lands_where_the_player_is_looking() {
    let mut game =
        Game::with_viewport(oxide_sim::Scenario::skirmish(), vec2(1280.0, 800.0)).unwrap();
    let notice = || Notice {
        text: "could not save settings: disk full".to_owned(),
        danger: true,
    };
    let deliver = |game: &mut Game, screen: &mut Screen| {
        let mut menu = None;
        deliver_notices(&mut vec![notice()], game, &mut menu, screen);
        menu
    };

    assert!(deliver(&mut game, &mut Screen::Playing).is_none());
    assert_eq!(
        game.presentation.toasts.len(),
        1,
        "live play: the HUD toast"
    );

    let mut settings = Screen::Settings {
        screen: SettingsScreen::open(&config::Config::default()),
        back: Box::new(Screen::Home(HomeScreen::open())),
    };
    assert!(deliver(&mut game, &mut settings).is_none());
    let Screen::Settings { screen, .. } = settings else {
        unreachable!()
    };
    assert!(
        screen.notice.is_some_and(|notice| notice.danger),
        "its own line"
    );

    let mut pause = Screen::Pause(PauseScreen::open(false, true));
    assert!(deliver(&mut game, &mut pause).is_none());
    let Screen::Pause(pause) = pause else {
        unreachable!()
    };
    assert_eq!(
        pause.subtitle("Skirmish"),
        "could not save settings: disk full",
        "the pause menu shows it above the veil"
    );

    for mut screen in [
        Screen::Home(HomeScreen::open()),
        Screen::Results(ResultsScreen::open()),
        Screen::FinalMap(FinalMapScreen::open()),
    ] {
        let menu = deliver(&mut game, &mut screen);
        assert_eq!(
            menu.map(|notice| notice.text).as_deref(),
            Some("could not save settings: disk full"),
            "{}",
            screen.mode()
        );
    }
    assert_eq!(game.presentation.toasts.len(), 1, "nothing else toasts");
}

#[test]
fn a_menu_notice_ages_only_while_a_screen_shows_it() {
    let mut notice = MenuNotice {
        text: "diagnostics exported".to_owned(),
        until: None,
    };
    // However long the final map hid it, its time starts on screen.
    assert_eq!(notice.show(100.0), Some("diagnostics exported"));
    assert_eq!(
        notice.show(100.0 + MENU_NOTICE_SECS - 0.5),
        Some("diagnostics exported")
    );
    assert_eq!(notice.show(100.0 + MENU_NOTICE_SECS), None);
}

#[test]
fn a_frame_that_leaves_live_play_runs_no_ticks() {
    let mut game =
        Game::with_viewport(oxide_sim::Scenario::skirmish(), vec2(1280.0, 800.0)).unwrap();
    let start = game.state.current_tick();
    let gap = SUSPENSION_GAP_SECS * 2.0;
    advance_live_match(&mut game, gap, None, true);
    assert_eq!(
        game.state.current_tick(),
        start,
        "the gap behind a pause request is never caught up"
    );
    advance_live_match(&mut game, gap, None, false);
    assert!(
        game.state.current_tick() > start,
        "a frame that stays in play runs what it owes"
    );
}
