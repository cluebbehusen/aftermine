# aftermine-opponent

`aftermine-opponent` is the reactive, best-effort opponent every bot seat runs.
The [specification](../docs/aftermine-opponent.md) is normative for this crate:
it says what the bot must do and what it may not do. The
[aftermine-opponent skill](../.agents/skills/aftermine-opponent/SKILL.md) holds
the working rules for changing it.

The bot plays under the same contracts as a human seat. It reads only its seat's
fog-honest `ObservationData`, pays ordinary costs, and acts only through
ordinary `PlayerCommand`s. It uses no floating point.

## How a decision works

On each decision tick the bot updates its memory from what it sees and from its
own failed orders, scores what it wants, then acts in a fixed order:

1. defense and other emergencies;
2. the saving target, once it is affordable;
3. wounded units to Repair Bays;
4. workers;
5. missions: settlements, local guards, lifts, attacks, clearances, strikes,
   raids, focus fire and scouting;
6. production toward what the army needs.

Every action spends from one running total that lives only inside the decision.
Missions command units that exist; production never promises a mission future
units. Difficulty caps how many unit orders one decision may give.

## Main pieces

- `decision` runs one decision: its precedence, running total and order
  allowance.
- `memory` holds what the seat remembers between decisions: enemy units, failed
  sites and given-up targets. `public` adds the buildings every seat can see.
- `map` is the immutable map model built once per match and shared by every
  seat: ground components, distances from each start, building spots, expansion
  sites, and the narrow gates on each way in.
- `frame` breaks ties in a frame anchored on the seat's start, so mirrored seats
  make mirrored choices.
- `workers` keeps harvesting saturated, staffs construction and repair, and
  pulls workers out of known enemy fire.
- `composition` turns what the seat has seen of the enemy into needs by role,
  and picks the unit that best fills a role at a producer.
- `investments`, `saving`, `expansion` and `income` score what the seat could
  build next, protect scrap for one saving target, and estimate income.
- `placement` checks a building site against the seat's own knowledge only.
- `defenses` places and upgrades guns, Arrays, Barricades, Scuttle Charges,
  Repair Bays and Lattices on the approaches to each base.
- `missions` holds the units the seat commands together: defense, local guards,
  the home reserve, attacks, lifts, settlements, clearances, strikes, raids,
  scouting, repair trips and focus fire.
- `warheads` decides when a Spire strike is worth its rounds. `breach` and
  `gunship` decide when to assemble a Maw or a Roc.
- `profile` resolves difficulty, stance and the seeded personality.
- `events` buffers the seat's own stalls and rejections between decisions.
- `checkpoint` saves what carries between decisions, and `trace` reports a
  decision for diagnostics.

## Hosting

`aftermine-kit` hosts one controller per bot seat, so live, headless, saved and
resumed sessions build it from the same scenario. The host records each seat's
own events after every tick and passes them with the next decision. The bot
stays silent once the match is decided, after its seat surrenders, or while it
has no built Foundry.

A `Checkpoint` holds only what the specification lists as persistent state. The
profile, decision interval and map model rebuild from the bound scenario on
restore. `Opponent::act_traced` returns a `Trace` of a decision, and
`Opponent::launches` reports the attacks the last decision launched. Both are
diagnostics that no decision reads.

## Development

Run commands from the workspace root:

```sh
cargo test -p aftermine-opponent --locked
cargo clippy -p aftermine-opponent --all-targets --locked -- -D warnings
cargo run --release -p aftermine-driver -- bot-eval skirmish --ticks 6000
```

The skill covers evaluation, the difficulty ladder, CPU budgets and what to
report at handoff.
