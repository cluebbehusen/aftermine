# aftermine-kit

`aftermine-kit` holds Aftermine-specific engine services shared by the shell and
the driver, so the graphical game and the headless harness use the same
controller hosting, checkpoint, replay, statistics, diagnostics, and rendering
code without either depending on the other. A few services, such as the headless
`runner` and `bench`, serve only the driver.

This is not the home for game rules or UI state. Rules stay in `aftermine-sim`,
while reusable game-independent primitives stay in `chassis`.

## Main pieces

- `controller` hosts one `aftermine-opponent` controller per bot seat and
  buffers each seat's own events between decisions.
- `bot_execution` collects bot commands in seat order, running due seats on a
  small worker pool. Live sessions can compute a decision ahead on controller
  clones and install it when its tick comes.
- `checkpoint` captures and restores a session at a tick boundary: scenario,
  world, controllers, pending inputs and statistics. Restoring never re-runs
  past ticks.
- `history` keeps a session's commands beside its checkpoint, so a resumed match
  keeps recording onto its whole replay.
- `load_replay`, `load_recording` and `save_recording` read and write
  `.amreplay` files with strict size bounds. `keyframes` stores a world every
  1,024 ticks so playback can seek quickly, and `recording` handles replays that
  start from a saved world rather than tick zero.
- `playback` seeks exactly anywhere between a recording's start and end.
- `runner` runs scenarios and replays headlessly; `bench` builds the mass-battle
  fixture.
- `stats` derives match summaries from simulation state.
- `render` is the deterministic CPU renderer used for previews and goldens, and
  `perceptual` compares rendered images.
- `build` describes the executable that wrote a file, as provenance only.
- `diagnostics` watches for panics and stalls and writes crash dumps and an
  incident log. It never feeds back into a match.

## Development

Run commands from the workspace root:

```sh
cargo test -p aftermine-kit --locked
cargo test -p aftermine-driver --test golden --locked
cargo clippy -p aftermine-kit --all-targets --locked -- -D warnings
```
