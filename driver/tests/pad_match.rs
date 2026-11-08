//! Selection, orders, placement and targeted orders on a controller, driven
//! through the real shell's input funnel with pad events: the same
//! `InjectEvent` requests `driver live inject-pad`, `inject-pad-down`,
//! `inject-pad-up` and `inject-pad-stick` send. Until a controller can arm
//! a placement or a targeted order itself, its key arms it. `#[ignore]`d
//! because each test spawns a real window; run them explicitly, under Xvfb
//! where there is no display:
//!
//! ```sh
//! xvfb-run -s "-screen 0 1280x800x24" \
//!     cargo test -p aftermine-driver --test pad_match -- --ignored --test-threads 1
//! ```
//!
//! The debug protocol reports no selection, so each test proves what is
//! selected by which units take the next order: X stages it, one tick runs
//! it, and the units' orders say who obeyed.

use aftermine_driver::auto::{ShellGuard, SpawnOptions, spawn_shell, ui};
use aftermine_driver::client::Client;
use aftermine_protocol::{
    FogView, Key, PadButton, PadFamily, RawEvent, Reply, Request, StateFilter, StateView, Stick,
    UnitView,
};
use aftermine_sim::{
    AttackTarget, BuildingId, BuildingKind, Command, Order, PlayerId, Rally, Target, UnitId,
    UnitKind,
};
use anyhow::{Context, Result, bail};
use chassis::grid::TilePos;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT_HOME: AtomicU64 = AtomicU64::new(0);

const PAD: u64 = 0;
/// How close, in tiles, the reticle must come to a spot it aims at.
const AIM_TOLERANCE: f64 = 0.15;

/// A spawned paused shell with an isolated HOME, driven by one pad.
struct PadMatch {
    shell: Option<ShellGuard>,
    client: Client,
    home: PathBuf,
}

impl PadMatch {
    fn spawn(port: u16) -> Result<Self> {
        let home = scratch_home(port)?;
        write_config(&home)?;
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
        let path = self.home.join("pad-drill.json");
        std::fs::write(&path, serde_json::to_vec_pretty(scenario)?)?;
        expect_ok(self.client.call(Request::LoadScenario {
            path: path.to_string_lossy().into_owned(),
        })?)?;
        expect_ok(self.client.call(Request::Pause)?)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while ui(&mut self.client)?.mode != "playing" {
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

    /// Holds A at `from`, sweeps through `through` and lets A go.
    fn paint(&mut self, from: [f64; 2], through: &[[f64; 2]]) -> Result<()> {
        self.aim(from)?;
        self.down(PadButton::South)?;
        for at in through {
            self.aim(*at)?;
        }
        self.up(PadButton::South)
    }

    /// Gives X at `at` and runs one tick, returning the world after it.
    fn order_at(&mut self, at: [f64; 2]) -> Result<StateView> {
        self.aim(at)?;
        self.press(PadButton::West)?;
        self.client.advance(1)?;
        self.state()
    }

    fn state(&mut self) -> Result<StateView> {
        self.client.state(StateFilter::default())
    }

    /// What seat 0 knows.
    fn fog(&mut self) -> Result<FogView> {
        match self.client.call(Request::QueryFogView {
            player: PlayerId(0),
        })? {
            Reply::Fog(view) => Ok(view),
            other => bail!("expected a fog view, got {other:?}"),
        }
    }

    /// Presses and lets go of `key`, which arms a mode as the radial menu
    /// will once a controller can open it.
    fn key(&mut self, key: Key) -> Result<()> {
        self.inject(RawEvent::KeyDown { key })?;
        self.inject(RawEvent::KeyUp { key })
    }

    /// Arms placement from the build palette: B, then the category's key,
    /// then the structure's.
    fn arm_build(&mut self, category: Key, structure: Key) -> Result<()> {
        self.key(Key::B)?;
        self.key(category)?;
        self.key(structure)
    }

    /// Taps A at `at`, selecting what the reticle picks there.
    fn select_at(&mut self, at: [f64; 2]) -> Result<()> {
        self.aim(at)?;
        self.press(PadButton::South)
    }

    /// Commands recorded so far.
    fn recorded(&mut self) -> Result<usize> {
        Ok(self.client.status()?.recorded_commands)
    }

    fn command(&mut self, player: u8, command: Command) -> Result<()> {
        expect_ok(self.client.call(Request::SendCommand {
            player: PlayerId(player),
            command,
        })?)
    }
}

impl Drop for PadMatch {
    fn drop(&mut self) {
        drop(self.shell.take());
        if let Err(error) = std::fs::remove_dir_all(&self.home)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "warning: could not remove pad drill HOME {}: {error}",
                self.home.display()
            );
        }
    }
}

