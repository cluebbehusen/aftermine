#!/bin/sh
# Plays one lane with Codex, headless, and returns when its match is over.
#
# Usage: run-codex.sh <lane-dir> <tick-limit> [resumes]
#
# Codex runs in the lane directory, with a home of its own and network
# access for the session's local port. Its events go to events.jsonl and its
# final message, shaped by the report schema, to report.json. When Codex
# stops while the match is still running, it is resumed with a reminder, at
# most `resumes` times (default 3). It fails when Codex still stops early or
# the session no longer answers.
set -eu

dir=$(cd "$1" && pwd)
limit=$2
resumes=${3:-3}
config="-c sandbox_mode=workspace-write -c sandbox_workspace_write.network_access=true"

# Codex loads instructions, memories, skills and plugins from its home, and
# a player must start from none of them. Each lane gets a home of its own
# beside the round's lanes, holding a link to the login and the model
# settings, so every lane plays the model the user plays.
user_home=${CODEX_HOME:-$HOME/.codex}
CODEX_HOME="$(dirname "$dir")/codex-homes/$(basename "$dir")"
export CODEX_HOME
mkdir -p "$CODEX_HOME"
ln -sf "$user_home/auth.json" "$CODEX_HOME/auth.json"
if [ -f "$user_home/config.toml" ]; then
	sed -n '/^\[/q; /^model/p' "$user_home/config.toml" >"$CODEX_HOME/config.toml"
fi
cd "$dir"

# shellcheck disable=SC2086 # $config holds separate flags.
codex exec $config --skip-git-repo-check --json \
	--output-schema report.schema.json -o report.json \
	"Read brief.md in this directory and follow it." </dev/null >>events.jsonl

thread=$(sed -n 's/.*"thread_id":"\([^"]*\)".*/\1/p' events.jsonl | head -n 1)
while [ "$resumes" -gt 0 ]; do
	status=$(./play status 2>&1 || true)
	case "$status" in
	*'"result": null'*) ;;
	*) break ;;
	esac
	tick=$(printf '%s\n' "$status" | sed -n 's/.*"tick": \([0-9]*\).*/\1/p')
	[ "${tick:-0}" -lt "$limit" ] || break
	resumes=$((resumes - 1))
	# shellcheck disable=SC2086
	codex exec resume $config --skip-git-repo-check --json \
		--output-schema report.schema.json -o report.json "$thread" \
		"The match is not over: ./play status shows no result. Keep playing to a result or the tick limit, then write the report." \
		</dev/null >>events.jsonl
done
status=$(./play status 2>&1) || {
	printf '%s\n' "$status" >&2
	echo "The session did not answer ./play status after Codex stopped." >&2
	exit 1
}
printf '%s\n' "$status"
case "$status" in
*'"result": null'*)
	tick=$(printf '%s\n' "$status" | sed -n 's/.*"tick": \([0-9]*\).*/\1/p')
	if [ "${tick:-0}" -lt "$limit" ]; then
		echo "Codex stopped at tick ${tick:-0} with no result, before the tick limit." >&2
		exit 1
	fi
	;;
esac
