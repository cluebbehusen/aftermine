# Shell architecture

`aftermine-shell` presents a deterministic match through a native macroquad
window. It owns wall-clock pacing, input, screen flow, rendering, audio and
persistence. Shared replay and diagnostic services live in `aftermine-kit`, and
the live and headless debug surfaces share `aftermine-protocol`.

This document states the contracts the shell keeps. It does not describe every
control or animation; those live in the code and its tests. Crate entry points
are in the [shell README](../shell/README.md), and live-driving procedures are
in the [aftermine-live-qa skill](../.agents/skills/aftermine-live-qa/SKILL.md).

## Session ownership

`Game` owns one live session: its starting `Scenario`, authoritative `State`,
bot controllers, pending commands, replay and keyframe recorders, clock and
presentation state. `Clock` holds whether time runs, its speed and the tick debt
a frame carries; the live match and the replay viewer each own one.
`Presentation` holds the camera, selection, interpolation, effects and audio
cues. Rendering and read-only UI queries borrow a `Scene`: the active world,
scenario, pending commands, presentation and clock. Neither `Presentation` nor
`Scene` owns or advances a simulation.

`Game::do_tick` is the only local path that advances state. It collects staged
human and debug commands and bot commands, records them at the current tick,
calls `State::tick`, then updates statistics and presentation. Fast advancement
may skip intermediate presentation work but uses the same recorded path. A
networked session runs the host's batches through `Game::run_batch`, which
shares the record, tick and update steps; only the host runs bots.

The shell may use floats, frame time and interpolation to read input and present
a match. Only tick-stamped `PlayerCommand`s cross into the simulation. Camera,
audio, selection and presentation caches cannot change authoritative state.

Local input uses the first seat without a bot in scenario order, or seat zero
for an all-bot scene. A networked session binds an explicit seat instead. This
choice never changes the configured controllers.

Color means ownership only. The viewer's own seats wear a fixed color, allies a
cool family and hostiles a warm family, each ranked by seat order, and every
owner-colored element reads the same per-view table.

`App` owns resources that outlive a screen: the live `Game`, configuration,
input funnel, New Match draft, assets, audio, debug channels and profiling. Each
`Screen` variant owns its local interaction state, and a screen opened over
another restores the one it displaced wholesale on leave. `app/screen.rs`
answers every per-screen question with one exhaustive match each.
`screen_flow::settle` runs on every change of screen, from any source, and runs
the old screen's exit step and the new one's enter step. Every new match,
however it starts, arrives through `App::install`. Screen update logic accepts
injected viewport and input state, so navigation is tested without a GPU window.

The New Match draft records each seat's difficulty and stance. Launch writes
distinct personality seeds into the `Scenario`; ordinary launches draw them from
wall-clock and process identity, and automation uses a fixed source. This is the
only point where the shell uses ambient entropy for a match. Restart, Rematch,
saves and replays keep the recorded setup.

### Bots

Live ticks and replay reconstruction run bots through
`aftermine_kit::bot_execution`. Due bots may think concurrently against the same
immutable state, and their commands join in seat order before they are recorded.
`State::tick` stays serial. After every tick, reconstruction included, `Game`
passes the report to `aftermine_kit::controller::record_events`.

Between ordinary live ticks, `Game` may compute one decision in the background
on cloned controllers and a shared `Arc<State>`. When it is consumed, world
identity, tick and seat order must match. A late decision is joined at its tick,
never skipped or replaced by a timeout. Saves read the pre-decision controllers,
prepared work is never saved, and replacing the session or world discards it.

### LAN matches

Match setup's LOCAL | LAN switch turns the setup screen into the host's lobby,
and `--host` hosts a fixed scenario. `App` owns the match's `netplay::Link` and
pumps it once per loop pass on every screen that holds the match, so menus sit
over a running match. The host paces ticks at 1x, seals each tick's human
orders, appends bot commands and runs the published batch. Clients run batches
as they arrive. Only the host pauses, and speed is fixed.

When the session waits for a lost player, every machine shows the waiting
screen, and only the host can drop the seat or hand it to a bot. A replacement
bot starts with no memory and plays from its seat's own view. Its personality
seed comes from the world's hash and the seat and is recorded in the scenario,
so clients mirror it and saves capture it.