fn scratch_home(port: u16) -> Result<PathBuf> {
    loop {
        let id = NEXT_HOME.fetch_add(1, Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!(
            "aftermine-pad-match-{}-{port}-{id}",
            std::process::id()
        ));
        match std::fs::create_dir(&home) {
            Ok(()) => return Ok(home),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
}

fn write_config(home: &Path) -> Result<()> {
    let config = include_bytes!("../src/quiet_config.json");
    for dir in [
        home.join("Library/Application Support/Aftermine"),
        home.join(".config/aftermine"),
        home.join("AppData/Aftermine"),
    ] {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("config.json"), config)?;
    }
    Ok(())
}

fn expect_ok(reply: Reply) -> Result<()> {
    match reply {
        Reply::Ok => Ok(()),
        other => bail!("expected ok, got {other:?}"),
    }
}

/// A 64 by 40 sandbox with scrap at (40, 20): seat 0 is yours and seat 1
/// a passive enemy, holding `units` and `buildings` as (seat, kind, x, y).
fn drill(units: &[(u8, UnitKind, i32, i32)], buildings: &[(u8, BuildingKind, i32, i32)]) -> Value {
    let mut map = vec![".".repeat(64); 40];
    map[20].replace_range(40..41, "s");
    let entity = |(player, kind, x, y): (u8, Value, i32, i32)| json!({ "player": player, "kind": kind, "x": x, "y": y });
    json!({
        "name": "Pad drill",
        "mode": "sandbox",
        "players": [
            { "name": "You", "scrap": 1000, "bot": false },
            { "name": "Red", "scrap": 0, "bot": false }
        ],
        "map": map,
        "units": units
            .iter()
            .map(|&(player, kind, x, y)| entity((player, json!(kind), x, y)))
            .collect::<Vec<_>>(),
        "buildings": buildings
            .iter()
            .map(|&(player, kind, x, y)| entity((player, json!(kind), x, y)))
            .collect::<Vec<_>>(),
    })
}

/// Lancers along y 20 from x 27 to 30, Sentinels at (34, 24) and (35, 18),
/// a Harvester at (40, 24), one more Lancer below the row and one far out
/// of view. Each stands more than a full-grown brush from every sweep the
/// tests paint past it. An enemy Sentinel at (38, 13) stands inside your
/// Sentinel's sight but outside either side's aggro range, so nothing
/// engages it unbidden.
fn field() -> Value {
    drill(
        &[
            (0, UnitKind::Lancer, 27, 20),
            (0, UnitKind::Lancer, 28, 20),
            (0, UnitKind::Lancer, 29, 20),
            (0, UnitKind::Lancer, 30, 20),
            (0, UnitKind::Lancer, 28, 24),
            (0, UnitKind::Sentinel, 34, 24),
            (0, UnitKind::Sentinel, 35, 18),
            (0, UnitKind::Harvester, 40, 24),
            (1, UnitKind::Sentinel, 38, 13),
            (0, UnitKind::Lancer, 60, 36),
        ],
        &[(0, BuildingKind::Array, 24, 16)],
    )
}

/// The center of tile `(x, y)`.
fn tile(x: i32, y: i32) -> [f64; 2] {
    [f64::from(x) + 0.5, f64::from(y) + 0.5]
}

fn unit_at(view: &StateView, x: i32, y: i32) -> Result<UnitId> {
    view.units
        .iter()
        .find(|unit| unit.tile == [x, y])
        .map(|unit| UnitId(unit.id))
        .with_context(|| format!("no unit stands at ({x}, {y})"))
}

fn unit(view: &StateView, id: UnitId) -> Result<&UnitView> {
    view.units
        .iter()
        .find(|unit| unit.id == id.0)
        .with_context(|| format!("unit {id:?} is gone"))
}

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

/// Which of `ids` the camera shows now, by the bounds the shell sweeps.
fn in_view(drill: &mut PadMatch, view: &StateView, ids: &[UnitId]) -> Result<Vec<UnitId>> {
    let [min_x, min_y, max_x, max_y] = drill.client.camera()?.world_rect;
    let mut shown = Vec::new();
    for id in ids {
        let [x, y] = unit(view, *id)?.pos;
        if (min_x..=max_x).contains(&x) && (min_y..=max_y).contains(&y) {
            shown.push(*id);
        }
    }
    Ok(sorted(shown))
}

/// Every listed unit, in id order.
fn sorted(ids: impl IntoIterator<Item = UnitId>) -> Vec<UnitId> {
    let mut ids: Vec<UnitId> = ids.into_iter().collect();
    ids.sort_unstable();
    ids
}

/// An empty patch of ground to move to.
const OPEN: [f64; 2] = [31.5, 26.5];

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn a_picks_and_a_held_sweep_paints() -> Result<()> {
    let mut drill = PadMatch::spawn(44_311)?;
    let view = drill.load(&field())?;
    let row: Vec<UnitId> = (27..=30)
        .map(|x| unit_at(&view, x, 20))
        .collect::<Result<_>>()?;
    let below = unit_at(&view, 28, 24)?;

    // A picks the one Lancer the reticle brackets.
    drill.aim(tile(29, 20))?;
    drill.press(PadButton::South)?;
    assert_eq!(obeying(&drill.order_at(OPEN)?), vec![row[2]]);

    // Holding A and sweeping along the row paints all of it, and the
    // Lancer below the row stays out.
    let view = drill.load(&field())?;
    assert!(obeying(&view).is_empty());
    drill.paint(tile(25, 20), &[tile(31, 20)])?;
    assert_eq!(obeying(&drill.order_at(OPEN)?), row);

    // B mid-sweep abandons it and keeps what was selected.
    drill.load(&field())?;
    drill.aim(tile(28, 24))?;
    drill.press(PadButton::South)?;
    drill.aim(tile(25, 20))?;
    drill.down(PadButton::South)?;
    drill.aim(tile(31, 20))?;
    drill.press(PadButton::East)?;
    drill.up(PadButton::South)?;
    assert_eq!(obeying(&drill.order_at(OPEN)?), vec![below]);
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn a_double_tap_selects_that_kind_in_view() -> Result<()> {
    let mut drill = PadMatch::spawn(44_312)?;
    let view = drill.load(&field())?;
    drill.aim(tile(29, 20))?;
    drill.press(PadButton::South)?;
    drill.press(PadButton::South)?;
    let lancers = in_view(&mut drill, &view, &own(&view, UnitKind::Lancer))?;
    assert_eq!(lancers.len(), 5, "every Lancer but the far one shows");
    assert_eq!(obeying(&drill.order_at(OPEN)?), lancers);
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn x_gives_each_contextual_order() -> Result<()> {
    let mut drill = PadMatch::spawn(44_313)?;

    // Move: ground under the reticle.
    let view = drill.load(&field())?;
    let lancer = unit_at(&view, 29, 20)?;
    drill.aim(tile(29, 20))?;
    drill.press(PadButton::South)?;
    let after = drill.order_at(OPEN)?;
    assert!(
        matches!(unit(&after, lancer)?.order, Some(Order::Advance { .. })),
        "a move: {:?}",
        unit(&after, lancer)?.order
    );

    // Attack: the enemy Sentinel, the drill's one hostile. The sim keeps a
    // seen unit as the contact its owner tracks it by.
    let view = drill.load(&field())?;
    let sentinel = unit_at(&view, 35, 18)?;
    drill.aim(tile(35, 18))?;
    drill.press(PadButton::South)?;
    let after = drill.order_at(tile(38, 13))?;
    assert!(
        matches!(
            unit(&after, sentinel)?.order,
            Some(Order::Attack {
                target: AttackTarget::Contact(_),
                ..
            })
        ),
        "an attack: {:?}",
        unit(&after, sentinel)?.order
    );

    // Harvest: the scrap at (40, 20).
    let view = drill.load(&field())?;
    let harvester = unit_at(&view, 40, 24)?;
    drill.aim(tile(40, 24))?;
    drill.press(PadButton::South)?;
    let after = drill.order_at(tile(40, 20))?;
    assert!(
        matches!(unit(&after, harvester)?.order, Some(Order::Harvest { .. })),
        "a harvest: {:?}",
        unit(&after, harvester)?.order
    );

    // Repair: an Array the enemy has wounded.
    let view = drill.load(&drill_repair())?;
    let harvester = unit_at(&view, 22, 22)?;
    let attacker = unit_at(&view, 34, 19)?;
    let array = view
        .buildings
        .iter()
        .find(|building| building.player == 0)
        .map(|building| aftermine_sim::BuildingId(building.id))
        .context("the drill stands an Array")?;
    drill.command(
        1,
        Command::Attack {
            units: vec![attacker],
            target: Target::Building(array).into(),
            queue: false,
        },
    )?;
    let full = view.buildings[0].hp;
    let deadline = Instant::now() + Duration::from_secs(30);
    while drill.state()?.buildings[0].hp == full {
        if Instant::now() > deadline {
            bail!("the enemy never wounded the Array");
        }
        drill.client.advance(5)?;
    }
    drill.command(
        1,
        Command::Run {
            units: vec![attacker],
            goal: chassis::grid::TilePos::new(60, 4),
            queue: false,
        },
    )?;
    let before = drill.state()?;
    assert!(
        matches!(unit(&before, harvester)?.order, None | Some(Order::Idle)),
        "the Harvester waits for its order: {:?}",
        unit(&before, harvester)?.order
    );
    drill.aim(tile(22, 22))?;
    drill.press(PadButton::South)?;
    let after = drill.order_at(tile(32, 18))?;
    assert!(
        matches!(unit(&after, harvester)?.order, Some(Order::Repair { building }) if building == array),
        "a repair: {:?}",
        unit(&after, harvester)?.order
    );
    Ok(())
}

/// Your Array at (32, 18) with an enemy Sentinel beside it, and a Harvester
/// posted beyond the reach an idle welder tends from, so only the order
/// sends it.
fn drill_repair() -> Value {
    drill(
        &[
            (0, UnitKind::Harvester, 22, 22),
            (1, UnitKind::Sentinel, 34, 19),
        ],
        &[(0, BuildingKind::Array, 32, 18)],
    )
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn lt_queues_orders_and_rt_narrows_by_kind() -> Result<()> {
    let mut drill = PadMatch::spawn(44_314)?;
    let view = drill.load(&field())?;
    let lancer = unit_at(&view, 29, 20)?;
    drill.aim(tile(29, 20))?;
    drill.press(PadButton::South)?;
    drill.down(PadButton::LeftTrigger)?;
    drill.order_at(OPEN)?;
    let after = drill.order_at([OPEN[0] + 4.0, OPEN[1]])?;
    drill.up(PadButton::LeftTrigger)?;
    let held = unit(&after, lancer)?;
    assert!(
        matches!(held.order, Some(Order::Advance { .. })),
        "{:?}",
        held.order
    );
    assert!(
        matches!(held.queue.as_slice(), [Order::Advance { .. }]),
        "the second order waits behind the first: {:?}",
        held.queue
    );

    // A sweep over the row and both Sentinels holds two kinds; RT narrows
    // to the Lancers, then the Sentinels, then gives the group back.
    let view = drill.load(&field())?;
    let lancers: Vec<UnitId> = (27..=30)
        .map(|x| unit_at(&view, x, 20))
        .collect::<Result<_>>()?;
    let sentinels = own(&view, UnitKind::Sentinel);
    drill.paint(tile(25, 20), &[tile(31, 20), tile(34, 24), tile(35, 18)])?;
    drill.press(PadButton::RightTrigger)?;
    assert_eq!(obeying(&drill.order_at(OPEN)?), lancers);
    drill.press(PadButton::RightTrigger)?;
    let after = drill.order_at([OPEN[0] + 4.0, OPEN[1]])?;
    let moved: Vec<UnitId> = sentinels
        .iter()
        .copied()
        .filter(|id| {
            unit(&after, *id).is_ok_and(|unit| {
                matches!(unit.order, Some(Order::Advance { goal }) if (goal.x, goal.y) == (35, 26))
            })
        })
        .collect();
    assert_eq!(moved, sentinels, "the Sentinels took the second order");
    drill.press(PadButton::RightTrigger)?;
    let after = drill.order_at([OPEN[0] - 4.0, OPEN[1]])?;
    let regrouped: Vec<UnitId> = after
        .units
        .iter()
        .filter(|unit| {
            matches!(unit.order, Some(Order::Advance { goal }) if (goal.x, goal.y) == (27, 26))
        })
        .map(|unit| UnitId(unit.id))
        .collect();
    assert_eq!(
        sorted(regrouped),
        sorted(lancers.into_iter().chain(sentinels))
    );
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn b_backs_out_of_a_mode_then_the_selection() -> Result<()> {
    let mut drill = PadMatch::spawn(44_315)?;
    let view = drill.load(&field())?;
    let lancer = unit_at(&view, 29, 20)?;
    drill.aim(tile(29, 20))?;
    drill.press(PadButton::South)?;
    // No pad button arms a targeted order yet, so Hunt comes from its key.
    drill.inject(RawEvent::KeyDown { key: Key::F })?;
    drill.inject(RawEvent::KeyUp { key: Key::F })?;
    drill.press(PadButton::East)?;
    let after = drill.order_at(OPEN)?;
    assert!(
        matches!(unit(&after, lancer)?.order, Some(Order::Advance { .. })),
        "B stood Hunt down, so X moves: {:?}",
        unit(&after, lancer)?.order
    );
    drill.press(PadButton::East)?;
    let recorded = drill.client.status()?.recorded_commands;
    drill.order_at([OPEN[0] + 4.0, OPEN[1]])?;
    assert_eq!(
        drill.client.status()?.recorded_commands,
        recorded,
        "with nothing selected, X orders nothing"
    );
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn the_bumpers_select_the_army_and_the_army_in_view() -> Result<()> {
    let mut drill = PadMatch::spawn(44_316)?;
    let view = drill.load(&field())?;
    let army: Vec<UnitId> = sorted(
        own(&view, UnitKind::Lancer)
            .into_iter()
            .chain(own(&view, UnitKind::Sentinel)),
    );
    drill.press(PadButton::LeftShoulder)?;
    assert_eq!(obeying(&drill.order_at(OPEN)?), army);
    let view = drill.load(&field())?;
    drill.aim(tile(29, 20))?;
    let shown = in_view(&mut drill, &view, &army)?;
    assert!(shown.len() < army.len(), "the far Lancer is out of view");
    drill.press(PadButton::RightShoulder)?;
    assert_eq!(obeying(&drill.order_at(OPEN)?), shown);
    Ok(())
}

/// A Harvester below the middle of the map, and your Turret at (31, 20),
/// whose tile refuses a site.
fn drill_build() -> Value {
    drill(
        &[(0, UnitKind::Harvester, 32, 23)],
        &[(0, BuildingKind::Turret, 31, 20)],
    )
}

/// Your sites of `kind` by anchor, in row order.
fn sites(view: &StateView, kind: BuildingKind) -> Vec<[i32; 2]> {
    let mut anchors: Vec<[i32; 2]> = view
        .buildings
        .iter()
        .filter(|building| building.player == 0 && building.kind == kind)
        .map(|building| building.anchor)
        .collect();
    anchors.sort_unstable_by_key(|[x, y]| (*y, *x));
    anchors
}

/// The Harvester's tile.
const BUILDER: [f64; 2] = [32.5, 23.5];

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn a_places_a_structure_centered_on_the_reticle() -> Result<()> {
    let mut drill = PadMatch::spawn(44_317)?;
    drill.load(&drill_build())?;
    drill.select_at(BUILDER)?;
    // Production, then Fabricator: a 2x2.
    drill.arm_build(Key::E, Key::Q)?;
    drill.aim([35.0, 21.0])?;
    drill.press(PadButton::South)?;
    drill.client.advance(1)?;
    assert_eq!(
        sites(&drill.state()?, BuildingKind::Fabricator),
        [[34, 20]],
        "the footprint centers on the reticle"
    );

    // One placement ends the mode, so the next A selects instead.
    let recorded = drill.recorded()?;
    drill.select_at([38.5, 18.5])?;
    drill.client.advance(1)?;
    assert_eq!(drill.recorded()?, recorded);
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn a_held_a_drags_a_row_of_barricades() -> Result<()> {
    let mut drill = PadMatch::spawn(44_318)?;
    drill.load(&drill_build())?;
    drill.select_at(BUILDER)?;
    // Defense, then Barricade.
    drill.arm_build(Key::R, Key::T)?;
    drill.aim(tile(28, 20))?;
    drill.down(PadButton::South)?;
    drill.aim(tile(33, 20))?;
    drill.up(PadButton::South)?;
    drill.client.advance(1)?;
    assert_eq!(
        sites(&drill.state()?, BuildingKind::Barricade),
        [[28, 20], [29, 20], [30, 20], [32, 20], [33, 20]],
        "a row past the Turret, which keeps its tile"
    );
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn a_held_left_trigger_keeps_placing() -> Result<()> {
    let mut drill = PadMatch::spawn(44_319)?;
    drill.load(&drill_build())?;
    drill.select_at(BUILDER)?;
    drill.arm_build(Key::R, Key::T)?;
    drill.down(PadButton::LeftTrigger)?;
    drill.aim(tile(28, 18))?;
    drill.press(PadButton::South)?;
    drill.aim(tile(30, 18))?;
    drill.press(PadButton::South)?;
    drill.up(PadButton::LeftTrigger)?;
    // Letting go ended placement: this A selects.
    drill.select_at(tile(35, 18))?;
    drill.client.advance(1)?;
    assert_eq!(
        sites(&drill.state()?, BuildingKind::Barricade),
        [[28, 18], [30, 18]]
    );
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn a_refused_spot_builds_nothing_and_placement_stands() -> Result<()> {
    let mut drill = PadMatch::spawn(44_320)?;
    drill.load(&drill_build())?;
    drill.select_at(BUILDER)?;
    drill.arm_build(Key::R, Key::T)?;
    let recorded = drill.recorded()?;
    drill.aim(tile(31, 20))?;
    drill.press(PadButton::South)?;
    drill.client.advance(1)?;
    assert_eq!(drill.recorded()?, recorded, "the Turret's tile refuses");
    drill.aim(tile(29, 20))?;
    drill.press(PadButton::South)?;
    drill.client.advance(1)?;
    assert_eq!(sites(&drill.state()?, BuildingKind::Barricade), [[29, 20]]);
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn b_backs_out_of_placement() -> Result<()> {
    let mut drill = PadMatch::spawn(44_321)?;
    drill.load(&drill_build())?;
    drill.select_at(BUILDER)?;
    drill.arm_build(Key::R, Key::T)?;

    // B mid-row drops the row; the release then builds nothing.
    let recorded = drill.recorded()?;
    drill.aim(tile(27, 20))?;
    drill.down(PadButton::South)?;
    drill.aim(tile(29, 20))?;
    drill.press(PadButton::East)?;
    drill.up(PadButton::South)?;
    drill.client.advance(1)?;
    assert_eq!(drill.recorded()?, recorded);

    // Placement stood, so A builds.
    drill.aim(tile(28, 22))?;
    drill.press(PadButton::South)?;
    drill.client.advance(1)?;
    assert_eq!(sites(&drill.state()?, BuildingKind::Barricade), [[28, 22]]);

    // Armed again, B ends placement, and A no longer builds.
    drill.arm_build(Key::R, Key::T)?;
    drill.press(PadButton::East)?;
    let recorded = drill.recorded()?;
    drill.aim(tile(35, 22))?;
    drill.press(PadButton::South)?;
    drill.client.advance(1)?;
    assert_eq!(drill.recorded()?, recorded);
    Ok(())
}

/// A Lancer, your Fabricator and your Spire, on open ground.
fn drill_aim() -> Value {
    drill(
        &[(0, UnitKind::Lancer, 29, 20)],
        &[
            (0, BuildingKind::Fabricator, 22, 24),
            (0, BuildingKind::Spire, 24, 14),
        ],
    )
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn a_or_x_confirms_each_targeted_order() -> Result<()> {
    let mut drill = PadMatch::spawn(44_322)?;
    let lancer_tile = tile(29, 20);

    // Hunt with A.
    let view = drill.load(&drill_aim())?;
    let lancer = unit_at(&view, 29, 20)?;
    drill.select_at(lancer_tile)?;
    drill.key(Key::F)?;
    drill.aim(OPEN)?;
    drill.press(PadButton::South)?;
    drill.client.advance(1)?;
    let after = drill.state()?;
    assert!(
        matches!(unit(&after, lancer)?.order, Some(Order::Hunt { goal }) if (goal.x, goal.y) == (31, 26)),
        "a hunt: {:?}",
        unit(&after, lancer)?.order
    );

    // Run with X.
    drill.load(&drill_aim())?;
    drill.select_at(lancer_tile)?;
    drill.key(Key::G)?;
    drill.aim(OPEN)?;
    drill.press(PadButton::West)?;
    drill.client.advance(1)?;
    let after = drill.state()?;
    assert!(
        matches!(unit(&after, lancer)?.order, Some(Order::Run { goal }) if (goal.x, goal.y) == (31, 26)),
        "a run: {:?}",
        unit(&after, lancer)?.order
    );

    // Patrol: A and X lay the waypoints, and Patrol again sends it.
    drill.load(&drill_aim())?;
    drill.select_at(lancer_tile)?;
    drill.key(Key::R)?;
    drill.aim(tile(34, 22))?;
    drill.press(PadButton::South)?;
    drill.aim(tile(34, 17))?;
    drill.press(PadButton::West)?;
    drill.key(Key::R)?;
    drill.client.advance(1)?;
    let after = drill.state()?;
    assert_eq!(
        unit(&after, lancer)?.patrolling,
        Some(true),
        "a patrol: {:?}",
        unit(&after, lancer)?.order
    );

    // Set rally with X, from the Fabricator.
    let view = drill.load(&drill_aim())?;
    let fabricator = building(&view, BuildingKind::Fabricator)?;
    drill.select_at(tile(22, 24))?;
    drill.key(Key::Y)?;
    drill.aim(tile(36, 24))?;
    drill.press(PadButton::West)?;
    drill.client.advance(1)?;
    let rally = drill
        .state()?
        .buildings
        .iter()
        .find(|b| b.id == fabricator.0)
        .and_then(|b| b.rally);
    assert_eq!(rally, Some(Rally::Tile(TilePos::new(36, 24))));

    // Launch with A, once the Spire has a warhead loaded.
    let view = drill.load(&drill_aim())?;
    let spire = building(&view, BuildingKind::Spire)?;
    drill.command(0, Command::OrderRound { building: spire })?;
    let deadline = Instant::now() + Duration::from_secs(60);
    while !drill
        .fog()?
        .magazines
        .iter()
        .any(|magazine| magazine.building == spire && magazine.loaded > 0)
    {
        if Instant::now() > deadline {
            bail!("the Spire never loaded its warhead");
        }
        drill.client.advance(100)?;
    }
    drill.select_at(tile(24, 14))?;
    drill.key(Key::M)?;
    drill.aim(tile(40, 26))?;
    drill.press(PadButton::South)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let impact = loop {
        if let Some(line) = drill
            .fog()?
            .warheads
            .iter()
            .find_map(|warhead| warhead.line)
        {
            break TilePos::containing(line.impact);
        }
        if Instant::now() > deadline {
            bail!("no warhead flew");
        }
        drill.client.advance(5)?;
    };
    assert_eq!(impact, TilePos::new(40, 26));
    Ok(())
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn rt_widens_one_kind_to_every_one_on_the_map() -> Result<()> {
    let mut drill = PadMatch::spawn(44_324)?;
    let view = drill.load(&field())?;
    drill.select_at(tile(27, 20))?;
    drill.press(PadButton::RightTrigger)?;
    assert_eq!(
        obeying(&drill.order_at(OPEN)?),
        own(&view, UnitKind::Lancer),
        "every Lancer, the one far out of view too"
    );
    Ok(())
}

/// Two Spires of yours and a Lancer, on open ground, with the scrap for a
/// warhead in each.
fn drill_salvo() -> Value {
    let mut scenario = drill(
        &[(0, UnitKind::Lancer, 29, 20)],
        &[
            (0, BuildingKind::Spire, 24, 14),
            (0, BuildingKind::Spire, 30, 14),
        ],
    );
    scenario["players"][0]["scrap"] = json!(2000);
    scenario
}

#[test]
#[ignore = "spawns a real window; run explicitly in the phase battery"]
fn launch_takes_the_bumpers_over_to_fire_more() -> Result<()> {
    let mut drill = PadMatch::spawn(44_325)?;
    let view = drill.load(&drill_salvo())?;
    let spires: Vec<BuildingId> = view
        .buildings
        .iter()
        .filter(|b| b.kind == BuildingKind::Spire)
        .map(|b| BuildingId(b.id))
        .collect();
    for &building in &spires {
        drill.command(0, Command::OrderRound { building })?;
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    while drill
        .fog()?
        .magazines
        .iter()
        .filter(|magazine| spires.contains(&magazine.building) && magazine.loaded > 0)
        .count()
        < spires.len()
    {
        if Instant::now() > deadline {
            bail!("the Spires never loaded their warheads");
        }
        drill.client.advance(100)?;
    }
    // A picks one Spire and RT widens to both, as the All button does.
    drill.select_at(tile(24, 14))?;
    drill.press(PadButton::RightTrigger)?;
    drill.key(Key::M)?;
    drill.press(PadButton::RightShoulder)?;
    drill.aim(tile(40, 26))?;
    drill.press(PadButton::South)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while drill.fog()?.warheads.len() < 2 {
        if Instant::now() > deadline {
            bail!("RB never raised the salvo to both warheads");
        }
        drill.client.advance(5)?;
    }
    Ok(())
}

/// Your one building of `kind`.
fn building(view: &StateView, kind: BuildingKind) -> Result<BuildingId> {
    view.buildings
        .iter()
        .find(|building| building.player == 0 && building.kind == kind)
        .map(|building| BuildingId(building.id))
        .with_context(|| format!("no {kind:?} stands"))
}

#[test]
fn the_drills_build() -> Result<()> {
    for scenario in [
        field(),
        drill_repair(),
        drill_build(),
        drill_aim(),
        drill_salvo(),
    ] {
        aftermine_sim::Scenario::from_json(&scenario.to_string())?.build()?;
    }
    Ok(())
}
