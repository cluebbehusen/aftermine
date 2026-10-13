# oxide-driver

`oxide-driver` is Oxide's headless harness and remote control. Its library
combines reusable inspection and automation tools; its CLI runs scenarios and
replays, renders maps, audits maps and match behavior, drives a live shell, or
serves the same debug protocol without a window.

The driver observes and orchestrates the game through public simulation and
protocol boundaries. It does not contain alternate gameplay rules, and its
automated players use the same command path as every other player.

Build provenance belongs to this executable. Its build script watches the driver
and shared dependency package trees plus shared build inputs, assets, and
scenarios; shell-only edits and private workspace notes do not contribute to its
dirty status. Reports retain both the original recording identity and this
exporter's identity. Source archives report unknown provenance.

## Main pieces

- Headless execution (`runner`, `render`, `playback`, and `stats`) lives in
  `oxide-kit`, shared with the shell.
- `client` speaks the debug protocol; `session` serves it windowlessly, and its
  save and load commands use replay files.
- `recovery-inspect <session-directory> [--export <new-report-directory>]`
  verifies an interrupted journal, reports how its session ended, and exports
  its completed replay prefix plus incident logs without needing a responsive
  shell.
- `replay_inspect` and `replay_summary` provide exact snapshots and compact
  match narratives. Checkpoint-origin recordings report their first available
  absolute tick, and summaries and inactivity windows cover only that segment.
  Both serialized schemas expose this boundary; inspection rejects requests for
  unavailable earlier ticks.
- `ledger` is the impact ledger: what every unit and building did over a match,
  valued in scrap. Events name a hit's shooter but not its damage or a death's
  killer, so it diffs every body's health tick to tick, net of reported repairs,
  and splits each loss among the shooters, shells, charges and Sapper blasts
  that hit that body. It also credits repair to the welder or Repair Bay,
  spotting to the nearest friendly body that saw a target its shooter could not
  (sight approximated by vision radius), hidden charges destroyed inside an
  Array's detection to that Array, deliveries and passive income by building,
  spending by category and phase, and each seat's net worth at a fixed period.
  Crew repair of buildings, radar warning and the harvester recovery trickle
  emit nothing it can credit. `replay-ledger <replay>...` re-executes replays
  and match recordings, not player saves, through it and pools seats by
  controller; `--json` prints each game's ledgers.
- `replay_health` (the `replay-health` command) measures late-game health from
  replays and match recordings, so bot evaluations and human playtests answer
  the same questions: each seat's income by source per minute, with worker
  deliveries split between scrap fields and wreck salvage, the share of matches
  reaching 30 minutes, wreck left on the map each minute and how much lies at a
  seat's home, the variety of standing and newly trained armed units from minute
  15 as entropy over unit kinds by value, and what each won battle turned into
  over the next minute. The winning force is every body that hit something lost
  in the battle; the follow-up counts the losers' production, income works and
  workers that force destroyed, and whether the winner's armed units outweigh
  the losers' near the battle a minute later. A win converts when the force
  razes production, destroys at least 200 scrap of income, or holds ground
  nearer the losers' buildings than its own; the report leads with pushes, the
  wins away from the winner's home. A seat's home is the ground within 12 tiles
  of its completed production buildings, and a seat earns nothing once it is out
  of the match. Battles are `replay-summary`'s. It accepts directories such as a
  `bot-eval --replay-dir`, prints a pooled report, and with `--json` prints each
  match's row and the pooled summary.
- `bot_eval` runs the player-facing controllers to a decision, tick ceiling, or
  stall-loop anomaly, and emits compact JSONL with candidate, scenario,
  tick-ceiling, exact-profile, and anomaly provenance. It can exchange complete
  controller configurations between seats for paired controller, personality or
  difficulty comparisons, including crossed personality seeds and geometry
  cells. Persisted batches are staged and never replace earlier evidence.
  Optional decision traces stream fog-honest controller diagnostics to a
  separate JSONL sidecar without entering compact rows or replays. A returned
  publication error rolls back files created by that invocation. Abrupt process
  termination can leave hidden staging files or a partial replay set because
  arbitrary final paths cannot be published atomically; inspect and remove that
  incomplete batch, then rerun it under a fresh candidate.
