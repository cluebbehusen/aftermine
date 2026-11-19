# /// script
# requires-python = ">=3.11"
# ///
"""Summarizes playtest lanes: result, pace and cost.

Usage: uv run lane-summary.py <round-dir> [lane...]

For each lane (every lane directory in the round when none is named) it
reads the lane's commands.log, its replay, its report and its events.jsonl
from Codex or Claude Code, and prints one row: the result and final tick, the player's
orders per game minute, how far it advanced at a time, how often it looked
between advances, the wall time it spent, and the tokens the player's events report.
`--json` prints the same rows as JSON lines.
"""

import json
import statistics
import sys
from pathlib import Path

TICKS_PER_MINUTE = 1200
LOOKS = {"digest"}
CLOCK = {"advance", "status"}


def lane_row(round_dir: Path, lane: str) -> dict:
    lane_dir = round_dir / lane
    calls = []
    log = lane_dir / "commands.log"
    if log.exists():
        for line in log.read_text().splitlines():
            stamp, _, call = line.partition("\t")
            calls.append((int(stamp), call.split()))
    steps = [
        int(words[1])
        for _, words in calls
        if len(words) > 1 and words[0] == "advance" and words[1].isdigit()
    ]
    looks = sum(1 for _, words in calls if words and words[0] in LOOKS)

    row = {"lane": lane, "advances": len(steps), "looks": looks}
    if steps:
        row["step_min"] = min(steps)
        row["step_median"] = statistics.median(steps)
        row["step_max"] = max(steps)
        row["looks_per_advance"] = round(looks / len(steps), 2)
    if calls:
        row["wall_seconds"] = calls[-1][0] - calls[0][0]

    replay_path = round_dir / "replays" / f"{lane}.json"
    if replay_path.exists():
        replay = json.loads(replay_path.read_text())
        ticks = replay.get("meta", {}).get("ticks") or 0
        orders = sum(1 for timed in replay["commands"] if timed["command"]["player"] == 0)
        row["final_tick"] = ticks
        row["orders"] = orders
        if ticks:
            row["orders_per_minute"] = round(orders * TICKS_PER_MINUTE / ticks, 1)

    report_path = lane_dir / "report.json"
    if report_path.exists():
        try:
            report = json.loads(report_path.read_text())
            row["result"] = report.get("result")
        except json.JSONDecodeError:
            row["result"] = "unreadable report"

    events = lane_dir / "events.jsonl"
    if events.exists():
        usage = {"input": 0, "cached": 0, "output": 0}
        for line in events.read_text().splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event.get("type") == "turn.completed":
                turn = event.get("usage", {})
                usage["input"] += turn.get("input_tokens", 0)
                usage["cached"] += turn.get("cached_input_tokens", 0)
                usage["output"] += turn.get("output_tokens", 0)
            elif event.get("type") == "result":
                # Claude Code's stream-json closes each run with its usage.
                turn = event.get("usage", {})
                cached = turn.get("cache_read_input_tokens", 0)
                usage["input"] += (
                    turn.get("input_tokens", 0)
                    + cached
                    + turn.get("cache_creation_input_tokens", 0)
                )
                usage["cached"] += cached
                usage["output"] += turn.get("output_tokens", 0)
        row["tokens"] = usage
    return row


def main() -> int:
    args = [arg for arg in sys.argv[1:] if arg != "--json"]
    as_json = "--json" in sys.argv[1:]
    if not args:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    round_dir = Path(args[0])
    lanes = args[1:] or sorted(
        path.name for path in round_dir.iterdir() if (path / "brief.md").exists()
    )
    for lane in lanes:
        row = lane_row(round_dir, lane)
        if as_json:
            print(json.dumps(row))
            continue
        tokens = row.get("tokens")
        spent = (
            f" | tokens {tokens['input']:,} in ({tokens['cached']:,} cached), {tokens['output']:,} out"
            if tokens
            else ""
        )
        print(
            f"{lane}: {row.get('result', 'no report')} at tick {row.get('final_tick', '?')}"
            f" | {row.get('orders', '?')} orders, {row.get('orders_per_minute', '?')} a minute"
            f" | steps {row.get('step_min', '?')}/{row.get('step_median', '?')}/{row.get('step_max', '?')}"
            f" ticks (min/median/max), {row.get('looks_per_advance', '?')} looks an advance"
            f" | {row.get('wall_seconds', '?')} s wall{spent}"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
