#!/usr/bin/env bash
# Soak test for masthead-daemon's resource budget.
#
# Runs the daemon on a private session bus with one masthead-bar subscriber,
# then measures the daemon's CPU time, resident memory and bytes written to
# storage over DURATION seconds. Exits non-zero if any budget is exceeded.
#
#   scripts/soak.sh                         # resting budget, 600 s
#   DURATION=60 TIERS="window" CPU_BUDGET_PCT=3 RSS_BUDGET_KB=16384 scripts/soak.sh
#   CONFIG=$'[devices]\nnvml = false' scripts/soak.sh   # config.toml contents
set -euo pipefail

BIN=${BIN:-target/release}
DURATION=${DURATION:-600}
TIERS=${TIERS:-}
CPU_BUDGET_PCT=${CPU_BUDGET_PCT:-0.1}
RSS_BUDGET_KB=${RSS_BUDGET_KB:-6144}
WRITE_BUDGET_BYTES=${WRITE_BUDGET_BYTES:-0}

if [ -z "${MASTHEAD_SOAK_INNER:-}" ]; then
    exec env MASTHEAD_SOAK_INNER=1 dbus-run-session -- "$0" "$@"
fi

# Keep the user's real config out of the measurement.
export XDG_CONFIG_HOME
XDG_CONFIG_HOME=$(mktemp -d)
if [ -n "${CONFIG:-}" ]; then
    mkdir -p "$XDG_CONFIG_HOME/masthead"
    printf '%s\n' "$CONFIG" >"$XDG_CONFIG_HOME/masthead/config.toml"
fi
trap 'kill "${B:-}" "${D:-}" 2>/dev/null || true; rm -rf "$XDG_CONFIG_HOME"' EXIT

"$BIN/masthead-daemon" &
D=$!
sleep 1
args=()
for t in $TIERS; do args+=(--tier "$t"); done
"$BIN/masthead-bar" --format plain "${args[@]}" >/dev/null &
B=$!
sleep 3 # discovery and the first samples are not steady state

stat_ticks() { awk '{print $14 + $15}' "/proc/$D/stat"; }
writes() { awk '/^write_bytes/ {print $2}' "/proc/$D/io"; }

t0=$(stat_ticks)
w0=$(writes)
sleep "$DURATION"
t1=$(stat_ticks)
w1=$(writes)
rss=$(awk '/^VmRSS/ {print $2}' "/proc/$D/status")
hwm=$(awk '/^VmHWM/ {print $2}' "/proc/$D/status")
threads=$(awk '/^Threads/ {print $2}' "/proc/$D/status")
hz=$(getconf CLK_TCK)

cpu_ms=$(((t1 - t0) * 1000 / hz))
budget_ms=$(awk -v d="$DURATION" -v p="$CPU_BUDGET_PCT" 'BEGIN {printf "%d", d * 1000 * p / 100}')
pct=$(awk -v c="$cpu_ms" -v d="$DURATION" 'BEGIN {printf "%.4f", c / (d * 10)}')
written=$((w1 - w0))

printf 'tiers:        panel %s\n' "$TIERS"
printf 'duration:     %s s\n' "$DURATION"
printf 'cpu:          %s ms (%s%% of one core; budget %s ms)\n' "$cpu_ms" "$pct" "$budget_ms"
printf 'rss:          %s KiB (peak %s KiB; budget %s KiB)\n' "$rss" "$hwm" "$RSS_BUDGET_KB"
printf 'disk writes:  %s bytes (budget %s)\n' "$written" "$WRITE_BUDGET_BYTES"
printf 'threads:      %s\n' "$threads"

fail=0
[ "$cpu_ms" -le "$budget_ms" ] || { echo "FAIL: CPU over budget"; fail=1; }
[ "$rss" -le "$RSS_BUDGET_KB" ] || { echo "FAIL: RSS over budget"; fail=1; }
[ "$written" -le "$WRITE_BUDGET_BYTES" ] || { echo "FAIL: disk writes over budget"; fail=1; }
exit "$fail"
