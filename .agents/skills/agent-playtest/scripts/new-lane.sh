#!/bin/sh
# Sets up one playtest lane and starts its session in the background.
#
# Usage: new-lane.sh <round-dir> <lane> <port> <map.json> <plan text> [session flags...]
#
# The lane directory holds everything its player may read: the map, the rules
# sheet, its brief, the report schema and a `play` wrapper that logs every
# call. The session's replay and log go beside the lanes, in replays/ and
# sessions/. Extra flags go to `aftermine-driver session`, for example
# --difficulty prime --personality-seed 9000 --tick-limit 12000; the tick
# limit is required, since the brief tells the player where the match stops.
# --min-step and --orders-per-minute hold the player to a pace, which the
# brief states. AFTERMINE_DRIVER names a driver binary to use instead of the
# repository's release build, so a rebuild never changes a running round.
# The lane's terms are recorded in sessions/<lane>.json, outside the lane,
# since they name the bot's difficulty and seed.
set -eu

if [ "$#" -lt 5 ]; then
	sed -n '4p' "$0" >&2
	exit 2
fi
round=$1
lane=$2
port=$3
map=$4
plan=$5
shift 5

limit=
step=
rate=
previous=
for flag in "$@"; do
	case $previous in
	--tick-limit) limit=$flag ;;
	--min-step) step=$flag ;;
	--orders-per-minute) rate=$flag ;;
	esac
	case $flag in
	--tick-limit=*) limit=${flag#--tick-limit=} ;;
	--min-step=*) step=${flag#--min-step=} ;;
	--orders-per-minute=*) rate=${flag#--orders-per-minute=} ;;
	esac
	previous=$flag
done
[ -n "$limit" ] || {
	echo "give the session a --tick-limit; the brief tells the player where the match stops" >&2
	exit 2
}
seconds=$((limit / 20))
stop=$(printf 'tick %s (%d:%02d)' "$limit" $((seconds / 60)) $((seconds % 60)))
pace=
if [ -n "$step" ] || [ -n "$rate" ]; then
	pace=' This playtest holds you to a pace:'
	[ -z "$step" ] || pace="$pace each advance runs at least $step ticks ($((step / 20)) s), short of the end;"
	[ -z "$rate" ] || pace="$pace you may send at most $rate orders in any minute of game time, and up to five of one unit queued at one structure at once (train --count) is one order;"
	pace="${pace%;}. The session refuses anything faster."
fi

skill=$(cd "$(dirname "$0")/.." && pwd)
repo=$(git -C "$skill" rev-parse --show-toplevel)
driver=${AFTERMINE_DRIVER:-$repo/target/release/aftermine-driver}
[ -x "$driver" ] || {
	echo "build the release driver first: cargo build --release -p aftermine-driver" >&2
	exit 1
}
mkdir -p "$round"
round=$(cd "$round" && pwd)
case "$round/" in
"$repo"/*)
	echo "keep the round outside the repository, so players do not load its instructions" >&2
	exit 1
	;;
esac
dir="$round/$lane"
[ ! -e "$dir" ] || {
	echo "$dir already exists" >&2
	exit 1
}
mkdir -p "$dir" "$round/replays" "$round/sessions"
# A driver named by AFTERMINE_DRIVER is copied into the round, so a player's
# wrapper names no directory outside the round.
if [ -n "${AFTERMINE_DRIVER:-}" ]; then
	mkdir -p "$round/driver"
	[ -x "$round/driver/aftermine-driver" ] || cp "$driver" "$round/driver/aftermine-driver"
	driver="$round/driver/aftermine-driver"
fi

cp "$map" "$dir/map.json"
"$driver" rules >"$dir/rules.txt"
cp "$skill/assets/report.schema.json" "$dir/report.schema.json"
cp "$skill/assets/play-help.txt" "$dir/play-help.txt"
{
	sed -e "s/@TICK_LIMIT@/$stop/" -e "s/@PACE@/$pace/" "$skill/references/player-brief.md"
	echo
	printf '%s\n' "$plan"
} >"$dir/brief.md"

cat >"$dir/play" <<EOF
#!/bin/sh
printf '%s\t%s\n' "\$(date +%s)" "\$*" >>"$dir/commands.log"
case "\${1:-help}" in
help | -h | --help) exec cat "$dir/play-help.txt" ;;
esac
if [ "\${1:-}" = digest ]; then
	case "\${2:-}" in -h | --help)
		echo "./play digest [--map]: what your seat knows now. --map adds a map with its legend."
		exit 0
		;;
	esac
	if [ "\${2:-}" = --map ]; then
		exec "$driver" live --addr 127.0.0.1:$port digest 0 --map "$dir/map.json"
	fi
	exec "$driver" live --addr 127.0.0.1:$port digest 0
fi
exec "$driver" live --addr 127.0.0.1:$port "\$@"
EOF
chmod +x "$dir/play"

"$driver" session --port "$port" --scenario "$dir/map.json" --player 0 \
	--replay-out "$round/replays/$lane.json" "$@" \
	>"$round/sessions/$lane.log" 2>&1 &
echo $! >"$round/sessions/$lane.pid"
{
	printf '{"lane": "%s", "port": %s, "map": "%s", "driver": "%s", "started": "%s", "flags": [' \
		"$lane" "$port" "$map" "$driver" "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
	sep=
	for flag in "$@"; do
		printf '%s"%s"' "$sep" "$flag"
		sep=', '
	done
	printf ']}\n'
} >"$round/sessions/$lane.json"
tries=20
until "$dir/play" status >/dev/null 2>&1; do
	tries=$((tries - 1))
	[ "$tries" -gt 0 ] || {
		echo "the session did not answer; see $round/sessions/$lane.log" >&2
		exit 1
	}
	sleep 0.5
done
: >"$dir/commands.log"
echo "$dir"
