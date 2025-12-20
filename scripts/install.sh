#!/bin/bash
# VT Code Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash [version]

set -e

GITHUB_REPO="vinhnx/vtcode"

# Parse arguments
TARGET_VERSION="$1"
VERBOSE="${VT_INSTALL_VERBOSE:-false}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

log() { echo -e "${BLUE}➜${NC} $1"; }
success() { echo -e "${GREEN}✓${NC} $1"; }
error() { echo -e "${RED}✗${NC} $1" >&2; }
warn() { echo -e "${YELLOW}⚠${NC} $1" >&2; }

verbose() {
    if [ "$VERBOSE" = "true" ]; then
        echo -e "${BLUE}[DEBUG]${NC} $1"
    fi
}

# Check dependencies
check_dependencies() {
    log "Checking dependencies..."
    
    if command -v curl >/dev/null 2>&1; then
        DOWNLOADER="curl"
        DOWNLOAD_CMD="curl -fsSL"
    elif command -v wget >/dev/null 2>&1; then
        DOWNLOADER="wget"
        DOWNLOAD_CMD="wget -q -O -"
    else
        error "Either curl or wget is required"
        exit 1
    fi
    
    verbose "Using downloader: $DOWNLOADER"
    
    # Check for jq for better JSON parsing
    if command -v jq >/dev/null 2>&1; then
        HAS_JQ=true
        verbose "jq found - will use for JSON parsing"
    else
        HAS_JQ=false
        verbose "jq not found - will use grep/sed fallback"
    fi
    
    success "Dependencies OK"
}

# Enhanced version extraction
get_latest_version() {
    local api_url="https://api.github.com/repos/$GITHUB_REPO/releases/latest"
    verbose "Fetching latest version from $api_url"
    
    local response
    if [ "$DOWNLOADER" = "curl" ]; then
        response=$(curl -fsSL "$api_url")
    else
        response=$(wget -q -O - "$api_url")
    fi
    
    if [ -z "$response" ]; then
        error "Failed to fetch release information"
        return 1
    fi
    
    local version
    if [ "$HAS_JQ" = "true" ]; then
        version=$(echo "$response" | jq -r '.tag_name // empty' 2>/dev/null || true)
    else
        # Fallback to grep/sed, but handle both 'v' prefix and without
        version=$(echo "$response" | grep '"tag_name":' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/' | sed -E 's/^v//')
    fi
    
    if [ -z "$version" ]; then
        error "Failed to parse version from GitHub API response"
        if [ "$VERBOSE" = "true" ]; then
            error "Response: $response"
        fi
        return 1
    fi
    
    # Remove 'v' prefix if present (ensure we don't double-prefix later)
    version=$(echo "$version" | sed -E 's/^v//')
    
    verbose "Latest version: $version"
    echo "$version"
}

# Check if release has assets before downloading
check_release_assets() {
    local version="$1"
    local platform="$2"
    
    local api_url="https://api.github.com/repos/$GITHUB_REPO/releases/tags/v${version}"
    verbose "Checking assets for release v${version}"
    
    local response
    if [ "$DOWNLOADER" = "curl" ]; then
        response=$(curl -fsSL "$api_url" 2>/dev/null || echo "")
    else
        response=$(wget -q -O - "$api_url" 2>/dev/null || echo "")
    fi
    
    if [ -z "$response" ]; then
        error "Failed to fetch release information for v${version}"
        return 1
    fi
    
    local asset_count
    if [ "$HAS_JQ" = "true" ]; then
        asset_count=$(echo "$response" | jq -r '.assets | length' 2>/dev/null || echo "0")
    else
        # Crude check - look for "assets": [...]
        asset_count=$(echo "$response" | grep -o '"assets":\s*\[' | wc -l)
        # This is very crude, better to just check if we see asset names
        if echo "$response" | grep -q "${platform}.tar.gz\|${platform}.zip"; then
            verbose "Found asset matching pattern for $platform"
            return 0
        fi
    fi
    
    if [ "$asset_count" = "0" ] 2>/dev/null; then
        error "Release v${version} exists but has no assets uploaded yet"
        error "The build workflow may still be running or was disabled"
        return 1
    fi
    
    verbose "Release v${version} has assets"
    return 0
}

# Detect platform with better error handling
detect_platform() {
    log "Detecting platform..."
    
    OS=$(uname -s 2>/dev/null || echo "")
    ARCH=$(uname -m 2>/dev/null || echo "")
    
    if [ -z "$OS" ] || [ -z "$ARCH" ]; then
        error "Failed to detect platform (OS: ${OS:-unknown}, ARCH: ${ARCH:-unknown})"
        error "Please report this issue with your system details"
        exit 1
    fi
    
    verbose "Detected OS: $OS, ARCH: $ARCH"
    
    case "$OS" in
        Darwin)
            case "$ARCH" in
                arm64|aarch64) PLATFORM="aarch64-apple-darwin" ;;
                x86_64) PLATFORM="x86_64-apple-darwin" ;;
                *) error "Unsupported macOS architecture: $ARCH"; exit 1 ;;
            esac
            ;;
        Linux)
            case "$ARCH" in
                x86_64) PLATFORM="x86_64-unknown-linux-gnu" ;;
                aarch64|arm64) PLATFORM="aarch64-unknown-linux-gnu" ;;
                armv7l) PLATFORM="armv7-unknown-linux-gnueabihf" ;;
                *) error "Unsupported Linux architecture: $ARCH"; exit 1 ;;
            esac
            ;;
        MINGW*|MSYS*|CYGWIN*)
            error "Windows detected. Please use the PowerShell installer:"
            error "irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex"
            exit 1
            ;;
        *)
            error "Unsupported OS: $OS (architecture: $ARCH)"
            error "Please check https://github.com/vinhnx/vtcode/releases for available builds"
            exit 1
            ;;
    esac
    
    verbose "Platform: $PLATFORM"
    success "Platform detected: $PLATFORM"
}

