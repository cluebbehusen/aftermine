# Simulation architecture

This document states the contracts `aftermine-sim` keeps: what a change must not
break. It does not describe every rule. Rules live in the code and its tests,
and balance lives in `sim/src/stats.rs`. The table at the end points each
contract to its source and its tests.

`aftermine-sim` does not depend on `aftermine-opponent`. It owns serializable
player knowledge and its fog filtering in `observation`; bot policy lives in
`aftermine-opponent`. Command and production geometry shared with predictors
lives in `geometry`, outside the private tick implementation.

## Authority and reproducibility

`State` is the complete authoritative world. Its only public mutation is
`State::tick(&[PlayerCommand])`, which advances exactly one fixed step. Camera
state, input, interpolation, effects, audio and UI never enter `State` and
cannot affect an outcome.

A `Scenario` holds the map, players, starting entities and bot setup needed to
begin a match. Browser metadata in it is ignored by the simulation. A replay
embeds the whole scenario, so reconstruction never depends on the original file.
The simulation has no random stream of its own. Each bot seat records a
personality seed in its configuration, and the bot derives its profile from it.

All outcome-relevant arithmetic uses fixed point or integers. Entity tables stay
in stable id order, and every selection rule ends in an explicit tie-breaker.
`State::hash` serializes authoritative state canonically; protocol views are not
a substitute for it.

`TickReport` and its `Event`s are output only. Consumers may use them for
statistics, effects, sound and assertions, or drop them. The simulation never
reads an event back.

## Map symmetry

Shipped maps are mirrored under a half-turn, and mirrored seats must get
mirrored outcomes. Every geometric tie therefore breaks in a frame relative to
the query, the footprint, the owner or the map. It never breaks by global entity
id, seat number or row-major scan order.

- Route ties use a query-oriented tile rank that reverses with the start-to-goal
  query under a half-turn.
- Group goals, escort slots, drop points, doorsteps, production spawns, eviction
  escapes and make-way relocation scan in the frame of the issuing unit, founder
  or producer, fixed when the command is issued.
- Equivalent workers, Repair Bay patients and perfectly stacked bodies separate
  by owner-local rank, rotated into a map-relative frame where geometry gives no
  direction.
- Target and harvest-source ties go by bearing from the unit, then by id.
- Units spawn facing the map centre, so mirrored placements have mirrored turn
  costs.
- Fixed-point vector scaling preserves exact negation, so opposite movement rays
  cannot drift apart by one raw unit.
- Passes that alternate direction for fairness, such as collision relaxation and
  kiln bites, alternate with tick parity.

Building contact outlines follow fixed artwork, so an asymmetric outline can
offer different frontage after a half-turn. That is the one accepted exception.

## State construction and trust boundary

`Scenario::build` is the normal constructor. It parses and validates authored
data, normalizes team ids, places Foundries and starting entities, and builds
tick-zero vision. A tiered map adds a layer of levels beside the terrain rows.
Parsing and deserialization share one level check: touching ground meets only at
overlapping levels, a cliff stands no higher than the ground beside it, and a
layer at a single level is stored as a flat map, so every map has one form.

Scenario mode defaults to an ordinary match. Sandbox mode makes Foundry anchors
optional, permits allied-only or disconnected scenes, and disables automatic
elimination and victory. Command, placement, ownership and cost checks still
apply, and a sandbox seat can command units without a Foundry until it
surrenders. Mode is serialized state: a sandbox cannot deserialize with
elimination stamps or a final result, and session checkpoints require matching
scenario and world modes.

All `State` fields are crate-private. Other crates get narrow immutable
accessors and change the world only by passing commands to `tick`.
`inspect_command_phase` is the one exception, for prediction. It clones the
state, applies only command validation and command-phase effects, shows a
restricted read-only view to a callback, and discards the clone. It never
advances or installs authoritative state.

Deserialization goes through a private mirror type and then
`State::validate_invariants`; there is no unchecked path. Validation covers,
among other things:

