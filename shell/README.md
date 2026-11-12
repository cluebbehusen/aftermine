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
- `pad` reads game controllers into that funnel: gilrs on Windows and Linux,
  Steam Deck included, and Apple's GameController through the miniquad fork on
  Mac, iPad and Apple TV, where the Siri Remote reads as a pad too: its center
  clicks and its ring and swipes are the D-pad. `menu_stick` turns the left
  stick into menu arrows.
- `screens` holds the menus, match setup (`screens/wizard`), the lobby, playback
  and results. `tutorial` holds the lessons and `tips` the match tips; `wording`
  words their how-to copy for the hands in use.
- `home_backdrop` plays a bundled recording behind Home.
- `render`, `panel` and `layout` draw the world and HUD and share hit-test
  geometry. `look` declares how each unit and defense is drawn.
- `radial` sets the command panel's cards, or the control groups, around a ring
  for the controller, which `input` opens and `render` draws.
- `theme` names the interface colors, stroke weights and type sizes.
- `assets`, `typography`, `mixer`, `audio_mix` and `soundtrack` own the art,
  fonts and sound.
- `debug_server` connects the frame loop to `aftermine-protocol`.
- `netplay` runs LAN lobbies and carries a running match between machines over
  `aftermine-net`.
- `saved_game`, `autosave` and `saves` own save files and the saves shelf.
- `platform` says whether the build runs on a desktop, a touch device (iOS) or a
  TV (tvOS), and which hands the player is using.

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

Set `AFTERMINE_SOUND_BANK` to a folder of `.wav` files to audition sounds: each
clip found there under its shipped name replaces the one in `assets/sounds/`.

`assets/licenses/third-party.txt` holds the license notices of every third-party
crate the game links, and every bundle ships it with the rest of `assets/`.
Regenerate it with `sh tools/gen_notices.sh` when `Cargo.lock` changes.

`--scenario path/to/scene.json` launches an authored scene directly; the
[sandbox-matches skill](../.agents/skills/sandbox-matches/SKILL.md) shows how to
build one. The root README covers [LAN play](../README.md#lan-play), and the
[netplay architecture](../docs/netplay-architecture.md) states its contracts.

## iPad, Apple TV and Mac apps

`apple/` holds the Xcode project, with Aftermine iPad, Aftermine Apple TV and
Aftermine Mac targets. Their build phases run Cargo for the platform and copy
`assets/` and `scenarios/` into the app. You need full Xcode signed in to your
Apple developer account. The iPad app also needs the `aarch64-apple-ios` Rust
target (plus `aarch64-apple-ios-sim` for the simulator) and an iPad with
Developer Mode on. The Apple TV app needs `aarch64-apple-tvos` (plus
`aarch64-apple-tvos-sim`) and an Apple TV paired with Xcode over Wi-Fi: open
Settings > Remotes and Devices > Remote App and Devices on the TV, then pair it
from Xcode's Devices and Simulators window. Set your signing team in the ignored
`apple/Local.xcconfig`, not in Xcode's Signing pane, which would write it into
the shared project. If you sign with your own team, set a bundle ID you control
there too; the example file shows how.

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
rustup target add aarch64-apple-tvos aarch64-apple-tvos-sim
cp apple/Local.xcconfig.example apple/Local.xcconfig   # set DEVELOPMENT_TEAM
cargo ipad
cargo tv
cargo mac
```

All three apps sync settings through iCloud's key-value store, which needs no
container. The iPad and Mac apps also keep saves in iCloud Drive, so the app ID
needs the iCloud capability with the container `iCloud.<bundle ID>`; signing
with your own team means registering that container for your bundle ID in the
developer portal. Apple TV has no iCloud Drive, and tvOS may clear any file
outside the app's user defaults, so the Apple TV app keeps its settings in user
defaults and its saves in its cache. All three apps also mirror saves through
CloudKit in the same container, which brings Apple TV's saves into the iCloud
Drive folder and back; the container needs CloudKit turned on in the developer
portal, and development builds use its Development database. A workspace
`cargo run` never looks for iCloud and keeps saves and settings on the Mac.

`cargo ipad` and `cargo tv` build the Release app, install it and launch it on a
paired device or a simulator. With several devices they ask which one;
`--device <name or id>` skips the question and `--list` prints the devices. A
locked iPad still gets the install, and you open the app once it is unlocked.

`cargo mac` builds the Release Mac app, signed to run on this Mac, and opens it,
quitting a copy that is still running. The first run registers the Mac with your
developer account. For a copy other Macs can open, pick the Aftermine Mac scheme
in Xcode, choose Product > Archive, then Distribute App > Direct Distribution,
which signs it with Developer ID and has Apple notarize it.

iPadOS 27 requires the `UIScene` lifecycle, and tvOS needs an Objective-C
binding no miniquad release uses, so the workspace pins a miniquad fork. The
workspace `Cargo.toml` explains why.

`uv run tools/gen_icon.py` exports the app icon for iOS, macOS and the desktop
window from `tools/production_icon_sources/`. Commit the sources and the
generated files together when changing the icon.

## Autosaves and diagnostics

A match autosaves every minute, when the window goes inactive, and when the
player leaves or quits, unless a save already holds that moment. Each match
keeps one autosave, which a named save or its finished replay replaces. Continue
resumes the newest autosave or local named save since the last finished match;
older ones stay on the Replays shelf. With iCloud Drive on, an iPad keeps saves,
autosaves and replays in the Aftermine folder in iCloud Drive, which the Files
app shows; otherwise they appear under On My iPad > Aftermine. Diagnostics
always stay under On My iPad. A Mac uses iCloud Drive only from a signed build
such as `cargo mac`'s, so `cargo run` keeps everything on the Mac.

Crash and freeze diagnostics are always on and write only when a panic, stall or
fatal error happens. A panic during a tick also leaves a crash dump with the
match's commands. Settings offers **Open diagnostics folder** (not on iPad) and
**Export diagnostic report**. Reports stay on the device.

The [shell architecture](../docs/shell-architecture.md) covers save durability,
retention and timing.
