#!/bin/bash

# Build and launch the VT Code desktop app for local development.
#
# Combines the two canonical commands from docs/development/desktop-workspace.md:
#
#   cargo build --locked -p vtcode
#   cargo run --locked -p vtcode-desktop
#
# `cargo run -p vtcode-desktop` alone rebuilds only the GUI and then respawns the
# previous backend binary, so every protocol-version bump shows the
# "incompatible desktop protocol versions" banner until the CLI is rebuilt.
# This script always builds the backend first, which is why it exists.
#
# Usage:
#   scripts/run-desktop.sh                 # debug build and launch
#   scripts/run-desktop.sh --release       # release build and launch
#   scripts/run-desktop.sh -- --flag value # extra args passed through to the app
#
# Run from anywhere; the script changes to the workspace root itself.

set -e

# Resolve the workspace root from this script's location, not the caller's cwd.
workspace="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$workspace"

profile="debug"
release_flag=()
if [ "$1" = "--release" ]; then
	profile="release"
	release_flag=(--release)
	shift
fi

echo "==> Building backend (vtcode, ${profile})"
cargo build --locked -p vtcode "${release_flag[@]}"

echo "==> Launching desktop GUI (${profile})"
exec cargo run --locked -p vtcode-desktop "${release_flag[@]}" -- "$@"