A client that loses its connection to an undecided match keeps its rejoin token
and tries to take its seat back for 30 seconds. The host snapshots the match,
and the client restores it as a new game that records from that tick. A rejoin
ticket in the data directory lets Home offer Rejoin later; it is deleted once
the seat can no longer be taken back. After a match is decided, the host closes
its connections only once every client has acknowledged the final batch, so late
hash reports are still checked.

A client saves only a watch-only recording. The host saves the undecided match
as a LAN checkpoint, by name and automatically. Loading one opens Match Setup
locked to the save, with a reserved seat for each joined player. A joiner
reclaims the seat its installation played, else the one played under its name;
an installation or name shared by two joiners identifies nobody.

### Lessons and match tips

A Basic Training lesson rides on its session as `Game::tutorial`. Each lesson is
a sandbox scene embedded from `shell/lessons/`, so it cannot be won or lost, and
its steps advance on what the world shows rather than on the commands that got
there. A session carrying a lesson is never saved. The first match after the
lessons is an ordinary session that its scenario name marks.

Match tips ride on the session as `Game::tips`, outside its save. A tip's rule
reads only the local seat's own forces, its current sight, remembered buildings,
the danger its Harvesters avoid and the HUD's attack alerts. No tip stages a
command.

## Frame and input flow

The frame loop:

1. Drains debug requests, deferring screenshot replies until rendering
   completes.
2. Polls the persistence worker and feeds the viewport to the camera.
3. Polls hardware input, appends injected input, and routes the events.
4. Pumps a running LAN link, which runs its match's ticks before any screen.
5. Updates and draws the active screen, which advances the live or playback
   clock unless paused or seeking, then settles any change of screen and
   delivers the frame's notices.
6. Captures requested screenshots, mixes audio, replies, and yields to
   presentation.

A transition that must show its destination at once reruns the loop inside the
same presented frame. A rerun reads no hardware input, so the key that opened a
screen never reaches it.

Presentation (camera glides, held pans, effects and music) reads frame time
clamped to `MAX_PRESENTATION_DT`. The live and replay clocks read unclamped time
and cap their own catch-up, and a negative or non-finite reading counts as no
time. Two consecutive live frames of `SUSPENSION_GAP_SECS` or more mean the app
stopped presenting, as a suspended iPad app or sleeping Mac does, and the match
opens the pause menu instead of catching up. Debug-server sessions are exempt.

`aftermine_protocol::RawEvent` is the common vocabulary for hardware and
injected input, so both take the same path. `input::apply_events` turns gameplay
events into camera and selection changes and staged commands. Cross-frame
gestures, held keys, touch state and control groups belong to the input layer.
Key bindings belong to the configuration alone; everything else borrows
`Config::bindings`. `ActionResolver` pairs each release with its original press,
so modifier changes cannot strand a held action. Keyboard card actions use the
same availability checks as clicks. The debug protocol's `SendCommand` stages an
already-semantic command and does not exercise UI mapping.

Coordinates are logical throughout input and layout; the hardware adapter
applies DPI conversion once. Touches arrive in the same ordered stream as mouse
events. A finger iOS takes away arrives as `TouchCancel` and completes no tap,
long-press, box or card press.

The HUD lays out in a pure pass (`render::hud`) that measures text through a
supplied function, so it runs headless. Each frame lays it out before input,
again before each press or release that follows another event in the batch, and
before drawing. Hit testing and drawing consume the same `LayoutModel`, so a
click always hits what the next frame shows. The supported layout floor is iPad
mini in landscape, 1133×744 logical points at the default UI scale; smaller
windows are overflow stress cases.

A selection holds units of one allegiance or buildings of one owner, ordered by
id. Foreign entities can be inspected while visible, but commands stay gated to
the controlled seat, and the simulation makes the final ownership, fog, cost,
placement and target checks. Dead entities and hostiles that leave sight drop
out of the selection. Panels and tooltips read simulation accessors. Static
capabilities may show for foreign selections, but current enemy orders, loads
and income stay private. Placement and support previews use authoritative
queries rather than copying game rules, and an unknown concealed mine cannot
change any player-visible feedback.

