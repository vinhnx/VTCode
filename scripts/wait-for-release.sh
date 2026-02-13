#!/usr/bin/env bash

# VT Code Release Monitor - Polls GitHub API for release assets
# Waits for binaries to be available and notifies when ready

set -euo pipefail

# Configuration
REPO="vinhnx/vtcode"
RELEASE_TAG="0.58.6"
GITHUB_API="https://api.github.com/repos/$REPO/releases/tags/$RELEASE_TAG"
POLL_INTERVAL=5  # seconds
MAX_WAIT=1800    # 30 minutes max
ELAPSED=0

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Required assets for your platform
get_required_assets() {
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64)
            echo "vtcode-0.58.6-aarch64-apple-darwin.tar.gz"
            ;;
        Darwin-x86_64)
            echo "vtcode-0.58.6-x86_64-apple-darwin.tar.gz"
            ;;
        Linux-x86_64)
            echo "vtcode-0.58.6-x86_64-unknown-linux-gnu.tar.gz"
            ;;
        MINGW*-x86_64|MSYS*-x86_64)
            echo "vtcode-0.58.6-x86_64-pc-windows-msvc.zip"
            ;;
        *)
            echo "ERROR: Unsupported platform" >&2
            exit 1
            ;;
    esac
}

# Logging
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

# Check if release has all required assets
check_release() {
    local response
    local release_state
    local has_binary=false
    local has_checksums=false
    local binary_asset
    
    response=$(curl -fsSL "$GITHUB_API" 2>/dev/null || echo "")
    
    if [[ -z "$response" ]]; then
        return 1
    fi
    
    # Check release state
    release_state=$(echo "$response" | grep -o '"draft":[^,]*' | cut -d: -f2 | tr -d ' ')
    if [[ "$release_state" == "true" ]]; then
        # Release still being created
        return 1
    fi
    
    # Get required binary for this platform
    binary_asset=$(get_required_assets)
    
    # Check for binary asset
    if echo "$response" | grep -q "\"name\": \"$binary_asset\""; then
        has_binary=true
    fi
    
    # Check for checksums
    if echo "$response" | grep -q '"name": "checksums.txt"'; then
        has_checksums=true
    fi
    
    if $has_binary && $has_checksums; then
        return 0  # All assets ready
    fi
    
    if $has_binary; then
        log_warning "Binary ready, waiting for checksums.txt..."
        return 1
    fi
    
    return 1  # Not ready yet
}

# Show current status
show_status() {
    local response
    local has_assets
    local asset_count
    local binary_asset
    
    response=$(curl -fsSL "$GITHUB_API" 2>/dev/null || echo "")
    
    if [[ -z "$response" ]]; then
        log_warning "Cannot reach GitHub API, retrying..."
        return
    fi
    
    asset_count=$(echo "$response" | grep -o '"name": "vtcode-' | wc -l)
    
    if [[ $asset_count -gt 0 ]]; then
        binary_asset=$(get_required_assets)
        if echo "$response" | grep -q "\"name\": \"$binary_asset\""; then
            log_success "Binary for your platform is ready: $binary_asset"
            has_assets=true
        else
            log_info "Binaries available: $asset_count/4"
        fi
    else
        log_info "Waiting for binaries to be built..."
    fi
}

# Show help
show_help() {
    cat <<'HELP'
VT Code Release Monitor - Wait for 0.58.6 binaries

Usage: ./scripts/wait-for-release.sh [options]

Options:
  -i, --interval N    Poll interval in seconds (default: 5)
  -m, --max-wait N    Maximum wait time in seconds (default: 1800)
  -a, --auto-install  Automatically run installer when ready
  -h, --help          Show this help message

Examples:
  ./scripts/wait-for-release.sh                    # Poll every 5 seconds
  ./scripts/wait-for-release.sh -i 10              # Poll every 10 seconds
  ./scripts/wait-for-release.sh -a                 # Auto-install when ready
  ./scripts/wait-for-release.sh -i 30 -m 3600     # Poll every 30s, max 1 hour

The script will exit successfully (0) when all binaries are ready.
HELP
}

# Parse arguments
AUTO_INSTALL=false
while [[ $# -gt 0 ]]; do
    case $1 in
        -i|--interval)
            POLL_INTERVAL=$2
            shift 2
            ;;
        -m|--max-wait)
            MAX_WAIT=$2
            shift 2
            ;;
        -a|--auto-install)
            AUTO_INSTALL=true
            shift
            ;;
        -h|--help)
            show_help
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            show_help
            exit 1
            ;;
    esac
done

# Main polling loop
log_info "Waiting for VT Code 0.58.6 binaries to be available..."
log_info "Release: https://github.com/$REPO/releases/tag/$RELEASE_TAG"
log_info "Polling every $POLL_INTERVAL seconds (max wait: $MAX_WAIT seconds)"
echo ""

while [[ $ELAPSED -lt $MAX_WAIT ]]; do
    if check_release; then
        echo ""
        log_success "All binaries ready! ✨"
        echo ""
        
        # Show platform-specific binary
        binary_asset=$(get_required_assets)
        log_success "Your platform binary: $binary_asset"
        echo ""
        
        # Show next steps
        log_success "Ready to install!"
        echo ""
        log_info "Run the installer:"
        echo "  curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash"
        echo ""
        
        if [[ "$AUTO_INSTALL" == "true" ]]; then
            log_info "Auto-installing in 5 seconds... (Ctrl+C to cancel)"
            sleep 5
            log_info "Starting installer..."
            curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
            exit $?
        else
            log_info "Alternatively, verify with: vtcode --version"
        fi
        
        exit 0
    fi
    
    # Show status
    show_status
    
    # Progress indicator
    printf '%b\r' "${BLUE}⏳${NC} Elapsed: ${ELAPSED}s / ${MAX_WAIT}s (Next check in ${POLL_INTERVAL}s)..." >&2
    
    sleep "$POLL_INTERVAL"
    ELAPSED=$((ELAPSED + POLL_INTERVAL))
done

echo ""
log_error "Timeout: Binaries not available after ${MAX_WAIT} seconds"
log_info "Check GitHub Actions for build status:"
log_info "  https://github.com/$REPO/actions"
log_info "Check release page:"
log_info "  https://github.com/$REPO/releases/tag/$RELEASE_TAG"
exit 1
