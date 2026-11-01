# aftermine-shell

`aftermine-shell` is the playable macroquad application. It turns input into
recorded simulation commands, advances the deterministic state, and presents the
result through rendering, sound, menus, saves, and replay playback.

The shell is a presentation layer. It may stage commands, but camera state,
frame timing, animation, audio and UI caches never change a game's outcome. The
crate is a binary, and this README is also its crate-level rustdoc.

## Main pieces

- `main` handles command-line arguments, window setup and startup.
- `app` runs each frame and owns screen flow. `app/screen` answers per-screen
  questions, `app/screen_flow` handles transitions, and `app/persistence` runs
  saving and loading on a background worker.
- `game` owns one live session: its world, recorder, bots and `game::Clock`.
  `game::Presentation` holds the camera, effects and UI state, and rendering
  borrows the active world through `game::Scene`.
- `input` and `action` are the single funnel for hardware and injected input.
  Key bindings live only in the settings.
- `screens` holds the menus, match setup (`screens/wizard`), the lobby, playback
  and results. `tutorial` holds the lessons and `tips` the match tips.
- `home_backdrop` plays a bundled recording behind Home.
- `render`, `panel` and `layout` draw the world and HUD and share hit-test
  geometry. `look` declares how each unit and defense is drawn.
- `theme` names the interface colors, stroke weights and type sizes.
- `assets`, `typography`, `mixer`, `audio_mix` and `soundtrack` own the art,
  fonts and sound.
- `debug_server` connects the frame loop to `aftermine-protocol`.
- `netplay` runs LAN lobbies and carries a running match between machines over
  `aftermine-net`.
- `saved_game`, `autosave` and `saves` own save files and the saves shelf.
- `platform` says whether the build is touch-only (iOS).

The [shell architecture](../docs/shell-architecture.md) describes the contracts
between these pieces.

## Development

Run commands from the workspace root:

```sh
cargo run -p aftermine-shell --release
cargo test -p aftermine-shell --locked
cargo run -p aftermine-driver -- smoke --spawn
```

A workspace build binds F1 to the debug overlay, which shows the whole world
through the fog. A packaged copy never offers it, and a LAN match refuses it.

`assets/licenses/third-party.txt` holds the license notices of every third-party
crate the game links, and every bundle ships it with the rest of `assets/`.
Regenerate it with `sh tools/gen_notices.sh` when `Cargo.lock` changes.

### LAN play

Every machine must run a build of the same commit. The host switches match setup
to LAN, which opens the other seats and shows the address to join. Other players
pick Join match, enter a name, and choose the host from the list or type its
address (the port defaults to 4200). The host can move or remove players and
starts once every open seat is filled. Tailscale and other routed networks need
the typed address.

From the command line, the host passes a scenario whose human seats have
`"bot": false`. The host takes the first and joining machines fill the rest in
order:

```sh
cargo run -p aftermine-shell --release -- --host 0.0.0.0:4200 --scenario duel.json
cargo run -p aftermine-shell --release -- --join 192.168.1.20:4200
```

## iPad build

`ios/` holds the Xcode project. Its build phases run Cargo for the device or
simulator and copy `assets/` and `scenarios/` into the app. You need full Xcode,
the `aarch64-apple-ios` Rust target (plus `aarch64-apple-ios-sim` for the
simulator) and an iPad with Developer Mode on. Set your signing team in the
ignored `ios/Local.xcconfig`, not in Xcode's Signing pane, which would write it
into the shared project. If you sign with your own team, set a bundle ID you
control there too; the example file shows how.

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
cp ios/Local.xcconfig.example ios/Local.xcconfig   # set DEVELOPMENT_TEAM
cargo ios
```

`cargo ios` builds the Release app, installs it and launches it on a paired iPad
or a simulator. With several devices it asks which one; `--device <name or id>`
skips the question and `--list` prints the devices. A locked iPad still gets the
install, and you open the app once it is unlocked.

iPadOS 27 requires the `UIScene` lifecycle, so the workspace pins a miniquad
fork with unreleased upstream work. The workspace `Cargo.toml` explains why.

`uv run tools/gen_icon.py` exports the app icon for iOS, macOS and the desktop
window from `tools/production_icon_sources/`. Commit the sources and the
generated files together when changing the icon.

## Sandbox sessions

`--scenario path/to/scene.json` launches an authored scene directly. With
`"mode": "sandbox"`, Foundries are optional and nobody wins or loses
automatically, but costs, ownership and command rules still apply. Local input
controls the first seat without a bot. Saves and replays keep sandbox rules. The
sandbox-matches skill shows how to build a scene.

## Autosaves and diagnostics

A match autosaves every minute, when the window goes inactive, and when the
player leaves or quits. Each match keeps one autosave, which its replay replaces
once the match is decided. Continue resumes the newest autosave since the last
finished match; older ones stay on the Replays shelf. On iPad, saves, replays
and diagnostics appear in the Files app under On My iPad > Aftermine.

Crash and freeze diagnostics are always on and write only when a panic, stall or
fatal error happens. A panic during a tick also leaves a crash dump with the
match's commands. Settings offers **Open diagnostics folder** (not on iPad) and
**Export diagnostic report**. Reports stay on the device.

The [shell architecture](../docs/shell-architecture.md) covers save durability,
retention and timing.
