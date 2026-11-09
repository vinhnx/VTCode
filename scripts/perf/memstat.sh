#!/usr/bin/env bash
# Capture memory-stall counters around a command via Linux `perf stat`.
# Usage: scripts/perf/memstat.sh [--] <command> [args...]
# Where `perf` is unavailable (macOS, restricted hosts), runs the command
# directly and says so; counters are Linux-only.
set -euo pipefail

if [[ "${1:-}" == "--" ]]; then
    shift
fi

if [[ $# -eq 0 ]]; then
    echo "usage: scripts/perf/memstat.sh [--] <command> [args...]" >&2
    exit 2
fi

if ! command -v perf >/dev/null 2>&1; then
    echo "memstat: 'perf' not found; running without counters (Linux perf_event only)." >&2
    exec "$@"
fi

exec perf stat -e cache-references,cache-misses,cycles,instructions -- "$@"
