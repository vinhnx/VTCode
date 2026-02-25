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
    exit 1
fi

echo "git-cliff version: $(git-cliff --version)"
echo ""

# Get GitHub token if available
GITHUB_TOKEN=""
if command -v gh >/dev/null 2>&1; then
    GITHUB_TOKEN=$(gh auth token 2>/dev/null || true)
fi

if [[ -n "$GITHUB_TOKEN" ]]; then
    echo "GitHub token found - using online mode"
    export GITHUB_TOKEN
else
    echo "No GitHub token - using offline mode"
fi

# Get the latest version for testing
LATEST_VERSION="0.82.4"

# Test 1: Generate changelog for last version with tag
echo ""
echo "=== Test 1: Latest version with tag ==="
git-cliff --config cliff.toml --tag "$LATEST_VERSION" --unreleased 2>&1 | head -50
echo ""

# Test 2: Generate changelog for last 3 versions
echo "=== Test 2: Last 3 versions ==="
git-cliff --config cliff.toml --latest 3 2>&1 | head -50
echo ""

# Test 3: Full changelog
echo "=== Test 3: Full changelog (first 100 lines) ==="
git-cliff --config cliff.toml 2>&1 | head -100
echo ""

echo "=== Tests complete ==="
echo ""
echo "To regenerate CHANGELOG.md for a release:"
echo "  git-cliff --config cliff.toml --tag <version> --unreleased --output CHANGELOG.md"
echo ""
echo "To preview unreleased changes:"
echo "  git-cliff --config cliff.toml --tag <version> --unreleased"