- Evaluation rows also record each seat's team and elimination tick, the
  producing build, omniscient failure detectors (repeated impossible orders,
  abandoned paid construction, starved production, `oxide-opponent` missions
  stuck in one phase, and an army idle at home) with per-producer idle
  diagnostics, the fate of armed ground units trained on severed ground, income
  against a saturated-economy estimate while the seat is still in the match, and
  the seat's impact ledger. `oxide-opponent` seats also record attack
  calibration: each attack its decisions launched, with the known defense,
  margin, need and strength sent that `Opponent::launches` reports, followed
  through its mission to how it ended and what its units dealt and lost. These
  are QA evidence computed from authoritative state and what a controller
  reports; they never reach a controller.
- `seat_summary` pools the seats of evaluation rows: failure incidents,
  deliveries, reactivity, income, the impact ledger and attack calibration, with
  their tables. `bot-summary <rows.jsonl>...` pools any `bot-eval` or
  `bot-ladder` rows by team layout and difficulty. Evaluation inputs live in
  `evaluation/`, not `scenarios/`, whose every file the shell menu, map gates
  and integrity tests read.
- `bot_ladder` expands a manifest from `evaluation/ladder/` into pairs of
  `oxide-opponent` against itself at two difficulty rungs, the higher rung in
  seat zero and then in seat one, both seats sharing one personality seed so the
  seats differ only in difficulty. `bot-ladder` publishes labelled rows and
  prints, for each comparison, the higher rung's share of decided legs with a
  Wilson interval, pairs by result, and whether it reaches the comparison's gate
  over enough decided pairs, overall and by stance and map family, with the
  higher rung's share of net worth by pair, then each rung's seats through
  `seat_summary`; `bot-ladder-report` re-reads published rows, keeping each
  manifest's comparisons apart.
- `bot_pressure` runs the staged scenarios in `evaluation/pressure/`: a scripted
  attacker seat presses one situation (an early rush, air harassment, an
  artillery siege, a transport drop) on a bot seat, and a check over
  authoritative state reports whether the bot answered by the deadline. The
  checks are QA evidence and never reach a controller.
- `bot_cost` times controller decisions on named workloads: a Skirmish duel, the
  seven-bot Skyhook game, and a staged mature-army match kept under
  `tests/fixtures/performance/` so it stays out of the shipped pool. It reports
  average and p99 wall time per decision, total CPU, and the fog-honest
  observation cost, per seat and pooled. Seats decide serially with tracing off;
  its command and final hashes equal an untimed run.
- `duel_lab` (the `duel-lab` command) fights unit kinds against each other at
  equal scrap on staged sandbox fields: every pairing on open, choke and cover
  layouts, in line and clump formations, from both seats. No controller runs:
  armies hunt each other, idle bodies periodically hunt the nearest live enemy,
  long guns get a Kestrel spotter hovering short of the fight, Sappers get
  attack orders on defenses they can see, unarmed support such as the Mantle
  deploys behind the guns and follows the nearest fighter, and upgraded defense
  tiers are refitted with ordinary upgrade commands before the fight. A side
  whose last fighter falls loses its support with it. It prints a matrix of
  value kept, flags kinds that win or lose every matchup, play alike or are
  outclassed, tier margins, seat-split legs and timeouts, and `--json` writes
  every leg. The results are reports for balance work, never pinned tests.
- `composition_search` (the `composition-search` command) looks for a mix of up
  to three unit kinds that nobody beats at equal scrap, using the duel lab's
  staged fights. Support kinds join only mixes with a kind that fights. Each
  candidate is one fixed army: its kinds split the budget evenly (or, with
  `--skewed`, one kind takes a double share), each kind but the cheapest buys
  its share, and the cheapest fills the rest to land as close to the budget as
  whole bodies allow. A round-robin over every candidate would take about a
  million duels, so it grows a pool instead: it solves the blend of pool
  candidates that no member beats, tests every candidate against that blend, and
  adds the strongest challenger until none gains anything. A mix that no
  candidate beats alone is a dominant composition. It reports each budget's
  blend with every member's army and best counter, plus the range of spends;
  `--json` writes them. It is offline evaluation and never feeds a controller.
