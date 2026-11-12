use super::*;
use crate::config::{MarkerPrefs, Volumes};

/// The store as other devices left it, by key.
type Cloud = BTreeMap<String, String>;

/// One device's config and its agreement with the store, driven the way
/// `SettingsSync` drives them, with a map in place of iCloud.
struct Device {
    config: Config,
    bases: Bases,
}

impl Device {
    fn launch(config: Config, cloud: &mut Cloud) -> Self {
        let mut device = Self {
            config,
            bases: Bases::new(),
        };
        device.catch_up(cloud);
        device
    }

    fn catch_up(&mut self, cloud: &mut Cloud) -> bool {
        let merge = merge(&mut self.config, &mut self.bases, |field| {
            cloud.get(&field.key()).cloned()
        });
        for (field, text) in merge.uploads {
            cloud.insert(field.key(), text);
        }
        merge.adopted
    }

    fn save(&mut self, cloud: &mut Cloud) {
        for (field, text) in changed_here(&self.config, &mut self.bases) {
            cloud.insert(field.key(), text);
        }
    }
}

fn key(field: SyncedField) -> String {
    field.key()
}

/// The config fields that stay on each device. `version` names the store
/// keys instead.
const DEVICE_FIELDS: [&str; 13] = [
    "version",
    "performance_display",
    "frame_rate",
    "ui_scale",
    "camera",
    "pad",
    "window",
    "touch",
    "last_join_address",
    "install_id",
    "lessons_done",
    "first_match_done",
    "cold_open_seen",
];

#[test]
fn every_config_field_either_syncs_or_stays_on_the_device() {
    let serde_json::Value::Object(fields) = serde_json::to_value(Config::default()).unwrap() else {
        panic!("a config serializes as an object");
    };
    let synced: std::collections::BTreeSet<&str> = SyncedField::ALL
        .iter()
        .map(|field| field.name().split('.').next().unwrap())
        .collect();
    for name in fields.keys() {
        let places = usize::from(synced.contains(name.as_str()))
            + usize::from(DEVICE_FIELDS.contains(&name.as_str()));
        assert_eq!(places, 1, "`{name}` must either sync or stay on the device");
    }
    assert_eq!(
        fields.len(),
        synced.len() + DEVICE_FIELDS.len(),
        "every listed field exists"
    );
    for field in SyncedField::ALL {
        assert!(key(field).len() <= 64, "iCloud keys hold at most 64 bytes");
    }
    // The fields split into one key per Settings row must keep covering
    // every part, or a new bus or marker preference would silently stay
    // behind.
    let parts = |value: serde_json::Value| -> Vec<String> {
        value.as_object().unwrap().keys().cloned().collect()
    };
    assert_eq!(
        parts(serde_json::to_value(Volumes::default()).unwrap()),
        ["effects", "master", "music", "ui"],
        "one volume key per bus"
    );
    assert_eq!(
        parts(serde_json::to_value(MarkerPrefs::default()).unwrap()),
        ["end", "scale", "start"],
        "marker timing carries start and end, marker size the scale"
    );
}

#[test]
fn a_fresh_device_takes_the_players_settings_and_writes_nothing() {
    let mut cloud = Cloud::new();
    let mut ipad = Device::launch(
        Config {
            ui_scale: 1.25,
            ..Config::default()
        },
        &mut cloud,
    );
    ipad.config.install_id();
    ipad.config.volumes.music = 0.25;
    ipad.config.reduced_motion = true;
    ipad.config.player_name = Some("Connor".to_owned());
    ipad.save(&mut cloud);
    let uploaded = cloud.clone();

    let mut fresh = Device::launch(Config::default(), &mut cloud);
    assert_eq!(
        cloud, uploaded,
        "a fresh install never overwrites the cloud"
    );
    assert_eq!(fresh.config.volumes.music, 0.25);
    assert!(fresh.config.reduced_motion);
    assert_eq!(fresh.config.player_name.as_deref(), Some("Connor"));
    assert_eq!(fresh.config.ui_scale, 1.0, "the interface size stays put");
    assert_eq!(fresh.config.install_id, None, "so does the LAN identity");
    assert_ne!(ipad.config.install_id, None);

    fresh.save(&mut cloud);
    assert_eq!(
        cloud, uploaded,
        "taking settings in is not a change to send back"
    );
}

