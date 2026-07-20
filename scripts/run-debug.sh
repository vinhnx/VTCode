#!/bin/bash

# VTCODE - Debug Mode Launch Script
# This script provides fast development builds

set -eo pipefail

restore_terminal_state() {
	if [[ -t 1 ]]; then
		# Best-effort restore for raw/alternate-screen/mouse modes in case vtcode aborts.
		printf '\r\033[K\033[?1049l\033[?2004l\033[?1004l\033[?1006l\033[?1015l\033[?1003l\033[?1002l\033[?1000l\033[<1u\033[?25h' >/dev/tty 2>/dev/null || true
		stty sane </dev/tty >/dev/tty 2>/dev/null || true
	fi
}

trap restore_terminal_state EXIT INT TERM

# Suppress macOS malloc warnings by REMOVING the env vars (not setting to 0)
# Setting to 0 triggers "can't turn off malloc stack logging" warnings
unset MallocStackLogging
unset MallocStackLoggingDirectory
unset MALLOCSTACKTOOLSDIR
unset MallocErrorAbort
unset MallocNanoZone

export VT_SESSION_DIR="${VT_SESSION_DIR:-$HOME/.vtcode/sessions}"

# Check if we're in the right directory
if [[ ! -f "Cargo.toml" ]]; then
	echo "Error: Please run this script from the vtcode project root directory"
	exit 1
fi

# --- Fast iteration tuning -------------------------------------------------
# For a local edit->run loop, incremental compilation rebuilds only changed
# crates and is dramatically faster than a from-scratch cache lookup. sccache
# requires incremental=false (see Cargo.toml [profile.dev]) and rejects any
# build with CARGO_INCREMENTAL=1, so we disable sccache here and let rust's
# incremental cache drive fast rebuilds instead. Use ./scripts/rrf.sh for
# release builds where sccache's cross-build cache wins.

# `unset` actually clears a wrapper that may be set in the parent shell;
# `${VAR:-}` only defaults on unset vars, so `export RUSTC_WRAPPER=""`
# would not drop a pre-existing value.
unset RUSTC_WRAPPER
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-1}"

# On Apple Silicon, rustc is heavy enough per-thread that pinning cargo
# jobs to the P-core count (vs. all logical CPUs) usually wins for the
# small incremental rebuilds that drive an edit->run loop: cargo's
# default counts E-cores too, and rustc threads thrashing the E-cores
# end up slower than fewer-but-faster P-core threads. No-op on non-Darwin
# hosts. Override via `CARGO_BUILD_JOBS=N ./scripts/run-debug.sh`.
if [[ -z "${CARGO_BUILD_JOBS}" && "$(uname -s)" == "Darwin" ]] &&
	sysctl -n hw.perflevel0.physicalcpu >/dev/null 2>&1; then
	export CARGO_BUILD_JOBS="$(sysctl -n hw.perflevel0.physicalcpu)"
fi

# Build optional args from environment
EXTRA_ARGS=()
if [[ -n "$MODEL" ]]; then
	EXTRA_ARGS+=(--model "$MODEL")
fi
if [[ -n "$PROVIDER" ]]; then
	EXTRA_ARGS+=(--provider "$PROVIDER")
fi
if [[ -n "$WORKSPACE" ]]; then
	EXTRA_ARGS+=(--workspace "$WORKSPACE")
fi

# Run with advanced features enabled by default.
# A single `cargo run` builds (incrementally) and launches in one pass; the
# previous `cargo build` + `cargo run` duplicated dependency-graph resolution.
# Note: Interactive chat is launched via the TUI without a subcommand.
# Increase stack floor for spawned threads in debug runs to reduce overflow risk.
export RUST_MIN_STACK="${RUST_MIN_STACK:-16777216}"
cargo run -- "${EXTRA_ARGS[@]}" --show-file-diffs --debug