- `tick_profile` shows where simulation time goes inside a window of recorded
  ticks. It rebuilds a replay to the window's first tick, re-simulates the
  window from a clone of that world under macOS's `sample`, and reports each
  function's share of the samples inside `State::tick`: the tick's phases, own
  work, inclusive time and an optional breakdown of one function. Repetitions
  are identical, so even a single tick gathers thousands of samples. Recorded
  commands replay; controllers do not run. Other platforms refuse the command.
  Its `tick-scan` companion times every tick of a replay in one straight pass
  and ranks the costliest windows, to choose where to profile.
  `bot-cost --save-replay` records a timed workload for both.
- `audit` (the `map-audit` command) measures map geometry: room per seat, route
  lengths, resources, and artillery pressure.
- `auto`, `smoke`, `shots`, and `profile` exercise the real shell where a
  headless run is not enough.
- `ios` (aliased as `cargo ios`) builds, installs, and launches the iPad app on
  a paired iPad or an iPad simulator.

Run `oxide-driver --help` for the current command tree. Procedures for live
shell QA belong in the repository's `oxide-live-qa` skill rather than here.

## Development

Run commands from the workspace root:

```sh
cargo run -p oxide-driver -- --help
cargo run -p oxide-driver -- run skirmish --ticks 2000 --all-bots
cargo run -p oxide-driver -- bot-eval skirmish --difficulty prime --paired
cargo test -p oxide-driver --locked
```

The [oxide-opponent skill](../.agents/skills/oxide-opponent/SKILL.md) owns the
ladder, trace capture, seed and profile provenance and anomaly interpretation.
The live-QA skill owns native inspection. Aggregate results do not establish
opponent quality or isolate simulation fairness.

`bot-eval --jobs N` bounds concurrent matches (default four, capped by available
CPUs and leg count). Multiple match workers disable nested bot-seat parallelism;
`--jobs 1` uses ordinary seat scheduling. Each worker stages replay and trace
output privately; results merge in input-plan order before publication. This
improves batch throughput, not individual tick latency.

## Performance regression checks

The
[performance and persistence procedure](../.agents/skills/oxide-live-qa/references/performance.md)
provides fresh-checkout workload commands, portable save-size and planning-work
guards, and scoped native timing requirements. Use its fixed inputs for
candidate/control comparisons; wall-clock thresholds are machine-qualified.

## Coverage boundaries

`tests/hashes.rs` runs six small controller-free scenarios for economy,
construction, ground combat, air transport, fog and targeting, and team victory.
Each scenario asserts accepted commands and the expected events before comparing
named milestone hashes. Independent execution, intermediate state round trips,
and reconstruction of the recorded command stream are checked throughout. The
hashes live in `tests/goldens/state-hashes.json`, which the cross-platform CI
matrix checks.

`tests/lockstep.rs` runs an `oxide-net` host and two clients in one process over
delayed links on a virtual clock. Each human seat's orders come from a bot
controller on that seat's own machine and cross the wire. It checks that every
machine executes identical batches under latency and jitter, and covers a
stalled client, a stuck client, a closed connection, a silent host, and a
diverged client. One test instead joins, starts, and plays a short match over
real localhost TCP with a real clock.

`tests/opponent_hashes.rs` checks exact recovery funding and repeated nearby
harvest deliveries through `oxide-opponent`. Behavioral assertions precede short
state and tick-stamped command hashes. Independent controllers, state round
trips, and replay reconstruction must agree at every step.

The regular integrity harness runs Skirmish, Twin Forges, and Basalt Spine for a
fixed tick horizon each, covering an open duel, team play, and constrained
terrain. It seats Standard/Balanced bots with personality seed zero and checks
state validity and serialization, including the initial and final states. It
stops after validating a terminal result without requiring a winner, activity
quotas, or a particular command stream. With `OXIDE_SOAK_TRACE_DIR` set, each
run also writes periodic state hashes and its replay into that directory; CI
compares those hash files across operating systems without a checked-in golden.
The exhaustive all-map soak is ignored by default and uses the same checks, with
a longer horizon for large, vast, and grand maps. The opening image and renderer
showcase cover drawing without depending on an autonomous midgame.

Run the exhaustive soak explicitly:

```sh
cargo test -p oxide-driver --test headless --locked every_shipped_scenario_preserves_state_integrity -- --ignored --exact --nocapture
```