#[test]
fn only_a_change_the_player_makes_goes_up() {
    // A launch may look before iCloud has delivered what another device
    // stored, so it must not send this device's own settings up in their
    // place: launching writes nothing, whatever the device holds.
    let mut cloud = Cloud::new();
    let mut mac = Config {
        team_colors: true,
        match_tips: true,
        ui_scale: 1.5,
        ..Config::default()
    };
    mac.volumes.effects = 0.75;
    let mut mac = Device::launch(mac, &mut cloud);
    assert!(cloud.is_empty(), "launching writes nothing");

    let mut ipad = Config::default();
    ipad.volumes.effects = 0.5;
    cloud.insert(
        key(SyncedField::EffectsVolume),
        SyncedField::EffectsVolume.json(&ipad),
    );
    assert!(mac.catch_up(&mut cloud), "the iPad's value arrives late");
    assert_eq!(mac.config.volumes.effects, 0.5, "and still wins");
    assert!(
        mac.config.match_tips,
        "a setting iCloud lacks stays as it is"
    );

    mac.config.team_colors = false;
    mac.save(&mut cloud);
    assert_eq!(
        cloud.keys().collect::<Vec<_>>(),
        [
            &key(SyncedField::TeamColors),
            &key(SyncedField::EffectsVolume)
        ],
        "the player's change goes up, and nothing else of this device's"
    );
    assert_eq!(cloud[&key(SyncedField::TeamColors)], "false");
}

#[test]
fn the_cloud_wins_where_a_device_with_its_own_settings_meets_it() {
    let mut cloud = Cloud::new();
    let mut first = Device::launch(Config::default(), &mut cloud);
    first.config.volumes.effects = 0.5;
    first.save(&mut cloud);

    let mut second = Config {
        match_tips: true,
        ..Config::default()
    };
    second.volumes.effects = 0.75;
    let second = Device::launch(second, &mut cloud);
    assert_eq!(second.config.volumes.effects, 0.5, "iCloud's value wins");
    assert!(second.config.match_tips, "a setting iCloud lacks stays");
    assert!(
        !cloud.contains_key(&key(SyncedField::MatchTips)),
        "until the player changes it there"
    );
}

#[test]
fn a_change_waits_in_the_cloud_through_unrelated_saves() {
    let mut cloud = Cloud::new();
    let mut ipad = Device::launch(Config::default(), &mut cloud);
    let mut mac = Device::launch(Config::default(), &mut cloud);

    ipad.config.volumes.music = 0.5;
    ipad.save(&mut cloud);
    // The Mac has not caught up yet: a window resize and a finished lesson
    // save its config, which must not send its old music volume back.
    mac.config.window = (1600, 1000);
    mac.config.finish_lesson(crate::tutorial::Lesson::Scrap);
    mac.save(&mut cloud);
    assert_eq!(
        cloud[&key(SyncedField::MusicVolume)],
        SyncedField::MusicVolume.json(&ipad.config)
    );

    assert!(mac.catch_up(&mut cloud));
    assert_eq!(mac.config.volumes.music, 0.5);
    assert_eq!(mac.config.window, (1600, 1000));
    assert!(!mac.catch_up(&mut cloud), "nothing new the second time");
}

#[test]
fn the_newest_change_wins() {
    let mut cloud = Cloud::new();
    let mut ipad = Device::launch(Config::default(), &mut cloud);
    let mut mac = Device::launch(Config::default(), &mut cloud);

    ipad.config.volumes.music = 0.5;
    ipad.save(&mut cloud);
    // The Mac changes the same setting before it hears of the iPad's change.
    mac.config.volumes.music = 0.75;
    mac.save(&mut cloud);

    assert!(
        !mac.catch_up(&mut cloud),
        "the Mac's own change is the newest"
    );
    assert_eq!(mac.config.volumes.music, 0.75);
    assert!(ipad.catch_up(&mut cloud));
    assert_eq!(ipad.config.volumes.music, 0.75);
}

