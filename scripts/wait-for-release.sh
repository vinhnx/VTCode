#!/usr/bin/env bash

# VT Code Release Monitor - Polls GitHub API for release assets
# Waits for binaries to be available and notifies when ready

set -euo pipefail

# Source common utilities
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# Configuration
REPO="vinhnx/vtcode"

RELEASE_TAG=$(get_current_version)
POLL_INTERVAL=5  # seconds
MAX_WAIT=1800    # 30 minutes max
ELAPSED=0
AUTO_INSTALL=false

# Required assets for your platform
get_required_assets() {
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64)
            echo "vtcode-$RELEASE_TAG-aarch64-apple-darwin.tar.gz"
            ;;
        Darwin-x86_64)
            echo "vtcode-$RELEASE_TAG-x86_64-apple-darwin.tar.gz"
            ;;
        Linux-x86_64)
            echo "vtcode-$RELEASE_TAG-x86_64-unknown-linux-musl.tar.gz"
            echo "vtcode-$RELEASE_TAG-x86_64-unknown-linux-gnu.tar.gz"
            ;;
        MINGW*-x86_64|MSYS*-x86_64)
            echo "vtcode-$RELEASE_TAG-x86_64-pc-windows-msvc.zip"
            ;;
        *)
            echo "ERROR: Unsupported platform" >&2
            exit 1
            ;;
    esac
}

# Check if release has all required assets
check_release() {
    local GITHUB_API="https://api.github.com/repos/$REPO/releases/tags/$RELEASE_TAG"
    local response
    local release_state
    local has_binary=false
    local has_checksums=false
    local binary_asset=""
    local required_assets
    
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
    required_assets=$(get_required_assets)
    while IFS= read -r asset; do
        [[ -z "$asset" ]] && continue
        if echo "$response" | grep -q "\"name\": \"$asset\""; then
            has_binary=true
            binary_asset="$asset"
            break
        fi
    done <<< "$required_assets"
    
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
    local GITHUB_API="https://api.github.com/repos/$REPO/releases/tags/$RELEASE_TAG"
    local response
    local has_assets
    local asset_count
    local binary_asset=""
    local required_assets
    
    response=$(curl -fsSL "$GITHUB_API" 2>/dev/null || echo "")
    
    if [[ -z "$response" ]]; then
        log_warning "Cannot reach GitHub API, retrying..."
        return
    fi
    
    asset_count=$(echo "$response" | grep -o '"name": "vtcode-' | wc -l)
    
    if [[ $asset_count -gt 0 ]]; then
        required_assets=$(get_required_assets)
        while IFS= read -r asset; do
            [[ -z "$asset" ]] && continue
            if echo "$response" | grep -q "\"name\": \"$asset\""; then
                binary_asset="$asset"
                break
            fi
        done <<< "$required_assets"

        if [[ -n "$binary_asset" ]]; then
            log_success "Binary for your platform is ready: $binary_asset"
            has_assets=true
        else
            log_info "Binaries available: $asset_count/5"
        fi
    else
        log_info "Waiting for binaries to be built..."
    fi
}

# Show help
show_help() {
    cat <<HELP
VT Code Release Monitor - Wait for binaries

Usage: ./scripts/wait-for-release.sh [options]

Options:
  -t, --tag VERSION   Version tag to wait for (default: from Cargo.toml)
  -i, --interval N    Poll interval in seconds (default: 5)
  -m, --max-wait N    Maximum wait time in seconds (default: 1800)
  -a, --auto-install  Automatically run installer when ready
  -h, --help          Show this help message

Examples:
  ./scripts/wait-for-release.sh                    # Wait for current version
  ./scripts/wait-for-release.sh -t 0.82.1          # Wait for specific version
  ./scripts/wait-for-release.sh -a                 # Auto-install when ready

The script will exit successfully (0) when all binaries are ready.
HELP
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -t|--tag)
            RELEASE_TAG=$2
            shift 2
            ;;
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
log_info "Waiting for VT Code $RELEASE_TAG binaries to be available..."
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
