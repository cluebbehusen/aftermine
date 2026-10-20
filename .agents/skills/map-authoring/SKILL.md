---
name: map-authoring
description:
  Create, edit, audit, and validate Oxide scenario maps and their player setups.
  Use for scenario JSON, terrain layout, quarry boundaries, pits, Extractor
  frames, scrap placement, Foundry anchors, teams, spawn rosters, map metadata,
  symmetry, route fairness, artillery pressure, contested frame clusters, or
  shipped-map gate failures.
---

# Oxide map authoring

Author a map as a gameplay space first. Use mechanical checks for terrain,
symmetry, and seat fairness, then play it to judge routes, pressure, and pacing.

This skill governs competitive maps meant for the shipped pool. For a
non-competitive staging rig — a practice drill or an exact staged scene with a
passive opponent — use the sandbox-matches skill instead.

## Use the scenario language exactly

Confirm the live parser in `sim/src/map.rs` and schema in `sim/src/scenario.rs`.
The current authorable bytes are:

- `.`: passable ground;
- `,`: cosmetic rubble on ordinary ground;
- `#`: rock, which blocks ground and direct ground fire;
- `^`: peak, which blocks ground, air, and every shot across it;
- `~`: pit, which blocks ground while air and fire cross;
- `=`: cliff between two levels, which blocks ground while air crosses;
- `/`: ramp, ground joining a level to the one above it;
- `s` / `S`: normal / rich scrap;
- `E`: top-left tile of a derelict 2x2 Extractor frame;
- `1`–`8`: top-left Foundry anchors for seats 1–8;
- `a`–`h`: top-left Foundry anchors for seats 9–16.

Do not author `w`; it is a rendered wreck marker. Keep every row the same width.
An `E` needs a clear 2x2 ground footprint and represents one fixed build site,
not four independent tiles.

## Build levels with care

A tiered map adds a `levels` array beside `map`: one row per map row, one glyph
per tile, `0`–`9` then `a`–`z` for levels 0 to 35. A cliff and a ramp both carry
the lower level they join; every other tile carries the level it stands on. A
cliff sits at the foot of its drop: the art draws the drop over the last tile of
the upper ground, which stays buildable, and the cliff tile reads as the lower
ground beneath it. A cliff two tiles thick reads as a taller drop. Leave
`levels` out of a flat map. The parser refuses ground at two levels touching,
diagonals included, unless one is a ramp joining them, and a cliff higher than
the ground beside it. A cliff band may part levels more than one apart; a ramp
always joins neighbouring levels.

Levels change fights: ground units trade direct fire only on a shared level or
from a ramp, higher ground between two units blocks their shots, artillery fires
down but never up, and blasts stay on their own level. Aircraft ignore levels. A
ramp holds only Barricades and Scuttle Charges. Make the high ground a decision:
a ramp to hold, a rim to shell from, a basin that pays for its exposure.

A level change is not a reason to go somewhere. Raised or sunken ground needs
something worth holding, such as frames one forward Foundry can support, rich
scrap, or a route that saves time; an empty shelf plays like the empty ground it
replaced. Keep a level's edge straight where a ramp crosses it or a frame stands
at its lip, and jog it only in stretches of several tiles, never one. Levels may
also part along an existing pit, peak, or rock wall, but those edges draw no
cliff, only a change of shade.

The map gates add authoring rules the parser leaves open:

- a ramp is a filled rectangle at least three tiles across, every tile carrying
  the same level, that runs through one cliff band, with its upper level along
  one end and its lower level along the other, a tile clear of peaks, pits, and
  the edge ring;
- each peak and pit stands on a single level;
- every cliff parts two levels, or it should be rock;
- Foundries, Extractor frames, and starting structures stand off ramps on a
  single level, and starting units stand off ramps.

Fill maintained browser metadata: `hook`, `pace`, `mode`, `richness`, and
`theme`. Accepted pace classes are `quick`, `standard`, `large`, `vast`, and
`grand`; richness is `lean`, `standard`, or `rich`. Treat `duration` as an
optional measured claim, not an estimate from dimensions. Add it only after the
current opponent and real play have produced evidence worth publishing.

Name the map like a place and write its `hook` as one plain sentence: what the
map shows and the choice it offers. Name only what a player sees there: pits,
rock walls, peaks, levels, ramps, lanes, gaps, frames, scrap, and ground that
only aircraft reach. Pits are dry quarry workings, so never mention water.
Numbers suit map features, such as two teams or three crossings, but give no
sizes or tile counts and no counts of frames or scrap. Leave out slogans and
dramatic turns of phrase.

## Choose the fairness class

The default, empty `meta.symmetry` value claims exact 180-degree authoring. For
a tile `(x, y)` on a `width` by `height` map, its image is
`(width - 1 - x, height - 1 - y)`. Rotate a footprint with top-left `(x, y)` and
size `(w, h)` to `(width - w - x, height - h - y)`.

Derive paired seats from rotated Foundry footprints, never from seat numbers.
The relation must be an involution and pair hostile seats. Each pair needs:

- mirrored terrain and levels, tile for tile;
- equal starting scrap;
- role-equivalent mirrored unit lists in stable order;
- mirrored prebuilt structures using their real footprint dimensions;
- equal access to scrap and Extractor frames.

Use `meta.symmetry: "metric"` only for intentionally non-mirrored layouts such
as free-for-alls. The audit then proves comparable room, routes, resources, and
frame access within the maintained tolerances instead of claiming identical
tiles.