- player, team, result, map, and vision-table consistency;
- sorted entity ids and monotonic next-id counters;
- hp, cooldown, progress, queue, coordinate, and tick envelopes;
- harvest-capacity bounds on scrap held by walking and transported units;
- valid owners, producible queues, entity references, and shell fields;
- coherent construction, salvage, recovery, ghost, radar, and memory state;
- canonical ordering for every collection whose order is observable.

Each new serialized field needs an invariant decision. The integrity tests
exercise meaningful malformed relationships and bounds, and round-trip states
the real simulation produces, so validation never becomes stricter than
reachable play. Extend the owning contract's coverage rather than adding a
primitive decoding check for each field.

## Tick pipeline

Phase order is game behavior. `State::tick` performs:

1. Capture any newly stranded economy's finite recovery entitlement and resolve
   provisional sites whose full footprints are visible.
2. Validate and apply this tick's commands in their recorded order, refunding
   unstarted sites after their last worker commitment is replaced, then hold
   every assembly cradle to its crew cap.
3. Apply recurring income, let autofilling magazines buy a round, assemble
   ordered rounds, advance production queues, and spawn completed units.
4. Cancel unstarted construction over known mines, then decay unclaimed
   tier-zero construction sites on their global cadence.
5. Run unit brains and building behavior, turn silo decks and launch aligned
   strikes, intercept shells and tracked warheads, land arriving shells and
   warheads, feed kilns, and resolve buffered damage, construction, salvage, and
   repair.
6. Resolve boarding and unloading after every unit has decided, then stand up
   the machine of every cradle whose last work landed.
7. Evict pathless ground bodies from newly claimed blocking footprints.
8. Follow paths, then resolve same-domain unit collisions. Parking radars unfold
   a tick further on machines that did not drive themselves, and fold on any
   that did.
9. Retain large aircraft motion and resolve due crash impacts against current
   positions.
10. Detonate armed Scuttle Charges under hostile post-movement bodies.
11. Schedule airborne crashes, remove dead entities, take apart units whose
    dismantling meter filled, deposit eligible wreck salvage, and refund
    unstarted sites with no surviving worker commitments.
12. Apply wreck decay on its global cadence.
13. Rebuild team-shared visibility and reconcile fog memory, including each
    team's hold on hostile warheads in flight. Activate or refund newly visible
    provisional sites. Newly discovered mines cancel unstarted sites before the
    next command, and cradle crews are held to their cap again, so orders that
    came up during the tick obey it. Tile goals whose clicked tile the owner's
    team has now explored take their spread slots, and units left without a
    route drop their stall counts.
14. Determine victory or draw from surviving, non-resigned teams and discard any
    remaining pending crashes, warheads in flight and warhead holds when the
    match ends.

Shots and hp work are buffered while actors decide against stable positions, hp
and entity tables. Orders, paths, harvesting and billing may still change in
their declared order; the brain phase is not an immutable snapshot. Damage
resolves before construction, salvage or repair, so fire wins a same-tick tie
and nothing repairs a destroyed target back into existence. Retaliation is
derived afterward from surviving victims.

Weapon hits, mine blasts and aircraft impacts share hp reduction and the
`DamageTaken` notification to the victim's owner, while keeping their own phases
and targeting rules. Zero damage, already-dead entities and provisional
scaffolds produce no notification.

Once a result exists, later ticks ignore commands and skip world phases, but the
tick counter still advances so external timelines stay aligned. Per-tick
acceleration structures, such as the unit spatial index and reachability labels,
are scratch: rebuilt where they are used, never serialized or hashed.

A destroyed airborne Condor, Skyhook or Roc leaves a pending crash whose contact
point is fixed at death. Pending crashes resolve in death-tick and unit-id order
and damage hostile ground units and buildings near the impact, never allies or
aircraft. They survive serialization and are discarded without damage once the
match is decided.

## Commands and unit programs

A `PlayerCommand` pairs an issuer with a `Command`. The command layer proves
ownership, eligibility, fog legality, cost, queue capacity, target validity and
placement. A rejected command produces an event and leaves the state unchanged.
Lists of unit or building ids have set semantics: dispatch sorts and
deduplicates them, while replay bytes keep the original payload.