# Download with error handling and verification
download_file() {
    local url="$1"
    local output="$2"
    
    verbose "Downloading: $url"
    verbose "Output: $output"
    
    # Check if URL exists first (avoid 404 during download)
    if [ "$DOWNLOADER" = "curl" ]; then
        if ! curl -fsSL --head "$url" >/dev/null 2>&1; then
            error "Asset not found at URL: $url"
            error "This may mean:"
            error "  - The release version doesn't exist"
            error "  - The platform $PLATFORM is not supported for this release"
            error "  - The build workflow failed or was disabled"
            return 1
        fi
    fi
    
    if [ "$DOWNLOADER" = "curl" ]; then
        if [ -n "$output" ]; then
            curl -fsSL --progress-bar -o "$output" "$url"
        else
            curl -fsSL "$url"
        fi
    elif [ "$DOWNLOADER" = "wget" ]; then
        if [ -n "$output" ]; then
            wget -q --show-progress -O "$output" "$url"
        else
            wget -q -O - "$url"
        fi
    fi
}

# Check if directory is writable
check_install_dir() {
    local dir="$1"
    if [ -d "$dir" ] && [ -w "$dir" ]; then
        return 0
    else
        return 1
    fi
}

# Install with cargo as fallback
install_with_cargo() {
    if command -v cargo >/dev/null 2>&1; then
        warn "Attempting to install with cargo (this may take a while)..."
        log "Running: cargo install vtcode --version ${VERSION}"
        
        if cargo install vtcode --version "${VERSION}"; then
            success "Successfully installed vtcode v${VERSION} via cargo"
            echo ""
            echo "Note: You may need to add ~/.cargo/bin to your PATH"
            echo "  export PATH=\"~/.cargo/bin:\$PATH\""
            exit 0
        else
            error "Cargo installation failed"
            return 1
        fi
    fi
    return 1
}

