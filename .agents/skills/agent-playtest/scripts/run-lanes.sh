#!/bin/sh
# Runs a round's bot lanes, a few at a time, and returns when all are over.
#
# Usage: run-lanes.sh <round-dir> <lanes-file> [at-once]
#
# Each line of the lanes file is one lane:
#
#   <lane> <port> <map.json> <plan-file> <player> [session flags...]
#
# where the plan file holds the plan's text and the player is claude, codex,
# or codex:<effort> for a Codex lane's reasoning effort. The session flags go
# to new-lane.sh and must include --tick-limit. Blank lines and lines starting
# with # are skipped; no field may contain spaces. Each lane is set up with
# new-lane.sh, played with run-claude.sh or run-codex.sh, and its session
# stopped once the player is done. At most `at-once` lanes (default 4) play at
# a time. A lane's setup and player output go to sessions/<lane>.run.log.
set -eu

if [ "${1:-}" = --lane ]; then
	round=$2
	lane=$3
	port=$4
	map=$5
	plan=$6
	player=$7
	shift 7
	scripts=$(cd "$(dirname "$0")" && pwd)
	limit=
	previous=
	for flag in "$@"; do
		[ "$previous" != --tick-limit ] || limit=$flag
		case $flag in
		--tick-limit=*) limit=${flag#--tick-limit=} ;;
		esac
		previous=$flag
	done
	log=$round/sessions/$lane.run.log
	if ! "$scripts/new-lane.sh" "$round" "$lane" "$port" "$map" "$(cat "$plan")" "$@" >"$log" 2>&1; then
		echo "$lane: setup failed, see $log"
		exit 0
	fi
	set +e
	case $player in
	claude) "$scripts/run-claude.sh" "$round/$lane" "$limit" ;;
	codex) "$scripts/run-codex.sh" "$round/$lane" "$limit" ;;
	codex:*) "$scripts/run-codex.sh" "$round/$lane" "$limit" 3 "${player#codex:}" ;;
	*)
		echo "unknown player $player: use claude, codex or codex:<effort>"
		false
		;;
	esac >>"$log" 2>&1
	status=$?
	set -e
	kill "$(cat "$round/sessions/$lane.pid")" 2>/dev/null || true
	if [ "$status" = 0 ]; then
		echo "$lane: done"
	else
		echo "$lane: the player failed, see $log"
	fi
	exit 0
fi

if [ "$#" -lt 2 ]; then
	sed -n '4p' "$0" >&2
	exit 2
fi
mkdir -p "$1/sessions"
round=$(cd "$1" && pwd)
lanes=$2
at_once=${3:-4}
grep -v -e '^[[:space:]]*#' -e '^[[:space:]]*$' "$lanes" |
	xargs -P "$at_once" -L 1 "$0" --lane "$round"
