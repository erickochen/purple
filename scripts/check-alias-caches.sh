#!/usr/bin/env bash
# Inventory gate for host-alias-keyed candidate collections on App state.
#
# Background: App::reload_hosts is the single place where every cache
# keyed on an SSH host alias is pruned when the host list changes. A
# new HashMap<String, _>, HashSet<String> or Arc<Mutex<HashSet<String>>>
# field on an App-state struct is a CANDIDATE for that contract.
# It may turn out to be keyed on a container id, provider name, group
# label, or something else entirely and need no prune. The point of
# this gate is to force a conscious classification on every addition.
#
# Scope: src/app.rs and any src/app/**/*.rs (recursive). New App-state
# files anywhere under src/app/ are scanned automatically.
#
# Multi-line tolerance: vault::cert_cache declares its type across
# several lines because the value is a complex tuple. A naive line-grep
# would miss it. The preprocessor below joins continuation lines of any
# `pub <name>:` or `pub(in crate::app) <name>:` declaration so the regex
# sees a single line per field.
#
# How to resolve a failure:
#   1. Decide whether the new field is keyed on a host alias. Trace
#      its insertion sites and cite what gets put in.
#   2. If alias-keyed: prune it in src/app.rs::reload_hosts AND seed
#      the ghost-sweep contract test
#      (handler::tests::reload_hosts_ghost_sweep_clears_every_alias_keyed_collection).
#   3. Add the new path:field to EXPECTED_LIST below, with a one-line
#      comment classifying it as alias or non-alias.

set -e

# Every String-keyed HashMap/HashSet on an App-state struct lives here.
# Sorted alphabetically. Comments mark whether the field is alias-keyed
# (= must prune) or keys on something else.
EXPECTED_LIST="app/container_state.rs:cache
app/containers_overview.rs:auto_list_in_flight
app/containers_overview.rs:collapsed_hosts
app/containers_overview.rs:entries
app/containers_overview.rs:in_flight
app/containers_overview.rs:in_flight_aliases
app/file_browser_state.rs:host_paths
app/host_state.rs:group_alias_map
app/host_state.rs:group_host_counts
app/key_push_state.rs:pin_outcomes
app/key_push_state.rs:selected
app/ping.rs:last_checked
app/ping.rs:status
app/provider_state.rs:expanded_providers
app/provider_state.rs:sync_history
app/provider_state.rs:syncing
app/snippet_state.rs:selected
app/tunnel_state.rs:active
app/tunnel_state.rs:demo_live_snapshots
app/tunnel_state.rs:summaries_cache
app/vault.rs:cert_cache
app/vault.rs:cert_checks_in_flight
app/vault.rs:cert_stat_throttle
app/vault.rs:sign_in_flight"
#
# Classification notes (kept out of EXPECTED_LIST so the diff stays clean):
#   alias, pruned in reload_hosts:
#     container_state::cache, ping::status, ping::last_checked,
#     file_browser_state::host_paths, tunnel_state::summaries_cache,
#     tunnel_state::demo_live_snapshots, vault::cert_cache,
#     vault::cert_checks_in_flight, vault::cert_stat_throttle,
#     vault::sign_in_flight,
#     containers_overview::auto_list_in_flight,
#     containers_overview::collapsed_hosts,
#     containers_overview::in_flight_aliases (on refresh_batch).
#   alias, self-pruning:
#     tunnel_state::active (worker drops entry when tunnel process exits).
#   transient, scoped to one push run, cleared by start_run, finish_run
#   and cancel_run:
#     key_push_state::pin_outcomes.
#   transient, scoped to one picker session, reset on screen exit:
#     key_push_state::selected,
#     snippet_state::selected (SnippetHostPick: reset on picker open via
#     reset_host_pick, cleared on Esc; never read after the run commits).
#   non-alias keys:
#     containers_overview::entries (x2 on InspectCache and LogsCache),
#     containers_overview::in_flight (x2 same caches): container id.
#     host_state::group_host_counts: group label ("Tag: prod" etc).
#     host_state::group_alias_map: outer key is group label; aliases live
#       in the Vec<String> value and follow hosts_state.list automatically.
#     provider_state::syncing, sync_history, expanded_providers: provider name.

# Preprocessor: join multi-line `pub <name>:` field declarations onto
# one line so the candidate regex below sees the full type.
# State machine: when a `pub <name>:` line opens with unbalanced `<` or `(`,
# accumulate subsequent lines until the balance closes and the buffer ends
# with `,` or `;`. Emit `<file>:<start_line>:<joined>`.
PREPROCESS_AWK='
function delta(s,    i, c, d) {
    d = 0
    for (i = 1; i <= length(s); i++) {
        c = substr(s, i, 1)
        if (c == "<" || c == "(") d++
        else if (c == ">" || c == ")") d--
    }
    return d
}
function rtrim(s) { sub(/[[:space:]]+$/, "", s); return s }
function closes_field(s) { s = rtrim(s); return (s ~ /[,;]$/) }
{
    if (buf != "") {
        buf = buf " " $0
        depth += delta($0)
        if (depth <= 0 && closes_field($0)) {
            print FILENAME ":" start_nr ":" buf
            buf = ""
            depth = 0
        }
        next
    }
    if ($0 ~ /^[[:space:]]*pub(\([^)]*\))?[[:space:]]+[a-z_]+:/) {
        start_nr = FNR
        buf = $0
        depth = delta($0)
        if (depth <= 0 && closes_field($0)) {
            print FILENAME ":" start_nr ":" buf
            buf = ""
            depth = 0
        }
    }
}
'

FOUND_LIST=$(
    find src/app.rs src/app -type f -name '*.rs' \
        | xargs awk "$PREPROCESS_AWK" \
        | grep -E 'pub(\([^)]*\))?[[:space:]]+[a-z_]+:.*(HashMap<\s*String\s*,|HashSet<\s*String\s*>|Arc<Mutex<HashSet<\s*String\s*>)' \
        | awk -F: '{
            match($0, /pub(\([^)]*\))?[[:space:]]+[a-z_]+/)
            chunk = substr($0, RSTART, RLENGTH)
            sub(/^pub(\([^)]*\))?[[:space:]]+/, "", chunk)
            sub(/^src\//, "", $1)
            print $1 ":" chunk
          }' \
        | sort \
        | uniq
)

EXPECTED_SORTED=$(echo "$EXPECTED_LIST" | sort | uniq)

if [ "$EXPECTED_SORTED" != "$FOUND_LIST" ]; then
    echo "ERROR: alias-keyed candidate inventory drift in src/app/."
    echo ""
    echo "Expected:"
    echo "$EXPECTED_SORTED" | sed 's/^/  /'
    echo ""
    echo "Found:"
    echo "$FOUND_LIST" | sed 's/^/  /'
    echo ""
    echo "Diff (- expected, + found):"
    diff <(echo "$EXPECTED_SORTED") <(echo "$FOUND_LIST") | sed 's/^/  /' || true
    echo ""
    echo "Resolution:"
    echo "  1. Decide whether the new field is host-alias-keyed."
    echo "  2. If yes: prune it in src/app.rs::reload_hosts AND seed it in"
    echo "     handler::tests::reload_hosts_ghost_sweep_clears_every_alias_keyed_collection."
    echo "  3. Update EXPECTED_LIST in scripts/check-alias-caches.sh and"
    echo "     add the classification note."
    exit 1
fi

echo "Alias-keyed inventory: $(echo "$FOUND_LIST" | wc -l | tr -d ' ') field(s) classified. OK"
exit 0
