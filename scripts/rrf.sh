#!/bin/bash

# VTCODE - Release-Fast Run Script
# This script runs vtcode with the release-fast profile for optimized performance

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Source common utilities
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

# shellcheck disable=SC1091
source "${SCRIPT_DIR}/terminal-restore-common.sh"

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
