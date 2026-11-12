//! Controller wiring through a real shell: pad events sent over the debug
//! socket, as `driver live inject-pad` sends them, reach the frame's input
//! funnel and come out as an order, a ring pick, a placement, the queue
//! strip and the Siri Remote's way to Pause. The rules behind each press
//! are the shell's unit tests; these drills prove that the window plays
//! them. `#[ignore]`d because each test spawns a real window; run them
//! explicitly, under Xvfb where there is no display:
//!
//! ```sh
//! xvfb-run -s "-screen 0 1280x800x24" \
//!     cargo test -p aftermine-driver --test pad_wiring -- --ignored --test-threads 1
//! ```
//!
//! The debug protocol reports no selection, so a drill proves what is
//! selected by which units take the next order: X stages it, one tick runs
//! it, and the units' orders say who obeyed.

use aftermine_driver::auto::{
    INJECTED_PAD, ScratchShell, connect_pad, inject, press_key, ui, write_home_config,
};
use aftermine_driver::client::Client;
use aftermine_protocol::{
    Key, PadButton, PadFamily, RawEvent, Request, StateFilter, StateView, Stick,
};
use aftermine_sim::{BuildingId, BuildingKind, Command, Order, PlayerId, UnitId, UnitKind};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// How close, in tiles, the reticle must come to a spot it aims at.
const AIM_TOLERANCE: f64 = 0.15;
/// The Siri Remote's pad id, beside the scripted controller's.
const REMOTE: u64 = 9;

/// A spawned paused shell on the driver's quiet settings, driven by one pad.
struct Drill {
    client: Client,
    shell: ScratchShell,
}

impl Drill {
    fn spawn(port: u16) -> Result<Self> {
        let mut shell = ScratchShell::new("pad-wiring")?;
        write_home_config(shell.home(), include_str!("../src/quiet_config.json"))?;
        let client = shell.spawn(port, true)?;
        Ok(Self { client, shell })
    }

