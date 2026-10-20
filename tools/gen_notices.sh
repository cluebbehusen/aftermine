#!/bin/sh
# Writes the license notices for every third-party crate the game links
# to assets/licenses/third-party.txt, which each bundle carries with the
# rest of assets/. The crate licenses require shipping these notices.
# Needs cargo-about 0.9.2. Run from the workspace root after Cargo.lock
# changes:
#
#   sh tools/gen_notices.sh
#
# With --check it writes nothing and fails if the file is stale.
set -eu

OUT=assets/licenses/third-party.txt

generate() {
    # Offline, so the notices come only from the crates' own files.
    cargo fetch --locked --quiet
    cargo about generate --frozen --fail \
        --manifest-path shell/Cargo.toml --config about.toml \
        --output-file "$1" tools/notices.hbs
}

if [ "${1:-}" = "--check" ]; then
    FRESH="$(mktemp)"
    trap 'rm -f "$FRESH"' EXIT
    generate "$FRESH"
    if ! cmp -s "$OUT" "$FRESH"; then
        echo "$OUT is stale; run sh tools/gen_notices.sh" >&2
        exit 1
    fi
else
    generate "$OUT"
fi
