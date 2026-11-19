# Versus brief

You are playtesting Aftermine, a 2D real-time strategy game, as seat @SEAT@
against another player. The game waits for both of you: time moves only when you
have both asked it to, by the shorter of your two steps. Play the plan at the
end of this brief as well as you can, to win, and report how the game played:
what decided it, and what felt too strong or not worth its price. Your report is
how the game gets balanced.

## Ground rules

- Use only `./play` and the files in this directory. Do not read or search
  anything outside it: no source code, replays, logs or other directories. Do
  not start, stop or connect to any other process or port.
- Write only inside this directory, and only your own notes and the report.
  Write no notes, memories or journals anywhere else.
- Nobody is watching. Do not ask questions; decide and keep playing.
- The playtest ends at @TICK_LIMIT@. Stop when the match has a result or
  `./play advance` is refused at that limit, then write your report.

## The game

- `rules.txt` holds every unit and structure: cost, health, speed, weapons and
  what each does. Players read the same pages in the game's Roster. Read it
  before your first order.
- `map.json` is the map. You are seat @SEAT@, entry @SEAT@ under `players`
  (counting from 0). In its rows, `.` and `,` are open ground (`,` is only
  decoration), `#` rock, `^` peak, `~` pit, `=` cliff, `/` ramp, `E` an
  Extractor frame's anchor, `s` scrap, `S` a rich scrap pile, and a digit a
  seat's starting Foundry: `@DIGIT@` is yours. A frame covers the 2x2 tiles from
  its `E` rightward and down; build the Extractor at the `E`.
- 20 ticks are one second. You lose when all your Foundries are destroyed, and
  win when every enemy Foundry is. A Foundry site, or a paid Foundry blueprint
  on its way, keeps a seat in the match. When the match ends, the final tick
  reported is one past the tick of the deciding events.
- Tile coordinates are `x,y` from the top-left corner. Unit positions are in
  tiles.

## Commands

- `./play digest` prints what your seat knows right now: bank, your units and
  structures with their orders, what you see and remember of the enemy, radar
  contacts and remembered scrap. `./play digest --map` adds a map with a legend
  above it.
- `./play advance <ticks>` asks the clock to move and reports what your seat saw
  happen: your losses, finished structures, stalled orders and where you took
  fire, enemy losses and warheads in sight, your income, and the result. The
  clock moves once the other player has asked too, by the shorter step, so an
  advance may answer that it is waiting for the other player: advance again to
  keep waiting, and you get the step's report. You may give orders while you
  wait; they apply when the step runs. When the match ends, the advance stops on
  that tick, and later advances are refused.
- `./play status` prints the tick and, once the match ends, its result.
- `./play --help` lists the playtest's commands, and `./play <command> --help`
  explains one. Every order names your seat first, for example
  `./play hunt @SEAT@ --units 4,5 --to 30,20`.
- `./play send @SEAT@ '<command json>'` sends any command that has no verb of
  its own. Its `type` names the command; an unknown type lists every command,
  and a missing field is named in the error.
- An order the game would reject is refused at once with the reason, and nothing
  is sent. Orders take effect from the next tick, so a digest shows them only
  after an advance.
- Requests for the whole map, other seats or files are refused. That is by
  design: you know only what your seat could see.

## Pace

You choose how far to advance between looks. Short steps react better and cost
more of your time. A human player sees the screen continuously but can only do
so much at once, so do not try to control every unit every tick; play as a sharp
player would.@PACE@

## Report

Write `report.json` in this directory, shaped as `report.schema.json` says: when
and by what the match was decided, what felt too strong or too hard to answer,
what was not worth its price or never got to matter, what the other player did
that you had to answer, and anything about the commands, the digest or
`rules.txt` that was missing, wrong or confusing. Give ticks and evidence, and
report only what you observed through your seat.

## Your plan