#[test]
fn a_value_the_load_checks_would_reject_is_ignored() {
    let mut duplicated = serde_json::to_value(crate::action::BindingMap::classic()).unwrap();
    let rows = duplicated["bindings"].as_array_mut().unwrap();
    rows.push(rows[0].clone());
    let bad = [
        (SyncedField::Bindings, duplicated.to_string()),
        (
            SyncedField::PlayerName,
            serde_json::to_string(&"\u{7}".repeat(3)).unwrap(),
        ),
        (SyncedField::MarkerTiming, "[-100.0,300.0]".to_owned()),
        (SyncedField::MarkerTiming, "[16.0,24.0]".to_owned()),
        (SyncedField::MarkerSize, "-1.0".to_owned()),
        (SyncedField::MasterVolume, "5.0".to_owned()),
        (SyncedField::MusicVolume, "-0.5".to_owned()),
        (SyncedField::ReducedMotion, "not json".to_owned()),
    ];
    for (field, text) in bad {
        let mut cloud = Cloud::new();
        let mut device = Device::launch(Config::default(), &mut cloud);
        cloud.insert(key(field), text.clone());
        assert!(!device.catch_up(&mut cloud), "{field:?} is refused");
        assert_eq!(device.config, Config::default());
        device.save(&mut cloud);
        assert_eq!(cloud[&key(field)], text, "and left alone, not fought over");

        let fresh = Device::launch(Config::default(), &mut cloud);
        assert_eq!(fresh.config, Config::default(), "{field:?} at launch too");
    }
}

#[test]
fn a_binding_list_that_misses_an_action_arrives_with_its_default_key() {
    use crate::action::{Action, BindingMap, Chord};
    use aftermine_protocol::Key;
    let mut older = BindingMap::classic();
    older.forget(Action::SelectArmy);
    assert!(older.rebind(Action::Patrol, Chord::bare(Key::J)));
    let text = serde_json::to_string(&older).unwrap();
    let arrived = |config: &Config| {
        assert_eq!(
            config.bindings.chord_for(Action::Patrol),
            Some(Chord::bare(Key::J)),
            "the other device's rebind arrives"
        );
        assert_eq!(
            config.bindings.chord_for(Action::SelectArmy),
            Some(Chord::ctrl(Key::A))
        );
    };

    let mut cloud = Cloud::new();
    cloud.insert(key(SyncedField::Bindings), text.clone());
    arrived(&Device::launch(Config::default(), &mut cloud).config);

    let mut cloud = Cloud::new();
    let mut device = Device::launch(Config::default(), &mut cloud);
    cloud.insert(key(SyncedField::Bindings), text);
    assert!(device.catch_up(&mut cloud));
    arrived(&device.config);
}

#[test]
fn defaults_a_read_binding_list_gains_never_go_up_over_a_newer_rebind() {
    use crate::action::{Action, BindingMap, Chord};
    use aftermine_protocol::Key;
    let mut older = BindingMap::classic();
    older.forget(Action::SelectArmy);
    let mut cloud = Cloud::new();
    cloud.insert(
        key(SyncedField::Bindings),
        serde_json::to_string(&older).unwrap(),
    );
    let mut mac = Device::launch(Config::default(), &mut cloud);
    let mut ipad = Device::launch(Config::default(), &mut cloud);
    let launched = cloud.clone();
    mac.save(&mut cloud);
    ipad.save(&mut cloud);
    assert_eq!(
        cloud, launched,
        "the defaults read in are no change to send"
    );

    assert!(
        mac.config
            .bindings
            .rebind(Action::Patrol, Chord::bare(Key::J))
    );
    mac.save(&mut cloud);
    let rebound = cloud[&key(SyncedField::Bindings)].clone();
    ipad.config.reduced_motion = true;
    ipad.save(&mut cloud);
    assert!(ipad.catch_up(&mut cloud));
    ipad.config.volumes.music = 0.25;
    ipad.save(&mut cloud);
    mac.catch_up(&mut cloud);

    assert_eq!(
        cloud[&key(SyncedField::Bindings)],
        rebound,
        "the store keeps the Mac's list"
    );
    for device in [&mac, &ipad] {
        assert_eq!(
            device.config.bindings.chord_for(Action::Patrol),
            Some(Chord::bare(Key::J))
        );
        assert_eq!(
            device.config.bindings.chord_for(Action::SelectArmy),
            Some(Chord::ctrl(Key::A))
        );
    }
    assert!(mac.config.reduced_motion && mac.config.volumes.music == 0.25);
}

