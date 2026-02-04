#!/bin/bash

# VTCODE - Release-Fast Run Script
# This script runs vtcode with the release-fast profile for optimized performance

set -e

# Check if we're in the right directory
if [[ ! -f "Cargo.toml" ]]; then
    echo "Error: Please run this script from the vtcode project root directory"
    exit 1
fi

echo "Running vtcode with release-fast profile (optimized build)..."
echo ""

# Build and run with the release-fast profile
cargo run --profile release-fast -- "$@"