Commands set intent; they rarely move or damage anything at once. Each unit has
one active `Order`, a bounded FIFO queue and a `looping` flag:

- a plain order replaces the current program, and a queued one appends to it;
- a finished order hands over to the next or leaves the unit idle, and a patrol
  rotates finished legs to the back;
- an overriding command clears the abandoned program as one unit;
- a walk whose goal cannot be reached completes on the reachable tile nearest
  it, reports `OrderStalled { NoRoute }` once unless the program loops, and the
  program continues;
- any other order that cannot be routed drops only itself, except that a refused
  chase, an empty bank or a full sling clears the whole program;
- `CancelOrder` removes one order, named by its `OrderKey` and counted from the
  end of each unit's program, so legs finished while the command is in flight
  never change which order goes. An unstarted site whose last Build order goes
  is refunded.

Re-issuing the active order without queueing continues it: its path and progress
survive.

Movement stances are distinct contracts. `Run` never engages. `Advance` keeps
moving but may take primary shots that are already aligned and in range. `Hunt`
acquires and pursues enemies along the route. `Attack` commits to its target.
Idle acquisition and retaliation may leash a unit back to its station. Work
orders (`Harvest`, `Build`, `Found`, `Repair`, `RepairUnit` and `Salvage`)
persist and are carried out by unit behavior over later ticks.

`Accompany` puts units on escort duty around one own unit. Slots stay fixed as
the party moves, and escorts fight only within set radii of the unit they guard
before returning to duty. Escort chains are loop-free: dispatch refuses an
escort the target already follows, and the validator refuses a state holding a
loop.

`Tend` is a worker's walk that welds on its own initiative. A patrol gives
workers tending legs, and an idle worker with idle welding on (`SetIdleWeld`, on
by default) sets out from its post to weld an own wounded machine or damaged
building nearby, read from a snapshot taken after the command phase. Tending
never interrupts another order, an empty bank only pauses it, and a queued
command replaces a trip as it would an idle program. A building with an active
or queued salvage commitment is never a patient.

Walks carry a `Goal`: the clicked tile, the spread slot the unit aims for, and
an optional endpoint. A tile goal must lie on the map, and an off-map one is
refused as `OutOfBounds`; an unreachable goal is never refused. A group spreads
over open tiles around the clicked tile only once the issuer's team has explored
it. Until then every member heads for the clicked tile and takes its slot at the
end of the tick its team explores it. Resolution reads the real map; only its
trigger is player knowledge, so spreading never reveals unexplored ground. A
slot or endpoint is stored only when it differs from the clicked tile, so a
plain reachable order serializes as a plain tile.

Reachability comes from 4-connected component labels over the passability routes
use, which match A* because routes never cut corners. A short walk, one that
stops somewhere other than its target, completes and reports once. It also
completes on touching a chain of idle bodies of its own team that reaches close
to its endpoint. Only own-team bodies count, so an enemy out of sight never
changes where a walk stops.

A producer's rally is a tile or an own unit, and units born under a unit rally
escort it. The validator refuses a unit rally that names a unit out of play or
another seat's unit.

`Scout` legs plan from team knowledge only: visible armed enemies' reach,
remembered defenses, radar contacts, recent incidents and known Scuttle Charges.
A scout with no safe view for `SCOUT_HOLD_TICKS` ends its leg with
`NoSafeVantage`.

## Movement and collision

Ground routes use deterministic eight-direction A* with no diagonal corner
cutting. Ground passability is open terrain with no undepleted scrap node and no
non-stealthy building footprint; a buried Scuttle Charge blocks nothing. Air
movement ignores rock, scrap and buildings, but peaks block aircraft too.

A path is advisory, not a reservation. Every ground step rechecks its next
waypoint, because construction can claim ground after the path was planned; an
invalid path is dropped and planned again. A site placed under a pathless ground
body gives it a real escape path while keeping its order and work progress.
Make-way relocation happens only after a foundation is accepted and paid.

