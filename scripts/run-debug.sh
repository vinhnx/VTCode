#!/bin/bash

# VTCODE - Debug Mode Launch Script
# This script provides fast development builds

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/terminal-restore-common.sh"

trap restore_terminal_state EXIT INT TERM

# Resolve the host OS once: the session-dir default, P-core job pinning, and
# the Gatekeeper pre-scan branch all branch on it.
HOST_OS="$(uname -s)"

# Suppress macOS malloc warnings by REMOVING the env vars (not setting to 0)
# Setting to 0 triggers "can't turn off malloc stack logging" warnings
unset MallocStackLogging
unset MallocStackLoggingDirectory
unset MALLOCSTACKTOOLSDIR
unset MallocErrorAbort
unset MallocNanoZone

if [[ -z "${VT_SESSION_DIR:-}" ]]; then
	case "${HOST_OS}" in
	Darwin)
		VT_STATE_DIR="$HOME/Library/Application Support/com.vinhnx.vtcode/state"
		;;
	*)
		if [[ "${XDG_STATE_HOME:-}" = /* ]]; then
			VT_STATE_DIR="$XDG_STATE_HOME/vtcode"
		else
			VT_STATE_DIR="$HOME/.local/state/vtcode"
		fi
		;;
	esac
	export VT_SESSION_DIR="$VT_STATE_DIR/sessions"
fi

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
if [[ -z "${CARGO_BUILD_JOBS}" && "${HOST_OS}" == "Darwin" ]]; then
	CARGO_BUILD_P_CORES="$(sysctl -n hw.perflevel0.physicalcpu 2>/dev/null || true)"
	if [[ -n "${CARGO_BUILD_P_CORES}" ]]; then
		export CARGO_BUILD_JOBS="${CARGO_BUILD_P_CORES}"
	fi
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

# Ad-hoc CLI args are forwarded last so a caller can override the defaults,
# e.g. `./scripts/run-debug.sh --help` or `./scripts/run-debug.sh --resume`.
LAUNCH_ARGS=(--show-file-diffs --debug "${EXTRA_ARGS[@]}" "$@")

if [[ "${HOST_OS}" == "Darwin" && -x /usr/bin/gktool ]]; then
	cargo build --bin vtcode

	VTCODE_DEBUG_TARGET_DIR="${CARGO_TARGET_DIR:-target}"
	if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
		VTCODE_DEBUG_TARGET_DIR="${VTCODE_DEBUG_TARGET_DIR}/${CARGO_BUILD_TARGET}"
	fi
	VTCODE_DEBUG_BUILD_DIR="${VTCODE_DEBUG_TARGET_DIR}/debug"
	VTCODE_DEBUG_BINARY="${VTCODE_DEBUG_BUILD_DIR}/vtcode"
	VTCODE_DEBUG_APP="${VTCODE_DEBUG_BUILD_DIR}/vtcode-debug.app"
	VTCODE_DEBUG_APP_CONTENTS="${VTCODE_DEBUG_APP}/Contents"
	VTCODE_DEBUG_APP_EXECUTABLE="${VTCODE_DEBUG_APP_CONTENTS}/MacOS/vtcode"
	VTCODE_DEBUG_INFO_PLIST="${VTCODE_DEBUG_APP_CONTENTS}/Info.plist"

	# Gatekeeper shows a transient verification sheet for newly built binaries.
	# Scan a stable local app bundle before launching the same debug executable.
	#
	# Only refresh the bundle when it does not already hold the current binary:
	# `cargo build` leaves the binary untouched when nothing changed, so a
	# relaunch without edits skips the copy/re-sign/re-scan entirely and keeps
	# the bundle's signature stable for Gatekeeper's cache. A real relink bumps
	# the binary mtime and forces the refresh.
	if [[ ! -f "${VTCODE_DEBUG_APP_EXECUTABLE}" || ! -f "${VTCODE_DEBUG_INFO_PLIST}" ||
		"${VTCODE_DEBUG_BINARY}" -nt "${VTCODE_DEBUG_APP_EXECUTABLE}" ]]; then
		VTCODE_DEBUG_INFO_TEMP="${VTCODE_DEBUG_INFO_PLIST}.tmp.$$"

		mkdir -p "${VTCODE_DEBUG_APP_CONTENTS}/MacOS"
		cp -f "${VTCODE_DEBUG_BINARY}" "${VTCODE_DEBUG_APP_EXECUTABLE}"
		cat >"${VTCODE_DEBUG_INFO_TEMP}" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key>
	<string>vtcode</string>
	<key>CFBundleIdentifier</key>
	<string>com.vinhnx.vtcode.debug</string>
	<key>CFBundleName</key>
	<string>VT Code Debug</string>
	<key>CFBundleDisplayName</key>
	<string>VT Code Debug</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleVersion</key>
	<string>0.0.0</string>
	<key>CFBundleShortVersionString</key>
	<string>0.0.0</string>
</dict>
</plist>
EOF
		mv -f "${VTCODE_DEBUG_INFO_TEMP}" "${VTCODE_DEBUG_INFO_PLIST}"
		/usr/bin/codesign --force --sign - --identifier com.vinhnx.vtcode.debug "${VTCODE_DEBUG_APP}"
		if ! /usr/bin/gktool scan "${VTCODE_DEBUG_APP}"; then
			printf 'Error: Gatekeeper could not pre-scan the VT Code debug app; refusing to launch it to avoid the macOS verification sheet.\n' >&2
			exit 1
		fi
	fi

	export PATH="${VTCODE_DEBUG_BUILD_DIR}:${PATH}"
	"${VTCODE_DEBUG_APP_EXECUTABLE}" "${LAUNCH_ARGS[@]}"
else
	cargo run -- "${LAUNCH_ARGS[@]}"
fi
