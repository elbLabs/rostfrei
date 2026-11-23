#!/usr/bin/env bash
# Run only at a fenced recovery point. See docs/completed-outcome-cutover.md.
set -euo pipefail
umask 077
fail() { echo "paired-store: $*" >&2; exit 1; }
[[ $# == 4 ]] || fail 'usage: paired-store.sh backup|restore DIRECTORY JOURNAL COMMAND_RESULTS'
mode=$1 dir=$2 journal=$3 results=$4
[[ $mode == backup || $mode == restore ]] || fail 'expected backup or restore'
for name in "$journal" "$results"; do
    [[ $name =~ ^[a-zA-Z0-9_][a-zA-Z0-9_-]*$ ]] || fail "invalid stream name: $name"
done
[[ $journal != "$results" ]] || fail 'expected two distinct stream names'
: "${NATS_URL:?set NATS_URL to the fenced broker endpoint}"
[[ $(nats --version) == 0.5.2 ]] || fail 'requires nats CLI 0.5.2'
js() { nats --no-context "$@"; }

# Capture policy (including both bindings), creation time and the retained range.
# Cluster leader, info-request time and consumer state are not stream identity.
evidence() {
    js stream info "$1" --json | jq -Se --arg name "$1" '
        select(.config.name == $name and .config.metadata["_nats.ver"] == "2.12.15"
               and .created and (.state.messages != null)) |
        {config, created, state: (.state | {messages, bytes, first_seq, last_seq})}'
}
checksums() {
    shasum -a 256 journal.json results.json {journal,results}/{backup.json,stream.tar.s2}
}

if [[ $mode == backup ]]; then
    mkdir -- "$dir"  # never reuse or overwrite a backup set
    cd -- "$dir"
    evidence "$journal" > journal.json
    evidence "$results" > results.json
    # Rostfrei records UTC creation times using Rust time's Display, while stream
    # info uses RFC3339Nano. Compare as strings to retain nanosecond precision.
    jq -es '
        def incarnation: .created | sub("T0?"; " ") |
            if contains(".") then . else sub("Z$"; ".0Z") end | sub("Z$"; " +00:00:00");
        .[0] as $j | .[1] as $r | $j.config.allow_atomic and
        ($j.config.metadata["rostfrei.split.state-incarnation"] == ($r | incarnation)) and
        ($r.config.metadata["rostfrei.split.journal-incarnation"] == ($j | incarnation))' \
        journal.json results.json >/dev/null || fail 'source streams are not a cross-bound split pair'
    js backup stream --check --no-progress --no-consumers "$journal" journal
    js backup stream --check --no-progress --no-consumers "$results" results
    evidence "$journal" | cmp journal.json -
    evidence "$results" | cmp results.json -
    checksums > SHA256SUMS.partial
    mv SHA256SUMS.partial SHA256SUMS
else
    cd -- "$dir"
    checksums | cmp SHA256SUMS - || fail 'incomplete or damaged backup set'
    for entry in journal results; do
        # The native restore addresses the name inside backup.json, not its directory.
        jq -es --arg name "${!entry}" 'all(.[]; .config.name == $name)' \
            "$entry.json" "$entry/backup.json" >/dev/null || fail "unexpected $entry stream"
    done
    js stream ls --all --names --json | jq -e --arg j "$journal" --arg r "$results" \
        '(. // []) | index($j) == null and index($r) == null' >/dev/null || fail 'cannot verify both destination streams are absent'
    js backup restore stream --no-progress journal
    js backup restore stream --no-progress results
    evidence "$journal" | cmp journal.json - || fail 'restored journal differs; keep destination fenced'
    evidence "$results" | cmp results.json - || fail 'restored results differ; keep destination fenced'
fi
