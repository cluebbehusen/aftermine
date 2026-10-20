# oxide-opponent

`oxide-opponent` is the reactive, best-effort opponent every bot seat runs. This
page is its normative specification: what it must do, what it must keep exact,
and what it may not do. The
[oxide-opponent skill](../.agents/skills/oxide-opponent/SKILL.md) holds the
working rules.

It plays under the same fair-opponent and determinism contracts as every
controller (see `AGENTS.md`): fog-honest information, ordinary costs and rules,
commands as its only effect, and no floating point.

## Principles

- **Reactive.** It responds to what it has seen: enemy air, defenses, pressure
  on its assets, relative wealth, and failed orders.
- **Best-effort.** It scores what it wants, issues ordinary commands, observes
  what happens and recovers. It does not prove that a plan is feasible before
  acting.
- **Legible.** A player should be able to read its reactions: "I built air, so
  it built Flak."

## What stays exact

The bot always knows, exactly:

- scrap on hand and what the current decision has committed;
- which units and buildings each mission owns;
- which workers are assigned to construction;
- which units are queued, which count toward composition;
- what each transport carries.

An emitted purchase is an attempt until a later observation confirms it; the
decision reserves its cost meanwhile. Fog honesty and recovery are exact too.

## What is approximate

Combat predictions, launch timing, danger, own income, enemy economy, and
whether a movement will succeed are estimates. The bot acts on them, accepts
occasional failed movement and missed opportunities, and recovers. It must not
wait for certainty: requiring a favorable estimate, a large enough force, a safe
route and reassuring reconnaissance all at once produces hesitation.

## Decisions

On each decision tick the bot updates memory from its fogged observation and its
own events, estimates the situation, updates its needs and investment list, then
selects actions in precedence order:

1. emergency defense and recovery;
2. the saving target, when it is affordable and can be placed;
3. free wounded units to a calm Repair Bay with room, and wounded kiln machines
   to safe known wreck, where they sit out missions and welding while they heal;
4. workers, to keep harvesting saturated: every known node or wreck whose trip
   repays a Harvester within a stance and greed horizon, filled as stance, greed
   and difficulty choose, with workers left without a place salvaging other
   known wreck; until the army reaches the stance minimum, workers that would
   cost more than the army wait for army production, and a seat that exposed
   puts up a Turret before any tech; a worker the simulation sent welding on its
   own stays free for these orders and leaves known fire like an idle one;
5. missions, advancing their phases and recruiting as needed;
6. production toward composition needs, from scrap not protected for saving.

Spending and ownership are coordinated through one running total that lives
inside the decision and never outlives it as a type. Difficulty limits unit
orders per decision; purchases do not count against that allowance. The
allowance is consumed as actions are selected, so only selected actions affect
the running total and mission state.

## Investments and saving

- Each decision scores concrete investments: production capacity, tech
  buildings, an expansion Foundry at a specific site, a defense at a specific
  spot, upgrades, and a unit a wanted role prefers that the scrap on hand cannot
  buy. Needs, capability needs, personality and situation set the scores.
  Production capacity comes from the income working producers leave unspent
  while their army roles have need, and producers queue far enough to stay busy
  until the next decision. Small stocks such as scouts, Scuttlers, Sappers and
  carriers never add a producer. Scuttlers draw on the producers the army
  leaves. A scout for a stale point no scout can take joins its producer's queue
  behind whatever is there, as worker recovery does, rather than wait for an
  idle producer while workers and the army keep every one busy. Two train ahead
  of the army: carriers for a seat whose ground reaches no enemy, since they are
  its army's only way there, and the Sappers attacks want against known
  defenses, which cost more than any army unit their producer trains and would
  otherwise never get one.
- The top investment is funded through its next purchasable step. The bot keeps
  one optional saving target with its price, reason and cancel conditions.
- Income is not observed directly. The bot estimates it from the change in bank
  since the previous decision plus its own spending in between.
- While saving, a share of income is protected for the target, capped by the
  target's price and the real bank. Other purchases spend only the unprotected
  part. Stance, personality and threat set the share.
- An affordable target is bought ahead of everything except emergencies. Targets
  switch only when another scores clearly higher or the reason disappears;
  protected scrap carries over. Emergencies may spend protected scrap. A
  rejected, cancelled or refunded purchase keeps the target and recomputes the
  protected amount from the bank.

## Missions and production

- Global production buys toward composition needs and the funded investment. No
  mission owns a future production contract.
- Counts come from need. What the bot owns, trains, builds or sends follows a
  need it can state from what it knows; a constant may bound computation or
  model a difficulty, stance or personality limit, never what the bot owns or
  sends.
