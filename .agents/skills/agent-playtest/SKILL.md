---
name: agent-playtest
description:
  Run rounds of agent playtests against Aftermine's bot. Claude and Codex
  players each take one seat in a fog-only session with an assigned plan, report
  where the bot played badly, and each weakness is diagnosed from the replay and
  the bot's decision trace. Use when asked to playtest the bot with agents, to
  find strategies that beat it, or to check whether a bot change closed a
  weakness an agent found.
---

# Agent playtests

An agent playtest finds weaknesses a person who always plays one way never
meets. Each lane is one agent playing seat 0 with one plan against the bot, in a
session that answers only what that seat could know. Agents find weaknesses;
staged tests keep fixed ones fixed. One agent game is one sample from a
nondeterministic player: report weaknesses with their tick evidence, never win
rates.

## Set up a round

1. Build the release driver from a clean commit, so `replay-trace` can confirm
   the bot that played is the bot it traces:

   ```sh
   cargo build --release --locked -p aftermine-driver
   ```

2. Pick a round directory outside the repository, such as
   `~/aftermine-playtests/<date>-<name>`. Players run there and must not load
   the repository's agent instructions.
3. For each lane, pick a plan from [the plans](references/plans.md), a free
   port, and the bot's terms, including a tick limit, then start it:

   ```sh
   .agents/skills/agent-playtest/scripts/new-lane.sh "$ROUND" claude-rush 4141 \
     scenarios/the-deep-cut.json "$PLAN_TEXT" \
     --difficulty prime --stance balanced --personality-seed 9000 \
     --tick-limit 12000
   ```

   Add `--min-step <ticks>` and `--orders-per-minute <n>` to hold the player to
   a pace, which the brief then states. Set `AFTERMINE_DRIVER` to a copied
   driver binary when the repository may be rebuilt while the round runs.

   The lane gets the shipped map unmodified, `rules.txt` from
   `aftermine-driver rules`, its brief (the
   [standing brief](references/player-brief.md) with the tick limit filled in,
   plus the plan), the [report schema](assets/report.schema.json), and a `play`
   wrapper that logs every call to `commands.log` and answers `--help` with
   [the playtest's commands](assets/play-help.txt). The session refuses
   omniscient reads, every other seat and every file request, refuses an order
   the game would reject with the reason, answers each advance with what the
   seat saw happen, stops at the result or the tick limit, and rewrites
   `$ROUND/replays/<lane>.json` after each advance. The lane's terms go to
   `$ROUND/sessions/<lane>.json`, outside the lane, since they name the bot's
   difficulty and seed. Give every lane of a comparison the same personality
   seed, so differences come from the players.

## Launch the players

Launch every lane at once; lanes share nothing but the machine.

- **Claude:** one background Agent per lane, general-purpose, with only this
  prompt: "Change to `<lane dir>`, read `brief.md` there and follow it. Write
  `report.json` in that directory when you finish." Add nothing about the bot or
  earlier rounds.
- **Codex:** run `scripts/run-codex.sh <lane dir> <tick limit>` in the
  background. It runs `codex exec` headless in the lane, forces the report into
  the schema, and resumes Codex when it stops before the match ends. Each lane
  gets a Codex home of its own under `$ROUND/codex-homes/`, linking your login
  and copying only your model settings, so no instructions, memories, skills or
  plugins of yours reach the player.

Do not poll the lanes. Each notifies when it finishes. Then
`uv run scripts/lane-summary.py "$ROUND"` prints each lane's result, orders per
game minute, step sizes, looks per advance, wall time and Codex tokens; take a
Claude lane's tokens from its completion notice.

## Audit each lane

Before trusting a report, check that the player stayed in its seat:

- `commands.log` holds every game call. Look for long runs of advances without a
  digest, which mean the player stopped reading the game.
- The Claude transcript or the Codex `events.jsonl` must show no reads outside
  the lane, no direct `aftermine-driver` calls and no other ports.
- A lane that broke a ground rule is reported as such and its findings
  discarded.

## Diagnose each weakness

The report says what the player saw, not why the bot did it. For each weakness,
read the replay with full knowledge and the bot's decisions over its ticks:

```sh
aftermine-driver replay-inspect "$ROUND/replays/<lane>.json" \
  --tick <from>,<to> --fog-seat 1 --map
aftermine-driver replay-trace "$ROUND/replays/<lane>.json" \
  --seat 1 --from <from> --to <to> > trace.jsonl
```

Each trace row is one decision: bank, purchases and saving target, missions with
their phase and size, the army roles it wanted, and `launches`, which give each
attack's need against the force it expected and what it sent. Name the mechanism
in `aftermine-opponent` that produced the behavior, following the
aftermine-opponent skill, before proposing a change. Merge weaknesses that share
a mechanism across lanes.

## Report the round

Write one report grouped by theme, not by lane. For each weakness: the lanes and
ticks that show it, the cause from the trace, and a proposed general fix that
would also hold against a person. Then list what the bot did well, how the
models and plans differed, harness problems the players raised, and each lane's
result, wall time and tokens. Results describe this round's matches only.

A fix that lands gets a staged scenario test that reproduces the weakness,
through the aftermine-opponent and sandbox-matches skills, so later rounds spend
agent time on new weaknesses instead of old ones.

## Clean up

Kill each session with the pid in `$ROUND/sessions/<lane>.pid`. Keep the round
directory until its report is written; nothing from it is committed.