    /// Loads `scenario`, paused on the Playing screen, and connects the pad.
    fn load(&mut self, scenario: &Value) -> Result<StateView> {
        let path = self.shell.home().join("pad-drill.json");
        std::fs::write(&path, serde_json::to_vec_pretty(scenario)?)?;
        self.client.call(Request::LoadScenario {
            path: path.to_string_lossy().into_owned(),
        })?;
        self.client.call(Request::Pause)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while ui(&mut self.client)?.mode != "playing" {
            if Instant::now() > deadline {
                bail!("the drill never reached the playing screen");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        connect_pad(&mut self.client, PadFamily::Xbox)?;
        self.state()
    }

    fn press(&mut self, button: PadButton) -> Result<()> {
        press(&mut self.client, INJECTED_PAD, button)
    }

    fn stick(&mut self, x: f32, y: f32) -> Result<()> {
        inject(
            &mut self.client,
            RawEvent::PadStick {
                pad: INJECTED_PAD,
                stick: Stick::Left,
                x,
                y,
            },
        )
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

    /// Taps A at `at`, selecting what the reticle picks there.
    fn select_at(&mut self, at: [f64; 2]) -> Result<()> {
        self.aim(at)?;
        self.press(PadButton::South)
    }

    /// Gives X at `at` and runs one tick, returning the world after it.
    fn order_at(&mut self, at: [f64; 2]) -> Result<StateView> {
        self.aim(at)?;
        self.press(PadButton::West)?;
        self.client.advance(1)?;
        self.state()
    }

    /// Points the stick at the open ring's slice named `title`, lets it
    /// go, which keeps it pointed, and picks it with A.
    fn pick(&mut self, title: &str) -> Result<()> {
        let slices = ui(&mut self.client)?.items;
        let index = slices
            .iter()
            .position(|slice| slice == title)
            .with_context(|| format!("no {title} slice in {slices:?}"))?;
        let angle = index as f32 * std::f32::consts::TAU / slices.len() as f32;
        self.stick(angle.sin(), -angle.cos())?;
        self.stick(0.0, 0.0)?;
        let pointed = ui(&mut self.client)?.selected;
        if pointed != Some(index) {
            bail!("the stick pointed at {pointed:?}, not {title} at {index}");
        }
        self.press(PadButton::South)
    }

    fn state(&mut self) -> Result<StateView> {
        self.client.state(StateFilter::default())
    }

    /// Commands recorded so far.
    fn recorded(&mut self) -> Result<usize> {
        Ok(self.client.status()?.recorded_commands)
    }
}

/// One full press of `button` on `pad`.
fn press(client: &mut Client, pad: u64, button: PadButton) -> Result<()> {
    inject(client, RawEvent::PadDown { pad, button })?;
    inject(client, RawEvent::PadUp { pad, button })
}

/// A 64 by 40 sandbox: seat 0 is yours, holding `units` and `buildings` as
/// (kind, x, y), and seat 1 a passive enemy with nothing.
fn drill(units: &[(UnitKind, i32, i32)], buildings: &[(BuildingKind, i32, i32)]) -> Value {
    let own = |kind: Value, x: i32, y: i32| json!({ "player": 0, "kind": kind, "x": x, "y": y });
    json!({
        "name": "Pad drill",
        "mode": "sandbox",
        "players": [
            { "name": "You", "scrap": 1000, "bot": false },
            { "name": "Red", "scrap": 0, "bot": false }
        ],
        "map": vec![".".repeat(64); 40],
        "units": units
            .iter()
            .map(|&(kind, x, y)| own(json!(kind), x, y))
            .collect::<Vec<_>>(),
        "buildings": buildings
            .iter()
            .map(|&(kind, x, y)| own(json!(kind), x, y))
            .collect::<Vec<_>>(),
    })
}

/// Three Lancers along y 20, a tile apart.
fn field() -> Value {
    drill(
        &[
            (UnitKind::Lancer, 28, 20),
            (UnitKind::Lancer, 29, 20),
            (UnitKind::Lancer, 30, 20),
        ],
        &[],
    )
}

/// A Harvester below the middle of the map.
fn drill_build() -> Value {
    drill(&[(UnitKind::Harvester, 32, 23)], &[])
}

/// Your Foundry, on open ground.
fn drill_foundry() -> Value {
    drill(&[], &[(BuildingKind::Foundry, 30, 18)])
}

/// The center of tile `(x, y)`.
fn tile(x: i32, y: i32) -> [f64; 2] {
    [f64::from(x) + 0.5, f64::from(y) + 0.5]
}

/// An empty patch of ground to move to, on tile (31, 26).
const OPEN: [f64; 2] = [31.5, 26.5];

/// Your units with an order other than idle, in id order.
fn obeying(view: &StateView) -> Vec<UnitId> {
    let mut ids: Vec<UnitId> = view
        .units
        .iter()
        .filter(|unit| unit.player == 0 && !matches!(unit.order, None | Some(Order::Idle)))
        .map(|unit| UnitId(unit.id))
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn a_selects_and_x_moves_to_the_ground_under_the_reticle() -> Result<()> {
    let mut drill = Drill::spawn(44_311)?;
    let view = drill.load(&field())?;
    let lancer = view
        .units
        .iter()
        .find(|unit| unit.tile == [29, 20])
        .map(|unit| UnitId(unit.id))
        .context("a Lancer stands at (29, 20)")?;
    drill.select_at(tile(29, 20))?;
    let seen = ui(&mut drill.client)?;
    assert_eq!(
        seen.hud_buttons,
        Some(PadFamily::Xbox),
        "the HUD names the pad's buttons"
    );
    assert_eq!(seen.menu_button, None, "a pad press hides Menu");
    assert_eq!(seen.controller_notice, None, "a controller is connected");
    let after = drill.order_at(OPEN)?;
    assert_eq!(
        obeying(&after),
        [lancer],
        "A picked the one Lancer the reticle brackets"
    );
    let order = &after
        .units
        .iter()
        .find(|unit| unit.id == lancer.0)
        .context("the Lancer is gone")?
        .order;
    assert!(
        matches!(order, Some(Order::Advance { goal }) if (goal.x, goal.y) == (31, 26)),
        "X moved it to the reticle's tile: {order:?}"
    );
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn a_picks_stop_and_the_squad_halts() -> Result<()> {
    let mut drill = Drill::spawn(44_312)?;
    let view = drill.load(&field())?;
    let mut army: Vec<UnitId> = view.units.iter().map(|unit| UnitId(unit.id)).collect();
    army.sort_unstable();
    drill.press(PadButton::LeftShoulder)?;
    assert_eq!(obeying(&drill.order_at(OPEN)?), army);
    drill.press(PadButton::North)?;
    assert_eq!(ui(&mut drill.client)?.title.as_deref(), Some("COMMANDS"));
    drill.pick("Stop")?;
    assert_eq!(ui(&mut drill.client)?.title, None, "Stop closes the ring");
    drill.client.advance(1)?;
    assert_eq!(obeying(&drill.state()?), Vec::new());
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn a_places_a_structure_centered_on_the_reticle() -> Result<()> {
    let mut drill = Drill::spawn(44_313)?;
    drill.load(&drill_build())?;
    drill.select_at(tile(32, 23))?;
    // The build palette's keys arm the placement: Production, then
    // Fabricator, a 2x2.
    for key in [Key::B, Key::E, Key::Q] {
        press_key(&mut drill.client, key)?;
    }
    drill.aim([35.0, 21.0])?;
    drill.press(PadButton::South)?;
    drill.client.advance(1)?;
    let sites: Vec<[i32; 2]> = drill
        .state()?
        .buildings
        .iter()
        .filter(|building| building.kind == BuildingKind::Fabricator)
        .map(|building| building.anchor)
        .collect();
    assert_eq!(sites, [[34, 20]], "the footprint centers on the reticle");

    // One placement ends the mode, so the next A selects instead.
    let recorded = drill.recorded()?;
    drill.select_at([38.5, 18.5])?;
    drill.client.advance(1)?;
    assert_eq!(drill.recorded()?, recorded);
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn the_left_stick_click_walks_and_cancels_the_queue() -> Result<()> {
    let mut drill = Drill::spawn(44_314)?;
    let view = drill.load(&drill_foundry())?;
    let foundry = BuildingId(view.buildings.first().context("the Foundry stands")?.id);
    for _ in 0..3 {
        drill.client.call(Request::SendCommand {
            player: PlayerId(0),
            command: Command::Train {
                building: foundry,
                kind: UnitKind::Harvester,
            },
        })?;
    }
    drill.client.advance(1)?;
    let queued = |view: &StateView| {
        view.buildings
            .iter()
            .find(|b| b.id == foundry.0)
            .and_then(|b| b.queue.as_ref())
            .map_or(0, Vec::len)
    };
    // D-pad up centers the Foundry under the reticle and A selects it.
    drill.press(PadButton::DpadUp)?;
    drill.press(PadButton::South)?;
    drill.press(PadButton::LeftStick)?;
    let strip = ui(&mut drill.client)?;
    assert_eq!(strip.title.as_deref(), Some("Queue"));
    assert_eq!(
        strip.items.len(),
        4,
        "three chips and Stop: {:?}",
        strip.items
    );
    assert_eq!(strip.selected, Some(3), "the chip nearest the band is lit");
    drill.press(PadButton::South)?;
    drill.client.advance(1)?;
    assert_eq!(queued(&drill.state()?), 2, "A cancelled the lit chip");
    let camera = drill.client.camera()?.center;
    drill.stick(1.0, 0.0)?;
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(drill.client.camera()?.center, camera, "the camera holds");
    drill.stick(0.0, 0.0)?;
    for _ in 0..4 {
        if ui(&mut drill.client)?.selected == Some(0) {
            break;
        }
        drill.press(PadButton::DpadUp)?;
    }
    assert_eq!(
        ui(&mut drill.client)?.selected,
        Some(0),
        "Stop tops the dock"
    );
    drill.press(PadButton::South)?;
    drill.client.advance(1)?;
    assert_eq!(queued(&drill.state()?), 0, "Stop cleared the queue");
    assert_eq!(
        ui(&mut drill.client)?.title,
        None,
        "with nothing left to press the strip closed"
    );
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run with --ignored --test-threads 1"]
fn a_remote_in_a_match_reaches_only_pause() -> Result<()> {
    let mut drill = Drill::spawn(44_315)?;
    drill.load(&field())?;
    inject(
        &mut drill.client,
        RawEvent::PadConnected {
            pad: REMOTE,
            family: PadFamily::Remote,
        },
    )?;
    let before = drill.recorded()?;
    for button in [
        PadButton::South,
        PadButton::West,
        PadButton::North,
        PadButton::DpadUp,
        PadButton::DpadDown,
        PadButton::DpadLeft,
        PadButton::DpadRight,
    ] {
        press(&mut drill.client, REMOTE, button)?;
        assert_eq!(ui(&mut drill.client)?.mode, "playing", "{button:?}");
    }
    assert_eq!(drill.recorded()?, before, "the remote gave no order");

    press(&mut drill.client, REMOTE, PadButton::East)?;
    assert_eq!(
        ui(&mut drill.client)?.mode,
        "pause_menu",
        "Back opens Pause"
    );
    // Pause is a menu, so the remote drives it like any other: a click on
    // its first row resumes.
    press(&mut drill.client, REMOTE, PadButton::South)?;
    assert_eq!(ui(&mut drill.client)?.mode, "playing", "Resume by click");
    Ok(())
}

#[test]
fn the_drills_build() -> Result<()> {
    for scenario in [field(), drill_build(), drill_foundry()] {
        aftermine_sim::Scenario::from_json(&scenario.to_string())?.build()?;
    }
    Ok(())
}