- Missions recruit from available units in a fixed order (defend, local guard,
  settlement, lift, attack, clearance, strike, raid), and each takes only the
  roles it uses: attack takes line, siege, anti-air and anti-heavy units, with
  Sappers, breachers and point defenders in support; raid takes Scuttlers,
  Sappers, or ground-attack aircraft too few for a strike; lift takes carriers
  and a payload; a clearance takes splash bombers while ground reaches no enemy.
- An attack, and a lift, runs one at a time and takes the whole free army it can
  use beyond the home defense: one push that wins what it meets beats several
  that each meet the enemy alone. Other missions may run several of a kind at
  once on distinct targets, each sized to its own target's need.
- A mission's need decides when it starts and whether it goes on: the known
  force it would meet, by a margin. Attacks and lifts then take every suitable
  free unit, and other missions take only the force they need. A mission keeps
  its force while its purpose holds, and releases it when the purpose disappears
  or recovery finishes. At a safe transition it can yield suitable units to an
  emergency; loaded passengers are released only after landing.
- Missions have phases (gather, travel, engage, withdraw, recover; load, fly,
  land, fight) and hysteresis, so repeated scoring does not turn each decision
  into a new plan.
- Capability needs, such as "this objective requires lift" or "enemy air
  requires anti-air", raise production and investment priorities. They reserve
  nothing and promise nothing to a mission.

An island settlement lifts one builder to a site no own worker can walk to, with
an escort while known enemies stand near it, using ordinary lift commands. It
only delivers: once the builder lands, the site is an ordinary build the funded
investment pays for. A missing Airworks or carrier is that investment's next
purchase, and the escort's shortfall raises production needs; no future units
are promised. The escort is sized as the site's local guard will be, holds the
landing while known enemies stand near, and joins that guard once the building
is placed. Expansion guards follow nearby known threats, without a quiet
minimum, and completed guns release mobile defenders. A late delivery turns its
passengers home; a builder already on the island builds there on the next
attempt.

## Persistent state

Checkpoints preserve all non-derivable state that affects future decisions. No
other non-derivable state persists between decisions:

- memory of enemy units and buildings, of failed objectives, of raided targets,
  of when each scouting point was last in sight, and of own units whose orders
  stalled for want of a route or a firing position;
- missions and their phases, and since when a free army that could attack has
  waited, which lowers the margin an attack must bring;
- the saving target; the investment list is rebuilt each decision;
- the previous bank and spending used to estimate income, and the smoothed
  estimate;
- the position of any random stream;
- the pending own-event buffer.

Immutable scenario-derived data, such as the map model, rebuilds on load. Any
retained derived data needs an explicit rebuild or invalidation rule. A new
checkpoint field needs a design review.

## Own events

The host session keeps each seat's own `OrderStalled` and `CommandRejected`
events, and every other seat's machine standing up from its cradle, in a
bounded, ordered buffer and passes it with the observation. A cradle is public
once its crew starts, so every seat learns where its machine stands. The buffer
keeps one stall per unit and reason and one rejection per reason; when it is
full, the oldest event is dropped. The buffer is saved with checkpoints. A
background decision reads it without draining it; events are consumed when that
decision's result is installed. A rejection that cannot be tied unambiguously to
one command triggers a check of the observed outcome; it never marks a
particular purchase as failed.

## Map knowledge

The simulation paths units itself, so the bot needs map knowledge only for
reachability, distance, placement and danger. It builds a small static model
once per match (ground-connected components, distance fields from home and enemy
starts, candidate sites, and the narrow passages every ground way from each
start to a hostile start runs through) and shares it between clones. On a tiered
map the model also keeps each tile's level, so the bot's checks on whether a
shot lands follow the simulation's level rules and it never plans a building
other than a wall or mine on a ramp. Danger is coarse known weapon coverage.
Placement uses a fog-honest check over the observation. A stall or rejection
triggers a re-plan instead of the order being proven in advance.

## Computation

- **Allowed:** bounded scoring of a small candidate set, flood fills over fixed
  grids, single-command legality and connectivity checks, and small bounded
  route searches. All work has finite inputs and a bounded number of visits or
  expansions.
- **Not allowed:** speculative search over action sequences, multi-step plan
  verification, joint future producer schedules, reproducing the simulation's
  command processing to pre-verify orders, and planning state that spans
  decisions.

## Reactivity

Each behavior is proven by a staged scenario test.

