# aftermine-driver

`aftermine-driver` is Aftermine's headless harness and remote control. Its
library combines reusable inspection and automation tools; its CLI runs
scenarios and replays, renders maps, audits maps and match behavior, drives a
live shell, or serves the same debug protocol without a window.

The driver observes and orchestrates the game through public simulation and
protocol boundaries. It does not contain alternate gameplay rules, and its
automated players use the same command path as every other player.

## Main pieces

Headless execution (`runner`, `render`, `playback` and `stats`) lives in
`aftermine-kit`, shared with the shell. The driver adds:

- `client` speaks the debug protocol to a running shell, and `session` serves it
  without a window.
- `replay_inspect` and `replay_summary` give exact snapshots and a readable
  account of a match: battles, expansions, eliminations and per-seat digests.
- `ledger` (`replay-ledger`) values what every unit and building did over a
  match in scrap: damage dealt, spotting, repair, income and spending.
- `pacing` reports who led at minutes 5, 10 and 15, lead changes and comebacks,
  from each side's net worth.
- `bot_eval` (`bot-eval`) plays bots to a result and writes one JSONL row per
  leg, with QA detectors for failures and reactivity. `seat_summary`
  (`bot-summary`) pools those rows.
- `bot_ladder` (`bot-ladder`) plays the difficulty rungs against each other and
  checks each comparison against its gate.
- `bot_pressure` (`bot-pressure`) runs staged scenarios where a scripted
  attacker presses one situation on a bot.
- `bot_cost` (`bot-cost`) times bot decisions on fixed workloads.
- `home_replays` (`home-replays`) records the matches behind Home into
  `assets/home`, each with a checkpoint at its fiercest fight.
- `tick_profile` (`tick-profile`) and `tick-scan` show where simulation time
  goes in a replay. `tick-profile` needs macOS.
- `audit` (`map-audit`) measures map geometry and derives each map's size and
  wealth badges.
- `auto`, `smoke`, `shots` and `profile` exercise the real shell where a
  headless run is not enough.
- `profile_graphics` (`profile-graphics`) runs named frame workloads in the real
  shell and reports where each frame's time goes.
- `ipad` (`cargo ipad`) and `tv` (`cargo tv`) build, install and launch the iPad
  and Apple TV apps.
- `mac` (`cargo mac`) builds and opens the Mac app.

All evaluation output is omniscient QA evidence and never reaches a controller.
Evaluation inputs live in `evaluation/`, not `scenarios/`. Run
`aftermine-driver --help`, or `--help` on a command, for its options.

## Development

Run commands from the workspace root:

```sh
cargo run -p aftermine-driver -- --help
cargo run -p aftermine-driver -- run skirmish --ticks 2000 --all-bots
cargo run -p aftermine-driver -- bot-eval skirmish --difficulty prime --paired
cargo test -p aftermine-driver --locked
```

The [aftermine-opponent skill](../.agents/skills/aftermine-opponent/SKILL.md)
covers bot evaluation, the ladder and how to read their reports. The
[aftermine-live-qa skill](../.agents/skills/aftermine-live-qa/SKILL.md) covers
driving the native shell.

## Performance regression checks

The
[performance and persistence procedure](../.agents/skills/aftermine-live-qa/references/performance.md)
has the fixed workloads and size guards to compare a change against its base.

## Test suites

- `tests/hashes.rs` runs six small scenarios without bots: economy,
  construction, ground combat, air transport, fog and targeting, and team
  victory. Each asserts the expected commands and events before comparing
  milestone hashes from `tests/goldens/state-hashes.json`, which CI checks on
  every platform.
- `tests/lockstep.rs` runs a netplay host and two clients in one process over
  delayed links on a virtual clock, and checks that every machine executes the
  same batches. It covers stalled, stuck, disconnected and diverged peers and a
  silent host. One test plays a short match over real localhost TCP.
- `tests/opponent_hashes.rs` checks `aftermine-opponent` behaviour with short
  state and command hashes, which re-bless separately from the simulation
  hashes.
- The integrity harness plays Skirmish, Twin Forges and Basalt Spine with bots
  for a fixed number of ticks and checks state validity and serialization
  throughout. With `AFTERMINE_SOAK_TRACE_DIR` set, it also writes periodic
  hashes that CI compares across operating systems.

The same checks over every shipped map are opt-in:

```sh
cargo test -p aftermine-driver --test headless --locked every_shipped_scenario_preserves_state_integrity -- --ignored --exact --nocapture
```

The native suites spawn a real shell window, so the workspace run skips them:

- `tests/menu_ux.rs` walks menus, screen transitions, settings and the replay
  shelf by mouse, keyboard, controller and remote.
- `tests/pad_wiring.rs` checks that controller presses reach a real window's
  match: an order, a ring pick, a placement, the queue strip and the remote's
  way to Pause.
- The live half of `tests/session_parity.rs` runs one script through a spawned
  shell and the windowless session and requires identical replies.
- `smoke --spawn` checks the debug socket, input funnel, camera, screenshots and
  recorder end to end, and that the saved replay reproduces the live hash.

Run the tests with `--ignored`, one at a time, and everything under
`xvfb-run -s "-screen 0 1280x800x24"` on Linux without a display:

```sh
cargo test -p aftermine-driver --test menu_ux --test session_parity --test pad_wiring --locked -- --ignored --test-threads 1
cargo run -p aftermine-driver --locked -- smoke --spawn
```

CI's `native-window` job runs `menu_ux`, `pad_wiring`, the live half of
`session_parity` and `smoke --spawn` this way, as a trial outside the required
checks.