Routes ignore bodies. A ground follower's lookahead never passes through a
friendly ground body standing still, and a body whose progress is cancelled by
contact for `STALL_REPLAN_TICKS` plans again. Only friendly bodies count,
because steering around an unseen enemy would reveal it; hostile bodies meet
only the collision resolver.

A hull wider than the Breaker's radius claims body tiles around its centre tile.
It routes only where all of them are passable, is seen when any of them is, and
denies hostile foundations on them.

Workers and contact weapons route to perimeter tiles, then make a short,
collision-checked approach to the building's contact outline. Each kind has an
outline per tier and per construction stage, traced from its art, and a shell
test keeps every outline close to its art. Outlines never change tile
passability. Work, melee and ranged attacks on buildings share one crowd-aware
position chooser. When two claims overlap, the unit nearer its own position
keeps its claim whatever its seat, so allied crowds cannot all yield to each
other; an exact tie goes to the earlier unit of one seat.

Ground motor speed is serialized and validated. A pathless ground unit can still
be braking, so group arrival and anchored collision priority require a motor
speed of zero. Motion reports split propulsion from collision correction for
observers and never affect state.

Turn-limited aircraft fly committed arcs and never steer into an arc they could
not fly out of. There is no landing command: a ground destination with nothing
queued hands over to an internal `Land` order near the goal, and a pad is chosen
only among tiles the owner's team has explored, since it appears in an order the
player can see. The validator refuses landed states a real landing could not
produce, but allows a parked airframe over scrap, since a crash can leave wreck
under one.

Ground collides only with ground and air only with air, and turn-limited
aircraft collide with nothing. Relaxation is deterministic: candidate pairs are
ordered once per tick, iteration direction alternates with tick parity, a
per-tick budget bounds displacement, terrain beats any push, and overlaps within
a small slop are left alone so rounding never keeps a parked crowd moving. A
correction requires every tile the proposed position touches to be passable. A
productive stationary unit holds its position against an approaching one.

## Economy, construction, salvage, and repair

A delivery credits the bank exactly once, on the last tick of an uninterrupted
unload of `UNLOAD_TICKS`. An interrupted or cancelled unload never loses cargo,
and the deposit event reports what the bank actually credited. `ReturnCargo`
prefers safe routes and keeps an explicit Foundry as its destination.

Recurring income runs in the production phase: Reclaimers and Refineries on
their cadences, restored Extractors' fixed income, the Foundries' baseline drip,
and Crucibles consuming wreck. A completed own Foundry within the support radius
raises an Extractor's yield without stacking, and a Deep Bore triples both
yields. Support is computed from the two footprints, so it adds no serialized
state. A seat with no machine that can harvest, in the world, aboard a transport
or prepaid in a queue, draws a finite recovery entitlement.

Extractor frames are immutable map features. Only an Extractor may claim one,
and destroying the Extractor frees the same frame. Discovering any tile of a
frame makes the whole frame eligible for a deferred build.

An accepted immediate build pays for and places a site that blocks ground from
that command on, except a buried Scuttle Charge. A deferred build pays for one
provisional scaffold, which gives no vision, occupancy, damage target or
progress. Once the owner's team sees the whole footprint, the simulation checks
placement against that knowledge: a blocker cancels the scaffold with a full
refund, and clear ground activates it under the same building id without another
charge. Hidden occupants never change acceptance, payment or the preview.

An unstarted site is cancelled with a full refund when its last worker
commitment goes, whether by replacement, Stop, failed travel, boarding or death.
Replacement construction can use those refunds atomically: a rejection keeps the
old sites and programs. A started site follows the ordinary decay and
health-based refund rules. Salvaging a built structure refunds through a
cumulative ledger so rounding cannot drift, leaves no wreck, and refunds prepaid
production.

Completed enemy Scuttle Charges stay hidden without detection, and a witnessed
charge leaves a last-seen marker. Known charges block placement. An undiscovered
charge allows placement as empty ground would; its discovery cancels the
overlapping site with a full refund. The first crew work over an undiscovered
armed charge sets it off before construction resolves, and several workers
cannot multiply one blast.