Anything that spends or cancels resolves against the commands already staged:
grouped upgrades, grouped training, magazine orders, strike picks and queue
cancels. A purchase staged while paused therefore cannot be bought twice. The
orders dock and waypoint chain show programs with staged commands applied,
through a cached `inspect_command_phase` that reruns only when the staged batch
changes. Grouped building controls derive from capabilities rather than a list
of building kinds, and spend available scrap in building id order.

Ground orders name whole tiles clamped onto the map. Entity lookups keep the
unclamped tile, so a click in the camera's edge slack never binds to whatever
stands on the edge. The waypoint chain draws each walk at the tile the player
clicked rather than at the slot the simulation resolved, so it never reveals
unexplored ground.

One `PlacementVerdict`, cached per tick, staged-command count, selection and
candidate spot, feeds the ghost's tint, the mode ribbon and the click. Its
reachability check counts fogged and unexplored ground as open, so it never
reads what fog hides; the simulation's own path check still decides. A dragged
row of placements is judged again stamp by stamp against the exact pending phase
when it is staged.

Notices from outside a screen's controls go through `App::notify` and appear
where the player is looking. Toasts report refusals and outcomes, not modes the
ribbon already names.

### Touch-only builds

`platform::TOUCH_ONLY` is true on iOS. Platform facts branch on the constant and
pure helpers take it as a parameter, so both variants compile and test on every
platform. Copy follows the hands in use instead: each frame publishes the last
pointer and whether a hardware key has been pressed as `platform::Hands`, and
hints name keys only once one has. Every screen offers a pointer path for what
Escape does.

Each finger records where it landed, and that decides what it may drive for its
whole life. `InputState::queue_held` merges Shift with the QUEUE toggle. Touch
placement shows a ghost that builds only on its check mark; the tap point is
never the anchor.

## Persistence and replay

`aftermine_kit::checkpoint::SessionCheckpoint` is the internal continuation
boundary: scenario, world, controller memory, pending commands and live
statistics at a completed tick. Capture borrows the host without draining inputs
or running bots. Restoration validates every piece before installing it and runs
no historical ticks. The session revision is independent of `SIM_VERSION`, and
restoration accepts only a matching revision and simulation version. A
fingerprint binds the captured scenario and world together; it checks
consistency, not authenticity. Each bot seat must supply exactly one controller
checkpoint. The shell adds the human seat, concession statistics and decorative
boundary exploration. Camera, selection and effects rebuild, and the clock opens
paused.

Player saves use `.amsave`: `AFTERSAV` magic, a little-endian 32-bit header
length, a JSON metadata header, and one checksummed Zstandard frame holding a
CBOR checkpoint. Headers are bounded to 64 KiB, and file size and decoded
payload to 256 MiB. The loader requires exact lengths and no trailing data,
checks revisions, and validates metadata against the restored session. There is
no format migration.

A save also carries the commands its recording holds since the scenario's start,
so a resumed match keeps recording onto its whole history. Loading checks their
shape. A background thread then replays them to the saved tick and compares the
world hash; a history that does not reproduce the saved world gives way to a
recording that starts at the saved world.

An `.amreplay` file has `AFTERREP` magic, a little-endian 32-bit header length,
a JSON header indexing its frames, one checksummed Zstandard frame holding the
compact JSON replay, and the keyframes in index order. A live match keeps the
world every 1,024 ticks, encoded off the frame thread. Keyframes are a cache
over the command log, never a second source of truth: each carries its tick and
world hash, and a restore checks the decoded world against both. The loader
bounds the header, the replay and each keyframe. Plain JSON recordings load
without keyframes.

Each match owns one autosave slot. Its autosave and its finished recording share
an id, so the finished recording replaces the autosave, and rotation keeps whole
matches. A decided match writes its recording as soon as it ends. A match this
machine can resume autosaves after each minute of play, counted in ticks over
speed so slow frames still count and an OS suspension does not, and when the
window goes inactive. A failed background save is retried, and the player hears
of a run of failures once. Saves use the chassis atomic-write path.