An omitted team means a one-seat team. Never place everyone on one team. Shipped
content keeps seat 0 human with no bot config; every other seat has `bot: true`
and `bot_config: {}`. Name each seat by its spawn position, such as North West
or Mid South East.

## Keep traffic three tiles wide

Every route a match actually uses must be at least three tiles wide: between
Foundries, Extractor frames, and starting units, no ground gap of one or two
tiles, with starting scrap counting as a wall until it is mined. Groups crowd
and even single units read badly squeezing through a narrow gap, and the
experimental machines need three tiles to pass. Widen a gap rather than seal it
where the route matters, and keep a scrap node either flush with a wall or three
tiles from it.

The rule covers ground where hostile seats meet. An island whose ground joins no
enemy keeps its authored shape, on an island map or beside contested ground. The
audit reports the narrow passages it finds and the map gates enforce the rule.

## Keep big frame clusters answerable

A cluster of Extractor frames no starting base owns must not pay one seat for
the whole match. When rival seats reach a big cluster about as soon as each
other, enough ground outside it must be able to shell its frames: at their level
or above, within the longest artillery reach, with no peak in the line.
Artillery fires down, never up, so a raised bench behind a ramp and an air-only
island across a wide pit are the usual traps; a sunken basin under an open rim
pays for its exposure. Fix a trap by bringing ground into reach, lowering the
cluster, or moving frames toward each base, and keep a reason to fight over the
cluster rather than emptying it.

The audit measures every cluster's doors, arrivals, and shelling ground, near
misses included, and the map gates enforce the rule.

## Design for decisions

Give every reachable region a reason to exist: salvage, a route, an expansion,
high ground pressure, a safe staging area, or a defensible approach. Avoid
decorative dead acreage and layouts where one uncontestable opening claim
decides the economy.

Check these questions while authoring:

- Can every seat fund a viable opening from nearby salvage?
- Can every starting seat on every shipped map be assigned its own distinct,
  fully visible, builder-reachable Extractor frame inside
  `EXTRACTOR_SUPPORT_RADIUS`? Overlapping support regions may expose more than
  one frame to a seat, but two seats cannot rely on the same opening claim.
- On most maps, does each seat also have a nearby natural frame outside its
  starting Foundry's support? Quick maps may instead make that second claim a
  shared contested objective.
- Do maps with large physical footprints carry additional Extractor value beyond
  the starting pockets, even when their pace label is only `standard`? This
  always applies to `large`, `vast`, and `grand` maps. Use otherwise empty
  regions deliberately: two or three frames that one well-placed forward Foundry
  can support make a strong expansion objective and are preferable to decorative
  dead acreage.
- Does every forward frame leave fair construction room for a new Foundry inside
  the support radius rather than prescribing one fragile anchor? Ground naturals
  need a builder route; transport-expansion islands need safe landing,
  frame-perimeter, and Foundry room even when no starting ground route exists.
- Is there a reason to leave the starting pocket?
- Are alternate routes meaningfully different rather than cosmetic twins?
- Is there room to build where armies and harvest lines actually travel?
- Can a losing player contest new value, or does the winner own every recovery
  path after one fight?
- Do terrain silhouettes make each movement and fire rule legible?
- On island maps, can the bot and player reach air or transport play before
  their ground economy deadlocks?

Cosmetic scenery stays separate from hashed terrain unless it deliberately
changes gameplay. Use the visual-assets workflow for quarry dressing and edge
art.

## Audit while iterating

Run the exact map audit from the repository root:

```sh
cargo run -q -p oxide-driver -- map-audit scenarios/<map>.json
cargo run -q -p oxide-driver -- map-audit scenarios/<map>.json --json
```

Inspect reachable room, nearest scrap, hostile ground and air routes, artillery
pressure, narrow passages, frame clusters, spawn spacing, pairings, and
Extractor access. The maintained bands and tolerances in
`driver/tests/map_gates.rs` are authoritative; do not duplicate their current
numbers in this skill.

Hold drafts to the same gates before promotion with
`OXIDE_MAP_GATES_DIR=map-drafts cargo test -p oxide-driver --test map_gates --locked`.

For a shipped-map layout or seat change, run the map gates and choose the
headless/native checks that exercise its changed routes, resources or opening.
An orchestration or metadata-only edit does not require a full match matrix.
Relevant commands are:

```sh
cargo test -p oxide-driver --test map_gates --locked
cargo test -p oxide-driver --test headless --locked
cargo run -q -p oxide-driver -- run scenarios/<map>.json --ticks 12000 --all-bots --map
```

Use the longer window already established by the tests for vast and grand maps.
Inspect the replay summary and watch representative games. A connected,
symmetric, decisive map can still be dull or confusing.

## Keep drafts out of the shipped pool

Draft in `map-drafts/`, never directly in `scenarios/`. The shipped-map sweep
and hash blessing read the entire scenarios directory.

A map retired from the pool that tests, benchmarks, or the built-in skirmish
still load moves to `fixtures/scenarios/`. The game never lists those maps.

Generate the local review page with:

```sh
uv run tools/map_review.py
```

When changing the review-page builder, run its orchestration and escaping tests:

```sh
uv run -m unittest tools.test_map_review
```

The review page presents drafts; it never promotes them. Moving a draft into
`scenarios/`, blessing hashes, and committing it require explicit user approval.
Inspect every changed hash and golden after an intended promotion.