An upgrade pays up front and takes the building offline as a site on its new
tier. It refits at one progress tick per tick and cannot be sped up, paused,
cancelled or left to decay. A Deep Bore upgrade starts only while a completed
own Foundry stands within the support radius.

An assembly cradle is paid in full when placed and never completes. At most
`ASSEMBLY_CREW_CAP` workers hold it, and a newcomer is admitted only in place of
a strictly slower holder; crews settle after the command phase and again at the
end of the tick. When its last work lands the cradle leaves the world and its
machine stands up at its centre, reported as `UnitAssembled`.

`Retire` takes own units apart at a Crucible over `RETIRE_TICKS`. Cleanup
removes them after every damage source of the tick, so a unit killed that tick
wrecks instead. The refund, scaled by health, stays below the repair rate, so
welding a machine and then retiring it always loses scrap.

Building repair and field welding bill per accepted hp. Repair Bays feed the
same unit-heal and building-work resolvers. Each Bay serves at most
`REPAIR_BAY_PATIENTS` units, Bays choose in building id order, and a later Bay
skips units an earlier one took. Units have first claim on limited scrap. A
structure with an active or queued salvage commitment gets no automatic repair,
which keeps repair and teardown mutually exclusive. Concurrent sources share the
patient's missing health, fully unusable paid work is refunded, and nothing
repairs a target destroyed by that tick's volley.

A machine with a kiln, the Maw, takes no weld or Bay care and heals only by
biting wreck. A bitten wreck is spent even if the machine dies that tick.

## Combat, weapons, and terrain

Weapons read immutable stats: range, minimum range, cooldown, damage, target
domains, splash, indirect fire, and whether a shot is hitscan or a projectile.
Buildings count as ground targets, and cooldowns are stored per weapon slot.

A weapon fires only once its bearing is within a small tolerance of the target.
Turret headings, gun bearings and mounted-gun bearings are serialized, and
validation refuses one on a kind without that mount.

`AttackTarget` resolves through team knowledge to a visible entity, a remembered
or public building, or a radar contact. Explicit attacks may approach without
sight. An automatic engagement never fires on a building out of current sight;
only an explicit order may shell a memory. A contact attack ends when sight and
radar are both lost.

Automatic acquisition prefers visible enemies over radar contacts and never
picks a remembered building. Exact ties go by bearing from the unit's nose, then
id. A unit fighting a victim it chose itself looks again only on ticks that are
multiples of `TARGET_RECHECK_TICKS`, the same ticks for every unit, and switches
only to a clearly better victim, so equal victims never trade places. A defense
takes the target it could fire on soonest, so it never swings between two
targets that stand still.

Direct ground-to-ground fire traces terrain: rock gives cover, buildings and
scrap do not. Fire involving aircraft and indirect fire ignore rock. Peaks block
everything. On tiered maps, direct fire needs both ends on a shared level (a
ramp shares the levels on either side), higher ground between them blocks it,
and indirect fire never lands above the shooter's highest level. Shots involving
aircraft ignore levels. Splash and blasts stop at peaks and stay on their own
level. Aircraft crashes and warheads ignore peaks and levels. All of these rules
live in `sim/src/tick/fire.rs`.

Projectile weapons launch a serialized `Shell` toward an aim point fixed when it
fires, and the shell is unguided after that. Predictive aim samples only state
from before the unit brains run. On arrival a building takes only a direct hit.

Point defense stops projectiles, never hitscan fire and never warheads. Charges
are its only limit, and shells are assigned to defenders greedily in one
complete order. Its state is validated on exactly the kinds that carry it.

A Roc's drones are real units that take no commands. They die with their host,
and validation refuses a drone without a live host of its owner or a bay over
capacity.

## Warheads

A Spire and a Lattice each keep a magazine of prepaid rounds. Rounds are paid
when ordered and refunded in full when cancelled or when the building is
salvaged; loaded rounds and destroyed buildings refund nothing. Autofill buys a
round in the production phase, in building id order, only when the bank covers
it without spending a stranded seat's recovery package.

