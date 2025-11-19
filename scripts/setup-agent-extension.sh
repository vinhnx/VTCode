#!/bin/bash
# Setup script for VT Code Agent Server Extension development/testing
# This script helps create a local build for Zed extension installation

set -e

echo "VT Code Agent Server Extension - Development Setup"

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ] || [ ! -d "zed-extension" ]; then
    echo "Error: This script must be run from the root VT Code directory"
    exit 1
fi

CURRENT_DIR=$(pwd)

# Build VT Code
echo "Building VT Code..."
cargo build --release

# Get version from Cargo.toml
VERSION=$(grep '^version = ' Cargo.toml | head -n1 | cut -d'"' -f2)
echo "Detected version: $VERSION"

# Create test package for the current platform
echo "Creating test package for current platform..."
PLATFORM=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

# Normalize architecture names
if [[ "$ARCH" == "arm64" ]]; then
    ARCH="aarch64"
elif [[ "$ARCH" == "x86_64" ]]; then
    ARCH="x86_64"
fi

# Determine extension for archive based on OS
if [[ "$PLATFORM" == "darwin" ]]; then
    PLATFORM_FULL="darwin-$ARCH"
    EXT="tar.gz"
elif [[ "$PLATFORM" == "linux" ]]; then
    PLATFORM_FULL="unknown-linux-musl-$ARCH"
    EXT="tar.gz"
else
    PLATFORM_FULL="pc-windows-msvc-$ARCH"
    EXT="zip"
fi

PACKAGE_DIR="vtcode-$VERSION-$PLATFORM_FULL"
echo "Creating package directory: $PACKAGE_DIR"

mkdir -p "$PACKAGE_DIR"
cp "target/release/vtcode*" "$PACKAGE_DIR/" 2>/dev/null || cp "target/release/vtcode" "$PACKAGE_DIR/"

cd "$PACKAGE_DIR" || exit 1

# Create archive
if [[ "$EXT" == "zip" ]]; then
    # On Windows, create zip
    zip -r "../vtcode-$VERSION-$PLATFORM_FULL.zip" vtcode*
    ARCHIVE_NAME="../vtcode-$VERSION-$PLATFORM_FULL.zip"
else
    # On Unix-like systems, create tar.gz
    tar -czf "../vtcode-$VERSION-$PLATFORM_FULL.tar.gz" vtcode*
    ARCHIVE_NAME="../vtcode-$VERSION-$PLATFORM_FULL.tar.gz"
fi

cd ..

echo "Created test archive: $(basename $ARCHIVE_NAME)"
CHECKSUM=$(shasum -a 256 "$ARCHIVE_NAME" | cut -d ' ' -f 1)
echo "SHA-256 checksum: $CHECKSUM"

echo ""
echo "✅ Test archive created successfully!"
echo "   Archive: $(pwd)/$(basename $ARCHIVE_NAME)"
echo "   Checksum: $CHECKSUM"
echo "   Platform: $PLATFORM_FULL"
echo ""
echo "📝 To update extension.toml for local development:"
echo "   1. Update the appropriate [agent_servers.vtcode.targets.$PLATFORM_FULL] section"
echo "   2. Update 'archive' field to point to your local archive (or upload to a server)"
echo "   3. Update 'sha256' field with the value above"
echo "   4. Install extension in Zed as dev extension: Command Palette → 'zed: install dev extension'"
echo ""
echo "🌐 For remote hosting, upload the archive to a web-accessible location"
echo "   and update extension.toml with the public URL."