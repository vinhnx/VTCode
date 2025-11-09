#!/bin/bash
# VT Code Installer for macOS and Linux
# Usage: curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash

set -e

# Colors (optional)
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

log() { echo -e "${BLUE}➜${NC} $1"; }
success() { echo -e "${GREEN}✓${NC} $1"; }
error() { echo -e "${RED}✗${NC} $1" >&2; }

# Detect platform
detect_platform() {
  OS=$(uname -s)
  ARCH=$(uname -m)
  
  case "$OS:$ARCH" in
    Darwin:arm64|Darwin:aarch64) PLATFORM="aarch64-apple-darwin" ;;
    Darwin:x86_64) PLATFORM="x86_64-apple-darwin" ;;
    Linux:x86_64) PLATFORM="x86_64-unknown-linux-gnu" ;;
    Linux:aarch64|Linux:arm64) PLATFORM="aarch64-unknown-linux-gnu" ;;
    Linux:armv7l) PLATFORM="armv7-unknown-linux-gnueabihf" ;;
    MINGW*|MSYS*|CYGWIN*) 
      error "Windows detected. Use PowerShell installer:"
      error "irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex"
      exit 1
      ;;
    *) error "Unsupported: $OS $ARCH"; exit 1 ;;
  esac
}

# Get latest version from GitHub
get_version() {
  log "Checking latest version..."
  VERSION=$(curl -fsSL https://api.github.com/repos/vinhnx/vtcode/releases/latest | grep tag_name | sed 's/.*"v\([^"]*\)".*/\1/')
  [ -z "$VERSION" ] && { error "Failed to fetch version"; exit 1; }
  success "Latest: v$VERSION"
}

# Download binary
download() {
  TEMP_DIR=$(mktemp -d)
  URL="https://github.com/vinhnx/vtcode/releases/download/v${VERSION}/vtcode-v${VERSION}-${PLATFORM}.tar.gz"
  ARCHIVE="$TEMP_DIR/vtcode.tar.gz"
  
  log "Downloading..." >&2
  curl -fsSL "$URL" -o "$ARCHIVE" || { error "Download failed"; rm -rf "$TEMP_DIR"; exit 1; }
  
  tar -xzf "$ARCHIVE" -C "$TEMP_DIR" || { error "Extract failed"; rm -rf "$TEMP_DIR"; exit 1; }
  [ -f "$TEMP_DIR/vtcode" ] || { error "Binary not found"; rm -rf "$TEMP_DIR"; exit 1; }
  
  success "Downloaded" >&2
  echo "$TEMP_DIR"
}

# Find installation path
get_install_path() {
  if [ -w /usr/local/bin ]; then
    echo /usr/local/bin
  elif [ -w /opt/local/bin ]; then
    echo /opt/local/bin
  elif mkdir -p "$HOME/.local/bin" 2>/dev/null && [ -w "$HOME/.local/bin" ]; then
    echo "$HOME/.local/bin"
  else
    error "No writable install directory found"
    exit 1
  fi
}

# Install
install_binary() {
  INSTALL_PATH=$(get_install_path)
  log "Installing to $INSTALL_PATH..." >&2
  
  cp "$1/vtcode" "$INSTALL_PATH/vtcode" || { error "Install failed"; exit 1; }
  chmod +x "$INSTALL_PATH/vtcode"
  rm -rf "$1"
  
  success "Installed" >&2
  
  # Verify
  if ! command -v vtcode &>/dev/null; then
    echo ""
    echo "Note: Please add to PATH or restart terminal:"
    echo "  export PATH=\"$INSTALL_PATH:\$PATH\""
  fi
}

# Main
main() {
  echo "VT Code Installer"
  echo "=================="
  echo ""
  
  detect_platform && log "Platform: $PLATFORM"
  get_version
  TEMP_DIR=$(download)
  install_binary "$TEMP_DIR"
  
  echo ""
  success "VT Code installed!"
  echo ""
  echo "Quick start:"
  echo "  export OPENAI_API_KEY=\"sk-...\""
  echo "  vtcode"
}

main "$@"