`Launch` holds a strike on any tile, seen or not, while a warhead is loaded. The
deck turns toward it and launches once it has stood aligned for the silo's ready
ticks. The warhead flies a straight line at a fixed speed, and validation
requires its stored flight time to match its line. On arrival it bursts with one
falloff splash against every hostile target, aircraft included, and its hits
join the tick's buffered volley.

Each team counts the ticks it has held each hostile warhead in true sight or
allied radar. At one threshold the warhead is tracked, which reveals its silo;
at a second it is locked on. Before arrivals land, a built, loaded Lattice stops
a locked-on warhead when both its impact point and its current position lie
inside the Lattice's ring. Pairs are assigned greedily in one complete order,
and `State::interception_forecast` runs the same assignment ahead.

`State::known_magazines` and `State::known_warheads` decide what a seat knows of
magazines and warheads. Both fog projections carry exactly that.

## Fog, memory, radar, and teams

Each seat has `visible` and `explored` grids. Visibility is rebuilt every tick
from completed allied buildings and allied units, and explored ground only
grows. Rock does not block vision. Teammates share byte-identical sight and
memory, computed once per team.

`ObservationData::fog_honest` is what bots read. It copies both grids in
canonical order, so a policy can tell current sight from remembered ground
without consulting authoritative state. The debug protocol's `FogView` projects
the same knowledge for players and agents, and a parity test in
`protocol/src/view/tests.rs` keeps the two in agreement.

Public buildings are the only hostile facts every seat holds regardless of fog:
a Deep Bore's owner, kind, tier and footprint from the moment its upgrade
starts, and an assembly cradle's with its progress from its first work. Neither
shows any other condition. Both projections derive them from current state, so
they add no memory.

Enemy buildings remain as last-seen ghosts until their footprint is seen again,
with everything about them as last seen. Scrap and wreck amounts also freeze at
their last visible value. Radar adds contacts outside true sight from allied
masts and from machines whose radar is up. A contact carries a tile and a domain
but no owner, kind or entity id. Radar matching across ticks uses only reported
movement, and contact ids are never reused.

A hit on a machine that can harvest or scout, or an allied loss, from an
attacker the victim's team cannot see leaves an incident. It remembers only the
victim's tile, for a bounded caution period, and never the attacker.

All allegiance checks go through normalized team ids. Teammates share vision,
cannot target one another, and win or lose together. A resigned seat can no
longer command, and its Foundries no longer count toward victory; its remaining
machines carry on by themselves.

The [opponent specification](aftermine-opponent.md) describes how bots decide.
The simulation applies the same command, cost and visibility rules to every
command source.

## Maintained entry points

This table names the first source and focused suites to inspect. It is a routing
map rather than an exhaustive test inventory.

