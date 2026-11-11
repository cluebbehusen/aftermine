//! The controller's rings, of commands on Y and of control groups on D-pad
//! right, driven through the real shell's input funnel with nothing but pad
//! events: the same `InjectEvent` requests `driver live inject-pad`,
//! `inject-pad-down`, `inject-pad-up` and `inject-pad-stick` send. `#[ignore]`d because each test spawns a real
//! window; run them explicitly, under Xvfb where there is no display:
//!
//! ```sh
//! xvfb-run -s "-screen 0 1280x800x24" \
//!     cargo test -p aftermine-driver --test pad_ring -- --ignored --test-threads 1
//! ```
//!
//! While a ring is open the UI view reports it as the menu that owns the
//! pad: its heading, its slices clockwise from the top, and the one the
//! stick points at. What a pick does shows in the world after a tick; the
//! debug protocol reports no selection, so which units take the next X
//! order proves what a group recalled.

use aftermine_driver::auto::{ShellGuard, SpawnOptions, spawn_shell, ui};
use aftermine_driver::client::Client;
use aftermine_protocol::{
    MouseButton, PadButton, PadFamily, RawEvent, Reply, Request, StateFilter, StateView, Stick,
    UiView,
};
use aftermine_sim::{BuildingId, BuildingKind, Order, PlayerId, UnitId, UnitKind};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT_HOME: AtomicU64 = AtomicU64::new(0);

const PAD: u64 = 0;
/// How close, in tiles, the reticle must come to a spot it aims at.
const AIM_TOLERANCE: f64 = 0.15;
/// Open ground in the drill, clear of every unit and structure.
const OPEN: [f64; 2] = [36.5, 26.5];

/// A spawned paused shell with an isolated HOME, driven by one pad.
struct Drill {
    shell: Option<ShellGuard>,
    client: Client,
    home: PathBuf,
}

impl Drill {
    fn spawn(port: u16) -> Result<Self> {
        Self::spawn_with(port, &quiet_config()?)
    }

    /// Spawns with `config` as the player's settings.
    fn spawn_with(port: u16, config: &Value) -> Result<Self> {
        let home = scratch_home(port)?;
        if let Err(error) = write_config(&home, config) {
            std::fs::remove_dir_all(&home).ok();
            return Err(error);
        }
        match spawn_shell(&SpawnOptions {
            port,
            paused: true,
            home: Some(home.clone()),
        }) {
            Ok((shell, client)) => Ok(Self {
                shell: Some(shell),
                client,
                home,
            }),
            Err(error) => {
                std::fs::remove_dir_all(&home).ok();
                Err(error)
            }
        }
    }