One app-owned worker performs save encoding, save and checkpoint disk I/O,
restoration and catalog scans, one operation at a time. Jobs carry unique ids,
so a stale result cannot replace a newer session. Restoration produces CPU data,
and presentation installs only on the frame thread. A load can be cancelled
until the restored session is committed, and cancelling keeps the previous
session. A window close during a save waits for it and for the exit save. The
debug protocol refuses session mutations while a load or save owns the boundary.

A recording may start from a world checkpoint rather than tick zero. Its
commands cannot precede that origin, playback and statistics cover only the
available suffix, and such a recording cannot resume live play because it holds
no controller memory.

`Game::from_replay` rebuilds a session from recorded commands. Bots observe the
reconstruction to rebuild their memory, but their regenerated commands are
discarded; the recorded log is authoritative.

### Read-only playback

`aftermine_kit::playback::Playback` owns replay-viewer state: no bots, no
recorder and no new commands. A seek restores the latest stored world at or
before its target and replays the rest. Opening a replay starts a background
fill that re-simulates the record, keeps worlds to seek from, and checks every
stored keyframe's hash against the world the log produces; a mismatch drops the
recording's keyframes. Seeks are exact with or without the fill.

`PlaybackSession` owns the engine, its own `Clock` and its own `Presentation`,
while `App` keeps the live game hidden behind it. Authoritative mutations are
refused, and playback writes no recording.

Unfinished records resume rather than play back, so a fog-free viewer can never
scout a live match. Completed records can be watched. Developer `--watch` may
open any compatible replay.

## Performance and diagnostics

The player-facing Performance readout only observes completed frame
measurements. Off adds no timing samples.

Crash and freeze diagnostics are always on and write nothing while play is
healthy. The main thread publishes its stage and tick through atomics, each bot
seat marks its running decision, and a watchdog records stalls and recoveries.
The panic hook records the message, location, thread and backtrace. A fatal
error without a panic is logged before exit, since a packaged app has no
console. Incident logs are bounded and count what they drop.

Every tick of a live or LAN match runs behind one unwind boundary. A panic there
writes a crash dump: the commands through the last completed tick as a standard
replay, and a manifest written last. A match whose recording starts at a saved
world has no controller memory to rebuild, so its dump reproduces simulation
panics but not bot panics.

Report export runs off the frame thread. Reports stay local, and diagnostics
never take screenshots or enable the debug server. Diagnostics are observational
and never become replay input; failures reproduce from the recorded replay, not
from timing.

## Debug protocol

`aftermine-protocol` owns JSON-lines envelopes, bounded TCP framing, input
events and state views. The live server binds loopback. Socket threads parse
requests and pass them to the main thread; they never touch the game. Requests
execute between frames, with screenshot replies deferred until GPU readback.

`DebugSession` and `dispatch_shared` define the common state and clock surface
for live play, read-only playback and the windowless session. Window-only
capabilities are offered only where they are real, and unsupported requests are
refused explicitly. A LAN match refuses clock, speed, load and performance
requests, refuses pause and resume on clients, and accepts `SendCommand` only
for the machine's bound seat.

`StateView` is an omniscient QA view, not an exact serialized state; use
`State::hash` for identity. `FogView::capture` is the player-knowledge view for
players and agents, and bots read the equivalent `ObservationData`; a parity
test keeps the two in agreement. Player-facing commands and effects must respect
that knowledge boundary.

## Rendering and audio

The GPU renderer owns camera clipping, fog composition, sprites, effects and
screen chrome. Interpolation smooths fixed ticks without moving simulation
entities. A drawn ground body may trail its simulation position or lean off its
heading within small fixed bounds, while selection, targeting and aim read
simulation truth. Presentation derives from authoritative events and state, and
seeks reset interpolation and rebuild persistent effects. Pause and speed follow
each session's `Clock`.

Cosmetic visuals never reveal unseen events. Destruction and projectile caches
keep the identity and pose an event needs after its entity is gone. Cosmetic
contact points never change simulation targeting, damage or splash.