#[test]
fn an_action_cleared_on_one_device_stays_unbound_on_the_other() {
    use crate::action::Action;
    let mut cloud = Cloud::new();
    let mut mac = Device::launch(Config::default(), &mut cloud);
    let mut ipad = Device::launch(Config::default(), &mut cloud);
    mac.config.bindings.unbind_slot(Action::SelectArmy, 0);
    mac.save(&mut cloud);
    assert!(ipad.catch_up(&mut cloud));
    assert_eq!(ipad.config.bindings, mac.config.bindings);
    let restarted = Config::from_text(Some(&ipad.config.text()));
    assert_eq!(restarted.bindings.chord_for(Action::SelectArmy), None);
    let relaunched = Device::launch(restarted, &mut cloud);
    assert_eq!(relaunched.config.bindings, mac.config.bindings);
    ipad.config.volumes.music = 0.25;
    ipad.save(&mut cloud);
    assert!(mac.catch_up(&mut cloud));
    assert_eq!(mac.config.bindings.chord_for(Action::SelectArmy), None);
    assert_eq!(
        cloud[&key(SyncedField::Bindings)],
        SyncedField::Bindings.json(&mac.config)
    );
}

#[test]
fn equal_settings_written_differently_go_back_at_most_once() {
    let mut cloud = Cloud::new();
    let mut device = Device::launch(Config::default(), &mut cloud);
    cloud.insert(key(SyncedField::MusicVolume), " 5e-1 ".to_owned());
    assert!(device.catch_up(&mut cloud));
    assert_eq!(device.config.volumes.music, 0.5);
    device.save(&mut cloud);
    let rewritten = cloud.clone();
    assert_eq!(rewritten[&key(SyncedField::MusicVolume)], "0.5");
    device.save(&mut cloud);
    assert!(!device.catch_up(&mut cloud));
    assert_eq!(cloud, rewritten, "settled after one write");
}

#[test]
fn settings_values_cross_the_store_bit_for_bit() {
    for value in [0.0, 0.1, 0.25, 0.3, 0.5, 0.75, 0.9, 1.0] {
        let config = Config {
            volumes: Volumes {
                master: value,
                effects: value,
                ui: value,
                music: value,
            },
            markers: MarkerPrefs {
                scale: value + 0.75,
                ..MarkerPrefs::default()
            },
            ..Config::default()
        };
        let mut copy = Config::default();
        for field in [
            SyncedField::MasterVolume,
            SyncedField::EffectsVolume,
            SyncedField::InterfaceVolume,
            SyncedField::MusicVolume,
            SyncedField::MarkerSize,
        ] {
            assert!(field.read(&mut copy, &field.json(&config)));
            assert_eq!(field.json(&copy), field.json(&config));
        }
        assert_eq!(copy.volumes.music.to_bits(), value.to_bits());
        assert_eq!(copy.markers.scale.to_bits(), (value + 0.75).to_bits());
    }
    for (start, end, _) in MarkerPrefs::TIMINGS {
        let mut config = Config::default();
        (config.markers.start, config.markers.end) = (start, end);
        let mut copy = Config::default();
        let timing = SyncedField::MarkerTiming;
        assert!(timing.read(&mut copy, &timing.json(&config)));
        assert_eq!(copy.markers.timing(), config.markers.timing());
    }
}

#[test]
fn edits_to_two_rows_on_two_devices_both_survive() {
    // Neither device has heard of the other's edit when it saves its own,
    // so each row needs its own key or one edit would undo the other.
    let mut cloud = Cloud::new();
    let mut ipad = Device::launch(Config::default(), &mut cloud);
    let mut mac = Device::launch(Config::default(), &mut cloud);
    ipad.config.volumes.music = 0.25;
    ipad.config.markers.set_timing(0);
    ipad.save(&mut cloud);
    mac.config.volumes.effects = 0.5;
    mac.config.markers.scale = 1.25;
    mac.save(&mut cloud);

    assert!(ipad.catch_up(&mut cloud));
    assert!(mac.catch_up(&mut cloud));
    for device in [&ipad, &mac] {
        assert_eq!(device.config.volumes.music, 0.25);
        assert_eq!(device.config.volumes.effects, 0.5);
        assert_eq!(device.config.markers.timing(), Some(0));
        assert_eq!(device.config.markers.scale, 1.25);
    }
}

#[test]
fn without_the_store_nothing_syncs() {
    // A workspace or automation run never opens the store, so settings
    // stay in the config file alone.
    let mut config = Config {
        team_colors: true,
        ..Config::default()
    };
    let (mut sync, changed) = SettingsSync::open(false, &mut config);
    assert!(!changed);
    sync.push(&config);
    assert!(!sync.pull(&mut config));
    assert!(config.team_colors);
}
