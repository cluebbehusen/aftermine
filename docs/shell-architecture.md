# Shell architecture

`oxide-shell` presents a deterministic match through a native macroquad window.
It owns wall-clock pacing, input, screen flow, rendering, audio, and persistence
UX. Shared replay and diagnostic services live in `oxide-kit`; the live and
headless debug surfaces share `oxide-protocol`.

Crate entry points are in the [shell README](../shell/README.md). Live-driving
commands and review procedures belong in the
[oxide-live-qa skill](../.agents/skills/oxide-live-qa/SKILL.md).

## Session ownership

`Game` owns one live session: its starting `Scenario`, authoritative `State`,
bot controllers, pending commands, replay recorder, clock, and presentation
state. The `Clock` holds whether time runs, its speed, and the tick debt a frame
carries; the live match and the replay viewer each own one. The `Presentation`
member holds camera, selection, interpolation, effects, and audio cues, and
learns only where between ticks its picture is drawn. Statistics, recording, and
bot execution stay with the live session. Rendering and read-only UI queries
receive a borrowed `Scene`: the active world, scenario, pending commands,
presentation, and clock. Neither `Presentation` nor `Scene` owns or advances a
simulation. Each view prepares a small stack-resident seat-style table from the
current viewer, teams, and colorblind setting. Color means ownership only: the
viewer's own seats wear a fixed self color, allies a cool family, and hostiles a
warm family, each ranked by seat order. Sprite accent masks, selection rings,
health bars, panel art, and minimap marks all wear the owner's color. World,
minimap, and result rendering share those lookups; no per-entity player scan or
mutable identity cache is needed. The table supports the scenario seat limit.

`Game::do_tick` is the only local live-shell path that advances state. It
collects pending human/debug commands and bot commands, records them at the
current tick, calls `State::tick`, then updates statistics and presentation from
the result. Fast advancement can suppress intermediate presentation work but
uses the same recorded tick path. A networked session instead executes complete
batches from the lockstep host through `Game::run_batch`, which shares the same
record, tick, and update steps; only the host runs bots.

Local input uses the first non-bot seat in scenario order, or seat zero for an
all-bot scene. A networked session binds an explicit human seat instead, and its
pending commands stay staged from sending until the batch that carries them
executes. This choice does not change the configured controllers. Live
scenarios, replay continuation and checkpoint restoration accept any valid bot
roster, including no bots; the New Match wizard authors one local seat. Sandbox
completion rules belong to the simulation, so headless and native sessions
reproduce the same open-ended scene.

Match setup's LOCAL | LAN switch makes the setup screen itself the host's lobby:
it opens a `netplay::HostLobby` (asking for the player's name first if none is
saved), opens every chair but the host's, and seats joiners from the draft.
Tapping an open chair's controller chip gives it back to its bot. A joiner takes
the first open chair in display order, or else a bot's; the host swaps occupants
between seats, removes players after a confirmation, and Start waits until every
open chair is filled. A running start freezes the setup until it resolves, and
Back or Escape calls it off. Leaving setup or switching to Local closes the
lobby, asking first when players would be dropped. A hosting lobby listening on
an IPv4 address other machines can reach broadcasts a discovery beacon, and the
Join form lists the lobbies its scanner hears. Home's Join Match and `--join`
open a `netplay::Lobby` client, whose waiting face draws a read-only copy of the
host's setup from the room the lobby protocol carries. `--host` hosts a fixed
scenario through `netplay::ScenarioHost`, which seats joiners with
`seating::Seating` and starts once every seat has someone to play it. Players'
names replace their seats' names in the started scenario. In-game hosts listen
on `oxide_net::DEFAULT_PORT`, and a join address without a port uses it. `App`
then owns the match's `netplay::Link` and pumps it once per loop pass on every
screen that holds the match, so menus sit over a running match, and a match
decided under the pause menu opens its report. Leaving the match says goodbye
and lets the link's connections flush: a client's seat surrenders, a host
leaving an undecided match tells its clients, and a host leaving a decided match
lets lagging clients still receive the deciding batches. When the session waits
for a lost player, every machine shows the waiting screen over the match, naming
who it waits for and for how long, and only the host can drop the seat or hand
it to a bot. A bot taking over starts with no memory and plays from its seat's
own view; its personality seed comes from the world's hash and the seat, and the
scenario records it, so clients mirror it and saves capture it. The host's link
keeps the lobby's listener for the match, and its `oxide_net::Desk` answers
probes and rejoins. A client that loses its connection to an undecided match
keeps the token Go gave it and tries for 30 seconds to take its seat back: the
host snapshots the waiting match as a
`oxide_kit::world_snapshot::WorldSnapshot`, and the client restores it as a new
game that records from that tick, keeping its camera. While it plays, a client
keeps a rejoin ticket in the data directory, written off the frame loop and
dropped once the seat can no longer be taken back: a result, a surrender, a
leave, or the host saying so. A client whose rejoin found no host keeps it, and
Home asks that host every few seconds whether the seat still waits, offering
Rejoin only when it does; a ticket an hour stale, from another build, or naming
a seat that is gone is deleted. A player the host has heard nothing from lately
shows as a pinned line on the HUD while play continues, as does a client's word
that the host paused. A seat that can no longer act retires from the session,
and a decided match concludes it, so neither is waited on. The host paces its
ticks at 1x, seals each tick's human orders, appends bot commands, and runs the
published batch; a client runs batches as they arrive with a small buffer and
catches up when behind. Once the match is decided, the host closes the
connections only after every client has acknowledged the final batch, so late
hash reports are still checked. Staged orders reach the session through the
game's outbox. Only the host pauses, and speed is fixed. A client saves only a
watch-only recording. The host saves the undecided match as a LAN checkpoint
instead: by name from its menu, and on its own every minute of play, when a wait
begins, when it drops or replaces a seat, when its window goes inactive, and
when it leaves. A LAN checkpoint carries every seat's unsealed orders and the
installation that played each joined seat, binds the host's seat rather than the
default local one. Loading one, from the shelf or from Continue when it is the
newest autosave, opens Match Setup locked to the save: its map, bots, teams, and
the host's seat are fixed, and each joined player still in the match has a
reserved seat. A joiner takes the reserved seat its installation played, else
the one played under its name, else the first free one; an installation or a
name another joiner shares identifies nobody. The host can swap players between
reserved seats or hand one to a bot, and Start waits for every reserved seat.
Start ships the saved world as a snapshot, and every player gets a fresh rejoin
token; once every player's seat went to a bot, the host resumes alone. Everyone
leaves through the ordinary leave save when the host is lost or a desync halts
the match.

