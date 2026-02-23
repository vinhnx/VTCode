#!/usr/bin/env bash

# Common utilities and configuration for VT Code scripts

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
MAGENTA='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m'

# Cargo configuration to prevent timeouts
export CARGO_HTTP_TIMEOUT=${CARGO_HTTP_TIMEOUT:-300}
export CARGO_NET_RETRY=${CARGO_NET_RETRY:-5}

# Logging functions
print_info() {
    printf '%b\n' "${BLUE}INFO:${NC} $1"
}

print_status() {
    printf '%b\n' "${BLUE}INFO:${NC} $1"
}

print_success() {
    printf '%b\n' "${GREEN}SUCCESS:${NC} $1"
}

print_warning() {
    printf '%b\n' "${YELLOW}WARNING:${NC} $1"
}

print_error() {
    printf '%b\n' "${RED}ERROR:${NC} $1"
}

# Compatibility logging functions (from various scripts)
log_info() {
    printf '%b\n' "${BLUE}ℹ${NC} $1" >&2
}

log_success() {
    printf '%b\n' "${GREEN}✓${NC} $1" >&2
}

log_warning() {
    printf '%b\n' "${YELLOW}⚠${NC} $1" >&2
}

log_error() {
    printf '%b\n' "${RED}✗${NC} $1" >&2
}

# Version detection
get_current_version() {
    local line
    # Try current directory first
    if [[ -f "Cargo.toml" ]]; then
        line=$(grep '^version = ' Cargo.toml | head -1 2>/dev/null || echo "")
    fi
    
    if [[ -z "$line" ]]; then
        # Try to find version in workspace members if not in root
        local root_toml="$(dirname "${BASH_SOURCE[0]}")/../Cargo.toml"
        if [[ -f "$root_toml" ]]; then
            line=$(grep '^version = ' "$root_toml" | head -1 2>/dev/null || echo "")
        fi
    fi
    
    if [[ -z "$line" ]]; then
        # Last resort: find any Cargo.toml
        line=$(find . -maxdepth 2 -name Cargo.toml -exec grep '^version = ' {} + | head -1 2>/dev/null || echo "")
    fi
    
    if [[ -z "$line" ]]; then
        echo "0.82.1" # Fallback to latest known
    else
        echo "${line#*\"}" | sed 's/\".*//'
    fi
}
