# aftermine-sim

`aftermine-sim` contains all Aftermine game rules. It is a pure, headless
simulation: given the same scenario and command log, it must produce the same
state on every platform. Rendering, hardware input, wall-clock time, and
presentation state belong elsewhere.

`State::tick(&[PlayerCommand])` is the only game-state transition. Humans, bots,
replays, and debug clients all enter through the same command types, so a bot is
an ordinary command source rather than a separate ruleset.

## Main pieces

- `State` owns the complete serializable world and validates its invariants.
- `Scenario` builds the initial state from authored map and player data.
- `command` and `event` define the simulation's input and output vocabulary.
- `tick` implements the fixed phase order for commands, production, movement,
  combat, cleanup, and victory.
- `stats` holds the unit and building tables and most balance constants.
- `observation` projects serializable player knowledge through fog and vision
  memory. It owns information access, not controller policy or navigation
  caches.
- `geometry` shares canonical command and production tie rules with predictors.
- `building_contact` owns the contact outlines, per kind, tier, and construction
  stage, that follow the building art for local interaction.
- `vision` provides visibility and explored-world state.

Rules live in the code and its tests. The contracts every change must keep, such
as the tick phase order, the trust boundary and the symmetry rules, are in
[`docs/simulation-architecture.md`](../docs/simulation-architecture.md).

## Development

Scenarios default to `mode: "match"`, which needs a Foundry per seat and
eliminates a seat that loses its last one. `mode: "sandbox"` makes Foundries
optional and never declares a winner, while keeping ordinary costs, ownership
and command rules. The sandbox-matches skill shows how to stage one.

Run commands from the workspace root:

```sh
cargo test -p aftermine-sim --locked
cargo test -p aftermine-sim --locked --test integration state_integrity::
cargo clippy -p aftermine-sim --all-targets --locked -- -D warnings
```