    /// Loads `scenario`, paused on the Playing screen, and connects the pad.
    fn load(&mut self, scenario: &Value) -> Result<StateView> {
        let path = self.home.join("ring-drill.json");
        std::fs::write(&path, serde_json::to_vec_pretty(scenario)?)?;
        expect_ok(self.client.call(Request::LoadScenario {
            path: path.to_string_lossy().into_owned(),
        })?)?;
        expect_ok(self.client.call(Request::Pause)?)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.ui()?.mode != "playing" {
            if Instant::now() > deadline {
                bail!("the drill never reached the playing screen");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.inject(RawEvent::PadConnected {
            pad: PAD,
            family: PadFamily::Xbox,
        })?;
        self.state()
    }

    /// One event into the funnel. The server holds later requests until a
    /// frame has applied it.
    fn inject(&mut self, event: RawEvent) -> Result<()> {
        expect_ok(self.client.call(Request::InjectEvent { event })?)
    }

    fn down(&mut self, button: PadButton) -> Result<()> {
        self.inject(RawEvent::PadDown { pad: PAD, button })
    }

    fn up(&mut self, button: PadButton) -> Result<()> {
        self.inject(RawEvent::PadUp { pad: PAD, button })
    }

    fn press(&mut self, button: PadButton) -> Result<()> {
        self.down(button)?;
        self.up(button)
    }

    fn stick(&mut self, x: f32, y: f32) -> Result<()> {
        self.inject(RawEvent::PadStick {
            pad: PAD,
            stick: Stick::Left,
            x,
            y,
        })
    }

    fn ui(&mut self) -> Result<UiView> {
        ui(&mut self.client)
    }

    /// The open ring's heading and slices; no heading while none is open.
    fn ring(&mut self) -> Result<(Option<String>, Vec<String>)> {
        let view = self.ui()?;
        Ok((view.title, view.items))
    }

    /// Pushes the stick toward the open ring's slice named `title` and
    /// lets it go, which keeps it pointed.
    fn point(&mut self, title: &str) -> Result<()> {
        let (_, slices) = self.ring()?;
        let index = slices
            .iter()
            .position(|slice| slice == title)
            .with_context(|| format!("no {title} slice in {slices:?}"))?;
        let angle = index as f32 * std::f32::consts::TAU / slices.len() as f32;
        self.stick(angle.sin(), -angle.cos())?;
        self.stick(0.0, 0.0)?;
        let pointed = self.ui()?.selected;
        if pointed != Some(index) {
            bail!("the stick pointed at {pointed:?}, not {title} at {index}");
        }
        Ok(())
    }

    /// Points at `title` and picks it with A.
    fn pick(&mut self, title: &str) -> Result<()> {
        self.point(title)?;
        self.press(PadButton::South)
    }

    /// Eases the left stick until the reticle marks `at`, then lets it go.
    /// The drill keeps every spot clear of the map's edge, so the reticle
    /// sits at the camera's center and the camera's center is its ground.
    fn aim(&mut self, at: [f64; 2]) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let camera = self.client.camera()?;
            let (dx, dy) = (at[0] - camera.center[0], at[1] - camera.center[1]);
            let distance = dx.hypot(dy);
            if distance <= AIM_TOLERANCE {
                return self.stick(0.0, 0.0);
            }
            if Instant::now() > deadline {
                bail!(
                    "the reticle never reached {at:?}; it rests at {:?}",
                    camera.center
                );
            }
            // Pan speed follows travel squared: ease off near the spot.
            let travel = (distance / 8.0).sqrt().clamp(0.12, 1.0);
            #[expect(clippy::cast_possible_truncation, reason = "a stick deflection")]
            self.stick(
                (dx / distance * travel) as f32,
                (dy / distance * travel) as f32,
            )?;
        }
    }

    /// Gives X at `at` and runs one tick, returning the world after it.
    fn order_at(&mut self, at: [f64; 2]) -> Result<StateView> {
        self.aim(at)?;
        self.press(PadButton::West)?;
        self.client.advance(1)?;
        self.state()
    }

    /// A left click on the world point `at`, as a mouse gives one.
    fn click_world(&mut self, at: [f64; 2]) -> Result<()> {
        let camera = self.client.camera()?;
        #[expect(clippy::cast_possible_truncation, reason = "a window position")]
        let (x, y) = (
            ((at[0] - camera.center[0]) * camera.zoom + camera.viewport[0] / 2.0) as f32,
            ((at[1] - camera.center[1]) * camera.zoom + camera.viewport[1] / 2.0) as f32,
        );
        self.inject(RawEvent::MouseMove { x, y })?;
        self.inject(RawEvent::MouseDown {
            button: MouseButton::Left,
            x,
            y,
        })?;
        self.inject(RawEvent::MouseUp {
            button: MouseButton::Left,
            x,
            y,
        })
    }

    fn state(&mut self) -> Result<StateView> {
        self.client.state(StateFilter::default())
    }
}

impl Drop for Drill {
    fn drop(&mut self) {
        drop(self.shell.take());
        if let Err(error) = std::fs::remove_dir_all(&self.home)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "warning: could not remove ring drill HOME {}: {error}",
                self.home.display()
            );
        }
    }
}

