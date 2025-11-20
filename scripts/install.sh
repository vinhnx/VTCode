#!/bin/bash
# VT Code Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash [version]

set -e

GITHUB_REPO="vinhnx/vtcode"

# Parse arguments
TARGET_VERSION="$1"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

log() { echo -e "${BLUE}➜${NC} $1"; }
success() { echo -e "${GREEN}✓${NC} $1"; }
error() { echo -e "${RED}✗${NC} $1" >&2; }

# Check dependencies
if command -v curl >/dev/null 2>&1; then
    DOWNLOADER="curl"
elif command -v wget >/dev/null 2>&1; then
    DOWNLOADER="wget"
else
    error "Either curl or wget is required"
    exit 1
fi

download_file() {
    local url="$1"
    local output="$2"
    
    if [ "$DOWNLOADER" = "curl" ]; then
        if [ -n "$output" ]; then
            curl -fsSL -o "$output" "$url"
        else
            curl -fsSL "$url"
        fi
    elif [ "$DOWNLOADER" = "wget" ]; then
        if [ -n "$output" ]; then
            wget -q -O "$output" "$url"
        else
            wget -q -O - "$url"
        fi
    fi
}

# Detect platform
detect_platform() {
    OS=$(uname -s)
    ARCH=$(uname -m)
    
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
            error "Unsupported OS: $OS"
            exit 1
            ;;
    esac
}

get_latest_version() {
    if [ "$DOWNLOADER" = "curl" ]; then
        curl -fsSL "https://api.github.com/repos/$GITHUB_REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"v([^"]+)".*/\1/'
    else
        wget -q -O - "https://api.github.com/repos/$GITHUB_REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"v([^"]+)".*/\1/'
    fi
}

# Main logic
detect_platform

if [ -z "$TARGET_VERSION" ] || [ "$TARGET_VERSION" = "latest" ]; then
    log "Checking latest version..."
    VERSION=$(get_latest_version)
else
    VERSION="${TARGET_VERSION#v}" # Remove 'v' prefix if present
fi

if [ -z "$VERSION" ]; then
    error "Failed to determine version"
    exit 1
fi

log "Installing vtcode v$VERSION for $PLATFORM"

# Prepare temp dir
TEMP_DIR=$(mktemp -d)
cleanup() {
    rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

# Download
FILENAME="vtcode-v${VERSION}-${PLATFORM}.tar.gz"
URL="https://github.com/$GITHUB_REPO/releases/download/v${VERSION}/${FILENAME}"
ARCHIVE="$TEMP_DIR/$FILENAME"

log "Downloading..."
if ! download_file "$URL" "$ARCHIVE"; then
    error "Download failed. Check version or internet connection."
    exit 1
fi

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
    exit 1
fi

# Install path
if [ -w /usr/local/bin ]; then
    INSTALL_DIR="/usr/local/bin"
elif [ -w /opt/local/bin ]; then
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

# Verify
if ! command -v vtcode >/dev/null 2>&1; then
    echo ""
    echo "Note: Please add $INSTALL_DIR to your PATH:"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
fi

echo ""
echo "Distribution Channels:"
echo "  Cargo: cargo install vtcode"
echo "  Brew:  brew install vinhnx/tap/vtcode"
echo "  NPM:   npm install -g @vinhnx/vtcode --registry=https://npm.pkg.github.com"
echo ""