Live ticks and replay reconstruction use `oxide_kit::bot_execution`. Due bots
may think concurrently against the same immutable state; their work joins in
input seat order before commands are recorded. `State::tick` remains serial.
After every tick, replay reconstruction included, `Game` hands the report to
`oxide_kit::controller::record_events`, which buffers each `oxide-opponent`
seat's own order failures for its next decision.

Between ordinary live ticks, `Game` may submit one background decision. It keeps
the pre-decision controllers and gives the worker a cloned working set plus an
`Arc<State>` shared with rendering. At consumption, world identity, tick and
seat order must match. The worker releases its world reference before publishing
complete controllers and commands; mutation requires unique world ownership. A
late decision is joined at its tick, never skipped or replaced by a timeout.
Bulk advancement and replay reconstruction collect synchronously when no job
exists. Both modes use the same bounded executor admission gate.

Save and controller inspection read the retained pre-decision controllers even
when the job has finished. Restored sessions recompute that decision once;
prepared work is not persisted and saving does not join it. Pause retains the
job; replacing the session or world discards it. Discarding its result does not
cancel the worker, which retains executor admission until computation finishes.

The shell may use floats, frame time, and interpolation to interpret interaction
and present a match. Only the resulting tick-stamped `PlayerCommand` crosses the
simulation boundary. Camera, audio, selection, and presentation caches cannot
change authoritative state. Seeks clear or rebuild timeline-local presentation.

`App` owns resources that outlive a screen: the live `Game`, configuration,
input funnel, New Match draft, tutorial, assets, audio services, debug channels,
and profiling collectors. Each `Screen` variant owns its local interaction
state. Settings, the Codex, the lobby, a save or load in flight, and Playback
hold the screen they displaced in `back` and restore it wholesale on leave, so a
pause menu comes back with its cursor and notice. The live game supplies the
backdrop for pause, results, and final-map inspection.

`app/screen.rs` answers every per-screen question in one place, each with one
exhaustive match: the visible session (a replay viewer's own, or the live
match), whether the screen is gameplay or a menu, whether it holds a live match,
whether its backdrop runs, whether a text field owns input, and its mode names.
Screen modules consume `RawEvent` values and return semantic outcomes.
`app/screen_flow.rs` applies those outcomes and draws the active screen. Every
place the frame replaces the screen, including debug requests, the end of a LAN
match, a quit, and a finished save or load, then calls `screen_flow::settle`. It
runs the left screen's exit step and the new screen's enter step, resets
transient input and the cursor, and releases a LAN link no screen holds: the
pause menu freezes the match on entry, and the final map freezes and spectates
it on entry and ends spectating on every way out. Every new match, whether
launched, hosted, restarted, rematched, loaded or loaded over the debug socket,
arrives through `App::install`, which retires the replaced match on the
persistence worker when it is free. `app.rs` owns frame orchestration;
`app/debug.rs` handles debug requests. Screen update logic accepts injected
viewport/input state so navigation can be tested without a GPU window.

The New Match draft records each seat's difficulty and stance. Successful launch
materializes distinct personality seeds into the `Scenario`. Ordinary launches
seed the shell's seed source from wall-clock and process identity; automation
uses a fixed source. This is the pre-scenario entropy boundary. Restart,
Rematch, and replay resume preserve the recorded setup without consulting it
again.

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

A transition that must show its destination at once asks for a rerun pass: the
loop re-enters inside the same presented frame under the new screen. A rerun
pass reads no hardware input, so the key that opened a screen never reaches it,
and the camera glide moves once per presented frame.

Presentation (camera glides, held pans, effects, and music) reads frame time
clamped to `MAX_PRESENTATION_DT`. The live and replay clocks read the unclamped
time and cap their own catch-up. A negative or non-finite clock reading counts
as no time. A live frame of `SUSPENSION_GAP_SECS` or more means the app stopped
presenting, as a suspended iPad app or a sleeping Mac does; the match opens the
pause menu with a notice instead of resuming. Frame time arrives one frame late,
so the rule requires two consecutive live frames, which keeps startup, launches,
and loads from reading as suspensions. Debug-server sessions are exempt.

`oxide_protocol::RawEvent` is the common hardware and injected-input vocabulary.
`input::apply_events` maps gameplay events into camera/selection changes and
staged commands. Cross-frame gestures, held keys, touch state, and control
groups belong to the input layer. Bindings belong to the configuration alone:
the input layer, the menus, Settings, and both read-only viewers borrow
`Config::bindings`, and the input layer never reads the configuration from disk.
`camera::controls` holds the camera hands every view shares: held-key panning
and wheel zoom everywhere, and for the two read-only viewers, middle-drag,
minimap steering, and finger pan and pinch. The debug protocol's `SendCommand`
stages an already-semantic command and does not exercise UI mapping.

`action::BindingMap` owns contextual primary and secondary bindings shared by
input dispatch, UI hints, and settings. `ActionResolver` pairs releases with the
original presses so aliases and modifier changes preserve held actions. Keyboard
card actions use the same availability checks as clicks. Configuration migration
preserves explicit remaps and unbindings without displacing custom chords.

Building controls share one capability-based selection model for single and
grouped selections. Upgrade ladders, weapons, production rosters, and
construction state determine available actions without a building-kind
whitelist. Buildings of one kind remain a group across tiers; an action applies
only to eligible members. Upgrade advances each eligible building one rung,
skips max-tier or offline members, and allocates available scrap in building-id
order. The card shows the recipient count, total cost, and per-tier
destinations. Its activation rechecks pending commands, as do defense orders, so
an upgrade already staged while paused cannot be bought twice or invalidate
another selected defense's target command. The Stop key clears completed
defenses first; a separate Scrap sites card can abandon fresh sites in the same
selection. Committed upgrades cannot be cancelled.

Selecting multiple own production buildings of one kind exposes their shared
roster. Each activation stages one ordinary `Train` per available factory in
building-id order, skips full or offline factories, and spends only available
scrap. Partial batches name the skipped reason. A command-phase projection
includes earlier staged purchases, refunds, and other spending before accepting
another activation.

The collective production dock groups paid units by kind, including active
heads, and shows total count, active count, and the next head's completion time.
A ready head may still await an open exit. Narrow windows use counted sprite
tiles with these details in the tooltip. Clicking a collective tile cancels one
waiting unit first, choosing the back of the lowest-id factory's queue; if only
active heads remain, it cancels the least-progressed head with building id
breaking ties. The tooltip identifies that target, and each click resolves
against pending commands again. Selecting one factory shows the exact ordered
queue and its per-slot cancellation controls. Mixed building kinds keep rally
controls and the first compatible producer's training shortcuts.

A world rally click, from the armed Set rally card or a right-click with only
buildings selected, picks the own unit under the cursor before the ground: new
units then escort it, and the rally line follows that unit's drawn position,
ending in a ring instead of a flag. Minimap rally clicks name ground only.

