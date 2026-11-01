# Aftermine

Aftermine is a 2D real-time strategy game about autonomous machines salvaging an
exhausted mining world. Rival machine crews compete for the last useful scrap at
the bottom of abandoned open-pit quarries.

It is also an experiment in AI-assisted game development. The game is built
around a pure deterministic simulation, a thin graphical shell, and a driver
that can inspect or play matches without a person at the keyboard. That
architecture makes bugs reproducible and lets every live match become a replay.

## Run the game

Aftermine requires a current Rust toolchain. From the repository root:

```sh
cargo run -p aftermine-shell --release
```

To play on an iPad, see [the iPad build](shell/README.md#ipad-build).

The main menu has the tutorial, match setup, LAN play, saved games and replays,
the unit roster, settings and controls. Bots come in four difficulties and three
stances (Turtle, Balanced and Aggressive), and each one has a seeded
personality. They play by the same rules as you, with no extra income, vision or
units. The [opponent specification](docs/aftermine-opponent.md) describes how
they play.

## The game

Harvesters gather scrap, bring it back to a Foundry, build structures, salvage
wrecks and repair damaged units. Repair Bays heal units near them. Foundries and
other production structures turn scrap into an army. Destroy every enemy Foundry
to win.

- Fog of war with remembered structures, radar contacts and shared team sight.
- Ground and air units with direct-fire, artillery, stealth, transport and
  repair roles.
- Expansion Foundries, Extractors on fixed map frames, structure upgrades, and
  wreck salvage that makes holding a battlefield pay.
- Spires, whose warheads can strike any tile, and Lattices that intercept them.
- Two experimental units, the Maw and the Roc, which workers assemble in cradles
  every player can see.
- Duels, team games and free-for-alls.

Rock blocks ground movement and direct fire. Peaks block everything, aircraft
and blasts included. Pits block ground movement, but aircraft and fire cross
them. On tiered maps, higher ground blocks direct fire, and artillery fires down
but never up.

The in-game Roster lists every unit and structure with its cost, weapons and
prerequisites.

## Controls

Most actions are also cards in the selection panel. The core keys:

| Input                           | Action                                                                  |
| ------------------------------- | ----------------------------------------------------------------------- |
| WASD, arrow keys or middle drag | Pan the camera.                                                         |
| Mouse wheel                     | Zoom.                                                                   |
| Left click or drag              | Select a unit, structure or group.                                      |
| Shift + click or drag           | Add to or remove from the selection.                                    |
| Right click                     | Give the contextual order: harvest, attack, repair or move.             |
| Shift + right click             | Queue the order.                                                        |
| `F`, then ground                | Move and fight anything on the way.                                     |
| `G`, then ground                | Move without stopping to fight.                                         |
| `B`                             | Open construction: pick a category, then a structure, with Q/E/R/T/Z/X. |
| Q/E/R/T/Z/X with a producer     | Train the unit in that slot.                                            |
| `X`                             | Stop the selection.                                                     |
| `1`–`5` / Ctrl + `1`–`5`        | Recall / assign a control group.                                        |
| `N`                             | Select the next idle Harvester.                                         |
| `Space`                         | Center the camera on your Foundry.                                      |
| `P`                             | Pause.                                                                  |
| `Esc`                           | Cancel the current action, clear the selection, or open the pause menu. |

Settings > Controls lists every action, and each one can be rebound.

On iPad:

- Tap to select, drag to pan and pinch to zoom.
- Long-press for the contextual order.
- Rest two fingers to draw a selection box. Double-tap a unit to select every
  unit of its kind on screen.
- The QUEUE toggle stands in for Shift.
- Rest a finger on a card to read it, then lift to use it or slide off to
  cancel.

## Saves and replays

Continue resumes the latest autosave, and named saves restore a match exactly
where it stopped, paused. Finished matches are kept as replays you can watch and
scrub from the Replays shelf. A save keeps the match's command history, so a
resumed match still records its whole replay.

Restart, Rematch, loading a save and watching a replay keep each opponent's
difficulty, stance and personality. New Match picks new personalities.

Aftermine is pre-launch, so saves and replays from other builds are not
supported. [`docs/versioning.md`](docs/versioning.md) explains what changes at
launch.

## Architecture

Each crate owns one layer of the game and has its own README with its purpose,
main modules and development commands.

- [`chassis/`](chassis/README.md) is the reusable deterministic-simulation
  foundation: fixed-point geometry, stable randomness and hashing, pathfinding,
  replay records, and durable writes. It contains no Aftermine rules or
  rendering.
- [`sim/`](sim/README.md) is `aftermine-sim`, the pure headless game. It owns
  every rule; `State::tick(&[PlayerCommand])` is its only state transition.
- [`opponent/`](opponent/README.md) is `aftermine-opponent`, the reactive,
  best-effort opponent every bot seat runs. It reads simulation-owned,
  fog-honest observations and keeps policy outside the rules.
- [`protocol/`](protocol/README.md) defines the JSON-lines debug contract,
  hardware-neutral input events, state views, fog-honest views, and transport
  shared by live and windowless sessions.
- [`net/`](net/README.md) is `aftermine-net`, lockstep multiplayer: the
  messages, the host and client sessions, and the TCP and UDP transport.
- [`kit/`](kit/README.md) contains Aftermine-specific services shared by the
  shell and driver, including headless running, replay playback, statistics,
  combat fixtures, and the deterministic CPU renderer.
- [`shell/`](shell/README.md) is the playable Macroquad application. It owns
  input, UI, rendering, audio, persistence, and the live debug server, but may
  affect a match only by staging recorded commands.
- [`driver/`](driver/README.md) is the headless and live QA harness. It runs and
  inspects matches and replays, audits maps, drives the real shell, profiles
  frames, and serves the same protocol without a window.

Supporting directories:

- [`scenarios/`](scenarios/) contains shipped match definitions and ASCII maps.
- [`assets/`](assets/) contains production sprites and sounds.
- [`tools/`](tools/) contains deterministic asset generators and review tools.
- [`.agents/skills/`](.agents/skills/) contains procedures for simulation work,
  the opponent, live QA, maps, visual assets and sound.

The core rule is:

> The same scenario and command log must produce a bit-identical state on every
> run and every supported platform.

Simulation code uses fixed-point arithmetic and explicitly ordered choices.
Humans, bots, replays, and the debug socket all submit the same tick-stamped
commands. Rendering and audio observe the result; they never feed back into it.

The contracts each layer keeps are in
[`docs/simulation-architecture.md`](docs/simulation-architecture.md),
[`docs/shell-architecture.md`](docs/shell-architecture.md) and
[`docs/netplay-architecture.md`](docs/netplay-architecture.md).

## Development

Common Rust checks (the complete gates, including coverage, are in
[AGENTS.md](AGENTS.md#validation)):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
```

The driver is the main inspection surface:

```sh
cargo run -p aftermine-driver -- --help
cargo run -p aftermine-driver -- run scenarios/smelter-yard.json --ticks 12000 --all-bots --map
cargo run -p aftermine-driver -- bot-eval skirmish --difficulty prime --stance balanced --paired
cargo run -p aftermine-driver -- replay-summary replays/match.json --minimaps sparse
cargo run -p aftermine-driver -- smoke --spawn
```

Use the headless runner for exact simulation questions and the native shell for
input, layout, animation, sound, or visual judgment. Aggregate match numbers can
find suspicious games; they cannot establish that a match was fun. Read the
replay summary, watch representative replays, and play changes yourself before
promoting them.

Repository invariants, workflow rules, and versioning live in
[`AGENTS.md`](AGENTS.md). Repeatable procedures live in self-describing skills
under [`.agents/skills/`](.agents/skills/).

## Project status

Aftermine is playable on desktop and iPad and is in active development.

Built with [Macroquad](https://macroquad.rs/),
[`fixed`](https://crates.io/crates/fixed), and
[`tiny-skia`](https://crates.io/crates/tiny-skia).
