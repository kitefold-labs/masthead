#!/usr/bin/env bash
# Copyright 2026 Kitefold Labs
# SPDX-License-Identifier: Apache-2.0
# Resource measurement for masthead-cosmic.
#
# Runs masthead-daemon on a private session bus and masthead-cosmic against the
# Wayland compositor named by WAYLAND_DISPLAY, then reports the applet's CPU
# time (from /proc/<pid>/schedstat, in ns), RSS, thread count and threads per
# `comm`. Same method as scripts/soak.sh, so the numbers are comparable.
# Exits non-zero if the resting CPU budget is exceeded.
#
#   WAYLAND_DISPLAY=wayland-1 scripts/applet-soak.sh
#   DURATION=60 RUNS=3 CPU_BUDGET_PCT=0.1 scripts/applet-soak.sh
#
# It measures idle only. To measure with a popover open, hover an item while
# the script runs; it does not drive the pointer.
set -euo pipefail

BIN=${BIN:-target/release}
DURATION=${DURATION:-60}
RUNS=${RUNS:-1}
CPU_BUDGET_PCT=${CPU_BUDGET_PCT:-0.1}

: "${WAYLAND_DISPLAY:?set WAYLAND_DISPLAY to a running compositor}"

if [ -z "${MASTHEAD_SOAK_INNER:-}" ]; then
    exec env MASTHEAD_SOAK_INNER=1 dbus-run-session -- "$0" "$@"
fi

export XDG_CONFIG_HOME
XDG_CONFIG_HOME=$(mktemp -d)
trap 'kill "${A:-}" "${D:-}" 2>/dev/null || true; rm -rf "$XDG_CONFIG_HOME"' EXIT

"$BIN/masthead-daemon" &
D=$!
sleep 1
"$BIN/masthead-cosmic" &
A=$!
sleep 5 # startup, font loading and the first samples are not steady state
kill -0 "$A" 2>/dev/null || { echo "FAIL: masthead-cosmic exited"; exit 1; }

cpu_ns() { awk '{print $1}' "/proc/$A/schedstat"; }
fail=0
for run in $(seq "$RUNS"); do
    t0=$(cpu_ns)
    sleep "$DURATION"
    t1=$(cpu_ns)
    rss=$(awk '/^VmRSS/ {print $2}' "/proc/$A/status")
    threads=$(awk '/^Threads/ {print $2}' "/proc/$A/status")
    pct=$(awk -v c="$((t1 - t0))" -v d="$DURATION" 'BEGIN {printf "%.4f", c / (d * 1e7)}')
    printf 'run %s: cpu %s ms (%s%% of one core), rss %s KiB, threads %s\n' \
        "$run" "$(((t1 - t0) / 1000000))" "$pct" "$rss" "$threads"
    awk -v p="$pct" -v b="$CPU_BUDGET_PCT" 'BEGIN {exit !(p > b)}' && fail=1
done
echo "threads by comm:"
cat /proc/"$A"/task/*/comm | sort | uniq -c | sort -rn | sed 's/^/  /'
[ "$fail" -eq 0 ] || echo "FAIL: CPU over budget"
exit "$fail"
