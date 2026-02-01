#!/usr/bin/env bash

# Trigger GitHub Actions release workflow for cross-platform builds
# Usage: ./scripts/trigger-ci-release.sh v0.74.3

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

print_info() {
    printf '%b\n' "${BLUE}INFO:${NC} $1"
}

print_success() {
    printf '%b\n' "${GREEN}SUCCESS:${NC} $1"
}

print_error() {
    printf '%b\n' "${RED}ERROR:${NC} $1"
}

print_warning() {
    printf '%b\n' "${YELLOW}WARNING:${NC} $1"
}

if [[ $# -ne 1 ]]; then
    print_error "Usage: $0 <tag>"
    print_info "Example: $0 v0.74.3"
    exit 1
fi

TAG="$1"

# Validate tag format
if ! [[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    print_error "Invalid tag format: $TAG (expected v0.74.3)"
    exit 1
fi

print_info "Triggering GitHub Actions release workflow for tag: $TAG"

# Check if gh CLI is authenticated
if ! gh auth status >/dev/null 2>&1; then
    print_error "GitHub CLI not authenticated"
    print_info "Run: gh auth login"
    exit 1
fi

# Trigger the release workflow
if gh workflow run release.yml \
    -f tag="$TAG" \
    --ref main; then
    print_success "Release workflow triggered for $TAG"
    print_info "Watch progress: gh run list --workflow=release.yml"
    print_info "Or visit: https://github.com/vinhnx/vtcode/actions/workflows/release.yml"
else
    print_error "Failed to trigger release workflow"
    exit 1
fi