| Contract                                          | Primary source                                                                                                                          | Focused evidence                                                                                                                                                                                                                                                                      |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Scenario build and authored map                   | `sim/src/scenario.rs`, `sim/src/map.rs`                                                                                                 | `sim/tests/integration/pits.rs`, `sim/tests/integration/extractors.rs`                                                                                                                                                                                                                |
| State, hashing, validation, and teams             | `sim/src/state.rs`, `chassis/src/hash.rs`                                                                                               | `sim/tests/integration/state_integrity.rs`, `sim/tests/integration/determinism.rs`, `sim/tests/integration/teams.rs`                                                                                                                                                                  |
| Placement, deferred founding, and upgrades        | `sim/src/state/placement.rs`, `sim/src/tick/commands.rs`, `sim/src/tick/brain.rs`, `sim/src/tick/brain/economy.rs`                      | `sim/tests/integration/behavior_construction.rs`, `sim/tests/integration/extractors.rs`, `sim/tests/integration/upgrades.rs`, `sim/tests/integration/foundries.rs`                                                                                                                    |
| Tick scheduling, production, cleanup, and charges | `sim/src/tick/mod.rs`, `sim/src/tick/production.rs`                                                                                     | `sim/tests/integration/behavior_rules.rs`, `sim/tests/integration/behavior_economy.rs`, `sim/tests/integration/field_kit.rs`                                                                                                                                                          |
| Command vocabulary and set semantics              | `sim/src/command.rs`, `sim/src/tick/commands.rs`                                                                                        | `sim/tests/integration/command_canonicalization.rs`, `sim/tests/integration/cancel_order.rs`, `sim/tests/integration/fuzz.rs`                                                                                                                                                         |
| Unit programs, routing, movement, and collision   | `sim/src/tick/brain.rs`, `sim/src/tick/brain/locomotion.rs`, `sim/src/tick/reach.rs`, `sim/src/tick/movement.rs`, `chassis/src/path.rs` | `sim/tests/integration/behavior_movement.rs`, `sim/tests/integration/movement_lab.rs`, `sim/tests/integration/peaks.rs`, `sim/tests/integration/pits.rs`                                                                                                                              |
| Boarding and unloading                            | `sim/src/tick/brain/logistics.rs`                                                                                                       | `sim/tests/integration/transports.rs`                                                                                                                                                                                                                                                 |
| Scouting                                          | `sim/src/tick/brain/scout.rs`, `sim/src/vision.rs`                                                                                      | `sim/tests/integration/scout.rs`                                                                                                                                                                                                                                                      |
| Escort duty                                       | `sim/src/tick/brain/escort.rs`, `sim/src/tick/brain/combat.rs`                                                                          | `sim/tests/integration/accompany.rs`                                                                                                                                                                                                                                                  |
| Harvesting, income, salvage, and repair           | `sim/src/tick/brain/economy.rs`, `sim/src/tick/brain/tend.rs`, `sim/src/tick/production.rs`                                             | `sim/tests/integration/harvest_zones.rs`, `sim/tests/integration/salvage.rs`, `sim/tests/integration/repair_unit.rs`, `sim/tests/integration/auto_weld.rs`, `sim/tests/integration/auto_weld_buildings.rs`, `sim/tests/integration/repair_bay.rs`, `sim/tests/integration/smelter.rs` |
| Retirement at a Crucible                          | `sim/src/tick/brain/retire.rs`, `sim/src/tick/mod.rs`                                                                                   | `sim/tests/integration/retire.rs`                                                                                                                                                                                                                                                     |
| Assembly cradles, kilns, and breachers            | `sim/src/tick/assembly.rs`, `sim/src/tick/brain/kiln.rs`, `sim/src/tick/brain/combat.rs`                                                | `sim/tests/integration/assembly.rs`                                                                                                                                                                                                                                                   |
| Weapons and simultaneous resolution               | `sim/src/stats.rs`, `sim/src/tick/brain/combat.rs`                                                                                      | `sim/tests/integration/behavior_combat.rs`, `sim/tests/integration/combat_edges.rs`, `sim/tests/integration/shells.rs`, `sim/tests/integration/peaks.rs`                                                                                                                              |
| Warheads, magazines, and interception             | `sim/src/strike.rs`, `sim/src/tick/ordnance.rs`, `sim/src/tick/brain/warheads.rs`                                                       | `sim/tests/integration/warheads.rs`                                                                                                                                                                                                                                                   |
| Mounted guns                                      | `sim/src/tick/brain/combat/mounts.rs`                                                                                                   | `sim/tests/integration/roc.rs`                                                                                                                                                                                                                                                        |
| Drone bays and drones                             | `sim/src/tick/drones.rs`, `sim/src/tick/brain/combat/drones.rs`                                                                         | `sim/tests/integration/drones.rs`                                                                                                                                                                                                                                                     |
| Point defense                                     | `sim/src/tick/brain/point_defense.rs`                                                                                                   | `sim/tests/integration/point_defense.rs`                                                                                                                                                                                                                                              |
| Fog, memory, radar, and stealth                   | `sim/src/vision.rs`, `sim/src/state.rs`                                                                                                 | `sim/tests/integration/bastion_acquisition.rs`, `sim/tests/integration/field_kit.rs`                                                                                                                                                                                                  |