`assets` loads the generated sprite atlas, whose manifest covers every sprite
key the shell resolves. Frame counts come from the atlas, the animation code
owns timing, and tests keep every selected frame below the shipped count. `look`
declares each unit and defense kind's presentation in one exhaustive match, so a
new kind must state each part. Facts the simulation already states, such as
which kinds fly, crash, scout or fight, are read from its stats.

Terrain art derives from map geometry alone. `GroundLayout` is computed on a
match's first draw and kept, so revealed terrain never changes as fog lifts;
peaks and pits that mirror each other under the map's half-turn step as mirror
images. `GroundCache` bakes terrain into textures once the zoom settles, and the
direct path draws the same pixels. Within a pass, untextured fills never sit
between atlas sprites, because each macroquad draw call keeps a vertex buffer
for the life of the app.

Map tiles show authoritative simulation fog. The replay viewer is fog-free, and
so is a seat out of an undecided match once no LAN teammate plays on whom it
could tip off. The off-map quarry margin's exploration is presentation-only and
is kept in player checkpoints.

`entity_lod` derives reduced-resolution entity textures at startup without
changing the atlas bytes. `strategic_markers` draws role and allegiance cues at
distant zoom from the same player knowledge as world rendering, and never
changes positions, selection or the simulation.

Own and allied missiles draw wherever they fly; a hostile missile draws only
while the viewer sees the tile under it. What a seat knows of a hostile warhead
from `State::known_warheads` draws above the fog regardless, and Lattice
launches follow `State::interception_forecast`.

Simulation events enqueue audio cues. `mixer::spec` is the one table of what the
shell decides per sound kind: clip, bus, mix weight, and whether it is a blast
or raises combat music. Blasts, building destruction and aircraft impacts are
audible through fog regardless of owner, at full gain inside the viewport and
fading to silence a fixed distance beyond it. Same-kind events coalesce to the
loudest emitter, and inaudible events use no voices and raise no combat music.
Continuous sounds are owned and stopped individually and rebuilt when
presentation resumes. Audio never feeds a simulation decision. Production sprite
and sound bytes stay owned by their generators and approval workflows.

The tiny-skia renderer in `aftermine-kit` produces whole-map CPU schematics. It
shares none of the native atlas, camera, HUD or animation. CPU goldens prove
deterministic schematic composition; presentation and input claims need the real
shell.

## Source and test map

| Contract                      | Primary source                                                                 | Behavioral evidence                                                                 |
| ----------------------------- | ------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------- |
| App ownership and screen flow | `shell/src/app.rs`, `shell/src/app/screen_flow.rs`, `shell/src/screens/`       | Screen tests, `driver/tests/menu_ux.rs`                                             |
| Live tick and recording       | `shell/src/game.rs`                                                            | Game tests, `driver/src/smoke.rs`                                                   |
| Input and shared geometry     | `shell/src/input.rs`, `shell/src/layout.rs`, `shell/src/panel.rs`              | Input and layout tests                                                              |
| Saves                         | `shell/src/autosave.rs`, `shell/src/saves.rs`                                  | Module tests                                                                        |
| Diagnostic persistence        | `kit/src/diagnostics.rs`, `shell/src/diagnostic_report.rs`                     | Module tests                                                                        |
| Playback and seeking          | `kit/src/playback.rs`, `shell/src/screens/playback.rs`                         | Playback tests                                                                      |
| Protocol capabilities and fog | `protocol/src/session.rs`, `protocol/src/view.rs`, `shell/src/debug_server.rs` | Protocol tests, `driver/tests/session_parity.rs`                                    |
| Native presentation           | `shell/src/look.rs`, `shell/src/render.rs`, `shell/src/assets.rs`              | Asset and look tests, `shell/tests/presentation_animation.rs`, native capture tests |
| CPU schematics                | `kit/src/render.rs`                                                            | `driver/tests/golden.rs`                                                            |
| Audio                         | `shell/src/mixer.rs`, `shell/src/audio_mix.rs`, `shell/src/soundtrack.rs`      | Module tests                                                                        |
| iPad build                    | `ios/`, `shell/src/platform.rs`                                                | iOS clippy in CI, device builds                                                     |