| Situation                                                  | Required response                                                                                                                                     |
| ---------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| Enemy air seen                                             | Anti-air proportional to seen enemy air                                                                                                               |
| Enemy heavies seen                                         | Anti-heavy units proportional to seen enemy heavies                                                                                                   |
| Enemy Airworks seen                                        | Raise the anti-air need before enemy air arrives                                                                                                      |
| Ground threat at base or expansion                         | Defend                                                                                                                                                |
| Air raid on base or expansion                              | Defend with anti-air or fighters                                                                                                                      |
| Richer economy than the enemy                              | Larger attacks, ground and air, since each attack takes the whole free army                                                                           |
| Known defenses at target or on route                       | Bring siege, or pick another target                                                                                                                   |
| Ground-severed enemy                                       | Splash bombers clear the anti-air around the lift's target; then a lift of the whole free army, after a recent look, lands out of reach of known fire |
| Local opportunity                                          | Attack when the free army beats the known army it would meet, above a stance-bounded minimum                                                          |
| Losing a fight                                             | Withdraw, with a threshold shaped by personality and difficulty                                                                                       |
| Won a defense with army left                               | Counterattack if the attack check passes                                                                                                              |
| Enemy artillery shelling assets                            | Prioritize and reach the artillery; raise the need for mobile and artillery counters                                                                  |
| Assets under indirect fire                                 | Go for the guns, not the spotters overhead                                                                                                            |
| Enemy unit mix                                             | Choose units by coarse suitability against seen enemies; anti-air that flies against enemy air that only fires at the ground                          |
| Enemy shellfire known                                      | Point defenders in proportion to the heavy shots it fires, kept over the army                                                                         |
| Expensive investment wanted                                | Save for it while keeping an army share                                                                                                               |
| Several enemies                                            | Choose a target with hysteresis                                                                                                                       |
| Wounded units and a calm Repair Bay                        | Drive or fly the free wounded into the reach of a Bay with a free slot, and keep them there while they heal                                           |
| Known strike worth a warhead                               | Build a Spire and its rounds; launch loaded Spires when the warheads past the known interceptors take at least their rounds' price                    |
| Enemy Spire tech known                                     | A Lattice over the most valuable base for a Deep Bore or a seen Crucible; cover over every base when one enemy holds both                             |
| Enemy Spire known                                          | Lattice cover over each base in proportion to known enemy Spires, and radar where a Spire's line enters a guarded ring                                |
| Known guns the army cannot attack, or an attack given up   | Assemble a Maw where it can drive out of the base, crewed by the fastest free workers; it joins the next attack and cuts the nearest defense          |
| Known anti-air over a severed target, or a lift there lost | Assemble a Roc; it leads the next lift, the other carriers following once it is over the landing, and fights beside the riders it set down            |
| Free empty Roc while ground reaches the enemy              | Join the next attack as a gun platform                                                                                                                |
| Enemy cradle known                                         | Raise the need its machine raises, anti-heavy for a Maw and anti-air for a Roc, before it stands up, and keep it once it stands up out of sight       |
| Wounded Maw and safe known wreck                           | Drive it where its hull fits and its kiln reaches the wreck, keep it there out of missions while its kiln feeds, and keep harvesters off that wreck   |

It also covers scouting, where a point whose scout was lost on the way or found
no safe view waits to go stale again rather than drawing scouts one after
another into the same fire, Scuttler raids, harvest-line harassment by air, team
relief, escorts, repair, harvester evacuation, emergency and voluntary static
defense sized to hold off the known threat, Extractor restoration, expansion
timing, tech prerequisites, memory of failed objectives, a response to stalled
production, focus fire, and pulling wounded units back between fights. It does
no other per-unit micro, except keeping point defenders over the army they
travel with and sending a wounded Roc home from a lift's fight. It builds every
building kind, reaches every upgrade tier, and trains or assembles every unit
kind.

## Difficulty, stance and personality

- **Personality** resolves deterministically from the seat's personality seed,
  bounded by stance and a fixed trait budget. Its traits weight every score and
  never grant or remove a capability.
- **Stance** bounds posture: minimum home defense, minimum attack size, and how
  early and how large attacks get. Offensive missions leave the home defense
  behind while an enemy could reach home; defense takes every unit.
- **Difficulty** sets cognitive and execution limits: how often a seat decides,
  which is its reaction time; how many unit orders one decision may give, which
  is its attention; how much strength an attack must bring over the known
  defense; and how fully it builds its economy: lower rungs staff their scrap
  nodes more thinly and want each kind of growth less, though never so little
  that they stop growing. The lowest rung also misjudges the enemy's strength by
  a deterministic draw from its seed. Memory fades at the same rate on every
  rung. Difficulty also sets how well a seat carries out its repertoire: lower
  rungs attack piecemeal, pick targets worse, focus fire on the nearest enemy or
  by a rough read of its wounds, leave reinforcements at home, end fights on
  time while winning, withdraw at even odds, and lift without a fresh look or
  without counting bomber cover. Every rung keeps every capability. Lower rungs
  make understandable mistakes, and higher rungs beat lower ones.
