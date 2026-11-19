#!/bin/sh
# Sets up a match between two players, one lane per seat, and starts its one
# session in the background.
#
# Usage: new-match.sh <round-dir> <match> <port> <map.json> <plan A> <plan B> [session flags...]
#
# Seat 0 plays from lane <match>-a on <port>, seat 1 from lane <match>-b on
# <port>+1, each with the versus brief and report schema; neither lane says
# who or what the other player is. The clock moves once both have asked, by
# the shorter step. Session flags are new-lane.sh's (a --tick-limit is
# required; --min-step and --orders-per-minute hold both players to one pace).
# The replay goes to replays/<match>.json and the session's terms to
# sessions/<match>.json, beside the lanes.
set -eu

if [ "$#" -lt 6 ]; then
	sed -n '4p' "$0" >&2
	exit 2
fi
round=$1
match=$2
port=$3
map=$4
plan_a=$5
plan_b=$6
shift 6

scripts=$(cd "$(dirname "$0")" && pwd)
skill=$(cd "$scripts/.." && pwd)
export LANE_SESSION=0
export LANE_BRIEF="$skill/references/versus-brief.md"
export LANE_SCHEMA="$skill/assets/versus-report.schema.json"
a=$(LANE_SEAT=0 "$scripts/new-lane.sh" "$round" "$match-a" "$port" "$map" "$plan_a" "$@")
b=$(LANE_SEAT=1 "$scripts/new-lane.sh" "$round" "$match-b" "$((port + 1))" "$map" "$plan_b" "$@")
round=$(cd "$round" && pwd)
# Both lanes call the same driver; take it from a lane's wrapper.
driver=$(sed -n 's/^exec "\([^"]*\)" live .*/\1/p' "$a/play" | tail -n 1)

"$driver" session --port "$port" --scenario "$a/map.json" --player 0,1 \
	--replay-out "$round/replays/$match.json" "$@" \
	>"$round/sessions/$match.log" 2>&1 &
echo $! >"$round/sessions/$match.pid"
{
	printf '{"match": "%s", "lanes": ["%s", "%s"], "port": %s, "map": "%s", "driver": "%s", "started": "%s", "flags": [' \
		"$match" "$match-a" "$match-b" "$port" "$map" "$driver" "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
	sep=
	for flag in "$@"; do
		printf '%s"%s"' "$sep" "$flag"
		sep=', '
	done
	printf ']}\n'
} >"$round/sessions/$match.json"
for lane in "$a" "$b"; do
	tries=20
	until "$lane/play" status >/dev/null 2>&1; do
		tries=$((tries - 1))
		[ "$tries" -gt 0 ] || {
			echo "the session did not answer; see $round/sessions/$match.log" >&2
			exit 1
		}
		sleep 0.5
	done
	: >"$lane/commands.log"
done
echo "$a"
echo "$b"
