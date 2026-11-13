use super::*;

/// A replay viewer over `back`.
fn viewer(session: PlaybackSession, back: Screen) -> Screen {
    Screen::Playback {
        session: Box::new(session),
        back: Box::new(back),
    }
}

#[test]
fn each_screen_kind_states_its_gameplay_and_performance_context() {
    for kind in [
        ScreenKind::Home,
        ScreenKind::Settings,
        ScreenKind::Codex,
        ScreenKind::Wizard,
        ScreenKind::Lobby,
        ScreenKind::Replays,
        ScreenKind::Results,
        ScreenKind::Pause,
        ScreenKind::Waiting,
        ScreenKind::Busy,
    ] {
        assert!(!kind.gameplay(), "{kind:?}");
        assert_eq!(kind.performance_context(), 0, "{kind:?}");
    }
    assert_eq!(
        [
            ScreenKind::Playing,
            ScreenKind::Playback,
            ScreenKind::FinalMap
        ]
        .map(|kind| (kind.gameplay(), kind.performance_context())),
        [(true, 1), (true, 2), (true, 3)]
    );
}

#[test]
fn a_menu_opened_from_pause_still_counts_as_over_pause() {
    let pause = || Screen::Pause(PauseScreen::open(false, true));
    let codex_over_pause = Screen::Codex {
        screen: CodexScreen::open(),
        back: Box::new(pause()),
    };
    assert!(pause().over_pause());
    assert!(codex_over_pause.over_pause());
    assert!(
        !Screen::Codex {
            screen: CodexScreen::open(),
            back: Box::new(Screen::Home(HomeScreen::open())),
        }
        .over_pause()
    );
    assert!(!Screen::Playing.over_pause());
}

#[test]
fn the_keyboard_follows_the_text_field_on_screen() {
    use crate::text_field::Keyboard;

    let mut pause = PauseScreen::open(false, true);
    assert_eq!(
        Screen::Pause(PauseScreen::open(false, true))
            .form()
            .map(|form| form.keyboard()),
        None
    );
    pause.begin_naming("Skirmish");
    assert_eq!(
        Screen::Pause(pause).form().map(|form| form.keyboard()),
        Some(Keyboard::Text)
    );
    assert!(!Screen::Playing.text_entry());

    let lobby = |name| Screen::Lobby {
        screen: Box::new(LobbyScreen::join(name, "10.0.0.2")),
        back: Box::new(Screen::Home(HomeScreen::open())),
    };
    assert_eq!(
        lobby("").form().map(|form| form.keyboard()),
        Some(Keyboard::Text),
        "a name first"
    );
    assert_eq!(
        lobby("Connor").form().map(|form| form.keyboard()),
        Some(Keyboard::Address),
        "then the address"
    );
    assert!(lobby("").text_entry());
}

#[test]
fn frame_context_follows_playback_speed_instead_of_the_hidden_live_clock() {
    use aftermine_protocol::DebugSession;

    let mut live = Game::new(aftermine_sim::Scenario::skirmish()).unwrap();
    live.clock.speed = 4.0;
    let replay = aftermine_kit::GameReplay::new(
        aftermine_sim::SIM_VERSION,
        "test",
        aftermine_sim::Scenario::skirmish(),
    );
    let mut playback = PlaybackSession::from_replay(replay).unwrap();
    for speed in [0.5, 1.0, 8.0, 64.0] {
        playback.set_speed(speed).unwrap();
        let screen = viewer(playback, Screen::Home(HomeScreen::open()));
        assert_eq!(screen.visible(&live).speed(), speed);
        assert_eq!(live.clock.speed, 4.0);
        let Screen::Playback { session, .. } = screen else {
            unreachable!()
        };
        playback = *session;
    }
    assert_eq!(Screen::Playing.visible(&live).speed(), 4.0);
    assert_eq!(
        Screen::Pause(PauseScreen::open(false, true))
            .visible(&live)
            .speed(),
        4.0
    );
}

#[test]
fn quits_save_every_hidden_live_match() {
    let home = || Screen::Home(HomeScreen::with_resumable(false));
    let pause = || Screen::Pause(PauseScreen::open(false, true));

    assert!(!home().holds_live_match());
    assert!(Screen::Playing.holds_live_match());
    assert!(pause().holds_live_match());
    assert!(Screen::Results(ResultsScreen::open(false)).holds_live_match());
    assert!(Screen::FinalMap(FinalMapScreen::open()).holds_live_match());

    let settings_from_home = Screen::Settings {
        screen: SettingsScreen::open(&crate::config::Config::default()),
        back: Box::new(home()),
    };
    let settings_from_pause = Screen::Settings {
        screen: SettingsScreen::open(&crate::config::Config::default()),
        back: Box::new(pause()),
    };
    assert!(!settings_from_home.holds_live_match());
    assert!(settings_from_pause.holds_live_match());

    let game = Game::new(aftermine_sim::Scenario::skirmish()).expect("game");
    let session = || super::super::live_playback(&game).expect("viewer");
    assert!(!viewer(session(), home()).holds_live_match());
    assert!(viewer(session(), pause()).holds_live_match());
    assert!(viewer(session(), Screen::Results(ResultsScreen::open(false))).holds_live_match());
}

