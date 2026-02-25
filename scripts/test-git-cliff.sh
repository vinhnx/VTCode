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
LATEST_VERSION="0.82.5"

# Find previous semver version
PREVIOUS_VERSION=$(git tag | grep -E '^[vV]?[0-9]+\.[0-9]+\.[0-9]+$' | sed 's/^[vV]//' | sort -t. -k1,1rn -k2,2rn -k3,3rn | awk -v ver="$LATEST_VERSION" '$0 != ver {print; exit}')

echo "Previous version: $PREVIOUS_VERSION"
echo "Current version: $LATEST_VERSION"
echo ""

# Test 1: Generate changelog with semver range
echo "=== Test 1: Changelog with semver range ($PREVIOUS_VERSION..HEAD) ==="
git-cliff --config cliff.toml --tag "$LATEST_VERSION" "${PREVIOUS_VERSION}..HEAD" 2>&1 | head -60
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
echo "  git-cliff --config cliff.toml --tag <version> --range <prev>..HEAD --output CHANGELOG.md"
echo ""
echo "To preview unreleased changes:"
echo "  git-cliff --config cliff.toml --tag <version> --range <prev>..HEAD"
