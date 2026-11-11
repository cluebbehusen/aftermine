use super::*;

#[test]
fn marker_preferences_round_trip_and_clamp_without_resetting_other_settings() {
    let dir = std::env::temp_dir().join(format!("aftermine-config-markers-{}", std::process::id()));
    let path = dir.join("config.json");
    let mut config = Config {
        ui_scale: 1.25,
        ..Config::default()
    };
    config.save_to(&path).unwrap();
    assert_eq!(Config::load_from(Some(path.clone())), config);

    config.markers = MarkerPrefs {
        start: 30.0,
        end: 22.0,
        scale: 1.5,
    };
    config.save_to(&path).unwrap();
    assert_eq!(Config::load_from(Some(path.clone())), config);

    config.markers = MarkerPrefs {
        start: -100.0,
        end: 300.0,
        scale: -1.0,
    };
    config.save_to(&path).unwrap();
    let loaded = Config::load_from(Some(path));
    assert_eq!(
        loaded.markers,
        MarkerPrefs {
            start: 31.0,
            end: 30.0,
            scale: 0.75
        }
    );
    assert_eq!(loaded.ui_scale, 1.25);
    assert_eq!(
        MarkerPrefs {
            start: f32::NAN,
            end: f32::INFINITY,
            scale: f32::NEG_INFINITY
        }
        .clamped(),
        MarkerPrefs::default()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_customized_map_survives_the_round_trip() {
    // A rebind must survive a restart, so the rest of the classic map
    // (including its one-based Slot(9)) must pass load validation.
    let dir = std::env::temp_dir().join(format!("aftermine-config-custom-{}", std::process::id()));
    let path = dir.join("config.json");
    let mut config = Config::default();
    assert!(
        config.bindings.rebind(
            crate::action::Action::Patrol,
            crate::action::Chord::bare(aftermine_protocol::Key::J)
        ),
        "J is free in the classic map"
    );
    config.save_to(&path).expect("save");
    let loaded = Config::load_from(Some(path));
    assert_eq!(
        loaded.bindings.chord_for(crate::action::Action::Patrol),
        Some(crate::action::Chord::bare(aftermine_protocol::Key::J)),
        "the customization survived validation"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_explicit_unbinding_survives_the_restart() {
    let dir = std::env::temp_dir().join(format!("aftermine-config-unbind-{}", std::process::id()));
    let path = dir.join("config.json");
    let mut config = Config::default();
    config
        .bindings
        .unbind_slot(crate::action::Action::Patrol, 0);
    config.save_to(&path).expect("save");
    let loaded = Config::load_from(Some(path));
    assert_eq!(
        loaded.bindings.chord_for(crate::action::Action::Patrol),
        None,
        "the deliberate unbinding persisted"
    );
    assert_eq!(loaded.bindings, config.bindings);
    std::fs::remove_dir_all(&dir).ok();
}

/// Default settings whose binding list never mentions `missing`, as a
/// file saved before the action existed.
fn saved_without(missing: crate::action::Action) -> Config {
    let mut config = Config::default();
    config.bindings.forget(missing);
    config
}

#[test]
fn an_action_the_saved_list_never_names_loads_with_its_default_key() {
    use crate::action::{Action, Chord};
    use aftermine_protocol::Key;
    let mut saved = saved_without(Action::SelectArmy);
    assert!(saved.bindings.rebind(Action::Patrol, Chord::bare(Key::J)));
    let loaded = Config::from_text(Some(&saved.text()));
    assert_eq!(
        loaded.bindings.chord_for(Action::SelectArmy),
        Some(Chord::ctrl(Key::A))
    );
    assert_eq!(
        loaded.bindings.chord_for(Action::Patrol),
        Some(Chord::bare(Key::J)),
        "the player's own rebind stays"
    );

    // A pan takes both of its keys.
    let loaded = Config::from_text(Some(&saved_without(Action::PanLeft).text()));
    assert_eq!(
        [0, 1].map(|slot| loaded.bindings.chord_at(Action::PanLeft, slot)),
        [Some(Chord::bare(Key::A)), Some(Chord::bare(Key::Left))]
    );
}

#[test]
fn a_list_written_before_cleared_actions_and_layouts_were_recorded_loads_filled() {
    use crate::action::{Action, Chord};
    use aftermine_protocol::Key;
    let saved = Config {
        ui_scale: 1.25,
        ..saved_without(Action::SelectArmy)
    };
    let mut text: serde_json::Value = serde_json::from_str(&saved.text()).unwrap();
    let bindings = text["bindings"].as_object_mut().unwrap();
    assert!(bindings.remove("unbound").is_some() && bindings.remove("layout").is_some());
    let loaded = Config::from_text(Some(&text.to_string()));
    assert_eq!(loaded.ui_scale, 1.25, "the file loads rather than resets");
    assert_eq!(
        loaded.bindings.chord_for(Action::SelectArmy),
        Some(Chord::ctrl(Key::A))
    );
}

#[test]
fn a_left_handed_list_fills_from_left_handed_defaults() {
    use crate::action::{Action, BindingMap, Chord};
    use aftermine_protocol::Key;
    let mut saved = Config {
        bindings: BindingMap::left_handed(),
        ..Config::default()
    };
    saved.bindings.forget(Action::SelectArmy);
    saved.bindings.forget(Action::PanLeft);
    let loaded = Config::from_text(Some(&saved.text()));
    assert_eq!(
        loaded.bindings.chord_for(Action::SelectArmy),
        Some(Chord::ctrl(Key::J))
    );
    assert_eq!(
        [0, 1].map(|slot| loaded.bindings.chord_at(Action::PanLeft, slot)),
        [Some(Chord::bare(Key::J)), Some(Chord::bare(Key::Left))]
    );
    let mut again = loaded.clone();
    again.bindings.forget(Action::SelectArmy);
    assert_eq!(
        Config::from_text(Some(&again.text())),
        loaded,
        "the layout survives the file"
    );
}

#[test]
fn an_unnamed_action_whose_default_key_the_list_holds_stays_unbound() {
    use crate::action::{Action, Chord};
    use aftermine_protocol::Key;
    let mut saved = saved_without(Action::SelectArmy);
    assert!(
        saved
            .bindings
            .rebind(Action::CycleIdleWorker, Chord::ctrl(Key::A))
    );
    let loaded = Config::from_text(Some(&saved.text()));
    assert_eq!(loaded.bindings.chord_for(Action::SelectArmy), None);
    assert_eq!(
        loaded.bindings.chord_for(Action::CycleIdleWorker),
        Some(Chord::ctrl(Key::A)),
        "the player's binding keeps the key"
    );
    assert_eq!(loaded.bindings, saved.bindings);
}

#[test]
fn a_rebind_after_the_load_saves_the_whole_list() {
    use crate::action::{Action, Chord};
    use aftermine_protocol::Key;
    let dir = std::env::temp_dir().join(format!("aftermine-config-fill-{}", std::process::id()));
    let path = dir.join("config.json");
    saved_without(Action::SelectArmy).save_to(&path).unwrap();
    let mut config = Config::load_from(Some(path.clone()));
    // What Settings > Controls does with a captured chord.
    assert!(
        config
            .bindings
            .rebind_slot(Action::Patrol, 0, Chord::bare(Key::J))
    );
    config.save_to(&path).unwrap();
    let written: Config = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        written.bindings.chord_for(Action::SelectArmy),
        Some(Chord::ctrl(Key::A))
    );
    assert_eq!(written.bindings, config.bindings);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn saving_twice_replaces_instead_of_failing() {
    // std's rename replaces existing destinations on every
    // platform; the second save must land and its content win.
    let dir = std::env::temp_dir().join(format!("aftermine-config-twice-{}", std::process::id()));
    let path = dir.join("config.json");
    let mut config = Config::default();
    config.save_to(&path).expect("first save");
    config.ui_scale = 1.5;
    config.save_to(&path).expect("second save replaces");
    let loaded = Config::load_from(Some(path));
    assert!((loaded.ui_scale - 1.5).abs() < 1e-6, "the newer config won");
    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(unix)]
#[test]
fn a_failed_save_leaves_the_old_config_standing() {
    // A read-only directory refuses the temp file; the previous
    // config must survive untouched and no temp may linger.
    use std::os::unix::fs::PermissionsExt as _;
    let dir = std::env::temp_dir().join(format!("aftermine-config-fail-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let path = dir.join("config.json");
    let config = Config::default();
    config.save_to(&path).expect("first save");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let changed = Config {
        ui_scale: 1.5,
        ..Config::default()
    };
    assert!(changed.save_to(&path).is_err(), "the failure surfaces");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let loaded = Config::load_from(Some(path));
    assert_eq!(loaded, config, "the old config survived the failed save");
    let temps: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
        .collect();
    assert!(temps.is_empty(), "no temp survives a failed save");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_config_round_trips_through_disk() {
    let dir = std::env::temp_dir().join(format!("aftermine-config-test-{}", std::process::id()));
    let path = dir.join("config.json");
    let config = Config {
        ui_scale: 1.25,
        volumes: Volumes {
            master: 0.5,
            ..Volumes::default()
        },
        ..Config::default()
    };
    config.save_to(&path).unwrap();
    let back = Config::load_from(Some(path));
    assert_eq!(back, config);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn performance_modes_the_last_join_address_and_the_name_persist() {
    let dir = std::env::temp_dir().join(format!(
        "aftermine-config-performance-{}",
        std::process::id()
    ));
    let path = dir.join("config.json");
    let mut config = Config {
        ui_scale: 1.25,
        ..Config::default()
    };
    config.volumes.master = 0.25;
    for mode in [
        PerformanceDisplay::Off,
        PerformanceDisplay::Fps,
        PerformanceDisplay::Detailed,
    ] {
        config.performance_display = mode;
        config.save_to(&path).unwrap();
        assert_eq!(Config::load_from(Some(path.clone())), config);
    }
    config.last_join_address = Some("connor-mbp:4200".to_owned());
    config.player_name = Some("Connor".to_owned());
    config.save_to(&path).unwrap();
    assert_eq!(Config::load_from(Some(path.clone())), config);

    // A name edited past what the game accepts is dropped on its own.
    config.player_name = Some("\u{7}".repeat(3));
    config.save_to(&path).unwrap();
    let loaded = Config::load_from(Some(path));
    assert_eq!(loaded.player_name, None);
    assert_eq!(loaded.ui_scale, 1.25, "the rest of the settings survive");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_hand_edited_ui_scale_loads_within_the_settings_range() {
    let dir = std::env::temp_dir().join(format!("aftermine-config-scale-{}", std::process::id()));
    let path = dir.join("config.json");
    for (stored, loaded) in [(0.5, 0.75), (1.25, 1.25), (3.0, 1.5)] {
        let config = Config {
            ui_scale: stored,
            ..Config::default()
        };
        config.save_to(&path).unwrap();
        assert_eq!(Config::load_from(Some(path.clone())).ui_scale, loaded);
    }
    assert_eq!(Config::sane_ui_scale(f32::INFINITY), 1.0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn controller_settings_round_trip_and_a_hand_edited_stick_speed_loads_in_range() {
    let dir = std::env::temp_dir().join(format!("aftermine-config-pad-{}", std::process::id()));
    let path = dir.join("config.json");
    let config = Config {
        pad: PadPrefs {
            stick_speed: 1.5,
            zoom_inverted: true,
            snap: ReticleSnap::Strong,
        },
        ..Config::default()
    };
    config.save_to(&path).unwrap();
    assert_eq!(Config::load_from(Some(path.clone())), config);
    for (stored, loaded) in [(0.1, 0.5), (0.75, 0.75), (9.0, 2.0)] {
        let config = Config {
            pad: PadPrefs {
                stick_speed: stored,
                ..PadPrefs::default()
            },
            ..Config::default()
        };
        config.save_to(&path).unwrap();
        assert_eq!(
            Config::load_from(Some(path.clone())).pad.stick_speed,
            loaded
        );
    }
    let unreadable = PadPrefs {
        stick_speed: f32::NAN,
        ..PadPrefs::default()
    };
    assert_eq!(unreadable.clamped(), PadPrefs::default());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn any_trouble_at_all_falls_back_to_defaults() {
    // Missing file.
    let missing = Config::load_from(Some(PathBuf::from("/definitely/not/here.json")));
    assert_eq!(missing, Config::default());
    // Garbage content.
    let dir = std::env::temp_dir().join(format!("aftermine-config-garbage-{}", std::process::id()));
    let path = dir.join("config.json");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&path, "not json at all").unwrap();
    assert_eq!(Config::load_from(Some(path.clone())), Config::default());
    // A future version resets rather than guessing.
    let future = Config {
        version: CONFIG_VERSION + 1,
        ..Config::default()
    };
    std::fs::write(&path, serde_json::to_string(&future).unwrap()).unwrap();
    assert_eq!(Config::load_from(Some(path)), Config::default());
    std::fs::remove_dir_all(&dir).ok();
}
#[test]
fn an_empty_secondary_slot_and_rebound_primary_survive_reload() {
    use crate::action::{Action, Chord};
    let dir =
        std::env::temp_dir().join(format!("aftermine-config-secondary-{}", std::process::id()));
    let path = dir.join("config.json");
    let mut config = Config::default();
    config.bindings.unbind_slot(Action::PanUp, 1);
    assert!(
        config
            .bindings
            .rebind(Action::PanUp, Chord::bare(aftermine_protocol::Key::I))
    );
    config.save_to(&path).unwrap();
    let loaded = Config::load_from(Some(path));
    assert_eq!(loaded.bindings, config.bindings);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_driver_quiet_config_has_the_current_shape() {
    // Driver captures install this file as the shell's settings, and a
    // stale shape would silently fall back to defaults.
    let config: Config =
        serde_json::from_str(include_str!("../../../driver/src/quiet_config.json")).unwrap();
    assert_eq!(config.version, CONFIG_VERSION);
    assert_eq!(config.volumes.master, 0.0);
    // A capture that launches to Home must not wait out the first-launch report.
    assert!(config.cold_open_seen);
    // Driver scripts press the classic keys. Loading fills only actions the
    // file never names, so a different chord, a cleared action or the
    // left-handed layout here would redirect or drop scripted presses.
    assert!(
        config.bindings == BindingMap::classic(),
        "quiet_config.json must carry the default bindings:\n{}",
        serde_json::to_string(&BindingMap::classic()).unwrap()
    );
}

#[test]
fn an_install_id_is_made_once_kept_and_a_bad_one_forgotten() {
    let mut config = Config::default();
    let id = config.install_id();
    assert_eq!(config.install_id(), id, "the first id sticks");

    let dir = std::env::temp_dir().join(format!(
        "aftermine-config-install-{}-{}",
        std::process::id(),
        crate::entropy::bits()
    ));
    let path = dir.join("config.json");
    config.save_to(&path).unwrap();
    let mut loaded = Config::load_from(Some(path.clone()));
    assert_eq!(loaded.install_id(), id, "a saved id survives a relaunch");

    let mut edited = config.clone();
    edited.install_id = Some("not hex".to_owned());
    edited.player_name = Some("Alex".to_owned());
    edited.save_to(&path).unwrap();
    let loaded = Config::load_from(Some(path));
    assert_eq!(loaded.install_id, None, "a bad id is forgotten");
    assert_eq!(
        loaded.player_name.as_deref(),
        Some("Alex"),
        "and nothing else with it"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_player_without_a_name_goes_by_their_installs_default() {
    for name in DEFAULT_NAMES {
        let valid = aftermine_net::PlayerName::new(name).map(String::from);
        assert_eq!(
            valid.as_deref(),
            Ok(name),
            "a default passes the name rules"
        );
    }
    let mut config = Config::default();
    assert_eq!(config.name().as_str(), "Anvil", "the first, without an id");
    for (bits, picked) in [(0, "Anvil"), (8, "Rivet"), (19, "Shale")] {
        config.install_id = Some(aftermine_net::InstallId::new(bits).to_hex());
        assert_eq!(config.name().as_str(), picked);
    }

    // Confirming the default leaves it a default, kept off iCloud.
    let default = config.name();
    assert!(!config.set_name(&default));
    assert_eq!(config.player_name, None);
    let alex = aftermine_net::PlayerName::new("Alex").unwrap();
    assert!(config.set_name(&alex));
    assert!(!config.set_name(&alex), "already theirs");
    assert_eq!(config.player_name.as_deref(), Some("Alex"));
    assert_eq!(config.name(), alex);
    assert!(config.set_name(&default), "given over another name");
    assert_eq!(config.player_name.as_deref(), Some("Shale"));
}

#[test]
fn finished_lessons_and_the_first_match_persist_once_each() {
    use crate::tutorial::Lesson;
    let mut config = Config::default();
    assert!(config.finish_lesson(Lesson::Scrap));
    assert!(config.finish_lesson(Lesson::Command));
    assert!(!config.finish_lesson(Lesson::Scrap), "already finished");
    assert_eq!(config.lessons_done, [Lesson::Command, Lesson::Scrap]);
    assert!(!config.first_match_done);
    assert!(config.finish_first_match());
    assert!(!config.finish_first_match(), "already finished");

    let dir = std::env::temp_dir().join(format!(
        "aftermine-config-lessons-{}-{}",
        std::process::id(),
        crate::entropy::bits()
    ));
    let path = dir.join("config.json");
    let mut edited = config.clone();
    edited.lessons_done = vec![Lesson::Scrap, Lesson::Command, Lesson::Scrap];
    edited.save_to(&path).unwrap();
    let loaded = Config::load_from(Some(path));
    assert_eq!(
        loaded.lessons_done, config.lessons_done,
        "a hand-edited list loads deduplicated and in course order"
    );
    assert!(loaded.first_match_done);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_cold_open_shows_until_a_saved_config_says_it_has() {
    let dir = std::env::temp_dir().join(format!("aftermine-cold-open-{}", std::process::id()));
    let path = dir.join("config.json");
    let mut config = Config::default();
    assert!(!config.cold_open_seen);
    config.cold_open_seen = true;
    config.save_to(&path).unwrap();
    assert!(Config::load_from(Some(path)).cold_open_seen);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn hand_edited_volumes_load_within_range() {
    let dir = std::env::temp_dir().join(format!("aftermine-config-volumes-{}", std::process::id()));
    let path = dir.join("config.json");
    let config = Config {
        volumes: Volumes {
            master: 5.0,
            effects: -1.0,
            ui: 0.5,
            music: 1.0,
        },
        ..Config::default()
    };
    config.save_to(&path).unwrap();
    assert_eq!(
        Config::load_from(Some(path)).volumes,
        Volumes {
            master: 1.0,
            effects: 0.0,
            ui: 0.5,
            music: 1.0,
        }
    );
    let unreadable = Volumes {
        master: f32::NAN,
        effects: f32::INFINITY,
        ui: f32::NEG_INFINITY,
        music: 0.25,
    };
    assert_eq!(
        unreadable.clamped(),
        Volumes {
            music: 0.25,
            ..Volumes::default()
        }
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_load_checks_settle_in_one_pass() {
    // Settings from another device pass the same checks and are taken only
    // when the checks leave them as they are, so a second pass must never
    // move what the first one settled.
    use crate::tutorial::Lesson;
    let messy = Config {
        ui_scale: 9.0,
        window: (1, 1),
        player_name: Some("\u{7}".repeat(3)),
        install_id: Some("not hex".to_owned()),
        lessons_done: vec![Lesson::Scrap, Lesson::Command, Lesson::Scrap],
        markers: MarkerPrefs {
            start: -100.0,
            end: 300.0,
            scale: -1.0,
        },
        volumes: Volumes {
            master: 5.0,
            ..Volumes::default()
        },
        touch: TouchPrefs {
            double_tap_ms: 0,
            long_press_ms: 0,
        },
        pad: PadPrefs {
            stick_speed: -3.0,
            ..PadPrefs::default()
        },
        ..Config::default()
    };
    let once = messy.sanitized();
    assert_ne!(
        once,
        Config::default().sanitized(),
        "the pass kept what it could"
    );
    assert_eq!(once.clone().sanitized(), once);
}

#[test]
fn config_text_round_trips_and_falls_back_to_defaults() {
    let config = Config {
        ui_scale: 1.25,
        install_id: Some(aftermine_net::InstallId::new(7).to_hex()),
        ..Config::default()
    };
    assert_eq!(Config::from_text(Some(&config.text())), config);
    assert_eq!(Config::from_text(None), Config::default());
    assert_eq!(Config::from_text(Some("not json")), Config::default());
    let other_version = Config {
        version: CONFIG_VERSION + 1,
        ..config
    };
    assert_eq!(
        Config::from_text(Some(&other_version.text())),
        Config::default()
    );
}