#[test]
fn the_waiting_screen_holds_the_lan_match() {
    use crate::screens::waiting::{Absent, WaitingScreen};
    let mut waiting = Screen::Waiting(WaitingScreen::open(
        vec![Absent {
            seat: aftermine_sim::PlayerId(1),
            name: "Alex".to_owned(),
            waited: std::time::Duration::ZERO,
        }],
        true,
    ));
    assert!(waiting.holds_live_match());
    assert_eq!(waiting.form().map(|form| form.keyboard()), None);
    assert_eq!(waiting.mode(), "waiting");
    assert_eq!(waiting.profile_mode(), "waiting");
}

#[test]
fn only_a_match_or_replay_playing_keeps_the_screen_on() {
    let home = || Screen::Home(HomeScreen::open());
    let pause = || Screen::Pause(PauseScreen::open(false, true));
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).expect("game");
    game.advance_ticks(20);
    game.clock.paused = false;
    assert!(Screen::Playing.keeps_screen_awake(&game, false));
    assert!(
        !Screen::Playing.keeps_screen_awake(&game, true),
        "a match held for a controller runs nothing"
    );
    assert!(!pause().keeps_screen_awake(&game, false));
    assert!(!home().keeps_screen_awake(&game, false));

    let mut playing = super::super::live_playback(&game).expect("viewer");
    playing.clock.paused = false;
    assert!(viewer(playing, home()).keeps_screen_awake(&game, false));
    let mut stopped = super::super::live_playback(&game).expect("viewer");
    stopped.clock.paused = true;
    assert!(!viewer(stopped, home()).keeps_screen_awake(&game, false));
    let unplayed = Game::new(aftermine_sim::Scenario::skirmish()).expect("game");
    let mut ended = super::super::live_playback(&unplayed).expect("viewer");
    ended.clock.paused = false;
    assert!(
        !viewer(ended, home()).keeps_screen_awake(&game, false),
        "a replay at its end plays nothing"
    );

    game.clock.paused = true;
    assert!(!Screen::Playing.keeps_screen_awake(&game, false));
}

#[test]
fn an_undecided_lan_match_keeps_the_screen_on_under_its_menus() {
    let lan = Game::networked(
        aftermine_sim::Scenario::skirmish(),
        aftermine_sim::PlayerId(0),
        crate::game::network::NetRole::Client,
        macroquad::math::vec2(1280.0, 800.0),
    )
    .expect("lan match");
    let pause = || Screen::Pause(PauseScreen::open(false, true));
    assert!(Screen::Playing.keeps_screen_awake(&lan, true));
    assert!(pause().keeps_screen_awake(&lan, false));
    assert!(
        Screen::Codex {
            screen: CodexScreen::open(),
            back: Box::new(pause()),
        }
        .keeps_screen_awake(&lan, false)
    );
    assert!(!Screen::Home(HomeScreen::open()).keeps_screen_awake(&lan, false));
}

#[test]
fn veils_the_top_screen_and_the_silent_report_are_one_kind_each() {
    let all = [
        ScreenKind::ColdOpen,
        ScreenKind::Home,
        ScreenKind::Settings,
        ScreenKind::Codex,
        ScreenKind::Lessons,
        ScreenKind::Wizard,
        ScreenKind::Lobby,
        ScreenKind::Playing,
        ScreenKind::Playback,
        ScreenKind::Replays,
        ScreenKind::Results,
        ScreenKind::FinalMap,
        ScreenKind::Pause,
        ScreenKind::Waiting,
        ScreenKind::Busy,
    ];
    let where_true = |fact: fn(ScreenKind) -> bool| -> Vec<ScreenKind> {
        all.into_iter().filter(|kind| fact(*kind)).collect()
    };
    assert_eq!(
        where_true(ScreenKind::veils_match),
        [ScreenKind::Pause, ScreenKind::Waiting]
    );
    assert_eq!(where_true(ScreenKind::top_screen), [ScreenKind::Home]);
    assert_eq!(where_true(|kind| !kind.scored()), [ScreenKind::ColdOpen]);
}

#[test]
fn motors_run_only_where_a_session_plays() {
    let home = || Screen::Home(HomeScreen::open());
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).expect("game");
    game.advance_ticks(20);
    game.clock.paused = false;
    assert!(Screen::Playing.motors_run(&game, false));
    assert!(!Screen::Playing.motors_run(&game, true));
    assert!(!Screen::Pause(PauseScreen::open(false, true)).motors_run(&game, false));
    assert!(!home().motors_run(&game, false));
    let mut playing = super::super::live_playback(&game).expect("viewer");
    playing.clock.paused = false;
    game.clock.paused = true;
    assert!(
        viewer(playing, home()).motors_run(&game, false),
        "a replay plays its own motors over a paused match"
    );
}