A Spire or Lattice selection shows its magazine cards: Order, Cancel, and the
Autofill toggle, and on a Spire the armed Launch card. A single own or allied
magazine docks its ordered rounds like a production queue: one chip per paid
round, the head wearing the assembly meter that also draws under the owner's
building, and each own chip cancelling its own position; an ally's chips are
read-only. Cancel refunds the latest round. A Launch click on the world or the
minimap names any tile, seen or not, for every selected loaded Spire; while
armed, and for a selected own Spire's held strike, the blast's two radii draw at
the tile with a line from the silo. Stop calls a held strike off.

Production panels reserve a fixed-width rally group with one shared flag beside
adjacent Set/Reset and Clear buttons. Clear stays visible but disabled until a
selected producer has a rally. Rally changes preserve the action band's height
and production-card positions. The group is wide enough to keep minimum touch
targets; production wraps in the remaining columns. Card rectangles and panel
bounds share the same separator and inset calculations.

Coordinates are logical throughout the input/layout pipeline; the hardware
adapter applies DPI conversion once. Touches arrive through the same ordered
input stream as mouse events. miniquad's iOS backend reports only the fingers
that changed, and a finger iOS takes away, as for palm rejection or a system
gesture, arrives as `TouchCancel` and completes no tap, long-press, box, row, or
card press. On iPad, `UIApplicationSupportsIndirectInputEvents` makes trackpad
and mouse clicks arrive as mouse events rather than touches. The HUD lays out in
a pure pass (`render::hud`) that measures text through a supplied function, so
it runs headless. Each frame lays it out before input, again before each press
or release that follows another event in the batch, and before drawing. Every
pass publishes the `LayoutModel` whose rectangles drive hit testing together
with the panel model, and drawing consumes the same pass, so a click always hits
what the next frame shows. The HUD's supported layout floor is iPad mini in
landscape, 1133×744 logical points at default UI scale; every modern iPad and
Steam Deck (1280×800) sit above it. At the floor, a Harvester carrying scrap, a
factory's rally controls with five products, and the grouped build palette each
fit one band. Smaller windows, and UI scales above 100% on the smallest devices,
are overflow stress cases. `layout::top_bar` places the top bar from measured
text: the bank, income, and unit count run left to right, then the idle and
under-attack badges, and the status hangs off the menu button. Touch-only builds
hold the menu button, the bank, and the build palette's Back card further from
the corners, which iPad screens round and iPadOS safe areas leave out.
`layout::group_column` stacks the control groups on a plate above the minimap's
right edge, mirroring the orders dock, and wraps into more columns when the
window is short. It shows once a group is saved or the selection can be saved,
never while the minimap is hidden, and a setting hides it; its slots and plate
are chrome. The top edge is left to taps because iPadOS takes a long-press there
as a window move.

Selections contain units of one allegiance or buildings of one owner, ordered by
id. Foreign entities can be inspected while visible, but commands remain gated
to the controlled seat. The simulation performs final ownership, fog, cost,
placement, and target validation. Dead entities and hostiles that leave sight
are removed from selection.

Double-clicks and touch double-taps select units or buildings of the picked
entity's kind and owner whose centers lie in the camera viewport. Building
groups span upgrade tiers and apply normal visibility and concealment checks.
Units take picking priority over buildings, as with single clicks.

Selection panels and tooltips derive their facts from simulation accessors.
Static capabilities may be shown for foreign selections; current enemy orders,
loads, and building income remain private. Placement and support previews use
authoritative queries rather than duplicating game rules. Each own Repair Bay
draws a tether to every machine `State::repair_bay_patients` picks, solid at the
fast rate and dotted at the slow rate, and a selected Bay shows one pip per
patient slot. Unknown concealed mines cannot alter player-visible picking or
placement feedback.

A unit panel offers only the verbs its selection can use: Hunt needs a unit that
can fight, Scout a unit that scouts, and Run either, since a scout's right-click
already scouts. Patrol and Accompany stay for every unit. Accompany (`E` by
default) arms a click on any own unit, aircraft included; the rest of the
selection escorts the picked unit, so picking one of the selection works. It is
never a right-click meaning. Stop is not a card. `Panel.stop` is a button in its
own box above the orders or production dock, published as
`LayoutModel.queue_stop` and hit-tested with the cards. It stops the selection
while any selected unit has orders, even when the dock's subject is idle, and
clears a defense's target preference. Over production it cancels every selected
producer's queue with full refunds, last job first, so each `CancelTrain` index
still names its job when it runs. The button takes the chip column's width and
names itself where the chips do. A single factory's dock is headed by the time
left on its queue, or "Ready" while a finished head waits for an exit.

A unit's orders dock and the selection's waypoint chain show programs with every
staged command applied. `Presentation` caches that projection for the tick, the
staged-command count, and the decorated selection; only a staged batch costs an
`inspect_command_phase` call, and it keeps the sites staged Build orders claim.
Pressing an own chip stages `CancelOrder` with the chip's key and how many later
chips share it, sending the rest of the selection along, so the order leaves
every selected own unit that still has it and the dock and waypoints renumber at
once. A chip for an unbuilt site or a planned one cancels the whole site
instead; a site only staged commands place is cancelled by kind and anchor,
since other seats' commands or earlier batches may take the id it was projected
with. An ally's chips do nothing.

Ground orders name whole tiles. Run, hunt, advance, patrol, and rally clicks
clamp the cursor's tile onto the map, since the camera's edge slack lets it rest
past the edge, and their acknowledgment ring draws at that tile's centre, where
the waypoint marker sits. Entity lookups keep the unclamped tile, so a slack
click never binds to whatever stands on the edge.

The selection's waypoint chain draws an own unit's walks, patrol legs, and drop
points at the tile the player clicked, explored or not, rather than at the slot
or endpoint the simulation resolved around it; a landing that took over a walk
still marks the walk's click. Markers are numbered by program position like the
dock's chips, so a leg with nothing left to draw at, such as a lost attack
contact, leaves a gap. A foreign unit's program draws nothing.

Notices from outside a screen's own controls, such as a failed save, a finished
diagnostic export, or the end of a LAN match, go through `App::notify` and land
where the player is looking: the HUD toast strip in live play or a replay, the
Settings and pause menus' own notice lines, and one shared menu line elsewhere,
including over the final map, which has no running clock to age a toast.

Toasts report refusals and outcomes, not armed modes, which the ribbon already
names, or their cancellation; only Patrol coaches its two-step start.
`Presentation::toast` and the tooltip's refusal line capitalize the first
letter, so refusal reasons stay lowercase fragments that also read inside longer
messages.

An own unit selection offers a Retire card, disabled with a reason until a
Crucible is complete. It has no default key, so a stray keypress cannot scrap an
army, and no right-click meaning; Shift queues it. A retiring own machine draws
a gold dismantling meter under its health bar, as a building under salvage does.

