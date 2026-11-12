use super::*;
use std::ffi::OsStr;
use std::io::Read;
use std::net::TcpListener;
use std::process::Command;

fn menu(items: &[&str], selected: Option<usize>) -> UiView {
    UiView {
        mode: "home".to_string(),
        title: None,
        selected,
        items: items.iter().map(|item| (*item).to_string()).collect(),
        visible_range: None,
        hover: None,
        chrome: None,
        panel_regions: None,
        menu_button: None,
        pause_button: None,
        group_column: None,
        army_strip: None,
        controller_notice: None,
        hud_buttons: None,
    }
}

#[test]
fn cargo_artifact_selection_ignores_noise_and_other_targets() {
    let output = br#"
not json
{"reason":"compiler-artifact","target":{"name":"aftermine-sim"},"executable":"/tmp/wrong"}
{"reason":"compiler-artifact","target":{"name":"Aftermine"},"executable":null}
{"reason":"build-finished","success":true}
{"reason":"compiler-artifact","target":{"name":"Aftermine"},"executable":"/tmp/Aftermine"}
"#;
    assert_eq!(
        shell_executable_from_cargo_output(output),
        Some(PathBuf::from("/tmp/Aftermine"))
    );
    assert_eq!(shell_executable_from_cargo_output(b"not json\n"), None);
}

#[test]
fn isolated_shells_override_every_platform_writable_root() {
    let home = PathBuf::from("/tmp/aftermine-isolated-home");
    let mut command = std::process::Command::new("Aftermine");
    command.env("HOME", "/host/home");
    command.env("XDG_CONFIG_HOME", "/host/config");
    command.env("XDG_DATA_HOME", "/host/data");
    command.env("APPDATA", "C:/host/data");

    isolate_home(&mut command, &home);

    let value = |key: &str| {
        command
            .get_envs()
            .find(|(name, _)| *name == OsStr::new(key))
            .and_then(|(_, value)| value)
            .map(std::ffi::OsStr::to_owned)
    };
    assert_eq!(value("HOME"), Some(home.clone().into_os_string()));
    assert_eq!(
        value("XDG_CONFIG_HOME"),
        Some(home.join(".config").into_os_string())
    );
    assert_eq!(
        value("XDG_DATA_HOME"),
        Some(home.join(".local/share").into_os_string())
    );
    assert_eq!(
        value("APPDATA"),
        Some(home.join("AppData").into_os_string())
    );
}

/// Walks `view` with a cursor `step` moves, returning the keys pressed.
fn walk(view: &UiView, needle: &str, step: impl Fn(usize, Key) -> usize) -> Vec<Key> {
    let mut walk = Walk::to_label(view, needle).unwrap();
    let mut selected = view.selected.unwrap_or(0);
    let mut keys = Vec::new();
    loop {
        match walk.next(selected) {
            Stride::Press(key) => {
                keys.push(key);
                selected = step(selected, key);
            }
            Stride::Arrived => return keys,
            Stride::Stuck => panic!("stuck after {keys:?}"),
        }
    }
}

#[test]
fn labeled_activation_prefers_an_exact_row_over_an_earlier_substring() {
    let view = menu(&["REPLAYS", "PLAY", "SETTINGS"], Some(0));
    assert_eq!(walk(&view, "play", |at, _| at + 1), vec![Key::Down]);
}

#[test]
fn a_walk_follows_a_wrapping_list_and_defaults_to_the_first_row() {
    let view = menu(&["PLAY", "SETTINGS", "QUIT"], Some(2));
    assert_eq!(walk(&view, "play", |at, _| (at + 1) % 3), vec![Key::Down]);
    let no_selection = menu(&["PLAY", "SETTINGS", "QUIT"], None);
    assert_eq!(
        walk(&no_selection, "quit", |at, _| (at + 1) % 3),
        vec![Key::Down, Key::Down]
    );
}

#[test]
fn a_walk_turns_right_when_down_cycles_a_grid_column() {
    // Two rows of three: Down keeps the column, Right reads on.
    let view = menu(&["a", "b", "c", "d", "e", "f"], Some(0));
    let keys = walk(&view, "e", |at, key| match key {
        Key::Down => (at + 3) % 6,
        _ => (at + 1) % 6,
    });
    assert_eq!(
        keys,
        [vec![Key::Down; 2], vec![Key::Right; 4]].concat(),
        "down cycles back to a, then right reads b, c, d and lands on e"
    );
}

#[test]
fn a_walk_gives_up_when_no_key_moves_the_cursor() {
    let view = menu(&["a", "b"], Some(0));
    let mut walk = Walk::to_label(&view, "b").unwrap();
    assert_eq!(walk.next(0), Stride::Press(Key::Down));
    assert_eq!(walk.next(0), Stride::Press(Key::Right));
    assert_eq!(walk.next(0), Stride::Stuck);
}

#[test]
fn labeled_activation_reports_the_visible_rows_when_no_row_matches() {
    let view = menu(&["PLAY", "SETTINGS"], Some(0));
    let error = Walk::to_label(&view, "credits").unwrap_err();
    assert_eq!(
        error.to_string(),
        "no row containing 'credits' in [\"PLAY\", \"SETTINGS\"]"
    );
}

#[test]
fn mode_waits_ride_out_only_named_transitional_screens() {
    assert_eq!(
        classify_mode("home", "home", &["saving"]),
        ModeWait::Arrived
    );
    assert_eq!(
        classify_mode("saving", "home", &["saving"]),
        ModeWait::Transitional
    );
    assert_eq!(
        classify_mode("pause_menu", "home", &["saving"]),
        ModeWait::Unexpected
    );
    assert_eq!(classify_mode("saving", "home", &[]), ModeWait::Unexpected);
}

/// A stand-in shell that exits at once with status 3.
fn exits_at_once() -> Command {
    let mut command;
    if cfg!(windows) {
        command = Command::new("cmd");
        command.args(["/C", "exit 3"]);
    } else {
        command = Command::new("sh");
        command.args(["-c", "exit 3"]);
    }
    command
}

#[test]
fn launch_refuses_a_port_that_already_answers_before_spawning_or_sending() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind a stand-in server");
    let port = listener.local_addr().expect("stand-in address").port();
    // Spawning this would fail with an error of its own.
    let never_spawned = Command::new("aftermine-no-such-shell");
    let Err(error) = launch(never_spawned, port, Duration::from_secs(5)) else {
        panic!("launch connected to the stand-in server");
    };
    assert!(
        error.to_string().contains("already answers"),
        "refused for the wrong reason: {error:#}"
    );
    // The refusal came from a connection that sent nothing and hung up.
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let (mut probe, _) = listener.accept().expect("launch probed the port");
    probe.set_nonblocking(false).expect("blocking probe");
    probe
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("probe read timeout");
    let mut sent = Vec::new();
    probe.read_to_end(&mut sent).expect("the probe hung up");
    assert!(sent.is_empty(), "the probe sent {sent:?}");
}

#[test]
fn launch_reports_a_shell_that_exits_before_answering_with_its_status() {
    let status = exits_at_once().status().expect("run the stand-in once");
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .expect("find a free port")
        .port();
    let started = Instant::now();
    let Err(error) = launch(exits_at_once(), port, Duration::from_secs(60)) else {
        panic!("a shell that exits never answers");
    };
    let message = error.to_string();
    assert!(
        message.contains("exited") && message.contains(&status.to_string()),
        "{message}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "noticed only after {:?}",
        started.elapsed()
    );
}