#[test]
fn mirror_arrivals_land_only_where_saves_are_listed() {
    assert!(Screen::Home(HomeScreen::open()).takes_mirror_arrivals());
    assert!(!Screen::Playing.takes_mirror_arrivals());
    assert!(!Screen::Pause(PauseScreen::open(false, true)).takes_mirror_arrivals());
}

#[test]
fn match_effects_age_behind_the_report_and_under_a_lan_matchs_menus() {
    let home = || Screen::Home(HomeScreen::open());
    let pause = || Screen::Pause(PauseScreen::open(false, true));
    for lan in [false, true] {
        assert_eq!(
            Screen::Results(ResultsScreen::open(lan)).effect_aging(lan),
            EffectAging::Always
        );
        assert_eq!(Screen::Playing.effect_aging(lan), EffectAging::Held);
    }
    for screen in [home(), pause()] {
        assert_eq!(screen.effect_aging(false), EffectAging::Held);
        assert_eq!(screen.effect_aging(true), EffectAging::WallClock);
    }
}

#[test]
fn frame_scripts_run_on_home_the_live_match_or_the_viewer() {
    let home = || Screen::Home(HomeScreen::open());
    assert_eq!(Screen::Playing.frame_script(true), Ok(ScriptStart::Session));
    assert_eq!(home().frame_script(false), Ok(ScriptStart::Backdrop));
    assert!(
        home().frame_script(true).is_err(),
        "only the live match plays"
    );
    assert!(
        Screen::Pause(PauseScreen::open(false, true))
            .frame_script(false)
            .is_err()
    );
}

#[test]
fn menus_and_a_held_match_hold_still_and_nothing_else_does() {
    use crate::screens::lessons::LessonsScreen;
    use crate::screens::waiting::{Absent, WaitingScreen};
    use crate::screens::wizard::NewMatchDraft;
    let mut game = Game::new(aftermine_sim::Scenario::skirmish()).expect("game");
    let home = || Screen::Home(HomeScreen::open());
    let pause = || Screen::Pause(PauseScreen::open(false, true));
    let settings = |back| Screen::Settings {
        screen: SettingsScreen::open(&crate::config::Config::default()),
        back: Box::new(back),
    };
    let still = [
        settings(home()),
        settings(pause()),
        Screen::Codex {
            screen: CodexScreen::open(),
            back: Box::new(home()),
        },
        Screen::Lessons {
            screen: LessonsScreen::open(&[], false),
            back: Box::new(home()),
        },
        Screen::Wizard(Wizard::open(&NewMatchDraft::default())),
        Screen::Replays(Shelf::open()),
        pause(),
    ];
    for screen in still {
        assert!(screen.still(&game, false, false), "{}", screen.mode());
    }
    let session = || super::super::live_playback(&game).expect("viewer");
    let moving = [
        Screen::ColdOpen(ColdOpenScreen::default()),
        home(),
        Screen::Lobby {
            screen: Box::new(LobbyScreen::join("Connor", "10.0.0.2")),
            back: Box::new(home()),
        },
        Screen::Playing,
        viewer(session(), home()),
        Screen::Results(ResultsScreen::open(false)),
        Screen::FinalMap(FinalMapScreen::open()),
        Screen::Waiting(WaitingScreen::open(
            vec![Absent {
                seat: aftermine_sim::PlayerId(1),
                name: "Alex".to_owned(),
                waited: std::time::Duration::ZERO,
            }],
            true,
        )),
    ];
    for screen in moving {
        assert!(!screen.still(&game, false, false), "{}", screen.mode());
    }
    assert!(
        home().still(&game, false, true),
        "reduced motion freezes the backdrop"
    );
    assert!(
        !viewer(session(), home()).still(&game, false, true),
        "a replay plays on under reduced motion"
    );
    assert!(
        Screen::Playing.still(&game, true, false),
        "held for a controller"
    );
    game.clock.paused = true;
    assert!(
        !Screen::Playing.still(&game, false, false),
        "a paused match still pans"
    );
}

#[test]
fn no_screen_holding_a_lan_match_holds_still() {
    let lan = Game::networked(
        aftermine_sim::Scenario::skirmish(),
        aftermine_sim::PlayerId(0),
        crate::game::network::NetRole::Client,
        macroquad::math::vec2(1280.0, 800.0),
    )
    .expect("lan match");
    let pause = || Screen::Pause(PauseScreen::open(false, true));
    assert!(!pause().still(&lan, false, false));
    assert!(!Screen::Playing.still(&lan, true, false));
    assert!(
        !Screen::Codex {
            screen: CodexScreen::open(),
            back: Box::new(pause()),
        }
        .still(&lan, false, false)
    );
    assert!(
        Screen::Codex {
            screen: CodexScreen::open(),
            back: Box::new(Screen::Home(HomeScreen::open())),
        }
        .still(&lan, false, false),
        "a menu off Home holds no match"
    );
}