Loaded Harvesters and Excavators expose a Return Cargo card and shortcut (`U` by
default). Worker selections use Return Cargo, a single selected transport uses
Unload, and selected buildings use Upgrade on the same key. Mixed unit
selections containing workers use Return Cargo. The action replaces current and
queued work, deposits at a reachable owned Foundry, and leaves the worker there.
A right-click on a completed owned Foundry sends loaded workers to that specific
building; a damaged Foundry also receives repair after delivery. For empty
welders the click repairs as usual, and on an unfinished site it constructs.
Cargo returns replace work even when Shift is held; they never resume an
interrupted harvest job.

### Touch-only builds

`platform::TOUCH_ONLY` is true on iOS, where a finger is always at hand and a
hardware keyboard and trackpad may join it. Platform facts and fingertip sizing
branch on the constant, and pure helpers take it as a parameter, so both
variants compile and test on every platform. Copy and hover follow the hands in
use instead: every screen tracks the last pointer and whether a hardware key has
been pressed (`input::track_hands`), and each frame publishes them as
`platform::Hands` for the copy code. Keys typed into a focused text field do not
count, since the on-screen keyboard sends some as key events.

Every screen offers a pointer path for what Escape does, on every platform. The
top bar ends in a menu button that opens the pause menu, and its clock or PAUSED
status toggles pause; both are `LayoutModel` chrome that `UiView` reports. The
New Match steps, replay playback, the final map, Settings, the roster, and the
replay shelf draw a top-left BACK button instead of a Back row, and playback
adds Play/Pause and a speed button that steps the speed keys' ladder. These ride
`Press::feed`, which also tells the screen when a button claimed an event; the
menus share `button::BackButton`, which acts as Escape does on each face.
Settings splits its rows across tabs on a `tabs::TabStrip` under its title,
which claims its own presses the same way; the Previous tab and Next tab menu
actions step it from the keyboard, and on the Controls tab, which reads raw
keys, the classic Q and E work too. Menu lists and the map grid scroll by a
finger dragged past `press::DRAG_SLOP` (`ScrollPress`). Every cursor steps
through `nav`: arrows wrap at the edges, Home and End land on the first and last
live cell, and paging stops at the ends. Menu rows stand for typed values
(`Menu<R>`), so a screen never maps a row index back to its meaning. Chrome
colors, stroke weights and the eight type sizes come from `theme`; world colors
stay with the renderer. The read-only viewers pan and pinch through
`ViewerTouch`, within the shared `camera::controls::ViewerHands`. Text entry
(the save name, the Settings name, and the Join form's name and host address) is
a `text_field::Form`: labeled fields in one row above Cancel and a confirm
button, each filtering what it accepts and checking its value before a commit.
On touch-only builds the frame loop raises and hides the on-screen keyboard to
follow the focused field and asks for the numbers-and-punctuation layout on the
address.

Hints, toasts, card descriptions, and the tutorial say tap or click for the last
pointer and name keys only once a hardware key has been pressed; copy that mixes
both has a variant for each pairing, and panel cards drop their hotkeys until
then. Card hover, the hover tooltip, and edge pan follow the mouse only while it
is the last pointer. A finger resting on chrome for `TOUCH_REST_MS` previews the
card beneath it with the hover tooltip. Chrome touches never fire a long-press,
so lifting in place still activates the card, and a finger that lands on one
card or group slot and lifts on another activates neither. The one exception is
a control-group slot, where a long-press saves the selection to that group and
spends the finger so its lift does not also recall. A dock chip that discards
work, whether an order, a site, a planned foundation, or a production slot,
ignores a lift after a hold: a finger held past the long-press threshold was
reading its preview, so it lifts without cancelling anything. A world-born or
group-slot finger draws a filling ring from the same rest threshold until its
long-press fires; the slot's ring draws above the HUD and minimap. Disabled
cards publish `CardAction::Refused`, so a tap or click toasts the reason their
hotkey gives.

Gameplay touch lives in `input::touch`. Each finger records where it landed
(`TouchBorn`: world, minimap, other chrome, or the placement ghost), and that
decides what it may drive for its whole life. A minimap finger steers the camera
through `render::minimap_world_clamped`, the same clamp the mouse uses, except
while rally, a strike, or patrol take minimap taps as targets. A two-finger
`Pair` starts undecided: a spread past `PAIR_PINCH_PX` zooms, and a pair that
rests for `BOX_REST_MS` claims a box whose corners follow the fingers. The box
appears only when claimed, so any box on screen can be dragged and selects when
a finger lifts; an undecided pair shows nothing and its lift selects nothing. A
pair with a non-world finger does neither, and one formed mid-pan or while a
mode is armed may zoom but never boxes.

Touch placement shows a `PlacementGhost` instead of placing on the tap. Picking
a building by finger drops it in the middle of the view, and a tap on open
ground moves it there, centered under the finger; while it is down a click moves
it too, so a trackpad taking over builds where the ghost shows. Dragging the
ghost moves it by whole tiles; one smaller on screen than a fingertip rides a
fingertip above the finger, so the spot and its tint stay in sight. Building
waits for the ghost's check mark (`LayoutModel.ghost_confirm`), which runs the
shared `place_at` checks at the ghost's anchor; the ghost itself only moves, and
the tap point is never the anchor. Its cross (`LayoutModel.ghost_cancel`) folds
a row back or gives the building back to the palette. Either presses only for a
finger that landed on it, and a finger that landed on either presses nothing
else, since they sit closer than a still finger may wander. The two hang side by
side below the ghost, or above it where the window's foot or the bottom HUD
(panel, ribbon, minimap, QUEUE toggle) leaves no room or over a row pulled
downward, and with the row handle they draw over the minimap and answer presses
before it, the QUEUE toggle, and the ribbon; the two also answer before the
handle, whose fingertip target reaches onto them when zoomed out. The row handle
(`LayoutModel.ghost_handle`), which a trackpad drags as a finger does, pulls out
a straight row along the axis the pointer leads on, a cell for each footprint it
travels past where the handle sits, so the handle's own offset adds nothing at
any zoom. The row keeps its own plan, judged as a mouse stroke's is: again at
once when the staged batch, the selection, or QUEUE moves, and twice a second as
the world drifts, since judging a row replays every stamp before each one. The
check mark judges it whole once more, with QUEUE as it stands, and builds it;
moving the ghost or a cancelled finger drops it. With queueing held, a
successful check moves the ghost on by the row's step, or by the chain's last
step, or one footprint east after the first. `InputState::placing` holds the
armed kind with its stroke and ghost, so no ghost outlives its mode. The mouse's
hover preview draws only while the mouse is the last pointer. While placement or
a patrol route is armed, a world finger never long-presses; other armed verbs
stand down for a long-press as they do for a right-click.
`InputState::queue_held` merges Shift with the QUEUE toggle that touch-only
builds show, which works like the iPad's Shift key: a tap latches it for the
action at hand, a double-tap locks it, and a later tap turns it off. Latched, it
lasts as long as an armed mode, a whole placement chain or patrol route, and
otherwise the next order or selection change; the check runs after every input
event and after the long-press timer. Letting go of either after a placement has
staged a build ends placement, so the next click selects instead of replacing
the chain just queued; a live stroke still settles at its release. A mouse drag
in placement mode plans a row instead of staging it: every footprint the cursor
crosses, sampled finely enough that a fast move leaves no gap and through the
release point, joins the plan if it can still afford and fit it, judged after
the staged batch and the stamps before it. Whether each stamp defers is derived
from current sight whenever the plan is judged, never kept, so ground that falls
into fog mid-drag is judged from memory. The planned stamps draw as tinted
ghosts, and the ribbon counts the row and its total. The release stages the
stamps in order, rechecking each against the exact pending phase; the first that
stages opens the row, replacing the program unless the stroke queued, and the
release toasts any it skips. Escape or a right-click drops the row and keeps
placing. However a stroke ends, a queue let go during it then ends a chain that
has staged.

A right-click with only scouts selected issues Scout to the clicked tile, enemy
or ground, after the transport boarding check; Run is the deliberate way to send
a scout into danger. A mixed selection keeps the ordinary contextual order.

The mode ribbon (`LayoutModel.mode_ribbon`) shows while a mode is armed,
centered above the panel band. While placement has a spot to judge, under the
cursor or at the touch ghost, the ribbon adds a status row: whether a click
there builds now, builds on arrival, or why it would be refused, and whether it
queues as the builder's next numbered order or replaces its orders. A site the
founder cannot reach is refused when the ground the player can see, or has
explored as impassable terrain, leaves it no way to the doorstep; fogged and
unexplored ground counts as open, so the check never reads what fog hides and
never refuses reachable ground, and the sim's own path check still decides. The
player's own buildings once the staged and planned batch lands also block, so a
row cannot seal a later stamp's doorstep unnoticed, and the search moves as the
sim's does, eight ways without cutting corners. A builder whose queue is full
takes no number, since another selected builder may take the order. Over chrome
or the minimap, where a click does something else, the row stays but goes blank.
The row is as wide as its widest possible status, so the ribbon holds still as
the verdict changes, and a queued ghost wears the same number as its dock chip
will. One `PlacementVerdict`, cached for the tick, the staged-command count, the
selection, and the candidate, feeds the ghost's tint, the ribbon, and the click,
so a spot the bank, a full queue, or an unreachable doorstep refuses tints red
too. The QUEUE toggle (`LayoutModel.queue_toggle`) shows on touch-only builds
while the human commands a selected unit or QUEUE is on. It sits over the
panel's left corner in the orders dock's column, and the dock rises to stack
above it, so queueing orders never moves the toggle. `ribbon_row_press`
hit-tests both before every other target: the toggle takes its tap and the
ribbon cancels its mode. The build palette's Back card
(`CardAction::ClosePalette`) stands in its own column at the band's left edge
and leaves construction in one press. The orders dock and its Stop button stay
stacked above the palette's left corner, so a builder's queued work remains
visible and cancelable while the next site is chosen. On touch a tap that picks
no unit or building while the palette is open and nothing is armed closes it and
keeps the selection, even over a scrap pile: dismissing never also acts. With
QUEUE lit (or Shift or Ctrl held), a roster chip drops its kind instead of
keeping only it. Taps and clicks select through `select::pick`, which gives a
fingertip a wider reach than a cursor. A press that picks nothing on a tile with
known salvage selects that tile as `Selection::pile`, read-only and exclusive:
any other selection drops it, and `Game` drops it once the viewer knows of no
salvage there. Its panel and the desktop hover readout share
`Scene::known_salvage`, which reads live amounts on visible ground and
remembered amounts under fog.

Coaching text waits for a stuck player. `hints::HintClock`, fed each frame's
screen mode and presses, fades coaching in after a delay without a press and
keeps it until the screen changes; draw code reads `hints::alpha`. Menu footers
carry the coaching (`Menu::draw_with_coaching`), and screens that mix
information with key help show only the information until then.

Touch-only builds hide rows they cannot use: Open diagnostics folder (no file
manager) and Quit (the platform closes apps). The Controls tab, the left-handed
preset, and edge pan stay for an attached keyboard or trackpad. A match
autosaves as the window goes inactive, so Continue resumes one the platform
terminates in the background.

## Persistence and replay

`oxide_kit::checkpoint::SessionCheckpoint` is the internal continuation
boundary: scenario, world, controller memory, pending commands, and live
statistics at a completed tick. Capture borrows the host without draining inputs
or running bots. Restore validates the pieces before installation and executes
no historical ticks. The session revision (`checkpoint::VERSION`) is independent
of `SIM_VERSION`; restoration accepts only a matching revision and simulation
version.

The session envelope fingerprints the captured scenario and world together.
Restoration rejects a change to either side of that pairing. Capture trusts the
host to supply the world's original scenario; this consistency fingerprint is
not authentication or proof that historical commands produced the snapshot. Each
configured bot seat must supply exactly one controller checkpoint, and the
restored roster must match the scenario's bot seats.

The shell adapter captures the core checkpoint without a recorder. It also keeps
the human seat, tutorial progress, concession statistics, and decorative
boundary exploration. Camera, selection, effects, and interpolation rebuild, and
the wall clock starts paused.

Player saves use `.oxsave`: eight-byte `OXIDESAV` magic, a little-endian 32-bit
header length, a JSON metadata header, and one checksummed Zstandard frame
containing a CBOR shell checkpoint. Headers are bounded to 64 KiB; both file
size and decoded payload are bounded to 256 MiB. The loader requires exact
lengths and no trailing frames or checkpoint data, checks revisions, and
validates metadata against the restored session. Controller payloads are CBOR
byte strings. No old player-save import or format migration is provided.

Restoration runs no historical ticks and starts a fresh world-origin recording
at the saved tick. Pending input remains pending until that tick executes. A
save is self-contained. Named saves persist until explicitly deleted. Each match
owns one autosave slot: its autosave or LAN autosave and its finished-match
recording share an id, so a save replaces the match's earlier file and the
finished recording replaces its autosave. Finished recordings go to their own
matches folder. Autosaves, LAN autosaves, and finished-match recordings rotate
separately, so rotation keeps whole matches. On iOS, named saves, finished
recordings, replays, and diagnostics live in the app's Documents folder, which
the Files app shows; autosaves, settings, and the LAN rejoin ticket stay in its
Library. Finished matches remain JSON recordings, containing the available
history since the current segment began.

A match continued from an autosave keeps that slot and starts as already saved;
any other load starts a new slot. A save records the slot and session content it
captured, and the session counts as saved only while it still holds that
content. Restart deletes the abandoned match's autosave once the worker is idle,
so a save still in flight cannot write it back. Leave Without Saving keeps the
autosave, since that choice only follows a failed save.

The app also saves on its own, through the same worker and without pausing. A
decided match writes its finished recording as soon as it ends, so a later exit
cannot lose it and Continue never offers it. A match this machine can resume
autosaves after each minute of play, and when the native window reports going
inactive; a LAN host's checkpoint takes the inactive save through its link. Play
counts the ticks the match ran over its speed, so slow or throttled frames still
count and an OS suspension does not. Automated runs are exempt. A failed
background save is retried after a delay, and the player hears of a run of
failures once.

One app-owned worker performs save encoding, save and checkpoint disk I/O,
decompression, restoration, and catalog scans. Smaller file operations run
synchronously on the frame thread: opening a shelf replay for playback, loading
the scenario picked in the New Match wizard, writing the configuration, and the
debug `SaveReplay` request. The worker admits one operation at a time; a
foreground screen can hold one pending intent while a cancelled catalog or load
finishes. Cancellation does not release admission early. Jobs have unique IDs so
a stale result cannot replace a newer session. Restoration produces CPU data;
presentation is installed only on the frame thread. Replaced session data is
dropped on the worker.

A loading frame is presented before dispatch. Loads can be cancelled until the
restored session is committed; cancellation retains the previous session. Named
saves keep the match paused until completion. Save-before-leaving failures
preserve Retry, Cancel, and Leave Without Saving. A window-close request during
a save waits for that operation and the required exit save. The debug protocol
reports `loading`/`saving` and refuses session mutations while those screens own
the boundary.

Recordings can additionally start from a versioned world checkpoint. Its
scenario and world fingerprint are validated, and commands cannot precede its
absolute start tick. Playback, inspection, and statistics execute only the
available suffix. Home and backward seeks stop at the recording origin; the
scrub bar spans that origin through the absolute end tick. Segment statistics
cannot reconstruct earlier events and explicitly describe only available
history. World-only recordings cannot resume live play because they lack
controller memory.

A named save reserves a collision-free destination; an unnamed save replaces its
slot's file. Both use the chassis atomic-write path. Failures are reported to
the player. Checkpoint discovery reads metadata only: eligibility is not full
validation. Continue tries eligible autosaves newest first on the worker,
skipping corrupt payloads and installing the already restored result. Saves in
an unsupported format are listed as unavailable and left on disk.

`Game::from_replay` reconstructs state from the recorded commands. Bots observe
reconstruction to restore their controller-local memory, but their regenerated
commands are discarded. The recorded log is authoritative, and the resumed game
continues recording onto it.

### Read-only playback

`oxide_kit::playback::Playback` owns replay-viewer state. It has no bots,
recorder, or new commands. Forward playback feeds the log into `State::tick`;
seeks restore an earlier in-memory checkpoint and replay the suffix. Checkpoint
storage is bounded, and the shell slices seeks across frames.

While Playback is visible, `App` retains a hidden live game. `PlaybackSession`
owns the playback engine, its own `Clock`, and its own `Presentation`. Rendering
borrows the engine state directly; stepping keeps only the previous positions,
headings, and effect metadata needed for interpolation and casualties. It
neither clones the world into a render vehicle nor constructs live bots or a
live recorder. Live and playback ticks use the same presentation update. Debug
state and clock requests target the engine; camera and overlay requests target
presentation. UI, profiling, and diagnostic context describe the visible
session. Authoritative session mutations are refused.

Playback writes no recording, since a watched replay is already a file on disk.

The ordinary shelf resumes unfinished records, preventing fog-free viewing from
scouting a live match. Completed records can be watched. Developer `--watch` may
inspect any compatible replay. Final Map is camera-only inspection of the final
live state.

## Performance and diagnostics

The player-facing Performance readout is independent of crash diagnostics and
debug profiling. It observes completed frame measurements without changing fog,
pausing a match, or arming capture. Off adds no timing samples. FPS measures
frame-start intervals, including presentation waits; Detailed also measures CPU
work after debug handling and before `next_frame().await`. These are not GPU
timestamps. Screen/session changes reset the display's bounded history.

Crash and freeze diagnostics are always on and write nothing while play is
healthy. At launch the shell installs one `kit::diagnostics::Monitor` for the
process. The main thread publishes its stage (input, bots, simulation,
presentation, draw, present wait or replay load) and tick through atomics, and
each bot seat marks its running decision. A watchdog records a stall when the
main thread or a seat stops making progress, then records its resumption. A
minimized window waiting for frames is not a stall, and a stall in the present
wait can be OS or driver delay rather than a deadlock. The panic hook records
the message, location, thread and a backtrace synchronously, then defers to the
previous hook. Each incident carries every thread's stage, display context and
recent per-stage frame timing, which otherwise stays in memory. Thresholds and
log limits live in `kit::diagnostics`.

Incidents go to `incidents.json` in the diagnostics folder. A minimize reaches
the watchdog when it happens, since a minimized window may run no further frame.
Each log keeps a bounded number of its newest incidents and counts what it
dropped. Diagnostics are observational and never become replay input. Simulation
and bot failures reproduce from the recorded replay, not from timing.

Every tick of a live or LAN match runs behind one unwind boundary. A panic there
writes a crash dump before unwinding continues: the commands through the last
completed tick as a standard replay, and a manifest, written last, with the
panic message, the build, how far the tick got, and the batch the simulation
never finished. Bots decide from the world alone, so a panic while their orders
are collected replays from that prefix. A match continued from a save records
from a world-only origin without controller memory, so its dump reproduces
simulation panics but not bot panics. The folder keeps the newest few dumps; the
interrupted world is never saved.

Report export runs off the frame thread. A report holds the live match's command
log as a standard replay when the match has played any ticks, the root incident
log, the newest crash dump, and a manifest, written last, naming the running
build and what the report contains. The replay's metadata keeps the recording
build, so build differences remain visible even when simulation versions permit
playback. Reports stay local, and diagnostics never take screenshots or enable
the debug server.

## Debug protocol

`oxide-protocol` owns JSON-lines envelopes, bounded TCP framing, input events,
and state views. The live server binds loopback. Socket threads parse requests
and pass them to the main thread; they never access the game. Requests execute
between frames, with screenshot replies deferred until GPU readback.

`DebugSession` and `dispatch_shared` define the common state and clock surface
for live play, read-only playback, and the driven headless session. Each
supplies its own clock semantics. Camera, input injection, native profiling, and
other window capabilities are supported only where they are real. Unsupported
operations are explicitly refused. A LAN match refuses clock, speed, load, and
performance-window requests before shared dispatch, refuses pause and resume on
clients, and accepts `SendCommand` only for the machine's bound seat.

`StateView` is an omniscient QA representation, not an exact serialized state;
use `State::hash` for deterministic identity. `FogView::capture` is the
player-knowledge view for players and agents; bots read the equivalent
`ObservationData`, and a parity test keeps the two in agreement. It hides
hostile intent and economy, exposes enemies under current sight, and otherwise
supplies only recorded ghosts, remembered salvage, radar contacts that name only
air or ground, and public buildings such as Deep Bores and started assembly
cradles. Player-facing commands and effects must respect that knowledge
boundary.

## Rendering and audio

The GPU renderer owns camera clipping, fog composition, sprites, effects, and
screen chrome. Interpolation smooths fixed simulation ticks without moving
simulation entities. Collision slides are eased: a drawn ground body may trail
its simulation position and lean its hull off the simulation heading within
small fixed bounds, while selection, targeting, and turret aim keep reading
simulation truth. Presentation derives from authoritative events and state;
seeks reset interpolation and rebuild any persistent effects needed at the new
tick. Pause and speed follow each session's `Clock`; reduced motion follows the
render preference.

Destruction and projectile caches retain the pre-removal identity, pose, and
visibility needed to present an event after its entity has gone. Cosmetic
visuals cannot reveal unseen events. Authoritative crash trajectories and impact
timing remain simulation-owned; rendering observes them without adding damage
rules.

Direct-fire reports resolve cosmetic contact against solid rendered body pixels.
Units use their drawn frame, rotation, interpolation and aircraft lift; visible
body-frame selection survives lethal removal. Reported unit strikes stay
anchored to the hit-tick pose while the target moves. Compact contact regions
derive from atlas alpha at load time and are shared by all instances. Projectile
launch history retains recorded flight duration and identified targets through
arrival, or until point defense shoots the shell down. When launch history is
absent after checkpoint restoration, payload recipients are reconstructed from
visible world bodies at the fixed impact point; landing reports can also use
pre-removal bodies. No historical ticks are executed. Unguided unit contacts
stay on the original course and require the nominal impact to overlap the body;
bomb spread and ground misses retain their positions. Surface flashes,
fragments, dust and scorch marks share the cosmetic contact point. Weapon and
recipient profiles distinguish rail, orb, mortar, shell, rocket and bomb
contacts. Cosmetic contact never changes simulation targeting, damage, shell
arrivals or splash centers.

`assets` loads the generated sprite atlas. Its manifest covers every resolved
sprite key; the renderer does not load individual sprite textures. Optional rigs
may fall back to composite sprites, but incomplete rig families are rejected.
Frame counts come from the atlas: a row family holds as many frames as it ships
consecutive `_actionN` or `_workN` keys, a rigged unit's from its mount layer,
and every rung of a building's upgrade ladder ships its own hull, and mount for
a defense. The animation code owns timing and which frame each moment shows;
tests keep every selected frame below the shipped count. A Surveyor's `_actionN`
frames unfold its parking radar from the simulation's deploy count, then rotate
the dish while deployed. The presentation controller retains scan timing and
hinge transitions. Driving disables radar immediately while the reflector
returns home and folds over one second; the separate hull keeps its tread
animation. Interrupted transitions reverse from the visible pose, and settled
folds discard their presentation state. A selected own scout draws its radar
ring only while the radar is up. Radar blips draw above the fog in no faction's
color, told apart by shape: a hollow diamond for a ground contact and only its
upper half for an air contact.

Procedural quarry boundaries, pits, peaks, and rock walls derive from map
geometry with fog-aware visibility. The boundary and peaks are one terrace
system measured from the open floor: a peak is the wall left standing, stepping
up once it is about five tiles thick, and a peak meeting the map edge is a spur
whose levels join the wall's. The wall's field reads unexplored tiles as floor
and each peak's as more peak, so neither discloses hidden terrain; both fields
are cached until the exploration they read changes. Rock walls draw layered
slabs from the generated library, sized by local rock thickness with unexplored
tiles counted as more rock, and machinery blockers stay off the edge ring the
boundary covers. The shell extends allied unit sight discs and
completed-building footprint sight into a bounded off-map quarry margin. Its
presentation-only exploration cache updates on every tick, including bulk
advances, and rebuilds during explicit command-log reconstruction. Player
checkpoints retain that cache directly. Map tiles show authoritative simulation
fog. The replay viewer is fog-free, and so is a seat out of an undecided match
once no LAN teammate plays on whom it could tip off; its chrome stays its own.
Animation, heading, and weapon effects use the relevant simulation state rather
than inventing movement or firing delays.

`look` declares each unit kind's presentation in one exhaustive match: draw
scale, gait and tread belts, the atlas rows of its locomotion poses, whether a
separate mount layer draws over its hull, its worker body, a large airframe's
shadow and lift, its weapon's report (sound, shot style and muzzle), and its
strategic marker. Each defense kind likewise declares its mount, whether its
hull's charge rack animates with it, and a direct-fire gun's report at every
rung. A new kind must state each of them there. Facts the simulation already
states are read from its stats instead of a hand list: rotorcraft (no flight
turn rate), large airframes (a crash), scouts, demolition, braced siege, a
weapon's projectile payload, which buildings fight or train, and upgrade
ladders. A kind with mounted guns, the Roc, draws its mount layer once per gun
at that gun's pivot on the hull, turned to the simulation's bearing for that gun
and interpolated the short way between ticks; each gun recoils only for its own
weapon slot's shots. The Roc's rig (`render::roc`) draws its hull's and guns'
ground shadows in place of the shared airframe shadow, then a wall, the gun
yokes and the guns under the hull, then the fans' blades and each of six drone
docks above it. Drones fill the docks rear pair first and leave front pair
first; `dock_motion` remembers when each dock filled or emptied, so its clamps
swing open as a drone lifts away, a landing drone glides in from where it was
last drawn, and a launched drone is drawn rising from its dock while the
simulation flies it from the hull's centre. The Roc's cargo hatch swings open
while a rider boards or leaves, and its aft thrusters glow at an engine heat
that rises while it moves and cools once it stops. A Roc cradle under assembly
draws the Roc facing east from its own ghost layer at the building opacity
steps, under the shared crane. A destroyed Roc keeps each gun's last drawn
bearing; its fall and its wreck's pieces draw the wall, yokes and guns under a
wreck hull with still fans and empty docks, each gun riding the piece that holds
its pivot. A drone is never selected, boxed or rallied to, and a selected own
Roc offers its bay's Order, Cancel and Autofill cards on the same hotkeys as a
magazine's.

`entity_lod` derives full, half, quarter, and eighth-resolution entity textures
at startup without changing authored atlas bytes. Each sprite packs only its
painted area, grown by one eighth-resolution texel of transparency and aligned
to it, so every level samples the area's edge as it would the whole sprite and
the quad covers just that area. Regions pack in descending size order to avoid
wasting full-height rows on small mips. Units with layered rigs omit unused
complete movement/action poses from this cache while retaining idle portraits
and the independent layers. Independent regions have extruded borders;
reduction, linear sampling, level blending, and compositing retain premultiplied
alpha. Physical destination size, including DPI and both axes, selects levels
with a fixed negative detail bias. Secondary UVs and blend weights travel in
vertex data; immutable page materials preserve batching without reordering
translucent layers. The material also handles ordinary straight-alpha 2D draws
and remains active until the screen boundary. Terrain and unrelated effects keep
nearest-neighbor sampling. Panel and roster portraits share this bank, frame
visible alpha bounds, and align to physical pixels. Layered portraits use union
bounds to preserve the relative positions of bases and mounts. Construction
subjects and scaffolds share the authored canvas; verb pictograms fill their
destination without portrait cropping.

`strategic_markers` supplies role and allegiance cues between two zoom
thresholds in logical pixels per tile. Buildings retain subdued footprints
beneath their markers; known unclaimed Extractor frames use amber brackets.
Resource summaries use world-anchored cells and follow the transition slightly
later. Each cell averages its tiles' visibility and the same bounded memory-age
fade as world salvage. Marker visibility shares player exploration, known
claims, apparent buildings, and remembered salvage with world rendering. Ghosts
remain distinct from live observations and draw at the tier they were last seen
at, or at a public building's tier when every seat knows it. A public enemy Deep
Bore out of sight draws above the fog as corner brackets and a drill glyph in
its owner's color, and as an outlined footprint on the minimap. A public enemy
assembly cradle draws its brackets around a hull outline over a bar of its
progress, and its minimap outline fills from the bottom as its crew raises it.
The first time a seat learns of one, and when it stands up its machine, the seat
gets an alert whose badge names the machine and whose minimap pulse is a square
rather than a ring. Markers do not change entity positions, selection, picking,
or simulation.

A seat's own and allied missiles draw, and their motor loops play, wherever they
fly, as its units do; a hostile warhead or interceptor only while the viewer
sees the tile under it (`render::sky::missile_seen`). An interception out of
sight ends in a puff of smoke for a seat whose missile it was, without the other
missile, and in nothing for anyone else; the warhead's owner is told it was
intercepted. What a seat knows of a hostile warhead from `State::known_warheads`
draws above the fog regardless: the strike rings once tracked, and a contact
marker where it flies out of sight, its ring filling toward lock while
untracked. The missile that flies is the round that sat in its launcher, at the
same place, size and bearing. A warhead eases off its deck's rail and then holds
the simulation's position on the straight line to its target; its height shows
only as its shadow falling away down and to the right, as an aircraft's does,
and a slight growth, with smoke from its nozzle (`render::sky::Flight`). The
Spire deck draws at its simulation heading with the round's assembly on it, its
locks releasing over the deck's ready ticks. From
`State::interception_forecast`, a Lattice's firing pod swings onto its
interceptor's path, opens its clamp and launches its own round, which climbs in
a straight line to meet the warhead nose to nose on the stop tick
(`render::sky::InterceptorPath`); the pod is empty from launch. Both missiles
fly on into a flash that swallows them, then a fireball rolls on along the
warhead's heading and smoke and fragments clear. A stop with no forecast flies
the warhead on a moment to meet an interceptor leaving as it is reported. Pods
hold their shot's bearing and swing back north to reload. The first time a seat
knows of a hostile warhead it gets an alert at the impact once known, or where
the warhead flies.

A Maw draws from runtime layers on its unit canvas. Each side's belts roll with
that side's travel, so a pivot turns them against each other. Its drum lowers
and spins while its own hits are recent or its kiln has wreck to eat
(`State::kiln_fuel`), and stows when both end. While the kiln feeds, scrap rides
the conveyor from the hood and heats on its way into the kiln, which glows while
it burns; the belt runs on until its last load is in. An assembly cradle's site
fades the stowed Maw in through the building opacity steps, facing east as the
finished machine stands up, under a bridge crane whose place follows assembly
progress, so it moves only while a crew raises the Maw. Its crew pads draw
beside it while the crew works from them, as far as the viewer knows, and each
pad's lamp lights while a crew member works from that pad; a rival's lamp lights
only while the viewer sees the pad. A Maw first met, as after a seek, starts in
the steady state of what it is doing.

Settings persists marker timing presets (Standard, Earlier, Later) and size in
`Config::markers`, applying changes immediately. A config without marker
settings adopts the defaults without resetting other preferences. Custom config
endpoints are clamped to finite, ordered values. Logical marker dimensions keep
readability consistent across display densities; sprite sampling independently
uses physical pixels.

Simulation events enqueue audio cues. `mixer::spec` is the one table of what the
shell decides per sound kind. The mixer applies user buses, repetition limits,
and camera-relative attenuation. Missile, artillery, bomb, mine and Sapper
detonations, building destruction, and aircraft ground impacts are audible
through fog regardless of ownership. Their distance gain is full inside the
camera viewport and fades linearly to silence a fixed tile distance beyond its
nearest edge, with the same range at every zoom. Zoom weighting also attenuates
heavy sounds at the widest view. Same-kind events coalesce to the loudest
emitter; inaudible events consume no voices and do not raise combat music. A
detonated charge uses only its demolition cue; other buildings destroyed in the
same tick keep their destruction cues. Visuals, target knowledge, launch
warnings, and missile motors follow sight rules. Continuous positional sounds
are owned and stopped individually; pause and screen transitions release them,
and resumed presentation can reconstruct them. Soundtrack state controls music
beds and crossfades. Audio never feeds a simulation decision. Production sprite
and sound bytes remain owned by their generators and approval workflows.

Piledriver anchor cues follow visible deployment and folding transitions. Its
direct-shot effect owns a single pending impact cue, consumed when the cosmetic
round reaches its target on the presentation clock. Pausing holds that cue;
effect expiry and timeline resets discard it. Reload is silent.

A Maw's work is heard as loops rather than reports. Its drum's grind swells from
the first hit, holds while it cuts and dies away once it stops; its kiln's hum
follows the belt's speed, running on until the last load reaches the kiln. Both
follow the machine's presented motion (`maw_motion`) and sound only while the
viewer sees it. A heard cut raises combat music as a weapon report does.

The tiny-skia renderer in `oxide-kit` produces whole-map CPU schematics. It does
not share the native atlas, camera, HUD, animation, or visual polish. Screenshot
responses identify their producer. CPU goldens prove deterministic schematic
composition; presentation and input claims require the real shell.

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