# Main logic
main() {
    # Check dependencies first
    check_dependencies
    
    # Detect platform
    detect_platform
    
    # Get version
    if [ -z "$TARGET_VERSION" ] || [ "$TARGET_VERSION" = "latest" ]; then
        log "Checking latest version..."
        VERSION=$(get_latest_version) || exit 1
    else
        # Remove 'v' prefix if present
        VERSION="${TARGET_VERSION#v}"
    fi
    
    if [ -z "$VERSION" ]; then
        error "Failed to determine version"
        exit 1
    fi
    
    # Check if release has assets before proceeding
    if ! check_release_assets "$VERSION" "$PLATFORM"; then
        warn "Missing pre-built binaries for v${VERSION} on $PLATFORM"
        warn "This is likely because:"
        warn "  1. The release workflow is disabled in GitHub Actions"
        warn "  2. The build is still in progress"
        warn "  3. This platform is not supported for this version"
        echo ""
        
        # Try cargo fallback
        if install_with_cargo; then
            exit 0
        fi
        
        error "Installation failed. Available alternatives:"
        error "  1. Build from source: cargo install vtcode"
        error "  2. Download manually: https://github.com/$GITHUB_REPO/releases"
        exit 1
    fi
    
    log "Installing vtcode v$VERSION for $PLATFORM"
    
    # Prepare temp dir
    TEMP_DIR=$(mktemp -d)
    cleanup() {
        verbose "Cleaning up temp directory: $TEMP_DIR"
        rm -rf "$TEMP_DIR"
    }
    trap cleanup EXIT
    verbose "Created temp directory: $TEMP_DIR"
    
    # Download
    FILENAME="vtcode-v${VERSION}-${PLATFORM}.tar.gz"
    URL="https://github.com/$GITHUB_REPO/releases/download/v${VERSION}/${FILENAME}"
    ARCHIVE="$TEMP_DIR/$FILENAME"
    
    log "Downloading..."
    log "  Source: $URL"
    if ! download_file "$URL" "$ARCHIVE"; then
        error "Download failed. Check version or internet connection."
        exit 1
    fi
    
    # Verify download
    if [ ! -f "$ARCHIVE" ] || [ ! -s "$ARCHIVE" ]; then
        error "Downloaded file is empty or missing"
        exit 1
    fi
    
    verbose "Download complete: $(ls -lh "$ARCHIVE" | awk '{print $5}')"
    
    # Extract
    log "Extracting..."
    tar -xzf "$ARCHIVE" -C "$TEMP_DIR"
    
    BINARY_SRC="$TEMP_DIR/vtcode"
    if [ ! -f "$BINARY_SRC" ]; then
        # Handle case where tarball might have a subdirectory
        BINARY_SRC=$(find "$TEMP_DIR" -name vtcode -type f | head -n 1)
    fi
    
    if [ ! -f "$BINARY_SRC" ]; then
        error "Binary not found in archive"
        error "Archive contents:"
        find "$TEMP_DIR" -type f -name "*" 2>/dev/null || true
        exit 1
    fi
    
    verbose "Binary found: $BINARY_SRC"
    
    # Install path
    INSTALL_DIR=""
    if check_install_dir "/usr/local/bin"; then
        INSTALL_DIR="/usr/local/bin"
    elif check_install_dir "/opt/local/bin"; then
        INSTALL_DIR="/opt/local/bin"
    else
        INSTALL_DIR="$HOME/.local/bin"
        mkdir -p "$INSTALL_DIR"
    fi
    
    INSTALL_PATH="$INSTALL_DIR/vtcode"
    
    log "Installing to $INSTALL_PATH..."
    cp "$BINARY_SRC" "$INSTALL_PATH"
    chmod +x "$INSTALL_PATH"
    
    success "Installed vtcode v$VERSION"
    
    # Verify installation
    if ! command -v vtcode >/dev/null 2>&1; then
        echo ""
        echo "Note: Please add $INSTALL_DIR to your PATH:"
        echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
        echo ""
        echo "After updating PATH, run: vtcode --version"
    else
        echo ""
        log "Verifying installation..."
        vtcode --version
    fi
    
    echo ""
    echo "Distribution Channels:"
    echo "  Cargo: cargo install vtcode"
    echo "  Brew:  brew install vinhnx/tap/vtcode"
    echo "  NPM:   npm install -g @vinhnx/vtcode --registry=https://npm.pkg.github.com"
    echo ""
    success "Installation complete!"
}

# Run main function
main "$@"