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
const DEVICE_FIELDS: [&str; 11] = [
    "version",
    "performance_display",
    "ui_scale",
    "camera",
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
    let synced: Vec<&str> = SyncedField::ALL.iter().map(|field| field.name()).collect();
    for name in fields.keys() {
        let places = usize::from(synced.contains(&name.as_str()))
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
}

#[test]
fn a_fresh_device_takes_the_players_settings_and_writes_nothing() {
    let mut cloud = Cloud::new();
    let mut ipad = Config {
        ui_scale: 1.25,
        reduced_motion: true,
        player_name: Some("Connor".to_owned()),
        ..Config::default()
    };
    ipad.volumes.music = 0.25;
    ipad.install_id();
    let ipad = Device::launch(ipad, &mut cloud);
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
fn a_device_uploads_only_the_settings_its_player_chose() {
    let mut cloud = Cloud::new();
    let config = Config {
        team_colors: true,
        ui_scale: 1.5,
        lessons_done: vec![crate::tutorial::Lesson::Scrap],
        ..Config::default()
    };
    Device::launch(config, &mut cloud);
    assert_eq!(
        cloud.keys().collect::<Vec<_>>(),
        [&key(SyncedField::TeamColors)],
        "defaults and device settings stay home"
    );
    assert_eq!(cloud[&key(SyncedField::TeamColors)], "true");
}

#[test]
fn the_cloud_wins_where_a_device_with_its_own_settings_meets_it() {
    let mut cloud = Cloud::new();
    let mut first = Config::default();
    first.volumes.effects = 0.5;
    Device::launch(first, &mut cloud);

    let mut second = Config {
        match_tips: true,
        ..Config::default()
    };
    second.volumes.effects = 0.75;
    let second = Device::launch(second, &mut cloud);
    assert_eq!(second.config.volumes.effects, 0.5, "iCloud's value wins");
    assert!(second.config.match_tips, "a setting iCloud lacks stays");
    assert_eq!(
        cloud[&key(SyncedField::MatchTips)],
        "true",
        "and goes up for the other devices"
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
        cloud[&key(SyncedField::Volumes)],
        volumes_json(&ipad.config)
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
        (
            SyncedField::Markers,
            r#"{"start":-100.0,"end":300.0,"scale":-1.0}"#.to_owned(),
        ),
        (
            SyncedField::Volumes,
            r#"{"master":5.0,"effects":1.0,"ui":1.0,"music":1.0}"#.to_owned(),
        ),
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
fn equal_settings_written_differently_go_back_at_most_once() {
    let mut cloud = Cloud::new();
    let mut device = Device::launch(Config::default(), &mut cloud);
    cloud.insert(
        key(SyncedField::Volumes),
        r#"{ "master": 1, "effects": 1, "ui": 1, "music": 0.5 }"#.to_owned(),
    );
    assert!(device.catch_up(&mut cloud));
    assert_eq!(device.config.volumes.music, 0.5);
    device.save(&mut cloud);
    let rewritten = cloud.clone();
    assert_eq!(
        rewritten[&key(SyncedField::Volumes)],
        volumes_json(&device.config)
    );
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
        for field in [SyncedField::Volumes, SyncedField::Markers] {
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
        assert!(SyncedField::Markers.read(&mut copy, &SyncedField::Markers.json(&config)));
        assert_eq!(copy.markers.timing(), config.markers.timing());
    }
}

#[test]
fn without_the_store_nothing_syncs() {
    // A workspace or automation run never opens the store, so settings
    // stay in the config file alone. (A build iCloud refuses gets no store
    // either; the iCloud crate's tests cover that.)
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

fn volumes_json(config: &Config) -> String {
    SyncedField::Volumes.json(config)
}
