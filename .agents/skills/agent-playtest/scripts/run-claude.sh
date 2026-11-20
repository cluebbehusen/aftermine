#!/bin/sh
# Plays one lane with Claude Code, headless, and returns when its match is over.
#
# Usage: run-claude.sh <lane-dir> <tick-limit> [resumes] [model]
#
# Claude runs in the lane directory in safe mode, with no setting sources but
# the lane's own: no user or repository CLAUDE.md, memory, skills, plugins or
# hooks reach the player (setting sources alone still load the user's
# CLAUDE.md). It may only run ./play, read the lane's files and edit
# report.json; anything else is denied without asking. Its events go to
# events.jsonl. When it stops while the match is still running, it is resumed
# with a reminder, at most `resumes` times (default 3). It fails when Claude
# still stops early or the session no longer answers.
set -eu

dir=$(cd "$1" && pwd)
limit=$2
resumes=${3:-3}
model=${4:-}
cd "$dir"

play() {
	prompt=$1
	shift
	# shellcheck disable=SC2086 # $model_flag is empty or one flag pair.
	claude -p "$prompt" $model_flag --safe-mode --setting-sources project --permission-mode dontAsk \
		--allowedTools "Bash(./play:*)" "Read(./**)" "Edit(./report.json)" \
		--output-format stream-json --verbose "$@" </dev/null >>events.jsonl 2>>run.log
}
model_flag=
[ -z "$model" ] || model_flag="--model $model"

play "Read brief.md in this directory and follow it. Write report.json in this directory when you finish."
session=$(sed -n 's/.*"session_id":"\([^"]*\)".*/\1/p' events.jsonl | head -n 1)
while [ "$resumes" -gt 0 ]; do
	status=$(./play status 2>&1 || true)
	case "$status" in
	*'"result": null'*) ;;
	*) break ;;
	esac
	tick=$(printf '%s\n' "$status" | sed -n 's/.*"tick": \([0-9]*\).*/\1/p')
	[ "${tick:-0}" -lt "$limit" ] || break
	resumes=$((resumes - 1))
	play "The match is not over: ./play status shows no result. Keep playing to a result or the tick limit, then write report.json." --resume "$session"
done
status=$(./play status 2>&1) || {
	printf '%s\n' "$status" >&2
	echo "The session did not answer ./play status after Claude stopped." >&2
	exit 1
}
printf '%s\n' "$status"
case "$status" in
*'"result": null'*)
	tick=$(printf '%s\n' "$status" | sed -n 's/.*"tick": \([0-9]*\).*/\1/p')
	if [ "${tick:-0}" -lt "$limit" ]; then
		echo "Claude stopped at tick ${tick:-0} with no result, before the tick limit." >&2
		exit 1
	fi
	;;
esac
