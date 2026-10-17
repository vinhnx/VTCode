#!/usr/bin/env bash
set -euo pipefail

# Measure process startup without provider credentials. Use a release binary
# when available; VTCODE_BENCH_INTERACTIVE=1 enables the manual first-frame run.
# Uses hyperfine (warmup, mean/stddev, outlier detection) when installed; set
# VTCODE_BASELINE_BIN to compare against a previous binary and VTCODE_BENCH_JSON
# to export results.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${VTCODE_BIN:-${repo_root}/target/release/vtcode}"
baseline="${VTCODE_BASELINE_BIN:-}"
runs="${VTCODE_BENCH_RUNS:-5}"
export VTCODE_STARTUP_TRACE="${VTCODE_STARTUP_TRACE:-0}"

if [[ ! -x "$binary" ]]; then
	cargo build --release --locked --manifest-path "${repo_root}/Cargo.toml"
fi

if [[ -n "$baseline" && ! -x "$baseline" ]]; then
	echo "VTCODE_BASELINE_BIN is not executable: $baseline" >&2
	exit 1
fi

measure() {
	local label="$1"
	shift
	local i start end
	for ((i = 1; i <= runs; i++)); do
		start="$(python3 -c 'import time; print(time.perf_counter_ns())')"
		"$binary" "$@" >/dev/null
		end="$(python3 -c 'import time; print(time.perf_counter_ns())')"
		awk -v l="$label" -v n="$i" -v s="$start" -v e="$end" \
			'BEGIN { printf "%s run=%d elapsed_ms=%.3f\n", l, n, (e-s)/1000000 }'
	done
}

# -N runs the binary without an intermediate shell so shell startup is not measured.
measure_hyperfine() {
	local label="$1"
	shift
	local cmd=(hyperfine -N --warmup 3 --runs "$runs" --command-name "$label" "$(printf '%q ' "$binary" "$@")")
	if [[ -n "$baseline" ]]; then
		cmd+=(--command-name "$label (baseline)" "$(printf '%q ' "$baseline" "$@")")
	fi
	if [[ -n "${VTCODE_BENCH_JSON:-}" ]]; then
		cmd+=(--export-json "${VTCODE_BENCH_JSON%.json}-${label}.json")
	fi
	"${cmd[@]}"
}

run_cases() {
	local runner="$1"
	"$runner" version --version
	"$runner" help --help
	# Builds the tool registry, so it is the heaviest provider-free case.
	"$runner" schema schema tools --format ndjson --name code_search
}

echo "binary=$binary runs=$runs"
# Isolated HOME keeps user config and credentials out of timed runs; the build above and
# the interactive run below use the real HOME.
iso_home="$(mktemp -d)"
trap 'rm -rf "$iso_home"' EXIT
if command -v hyperfine >/dev/null 2>&1; then
	(HOME="$iso_home" && export HOME && run_cases measure_hyperfine)
else
	if [[ -n "$baseline" ]]; then
		echo "VTCODE_BASELINE_BIN requires hyperfine (brew install hyperfine)" >&2
		exit 1
	fi
	(HOME="$iso_home" && export HOME && run_cases measure)
fi

if [[ "${VTCODE_BENCH_INTERACTIVE:-0}" == "1" ]]; then
	echo "interactive: stop after phase=first_ui_render"
	VTCODE_STARTUP_TRACE=1 "$binary"
fi
