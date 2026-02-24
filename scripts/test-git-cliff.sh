#!/usr/bin/env bash

# Test script to compare git-cliff output with current changelog

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

echo "=== Testing git-cliff configuration ==="
echo ""

# Check if git-cliff is installed
if ! command -v git-cliff >/dev/null 2>&1; then
    echo "git-cliff is not installed."
    echo "Install with: cargo install git-cliff"
    echo ""
    echo "Or test with Docker:"
    echo "  docker run --rm -v \"\$(pwd):/app\" -w /app ghcr.io/orhunp/git-cliff:latest --config cliff.toml"
    exit 1
fi

echo "git-cliff version: $(git-cliff --version)"
echo ""

# Test 1: Generate changelog for last 3 versions
echo "=== Test 1: Last 3 versions ==="
git-cliff --config cliff.toml --latest 3 2>&1 | head -50
echo ""

# Test 2: Generate unreleased changelog (from last tag to HEAD)
echo "=== Test 2: Unreleased changes ==="
git-cliff --config cliff.toml --unreleased 2>&1 | head -50
echo ""

# Test 3: Full changelog (compare with existing CHANGELOG.md)
echo "=== Test 3: Full changelog (first 100 lines) ==="
git-cliff --config cliff.toml 2>&1 | head -100
echo ""

echo "=== Tests complete ==="
echo ""
echo "To regenerate CHANGELOG.md:"
echo "  git-cliff --config cliff.toml --output CHANGELOG.md"
echo ""
echo "To preview unreleased changes:"
echo "  git-cliff --config cliff.toml --unreleased"
