#!/bin/bash

# VTCODE - Release-Fast Run Script
# This script runs vtcode with the release-fast profile for optimized performance

set -eo pipefail

restore_terminal_state() {
  if [[ -t 1 ]]; then
    # Best-effort restore for raw/alternate-screen/mouse modes in case vtcode aborts.
    printf '\r\033[K\033[?1049l\033[?2004l\033[?1004l\033[?1006l\033[?1015l\033[?1003l\033[?1002l\033[?1000l\033[<1u\033[?25h' > /dev/tty 2>/dev/null || true
    stty sane < /dev/tty > /dev/tty 2>/dev/null || true
  fi
}

trap restore_terminal_state EXIT INT TERM

# Check if we're in the right directory
if [[ ! -f "Cargo.toml" ]]; then
    echo "Error: Please run this script from the vtcode project root directory"
    exit 1
fi

echo "Running vtcode with release-fast profile (optimized build)..."
echo ""

# Build and run with the release-fast profile
# Increase stack floor for spawned threads to reduce overflow risk.
export RUST_MIN_STACK="${RUST_MIN_STACK:-16777216}"
cargo run --profile release-fast -- "$@"
