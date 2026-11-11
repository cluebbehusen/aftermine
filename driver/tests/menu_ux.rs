//! Menu interaction contracts, driven through the automation harness
//! against a real spawned shell. `#[ignore]`d because they spawn real
//! windows: CI has no display and the workspace suite must stay headless.
//! Run them explicitly:
//!
//! ```sh
//! cargo test -p aftermine-driver --test menu_ux -- --ignored --test-threads 1
//! ```
//!
//! Every spawned shell gets its own scratch HOME and platform-specific
//! config/data roots. The guard removes that tree only after the child
//! has exited, so even autosave-producing walks cannot touch real data.

use aftermine_driver::auto::{
    INJECTED_PAD, ShellGuard, SpawnOptions, activate_labeled, activate_labeled_by_pad, assert_mode,
    connect_pad, inject, press_key, press_pad, select_labeled, select_tab, spawn_shell, ui,
    wait_for_mode,
};
use aftermine_driver::client::Client;
use aftermine_protocol::{Key, MouseButton, PadButton, PadFamily, RawEvent, Request, Stick};
use anyhow::{Result, bail};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT_HOME: AtomicU64 = AtomicU64::new(0);

/// Owns both the spawned shell and its isolated HOME. Dropping the
/// shell first matters: a final autosave must finish before the
/// scratch tree is removed.
struct MenuShellGuard {
    shell: Option<ShellGuard>,
    home: PathBuf,
}

impl Drop for MenuShellGuard {
    fn drop(&mut self) {
        drop(self.shell.take());
        if let Err(error) = std::fs::remove_dir_all(&self.home)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "warning: could not remove menu UX scratch HOME {}: {error}",
                self.home.display()
            );
        }
    }
}

