#!/usr/bin/env bash
set -euo pipefail

SESSION="sqlitegis-fuzz"
ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
TARGETS=(
    fuzz_union_disjoint
    fuzz_sym_difference_disjoint
    fuzz_ewkb_decode
    fuzz_mbr_differential
    fuzz_text_parsers
    fuzz_geodesic
)

# The GEOS differential target links against system libgeos (libgeos-dev). Add
# it only when that library is present so the script still runs on machines
# without it, rather than failing to compile one pane.
if pkg-config --exists geos 2>/dev/null || command -v geos-config >/dev/null 2>&1; then
    TARGETS+=(fuzz_geos_differential)
fi

# libFuzzer runtime knobs (passed after `--` to cargo-fuzz):
#   -timeout=15        abort a single input after 15s. Generous headroom
#                      against pathological geometries that might push the
#                      reference BooleanOps path into long sweeps.
#   -max_len=65536     cap generated input size at 64 KiB. The Pair input
#                      is tiny (Arbitrary derives over a handful of ints
#                      and bools), so this only bounds the random byte
#                      stream libFuzzer feeds the Arbitrary impl.
#   -rss_limit_mb=8192 raise libFuzzer's RSS ceiling from the 2 GiB default.
#                      Disjoint-MBR pairs take the bytes-only fastpath
#                      which barely allocates, but overlapping pairs go
#                      through full geozero decode + i_overlay sweep
#                      under ASAN. The allocator fragments over time and
#                      8 GiB gives ample headroom on this machine.
LIBFUZZER_ARGS=(-timeout=15 -max_len=65536 -rss_limit_mb=8192)

if tmux has-session -t "$SESSION" 2>/dev/null; then
    echo "Session '$SESSION' already exists. Attach with: tmux attach -t $SESSION"
    exit 1
fi

run_target() {
    local target="$1"
    # cargo-fuzz is invoked from the repo root. It discovers the fuzz/
    # subcrate automatically. `cd` first so the command works regardless
    # of where the user launched the script.
    printf 'cd %q && cargo +nightly fuzz run %q -- %s; read -r -p '\''Press enter to close...'\''' \
        "$ROOT_DIR" "$target" "${LIBFUZZER_ARGS[*]}"
}

# Create session with the first target.
tmux new-session -d -s "$SESSION" -n fuzz "$(run_target "${TARGETS[0]}")"
# Show pane titles in the border (off by default in most tmux configs).
tmux set-option -t "$SESSION" pane-border-status top
# Label the first pane after its target.
tmux select-pane -t "${SESSION}:0.0" -T "${TARGETS[0]}"

# Split into more panes for the remaining targets, labelling each one.
pane_index=0
for target in "${TARGETS[@]:1}"; do
    sleep 1
    tmux split-window -t "$SESSION" "$(run_target "$target")"
    pane_index=$((pane_index + 1))
    tmux select-pane -t "${SESSION}:0.${pane_index}" -T "$target"
    tmux select-layout -t "$SESSION" tiled
done

tmux attach -t "$SESSION"
