#!/usr/bin/env bash
# Installer smoke test: runs site/install.sh against a stub curl and checks
# which release it asks for. The stub fails every download, so nothing is
# ever installed.
#
# Requires: node (for scripts/site/build.mjs).
# Run: ./tests/install_smoke.sh
#
# Exit code 0 = all checks passed, 1 = a check failed.

# shellcheck disable=SC2329 # helpers run through expect and the EXIT trap
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v node >/dev/null 2>&1; then
    printf "node not found. Install it to run the installer smoke test.\n"
    exit 1
fi

# Fixture versions. Neither has to exist on GitHub, since curl is stubbed.
RELEASE_VERSION="9.8.7"
PINNED_VERSION="1.2.3"
SOURCE="site/install.sh"
GENERATED="site/worker.generated.ts"
DOWNLOAD_PREFIX="https://github.com/erickochen/purple/releases/download/"

work="$(mktemp -d)"
mkdir -p "$work/bin" "$work/home"
# Put back any preview build that was there before the test.
[ -f "$GENERATED" ] && cp -p "$GENERATED" "$work/generated.bak"
cleanup() {
    if [ -f "$work/generated.bak" ]; then
        cp -p "$work/generated.bak" "$GENERATED"
    else
        rm -f "$GENERATED"
    fi
    rm -rf "$work"
}
trap cleanup EXIT

# Stub curl: record every URL, then fail the way curl -f fails on a 404.
cat > "$work/bin/curl" <<'STUB'
#!/bin/sh
for arg in "$@"; do
    case "$arg" in
        http://*|https://*) printf '%s\n' "$arg" >> "$CURL_LOG" ;;
    esac
done
exit 22
STUB
chmod +x "$work/bin/curl"

FAIL=0
STATUS=0
OUTPUT=""
URLS=""

# expect <label> <command...>: runs the command and reports ok or FAIL.
expect() {
    local label="$1"
    shift
    if "$@"; then
        printf "  ok    %s\n" "$label"
    else
        printf "  FAIL  %s\n" "$label"
        FAIL=1
    fi
}

# run_installer <script> [PURPLE_VERSION]. Sets STATUS, OUTPUT and URLS.
run_installer() {
    local vars=(PATH="$work/bin:$PATH" HOME="$work/home" CURL_LOG="$work/curl.log")
    if [ $# -ge 2 ]; then
        vars+=(PURPLE_VERSION="$2")
    fi
    : > "$work/curl.log"
    set +e
    OUTPUT="$(env -u PURPLE_VERSION "${vars[@]}" sh "$1" 2>&1)"
    STATUS=$?
    set -e
    URLS="$(cat "$work/curl.log")"
    cat "$work/curl.log" >> "$work/all-urls.log"
}

asked_for() {
    printf '%s\n' "$URLS" | head -1 | grep -qF "${DOWNLOAD_PREFIX}v$1/purple-$1-"
}

failed_saying() {
    [ "$STATUS" -ne 0 ] && [[ "$OUTPUT" == *"$1"* ]]
}

failed_before_download_saying() {
    [ -z "$URLS" ] && failed_saying "$1"
}

says() {
    [[ "$OUTPUT" == *"$1"* ]]
}

build_rejects() {
    ! PURPLE_RELEASE_VERSION="$1" node scripts/site/build.mjs >/dev/null 2>&1
}

never_called_api() {
    ! grep -q "api.github.com" "$work/all-urls.log"
}

only_release_downloads() {
    ! grep -v "^$DOWNLOAD_PREFIX" "$work/all-urls.log" | grep -q .
}

printf "build.mjs\n"
for bad in "" "v$RELEASE_VERSION" "9.8"; do
    expect "rejects PURPLE_RELEASE_VERSION=\"$bad\"" build_rejects "$bad"
done

cargo_version="$(grep -m1 '^version = ' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')"
OUTPUT="$(env -u PURPLE_RELEASE_VERSION node scripts/site/build.mjs)"
expect "falls back to Cargo.toml ($cargo_version)" says "version $cargo_version from Cargo.toml"

PURPLE_RELEASE_VERSION="$RELEASE_VERSION" node scripts/site/build.mjs >/dev/null
# The embedded script is an escaped template literal. Find its closing
# backtick and evaluate it to get the exact text the site serves.
# shellcheck disable=SC2016 # JavaScript source, not shell
node --input-type=module -e '
import { readFileSync } from "node:fs";
const marker = "const INSTALL_SCRIPT = `";
const ts = readFileSync(process.argv[1], "utf8");
const start = ts.indexOf(marker) + marker.length;
let end = start;
while (ts[end] !== "`") end += ts[end] === "\\" ? 2 : 1;
process.stdout.write(eval("`" + ts.slice(start, end) + "`"));
' "$GENERATED" > "$work/install-built.sh"
sed "s/__VERSION__/$RELEASE_VERSION/g" "$SOURCE" > "$work/install-expected.sh"
expect "embeds the installer with only the version filled in" \
    cmp -s "$work/install-expected.sh" "$work/install-built.sh"

printf "built installer\n"
run_installer "$work/install-built.sh"
expect "downloads the built-in release v$RELEASE_VERSION" asked_for "$RELEASE_VERSION"
expect "reports the failed download" failed_saying "Failed to download v$RELEASE_VERSION"

run_installer "$work/install-built.sh" "$PINNED_VERSION"
expect "PURPLE_VERSION=$PINNED_VERSION downloads v$PINNED_VERSION" asked_for "$PINNED_VERSION"
expect "says it uses the requested version" says "Using requested version v$PINNED_VERSION"

run_installer "$work/install-built.sh" "v$PINNED_VERSION"
expect "PURPLE_VERSION=v$PINNED_VERSION downloads v$PINNED_VERSION" asked_for "$PINNED_VERSION"

run_installer "$work/install-built.sh" "latest"
expect "rejects PURPLE_VERSION=latest before any download" \
    failed_before_download_saying "Invalid PURPLE_VERSION"

printf "source installer (not built)\n"
run_installer "$SOURCE"
expect "stops without a version and downloads nothing" \
    failed_before_download_saying "has no release version"

run_installer "$SOURCE" "$PINNED_VERSION"
expect "PURPLE_VERSION=$PINNED_VERSION still works" asked_for "$PINNED_VERSION"

printf "network\n"
expect "never calls api.github.com" never_called_api
expect "only requests release downloads" only_release_downloads

exit "$FAIL"