fn scratch_home(port: u16) -> Result<PathBuf> {
    loop {
        let id = NEXT_HOME.fetch_add(1, Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!(
            "aftermine-menu-ux-{}-{port}-{id}",
            std::process::id()
        ));
        match std::fs::create_dir(&home) {
            Ok(()) => return Ok(home),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
}

/// Spawns an automation-mode shell (via the shared harness vocabulary)
/// and walks Home -> Play so tests operate on the scenario list (the
/// widget with enough rows to scroll).
fn spawn_at_map_list(port: u16) -> Result<(MenuShellGuard, Client)> {
    let (guard, mut client) = spawn(port)?;
    // By label, never by blind Enter: if a fixture later adds an
    // autosave, Home's first row becomes Continue.
    activate_labeled(&mut client, "play")?;
    Ok((guard, client))
}

fn spawn(port: u16) -> Result<(MenuShellGuard, Client)> {
    spawn_prepared(port, |_| Ok(()))
}

/// Like [`spawn`], after `prepare` has filled the scratch HOME, which is
/// also the shell's working directory.
fn spawn_prepared(
    port: u16,
    prepare: impl FnOnce(&std::path::Path) -> Result<()>,
) -> Result<(MenuShellGuard, Client)> {
    let home = scratch_home(port)?;
    if let Err(error) = prepare(&home) {
        std::fs::remove_dir_all(home).ok();
        return Err(error);
    }
    let spawned = spawn_shell(&SpawnOptions {
        port,
        paused: false,
        home: Some(home.clone()),
    });
    match spawned {
        Ok((shell, client)) => Ok((
            MenuShellGuard {
                shell: Some(shell),
                home,
            },
            client,
        )),
        Err(error) => {
            std::fs::remove_dir_all(home).ok();
            Err(error)
        }
    }
}

fn hover(client: &mut Client, x: f32, y: f32) -> Result<()> {
    client.call(Request::InjectEvent {
        event: RawEvent::MouseMove { x, y },
    })?;
    Ok(())
}

/// The x the row tests probe and click at. The map list is a card grid,
/// and the window's horizontal center (640 at the pinned 1280x800) is a
/// column gutter where no card hovers, so the probe column sits inside the
/// second card column instead.
const GRID_X: f32 = 500.0;

/// Sweeps the window's vertical axis and returns every y where the
/// hover highlight changed, one entry per row boundary crossed, in
/// top-to-bottom order. Only two tall card rows fit the default window,
/// so the sweep steps finely and two hits suffice.
fn find_rows(client: &mut Client) -> Result<Vec<(f32, usize)>> {
    // Row discovery watches the hover highlight, the pointer's only
    // effect on a healthy menu.
    let mut prev = ui(client)?.hover;
    let mut hits: Vec<(f32, usize)> = Vec::new();
    for step in 0..64 {
        let y = 160.0 + step as f32 * 10.0;
        hover(client, GRID_X, y)?;
        let hovered = ui(client)?.hover;
        if hovered != prev
            && let Some(h) = hovered
        {
            hits.push((y, h));
        }
        prev = hovered;
    }
    if hits.len() < 2 {
        bail!("could not locate menu rows by sweeping ({hits:?})");
    }
    Ok(hits)
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn a_stationary_pointer_never_changes_the_row_beneath_it() -> Result<()> {
    let (_guard, mut client) = spawn_at_map_list(4141)?;
    let rows = find_rows(&mut client)?;
    let top_row_y = rows[0].0;

    // Push the keyboard cursor deep so the window scrolls away from the
    // top, then park the pointer on the topmost visible row and wiggle
    // it by one pixel, like a real cursor at rest. Nothing about the row
    // under the pointer may change: not the hover, not the selection,
    // not the window.
    for _ in 0..6 {
        press_key(&mut client, Key::Down)?;
    }
    hover(&mut client, GRID_X, top_row_y)?;
    let settled = ui(&mut client)?;
    for i in 0..6 {
        hover(&mut client, GRID_X, top_row_y + (i % 2) as f32)?;
        let now = ui(&mut client)?;
        assert_eq!(now.hover, settled.hover, "the hover crawled");
        assert_eq!(now.selected, settled.selected, "the selection crawled");
        assert_eq!(
            now.visible_range, settled.visible_range,
            "the window crawled beneath a stationary pointer"
        );
    }
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn menu_rows_activate_on_release_not_on_press() -> Result<()> {
    let (_guard, mut client) = spawn_at_map_list(4142)?;
    // A high row: always a scenario, never Quit — pressing Quit would
    // close the shell before any assert could speak.
    let row_y = find_rows(&mut client)?[1].0;
    let before = ui(&mut client)?.mode;

    // Press without releasing: nothing may activate yet — a press is a
    // commitment only when it releases inside the same control.
    client.call(Request::InjectEvent {
        event: RawEvent::MouseDown {
            button: MouseButton::Left,
            x: GRID_X,
            y: row_y,
        },
    })?;
    let held = ui(&mut client)?.mode;
    assert_eq!(held, before, "a menu row activated on mouse-down");

    // Releasing far outside the row cancels instead of activating.
    client.call(Request::InjectEvent {
        event: RawEvent::MouseMove { x: 20.0, y: 20.0 },
    })?;
    client.call(Request::InjectEvent {
        event: RawEvent::MouseUp {
            button: MouseButton::Left,
            x: 20.0,
            y: 20.0,
        },
    })?;
    let released = ui(&mut client)?.mode;
    assert_eq!(released, before, "a drag-away release still activated");

    // A real click still works: a click inside the card selects it,
    // and a second click on the selected card advances to match setup.
    client.call(Request::InjectEvent {
        event: RawEvent::MouseMove {
            x: GRID_X,
            y: row_y,
        },
    })?;
    for _ in 0..2 {
        client.call(Request::InjectEvent {
            event: RawEvent::MouseDown {
                button: MouseButton::Left,
                x: GRID_X,
                y: row_y,
            },
        })?;
        client.call(Request::InjectEvent {
            event: RawEvent::MouseUp {
                button: MouseButton::Left,
                x: GRID_X,
                y: row_y,
            },
        })?;
    }
    assert_eq!(
        ui(&mut client)?.mode,
        "match_setup",
        "click selects, click again activates"
    );
    press_key(&mut client, Key::Escape)?;
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn every_screen_transition_answers_the_walk() -> Result<()> {
    // One pass over the whole screen graph, asserting the shell's own
    // mode report at each hop. Navigation is by row LABEL, never by
    // index: Home's rows shift with resumable state.
    let (_guard, mut client) = spawn(4143)?;
    assert_mode(&mut client, "home", "boot")?;

    activate_labeled(&mut client, "settings")?;
    assert_mode(&mut client, "settings", "Home > Settings")?;
    select_tab(&mut client, "controls")?;
    press_key(&mut client, Key::Escape)?;
    assert_mode(&mut client, "home", "Settings Controls tab > Esc")?;

    activate_labeled(&mut client, "saves & replays")?;
    assert_mode(&mut client, "saves", "Home > Saves & Replays")?;
    select_tab(&mut client, "replays")?;
    press_key(&mut client, Key::Escape)?;
    assert_mode(&mut client, "home", "Shelf Replays tab > Esc")?;

    activate_labeled(&mut client, "join match")?;
    assert_mode(&mut client, "join_match", "Home > Join Match")?;
    // A fresh settings file goes by the first default name, so the address
    // field takes the text.
    inject(&mut client, RawEvent::Text { ch: '7' })?;
    let fields = ui(&mut client)?.items;
    assert!(
        fields.len() == 2 && fields[0].ends_with("Anvil") && fields[1].ends_with("7_"),
        "the address field takes text: {fields:?}"
    );
    press_key(&mut client, Key::Tab)?;
    inject(&mut client, RawEvent::Text { ch: '7' })?;
    let fields = ui(&mut client)?.items;
    assert!(
        fields[0].ends_with("Anvil7_"),
        "Tab moves to the name field: {fields:?}"
    );
    press_key(&mut client, Key::Escape)?;
    assert_mode(&mut client, "home", "Join Match > Esc")?;

    activate_labeled(&mut client, "play")?;
    assert_mode(&mut client, "main_menu", "Home > Play")?;
    activate_labeled(&mut client, "skirmish")?;
    assert_mode(&mut client, "match_setup", "map picked")?;
    // Back walks to the grid without losing the draft.
    press_key(&mut client, Key::Escape)?;
    assert_mode(&mut client, "main_menu", "setup > Esc")?;
    activate_labeled(&mut client, "skirmish")?;
    assert_mode(&mut client, "match_setup", "map re-picked")?;
    // Start is preselected: Enter launches the classic matchup.
    press_key(&mut client, Key::Enter)?;
    assert_mode(&mut client, "playing", "Start launches the match")?;

    press_key(&mut client, Key::Escape)?;
    assert_mode(&mut client, "pause_menu", "Playing > Esc")?;
    activate_labeled(&mut client, "resume")?;
    assert_mode(&mut client, "playing", "pause > Resume")?;
    press_key(&mut client, Key::Escape)?;
    activate_labeled(&mut client, "restart")?;
    assert_mode(&mut client, "confirm_pause", "destructive choices confirm")?;
    // Cancel must sit preselected: bare Enter declines the destruction.
    press_key(&mut client, Key::Enter)?;
    assert_mode(&mut client, "pause_menu", "confirm > default is Cancel")?;
    // Settings opens over the paused match and leaving returns to the
    // SAME pause menu — the payload waits intact, not a fresh Home.
    activate_labeled(&mut client, "settings")?;
    assert_mode(&mut client, "settings", "Pause > Settings")?;
    select_tab(&mut client, "controls")?;
    press_key(&mut client, Key::Escape)?;
    assert_mode(&mut client, "pause_menu", "Settings > Esc returns to pause")?;
    activate_labeled(&mut client, "main menu")?;
    activate_labeled(&mut client, "main menu")?;
    wait_for_mode(
        &mut client,
        "home",
        &["saving"],
        "pause > Main Menu confirmed",
    )?;
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn a_controller_walks_the_menus_through_the_same_funnel() -> Result<()> {
    // Injected pad events take the path hardware pads take: the D-pad
    // moves the cursor, the bottom face button confirms, and the right one
    // goes back, on screens that read only canonical menu keys.
    let (_guard, mut client) = spawn(4145)?;
    assert_mode(&mut client, "home", "boot")?;
    connect_pad(&mut client, PadFamily::PlayStation)?;
    let before = ui(&mut client)?.selected;
    press_pad(&mut client, PadButton::DpadDown)?;
    assert_ne!(
        ui(&mut client)?.selected,
        before,
        "the D-pad moves the cursor"
    );
    press_pad(&mut client, PadButton::North)?;
    assert_mode(&mut client, "home", "a button with no menu role")?;

    activate_labeled_by_pad(&mut client, "play")?;
    assert_mode(&mut client, "main_menu", "Home > Play by pad")?;
    activate_labeled_by_pad(&mut client, "smelter yard")?;
    assert_mode(&mut client, "match_setup", "map picked by pad")?;
    press_pad(&mut client, PadButton::East)?;
    assert_mode(&mut client, "main_menu", "setup > Circle")?;
    press_pad(&mut client, PadButton::East)?;
    assert_mode(&mut client, "home", "map list > Circle")?;

    // Join Match opens with its address field taking text, where keys keep
    // their conventional meaning; the pad must still find the way out.
    activate_labeled_by_pad(&mut client, "join match")?;
    assert_mode(&mut client, "join_match", "Home > Join Match by pad")?;
    press_pad(&mut client, PadButton::East)?;
    assert_mode(&mut client, "home", "Join Match > Circle")?;
    Ok(())
}

fn tilt_left_stick(client: &mut Client, x: f32, y: f32) -> Result<()> {
    inject(
        client,
        RawEvent::PadStick {
            pad: INJECTED_PAD,
            stick: Stick::Left,
            x,
            y,
        },
    )
}

/// Every distinct selection the menu shows over `span`, sampled as fast
/// as the shell answers.
fn selections_over(client: &mut Client, span: Duration) -> Result<Vec<usize>> {
    let until = Instant::now() + span;
    let mut seen = Vec::new();
    while Instant::now() < until {
        if let Some(selected) = ui(client)?.selected
            && !seen.contains(&selected)
        {
            seen.push(selected);
        }
    }
    Ok(seen)
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn a_controller_stick_bumpers_and_triggers_drive_menus() -> Result<()> {
    let (_guard, mut client) = spawn(4146)?;
    assert_mode(&mut client, "home", "boot")?;
    connect_pad(&mut client, PadFamily::Xbox)?;

    // The left stick presses Down once as it crosses the threshold...
    let before = ui(&mut client)?.selected;
    tilt_left_stick(&mut client, 0.0, 1.0)?;
    let first = ui(&mut client)?.selected;
    assert_ne!(first, before, "a push presses the arrow at once");
    // ...and held there keeps pressing it.
    let held = selections_over(&mut client, Duration::from_millis(1500))?;
    assert!(
        held.len() >= 3,
        "a held stick repeats the arrow: saw only {held:?}"
    );
    tilt_left_stick(&mut client, 0.0, 0.0)?;
    ui(&mut client)?;
    let resting = selections_over(&mut client, Duration::from_millis(800))?;
    assert_eq!(resting.len(), 1, "a centered stick is quiet: {resting:?}");

    // The bumpers step the tabs.
    activate_labeled_by_pad(&mut client, "settings")?;
    assert_mode(&mut client, "settings", "Home > Settings by pad")?;
    press_pad(&mut client, PadButton::RightShoulder)?;
    assert_mode(&mut client, "settings_audio", "RB steps right")?;
    press_pad(&mut client, PadButton::LeftShoulder)?;
    press_pad(&mut client, PadButton::LeftShoulder)?;
    assert_mode(&mut client, "controls", "LB steps left and wraps")?;
    press_pad(&mut client, PadButton::East)?;
    assert_mode(&mut client, "home", "Settings > B")?;

    // The triggers page the map grid, stopping at its ends.
    activate_labeled_by_pad(&mut client, "play")?;
    assert_mode(&mut client, "main_menu", "Home > Play by pad")?;
    assert_eq!(ui(&mut client)?.selected, Some(0));
    press_pad(&mut client, PadButton::RightTrigger)?;
    let paged = ui(&mut client)?.selected.unwrap_or(0);
    assert!(paged > 1, "RT moves a page, not one card: {paged}");
    press_pad(&mut client, PadButton::LeftTrigger)?;
    assert_eq!(ui(&mut client)?.selected, Some(0), "LT pages back");
    Ok(())
}

/// Polls the shell's status until `done` holds or a bounded wait passes.
fn wait_for_status(
    client: &mut Client,
    what: &str,
    done: impl Fn(&aftermine_protocol::StatusView) -> bool,
) -> Result<aftermine_protocol::StatusView> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = client.status()?;
        if done(&status) {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            bail!("{what}: status never arrived, last {status:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn a_controller_watches_a_replay() -> Result<()> {
    let recording =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/home/crossroads.amreplay");
    let (_guard, mut client) = spawn_prepared(4147, |home| {
        let replays = home.join("replays");
        std::fs::create_dir_all(&replays)?;
        std::fs::copy(&recording, replays.join("crossroads.amreplay"))?;
        Ok(())
    })?;
    assert_mode(&mut client, "home", "boot")?;
    connect_pad(&mut client, PadFamily::PlayStation)?;
    activate_labeled_by_pad(&mut client, "saves & replays")?;
    // The shelf opens on the tab that holds records once it has looked.
    wait_for_listed(&mut client)?;
    assert_mode(
        &mut client,
        "replays",
        "the shelf opens where the replay is",
    )?;
    press_pad(&mut client, PadButton::RightShoulder)?;
    assert_mode(&mut client, "saves", "R1 steps the tab")?;
    press_pad(&mut client, PadButton::LeftShoulder)?;
    assert_mode(&mut client, "replays", "L1 steps it back")?;
    press_pad(&mut client, PadButton::West)?;
    assert_mode(&mut client, "confirm_delete", "Square asks before deleting")?;
    press_pad(&mut client, PadButton::East)?;
    assert_mode(&mut client, "replays", "Circle keeps the replay")?;
    // The only record, so the cursor is already on it.
    press_pad(&mut client, PadButton::South)?;
    wait_for_mode(
        &mut client,
        "playback",
        &["replays", "loading"],
        "Cross watches",
    )?;

    let playing = client.status()?;
    assert!(!playing.paused, "a replay opens playing");
    press_pad(&mut client, PadButton::South)?;
    let paused = wait_for_status(&mut client, "Cross pauses", |s| s.paused)?;
    press_pad(&mut client, PadButton::RightShoulder)?;
    wait_for_status(&mut client, "R1 seeks forward 25 seconds", |s| {
        s.tick == paused.tick + 500
    })?;
    press_pad(&mut client, PadButton::LeftShoulder)?;
    wait_for_status(&mut client, "L1 seeks back 25 seconds", |s| {
        s.tick == paused.tick
    })?;
    press_pad(&mut client, PadButton::RightTrigger)?;
    wait_for_status(&mut client, "R2 speeds up", |s| {
        (s.speed - 2.0).abs() < f64::EPSILON
    })?;
    press_pad(&mut client, PadButton::LeftTrigger)?;
    press_pad(&mut client, PadButton::LeftTrigger)?;
    wait_for_status(&mut client, "L2 slows down", |s| {
        (s.speed - 0.5).abs() < f64::EPSILON
    })?;
    press_pad(&mut client, PadButton::South)?;
    wait_for_status(&mut client, "Cross plays again", |s| !s.paused)?;
    // The shelf is a front door: leaving a replay lands on Home.
    press_pad(&mut client, PadButton::East)?;
    wait_for_mode(
        &mut client,
        "home",
        &["playback"],
        "Circle leaves the replay",
    )?;
    Ok(())
}

/// Waits for the shelf's catalog to list the recording.
fn wait_for_listed(client: &mut Client) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while ui(client)?.mode != "replays" || ui(client)?.items.is_empty() {
        if Instant::now() >= deadline {
            bail!("the replays tab never listed the recording");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn a_modifier_held_on_another_screen_still_captures_its_chord() -> Result<()> {
    // A Ctrl pressed on another Settings tab must still read as held on
    // Controls, so the rebind captures the chord rather than a bare key.
    // The shared spawn helper keeps the persisted rebind inside this
    // test's scratch HOME.
    let (_guard, mut client) = spawn(4144)?;

    activate_labeled(&mut client, "settings")?;
    // Ctrl goes down HERE, on the General tab...
    client.call(Request::InjectEvent {
        event: RawEvent::KeyDown { key: Key::Ctrl },
    })?;
    select_tab(&mut client, "controls")?;
    // ...arm a binding row and press the key while Ctrl is still held.
    activate_labeled(&mut client, "pan up:")?;
    press_key(&mut client, Key::K)?;
    client.call(Request::InjectEvent {
        event: RawEvent::KeyUp { key: Key::Ctrl },
    })?;
    let view = ui(&mut client)?;
    let bound = view
        .items
        .iter()
        .find(|item| item.contains("Ctrl+K"))
        .cloned();
    if bound.is_none() {
        bail!(
            "no row shows Ctrl+K after a held-modifier rebind; rows: {:?}",
            view.items
        );
    }
    Ok(())
}

/// Waits until the settings file under `home` binds `key` to `action` in
/// either slot. The shell writes settings off its frame thread, so killing it
/// the moment a row shows a change could lose the write.
fn wait_for_saved_binding(home: &std::path::Path, key: &str, action: &str) -> Result<()> {
    let saved = || {
        [
            "Library/Application Support/Aftermine",
            ".config/aftermine",
            "AppData/Aftermine",
        ]
        .iter()
        .filter_map(|dir| std::fs::read(home.join(dir).join("config.json")).ok())
        .filter_map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .any(|config| {
            ["bindings", "secondary"].iter().any(|slot| {
                config["bindings"][slot].as_array().is_some_and(|bindings| {
                    bindings.iter().any(|binding| {
                        binding["chord"]["key"] == key && binding["action"] == action
                    })
                })
            })
        })
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !saved() {
        if Instant::now() >= deadline {
            bail!("the settings file never bound {key} to {action}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn controls_persist_secondary_bindings_and_keep_camera_keys_live_in_construction() -> Result<()> {
    let (mut guard, mut client) = spawn(4159)?;
    activate_labeled(&mut client, "settings")?;
    select_tab(&mut client, "controls")?;
    select_labeled(&mut client, "pan up:")?;
    press_key(&mut client, Key::Right)?;
    press_key(&mut client, Key::Enter)?;
    press_key(&mut client, Key::I)?;
    let rows = ui(&mut client)?.items;
    assert!(
        rows.iter().any(|row| row.contains("Pan up: W | [I]")),
        "{rows:?}"
    );
    wait_for_saved_binding(&guard.home, "i", "PanUp")?;
    drop(client);
    drop(guard.shell.take());
    let (shell, reconnected) = spawn_shell(&SpawnOptions {
        port: 4159,
        paused: true,
        home: Some(guard.home.clone()),
    })?;
    guard.shell = Some(shell);
    client = reconnected;
    activate_labeled(&mut client, "settings")?;
    select_tab(&mut client, "controls")?;
    assert!(
        ui(&mut client)?
            .items
            .iter()
            .any(|row| row.contains("Pan up: [W] | I"))
    );
    let mut scenario = aftermine_sim::Scenario::skirmish();
    scenario.players[0].scrap = 10000;
    scenario
        .buildings
        .push(aftermine_sim::scenario::BuildingSpec {
            player: 0,
            kind: aftermine_sim::BuildingKind::Fabricator,
            x: 9,
            y: 3,
        });
    let path = guard.home.join("controls-scenario.json");
    std::fs::write(&path, serde_json::to_vec(&scenario)?)?;
    client.call(Request::LoadScenario {
        path: path.to_string_lossy().into_owned(),
    })?;
    client.call(Request::Pause)?;
    press_key(&mut client, Key::B)?;
    press_key(&mut client, Key::R)?;
    press_key(&mut client, Key::Q)?;
    client.call(Request::InjectEvent {
        event: RawEvent::MouseMove { x: 640.0, y: 400.0 },
    })?;
    client.call(Request::InjectEvent {
        event: RawEvent::Wheel { delta: 4.0 },
    })?;
    std::thread::sleep(std::time::Duration::from_secs(1));
    let camera = |client: &mut Client| -> Result<[f64; 2]> {
        let camera = client.camera()?;
        Ok(camera.center)
    };
    let before = camera(&mut client)?;
    for key in [Key::W, Key::I] {
        client.call(Request::InjectEvent {
            event: RawEvent::KeyDown { key },
        })?;
    }
    std::thread::sleep(std::time::Duration::from_millis(120));
    client.call(Request::InjectEvent {
        event: RawEvent::KeyUp { key: Key::W },
    })?;
    let halfway = camera(&mut client)?;
    std::thread::sleep(std::time::Duration::from_millis(120));
    client.call(Request::InjectEvent {
        event: RawEvent::KeyUp { key: Key::I },
    })?;
    let after = camera(&mut client)?;
    assert!(
        halfway[1] < before[1] && after[1] < halfway[1],
        "{before:?} -> {halfway:?} -> {after:?}"
    );
    std::thread::sleep(std::time::Duration::from_millis(100));
    let stopped = camera(&mut client)?;
    assert_eq!(after, stopped);
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn a_remote_walks_the_menus_and_swipes_up_onto_the_tabs() -> Result<()> {
    let (_guard, mut client) = spawn(4160)?;
    assert_mode(&mut client, "home", "boot")?;
    connect_pad(&mut client, PadFamily::Remote)?;
    let before = ui(&mut client)?.selected;
    press_pad(&mut client, PadButton::DpadDown)?;
    assert_ne!(
        ui(&mut client)?.selected,
        before,
        "a swipe moves the cursor"
    );

    activate_labeled_by_pad(&mut client, "settings")?;
    assert_mode(&mut client, "settings", "Home > Settings by remote")?;
    let general = ui(&mut client)?.items;
    press_pad(&mut client, PadButton::DpadUp)?;
    press_pad(&mut client, PadButton::DpadRight)?;
    let next = ui(&mut client)?.items;
    assert_ne!(next, general, "swiping up then right changes the tab");
    press_pad(&mut client, PadButton::DpadLeft)?;
    assert_eq!(ui(&mut client)?.items, general, "swiping left comes back");
    press_pad(&mut client, PadButton::DpadDown)?;
    press_pad(&mut client, PadButton::DpadRight)?;
    assert_eq!(
        ui(&mut client)?.items,
        general,
        "back on the list, sideways changes a value and never the tab"
    );
    press_pad(&mut client, PadButton::East)?;
    assert_mode(&mut client, "home", "Settings > Back")?;
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn a_remote_finds_a_replay_on_the_other_tab_and_watches_it() -> Result<()> {
    let recording =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/home/crossroads.amreplay");
    let (_guard, mut client) = spawn_prepared(4161, |home| {
        let replays = home.join("replays");
        std::fs::create_dir_all(&replays)?;
        std::fs::copy(&recording, replays.join("crossroads.amreplay"))?;
        Ok(())
    })?;
    assert_mode(&mut client, "home", "boot")?;
    connect_pad(&mut client, PadFamily::Remote)?;
    activate_labeled_by_pad(&mut client, "saves & replays")?;
    wait_for_listed(&mut client)?;
    press_pad(&mut client, PadButton::DpadUp)?;
    press_pad(&mut client, PadButton::DpadRight)?;
    assert_mode(&mut client, "saves", "up then right reaches the other tab")?;
    press_pad(&mut client, PadButton::DpadLeft)?;
    assert_mode(&mut client, "replays", "left steps back")?;
    press_pad(&mut client, PadButton::South)?;
    assert_mode(&mut client, "replays", "a click returns to the list")?;
    // The only record, so the cursor is already on it.
    press_pad(&mut client, PadButton::South)?;
    wait_for_mode(
        &mut client,
        "playback",
        &["replays", "loading"],
        "a click watches",
    )?;

    press_pad(&mut client, PadButton::PlayPause)?;
    let paused = wait_for_status(&mut client, "Play/Pause pauses", |s| s.paused)?;
    press_pad(&mut client, PadButton::DpadRight)?;
    wait_for_status(&mut client, "a swipe right seeks forward", |s| {
        s.tick == paused.tick + 500
    })?;
    press_pad(&mut client, PadButton::DpadLeft)?;
    wait_for_status(&mut client, "a swipe left seeks back", |s| {
        s.tick == paused.tick
    })?;
    press_pad(&mut client, PadButton::DpadUp)?;
    wait_for_status(&mut client, "a swipe up speeds up", |s| {
        (s.speed - 2.0).abs() < f64::EPSILON
    })?;
    press_pad(&mut client, PadButton::DpadDown)?;
    press_pad(&mut client, PadButton::DpadDown)?;
    wait_for_status(&mut client, "a swipe down slows down", |s| {
        (s.speed - 0.5).abs() < f64::EPSILON
    })?;
    // In pan, swipes leave the replay's own controls alone.
    press_pad(&mut client, PadButton::South)?;
    let before = client.status()?;
    press_pad(&mut client, PadButton::DpadRight)?;
    press_pad(&mut client, PadButton::DpadUp)?;
    inject(
        &mut client,
        RawEvent::PadSwipe {
            pad: INJECTED_PAD,
            x: 0.5,
            y: 0.0,
        },
    )?;
    let after = client.status()?;
    assert_eq!(after.tick, before.tick, "a swipe in pan seeks nothing");
    assert!(
        (after.speed - before.speed).abs() < f64::EPSILON,
        "a swipe in pan changes no speed"
    );
    assert!(after.paused, "a click only changes mode");
    press_pad(&mut client, PadButton::East)?;
    wait_for_mode(&mut client, "home", &["playback"], "Back leaves the replay")?;
    Ok(())
}