fn scratch_home(port: u16) -> Result<PathBuf> {
    loop {
        let id = NEXT_HOME.fetch_add(1, Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!(
            "aftermine-pad-ring-{}-{port}-{id}",
            std::process::id()
        ));
        match std::fs::create_dir(&home) {
            Ok(()) => return Ok(home),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
}

/// The driver's quiet settings: no music, sound or first-run prompts.
fn quiet_config() -> Result<Value> {
    Ok(serde_json::from_slice(include_bytes!(
        "../src/quiet_config.json"
    ))?)
}

fn write_config(home: &Path, config: &Value) -> Result<()> {
    let config = serde_json::to_vec_pretty(config)?;
    for dir in [
        home.join("Library/Application Support/Aftermine"),
        home.join(".config/aftermine"),
        home.join("AppData/Aftermine"),
    ] {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("config.json"), &config)?;
    }
    Ok(())
}

fn expect_ok(reply: Reply) -> Result<()> {
    match reply {
        Reply::Ok => Ok(()),
        other => bail!("expected ok, got {other:?}"),
    }
}

/// A 64 by 40 sandbox with scrap at (40, 20): seat 0 is yours with 1000
/// scrap and seat 1 a passive enemy with nothing.
fn scene(units: &[(UnitKind, i32, i32)], buildings: &[(BuildingKind, i32, i32)]) -> Value {
    let mut map = vec![".".repeat(64); 40];
    map[20].replace_range(40..41, "s");
    json!({
        "name": "Ring drill",
        "mode": "sandbox",
        "players": [
            { "name": "You", "scrap": 1000, "bot": false },
            { "name": "Red", "scrap": 0, "bot": false }
        ],
        "map": map,
        "units": units
            .iter()
            .map(|(kind, x, y)| json!({ "player": 0, "kind": kind, "x": x, "y": y }))
            .collect::<Vec<_>>(),
        "buildings": buildings
            .iter()
            .map(|(kind, x, y)| json!({ "player": 0, "kind": kind, "x": x, "y": y }))
            .collect::<Vec<_>>(),
    })
}

/// Two Sentinels.
fn squad() -> Value {
    scene(
        &[(UnitKind::Sentinel, 30, 20), (UnitKind::Sentinel, 31, 20)],
        &[],
    )
}

/// The center of tile `(x, y)`.
fn tile(x: i32, y: i32) -> [f64; 2] {
    [f64::from(x) + 0.5, f64::from(y) + 0.5]
}

/// Your units of `kind`, in id order.
fn own(view: &StateView, kind: UnitKind) -> Vec<UnitId> {
    let mut ids: Vec<UnitId> = view
        .units
        .iter()
        .filter(|unit| unit.player == 0 && unit.kind == kind)
        .map(|unit| UnitId(unit.id))
        .collect();
    ids.sort_unstable();
    ids
}

/// Your units with an order other than idle, in id order.
fn obeying(view: &StateView) -> Vec<UnitId> {
    let mut ids: Vec<UnitId> = view
        .units
        .iter()
        .filter(|unit| unit.player == 0 && !matches!(unit.order, Some(Order::Idle) | None))
        .map(|unit| UnitId(unit.id))
        .collect();
    ids.sort_unstable();
    ids
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_string()).collect()
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn y_opens_the_unit_ring_and_b_closes_it_keeping_the_selection() -> Result<()> {
    let mut drill = Drill::spawn(44_330)?;
    let view = drill.load(&squad())?;
    drill.press(PadButton::LeftShoulder)?;
    drill.press(PadButton::North)?;
    assert_eq!(
        drill.ring()?,
        (
            Some("COMMANDS".to_string()),
            strings(&["Run", "Hunt", "Patrol", "Accompany", "Retire", "Stop"])
        )
    );
    assert_eq!(drill.ui()?.selected, None, "nothing points until the stick");
    drill.press(PadButton::East)?;
    assert_eq!(drill.ring()?, (None, Vec::new()));
    // B closed only the ring: the squad is still selected and takes X.
    let after = drill.order_at(OPEN)?;
    assert_eq!(obeying(&after), own(&view, UnitKind::Sentinel));
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn a_picks_stop_and_the_squad_halts() -> Result<()> {
    let mut drill = Drill::spawn(44_331)?;
    let view = drill.load(&squad())?;
    drill.press(PadButton::LeftShoulder)?;
    let moving = drill.order_at(OPEN)?;
    assert_eq!(obeying(&moving), own(&view, UnitKind::Sentinel));
    drill.press(PadButton::North)?;
    drill.pick("Stop")?;
    assert_eq!(drill.ring()?, (None, Vec::new()), "Stop closes the ring");
    drill.client.advance(1)?;
    assert_eq!(obeying(&drill.state()?), Vec::new());
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn a_harvester_builds_through_the_build_rings() -> Result<()> {
    let mut drill = Drill::spawn(44_332)?;
    let view = drill.load(&drill_harvester())?;
    let harvester = own(&view, UnitKind::Harvester)[0];
    drill.press(PadButton::DpadDown)?;
    drill.press(PadButton::North)?;
    assert_eq!(
        drill.ring()?.1,
        strings(&[
            "Patrol",
            "Accompany",
            "Salvage",
            "Repair",
            "Retire",
            "Build",
            "Return cargo",
            "Stop"
        ])
    );
    drill.pick("Build")?;
    assert_eq!(drill.ring()?.0.as_deref(), Some("BUILD"));
    drill.pick("Defense")?;
    assert_eq!(
        drill.ring()?,
        (
            Some("DEFENSE".to_string()),
            strings(&["Turret", "Flak Turret", "Bastion", "Barricade"])
        )
    );
    drill.press(PadButton::East)?;
    assert_eq!(drill.ring()?.0.as_deref(), Some("BUILD"));
    drill.press(PadButton::East)?;
    assert_eq!(drill.ring()?.0.as_deref(), Some("COMMANDS"));
    drill.pick("Build")?;
    drill.pick("Defense")?;
    drill.pick("Turret")?;
    assert_eq!(drill.ring()?, (None, Vec::new()));
    // The pick armed placement as the build keys do: a click places it.
    let site = tile(26, 24);
    drill.click_world(site)?;
    drill.client.advance(1)?;
    let after = drill.state()?;
    let order = &after
        .units
        .iter()
        .find(|unit| unit.id == harvester.0)
        .context("the Harvester is gone")?
        .order;
    assert!(
        matches!(
            order,
            Some(
                Order::Found {
                    kind: BuildingKind::Turret,
                    ..
                } | Order::Build { .. }
            )
        ),
        "the Harvester goes to raise the Turret: {order:?}"
    );
    Ok(())
}

/// One Harvester, idle.
fn drill_harvester() -> Value {
    scene(&[(UnitKind::Harvester, 30, 20)], &[])
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn a_foundry_trains_counts_and_cancels_from_its_ring() -> Result<()> {
    let mut drill = Drill::spawn(44_333)?;
    drill.load(&scene(&[], &[(BuildingKind::Foundry, 30, 18)]))?;
    drill.press(PadButton::DpadUp)?;
    drill.press(PadButton::South)?;
    drill.press(PadButton::North)?;
    assert_eq!(
        drill.ring()?.1,
        strings(&[
            "Set rally",
            "Clear rally",
            "Harvester",
            "Sentinel",
            "Scuttler",
            "Excavator",
            "Surveyor"
        ])
    );
    drill.pick("Harvester")?;
    drill.press(PadButton::South)?;
    assert!(
        drill.ring()?.0.is_some(),
        "training leaves the ring open for more"
    );
    drill.client.advance(1)?;
    let queued = |view: &StateView| {
        view.buildings
            .iter()
            .find(|building| building.kind == BuildingKind::Foundry)
            .and_then(|building| building.queue.clone())
            .unwrap_or_default()
    };
    assert_eq!(
        queued(&drill.state()?),
        vec![UnitKind::Harvester, UnitKind::Harvester]
    );
    drill.press(PadButton::West)?;
    drill.client.advance(1)?;
    assert_eq!(queued(&drill.state()?), vec![UnitKind::Harvester]);
    // Holding B closes the ring however deep it stands.
    drill.down(PadButton::East)?;
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(drill.ring()?, (None, Vec::new()));
    drill.up(PadButton::East)?;
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn a_mixed_selection_opens_its_harvesters_from_one_slice() -> Result<()> {
    let mut drill = Drill::spawn(44_334)?;
    drill.load(&scene(
        &[(UnitKind::Sentinel, 30, 20), (UnitKind::Harvester, 26, 20)],
        &[],
    ))?;
    drill.aim(tile(26, 20))?;
    drill.press(PadButton::South)?;
    drill.down(PadButton::LeftTrigger)?;
    drill.aim(tile(30, 20))?;
    drill.press(PadButton::South)?;
    drill.up(PadButton::LeftTrigger)?;
    drill.press(PadButton::North)?;
    assert_eq!(
        drill.ring()?.1,
        strings(&[
            "Run",
            "Hunt",
            "Patrol",
            "Accompany",
            "Harvesters",
            "Retire",
            "Stop"
        ])
    );
    drill.pick("Harvesters")?;
    assert_eq!(
        drill.ring()?,
        (
            Some("HARVESTERS".to_string()),
            strings(&["Salvage", "Repair", "Build", "Return cargo"])
        )
    );
    drill.press(PadButton::North)?;
    assert_eq!(drill.ring()?, (None, Vec::new()), "Y closes every ring");
    Ok(())
}

/// Two Sentinels and, apart from them, a Scuttler.
fn squads() -> Value {
    scene(
        &[
            (UnitKind::Sentinel, 30, 20),
            (UnitKind::Sentinel, 31, 20),
            (UnitKind::Scuttler, 26, 24),
        ],
        &[],
    )
}

/// More open ground, clear of [`OPEN`] and of every unit.
const FAR: [f64; 2] = [22.5, 14.5];

fn group_titles() -> Vec<String> {
    strings(&["Group 1", "Group 2", "Group 3", "Group 4", "Group 5"])
}

/// Saves the Scuttler to Group 2 and the whole army to Group 1, then
/// clears the selection, all on the pad: the reticle and A select, LB the
/// army, and D-pad right then X on a slot saves it.
fn assign_squads(drill: &mut Drill) -> Result<()> {
    drill.aim(tile(26, 24))?;
    drill.press(PadButton::South)?;
    drill.press(PadButton::DpadRight)?;
    assert_eq!(drill.ring()?, (Some("GROUPS".to_string()), group_titles()));
    assert_eq!(drill.ui()?.selected, None, "nothing points until the stick");
    drill.point("Group 2")?;
    drill.press(PadButton::West)?;
    assert_eq!(drill.ring()?, (None, Vec::new()), "X closes the ring");
    drill.press(PadButton::LeftShoulder)?;
    drill.press(PadButton::DpadRight)?;
    drill.point("Group 1")?;
    drill.press(PadButton::West)?;
    // B with nothing armed clears the selection: X then orders nothing.
    drill.press(PadButton::East)?;
    let recorded = drill.client.status()?.recorded_commands;
    drill.order_at(OPEN)?;
    assert_eq!(drill.client.status()?.recorded_commands, recorded);
    Ok(())
}

/// Recalls `slot` from the group ring and gives X at `at`, returning the
/// units that then obey.
fn recall_and_order(drill: &mut Drill, slot: &str, at: [f64; 2]) -> Result<Vec<UnitId>> {
    drill.press(PadButton::DpadRight)?;
    drill.pick(slot)?;
    assert_eq!(drill.ring()?, (None, Vec::new()), "A closes the ring");
    Ok(obeying(&drill.order_at(at)?))
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn dpad_right_assigns_and_recalls_groups_on_the_pad_alone() -> Result<()> {
    let mut drill = Drill::spawn(44_335)?;
    let view = drill.load(&squads())?;
    assign_squads(&mut drill)?;
    let scuttler = own(&view, UnitKind::Scuttler);
    assert_eq!(recall_and_order(&mut drill, "Group 2", OPEN)?, scuttler);
    let mut army = [own(&view, UnitKind::Sentinel), scuttler].concat();
    army.sort_unstable();
    assert_eq!(recall_and_order(&mut drill, "Group 1", FAR)?, army);
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn each_rings_button_opens_it_in_the_others_place_and_closes_its_own() -> Result<()> {
    let mut drill = Drill::spawn(44_336)?;
    let view = drill.load(&squad())?;
    drill.press(PadButton::LeftShoulder)?;
    let heading = |drill: &mut Drill| -> Result<Option<String>> { Ok(drill.ring()?.0) };
    drill.press(PadButton::North)?;
    assert_eq!(heading(&mut drill)?.as_deref(), Some("COMMANDS"));
    drill.press(PadButton::DpadRight)?;
    assert_eq!(heading(&mut drill)?.as_deref(), Some("GROUPS"));
    drill.press(PadButton::North)?;
    assert_eq!(heading(&mut drill)?.as_deref(), Some("COMMANDS"));
    drill.press(PadButton::North)?;
    assert_eq!(heading(&mut drill)?, None);
    drill.press(PadButton::DpadRight)?;
    assert_eq!(heading(&mut drill)?.as_deref(), Some("GROUPS"));
    drill.press(PadButton::DpadRight)?;
    assert_eq!(heading(&mut drill)?, None);
    drill.press(PadButton::DpadRight)?;
    drill.press(PadButton::East)?;
    assert_eq!(heading(&mut drill)?, None);
    // Every ring closed without touching the selection: the squad takes X.
    let after = drill.order_at(OPEN)?;
    assert_eq!(obeying(&after), own(&view, UnitKind::Sentinel));
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn groups_assign_and_recall_with_the_group_bar_hidden() -> Result<()> {
    let mut config = quiet_config()?;
    config["control_groups"] = json!(false);
    let mut drill = Drill::spawn_with(44_337, &config)?;
    let view = drill.load(&squads())?;
    assign_squads(&mut drill)?;
    let scuttler = own(&view, UnitKind::Scuttler);
    assert_eq!(recall_and_order(&mut drill, "Group 2", OPEN)?, scuttler);
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn x_switches_auto_repair_on_the_repair_slice() -> Result<()> {
    let mut drill = Drill::spawn(44_338)?;
    let view = drill.load(&drill_harvester())?;
    let worker = own(&view, UnitKind::Harvester)[0];
    let auto_repair = |view: &StateView| {
        view.units
            .iter()
            .find(|unit| unit.id == worker.0)
            .and_then(|unit| unit.idle_weld)
    };
    let before = auto_repair(&view);
    drill.press(PadButton::DpadDown)?;
    drill.press(PadButton::North)?;
    drill.point("Repair")?;
    drill.press(PadButton::West)?;
    drill.client.advance(1)?;
    let after = auto_repair(&drill.state()?);
    assert!(
        before.is_some() && after == before.map(|on| !on),
        "X switched auto-repair: {before:?} then {after:?}"
    );
    Ok(())
}

/// One Lattice of yours.
fn drill_lattice() -> Value {
    scene(&[], &[(BuildingKind::Lattice, 30, 18)])
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn a_held_left_trigger_turns_order_into_fill() -> Result<()> {
    let mut drill = Drill::spawn(44_339)?;
    let view = drill.load(&drill_lattice())?;
    let lattice = view
        .buildings
        .iter()
        .find(|b| b.kind == BuildingKind::Lattice)
        .map(|b| BuildingId(b.id))
        .context("the Lattice stands")?;
    drill.aim([31.0, 19.0])?;
    drill.press(PadButton::South)?;
    drill.press(PadButton::North)?;
    drill.point("Order")?;
    drill.down(PadButton::LeftTrigger)?;
    assert!(
        drill
            .ring()?
            .1
            .iter()
            .any(|slice| slice.starts_with("Fill")),
        "LT shows Order as Fill"
    );
    drill.press(PadButton::South)?;
    drill.up(PadButton::LeftTrigger)?;
    drill.client.advance(1)?;
    let ordered = match drill.client.call(Request::QueryFogView {
        player: PlayerId(0),
    })? {
        Reply::Fog(fog) => fog
            .magazines
            .iter()
            .find(|magazine| magazine.building == lattice)
            .and_then(|magazine| magazine.detail)
            .map(|detail| detail.ordered),
        other => bail!("expected a fog view, got {other:?}"),
    };
    assert_eq!(ordered, Some(2), "one A filled both empty slots");
    Ok(())
}

#[test]
fn the_drills_build() -> Result<()> {
    for scenario in [
        squad(),
        squads(),
        drill_harvester(),
        drill_lattice(),
        scene(&[], &[(BuildingKind::Foundry, 30, 18)]),
    ] {
        aftermine_sim::Scenario::from_json(&scenario.to_string())?.build()?;
    }
    Ok(())
}
