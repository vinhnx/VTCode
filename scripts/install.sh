#!/bin/sh
set -eu

# VT Code Installer - macOS & Linux
# Usage: curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
#        curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash -s v0.85.0

VERSION="${1:-latest}"
INSTALL_DIR="${VTCode_INSTALL_DIR:-$HOME/.local/bin}"

step() { printf '==> %s\n' "$1" >&2; }
die() { printf 'Error: %s\n' "$1" >&2; exit 1; }

# Normalize version: remove 'v' prefix, handle 'latest'
normalize_version() {
    case "$1" in
        "" | latest) echo "latest" ;;
        v*) echo "${1#v}" ;;
        *) echo "$1" ;;
    esac
}

# Download with curl or wget
download() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$2" "$1"
    else
        die "curl or wget is required"
    fi
}

fetch_latest_version() {
    release_json=$(curl -fsSL "https://api.github.com/repos/vinhnx/vtcode/releases/latest")
    version=$(echo "$release_json" | sed -n 's/.*"tag_name":[[:space:]]*"v\([^"]*\)".*/\1/p' | head -n 1)
    [ -z "$version" ] && die "Failed to fetch latest version"
    echo "$version"
}

# Detect platform
get_platform() {
    os=$(uname -s)
    arch=$(uname -m)
    
    case "$os" in
        Darwin) os_name="apple-darwin" ;;
        Linux) os_name="unknown-linux-musl" ;;
        *) die "Unsupported OS: $os. Use install.ps1 for Windows." ;;
    esac
    
    case "$arch" in
        x86_64 | amd64) arch_name="x86_64" ;;
        arm64 | aarch64) 
            arch_name="aarch64"
            # Detect Rosetta 2 on macOS Intel
            if [ "$os" = "darwin" ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = "1" ]; then
                : # Keep aarch64 for Rosetta
            fi
            ;;
        *) die "Unsupported architecture: $arch" ;;
    esac
    
    echo "${arch_name}-${os_name}"
}

# Add to PATH if needed
add_to_path() {
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) return 0 ;;
    esac
    
    profile="$HOME/.profile"
    case "${SHELL:-}" in
        */zsh) profile="$HOME/.zshrc" ;;
        */bash) profile="$HOME/.bashrc" ;;
    esac
    
    line="export PATH=\"$INSTALL_DIR:\$PATH\""
    if [ -f "$profile" ] && grep -qF "$line" "$profile" 2>/dev/null; then
        step "PATH already configured in $profile"
        return 0
    fi
    
    printf '\n# VT Code installer\n%s\n' "$line" >> "$profile"
    step "Added PATH to $profile (restart shell or run: export PATH=\"$INSTALL_DIR:\$PATH\")"
}

# Main
main() {
    step "Installing VT Code"
    
    # Detect platform
    platform=$(get_platform)
    step "Platform: $platform"
    
    # Resolve version
    norm_version=$(normalize_version "$VERSION")
    if [ "$norm_version" = "latest" ]; then
        resolved_version=$(fetch_latest_version)
    else
        resolved_version="$norm_version"
    fi
    step "Version: $resolved_version"
    
    # Build download URL
    asset="vtcode-${resolved_version}-${platform}.tar.gz"
    url="https://github.com/vinhnx/vtcode/releases/download/v${resolved_version}/${asset}"
    
    # Create temp directory
    tmp_dir=$(mktemp -d)
    trap 'rm -rf "$tmp_dir"' EXIT INT TERM
    
    # Download and extract
    step "Downloading..."
    download "$url" "$tmp_dir/$asset"
    
    step "Extracting..."
    tar -xzf "$tmp_dir/$asset" -C "$tmp_dir"
    
    # Install
    step "Installing to $INSTALL_DIR"
    mkdir -p "$INSTALL_DIR"
    cp "$tmp_dir/vtcode" "$INSTALL_DIR/vtcode"
    chmod 0755 "$INSTALL_DIR/vtcode"
    
    # PATH
    add_to_path
    
    step "Done! Run: vtcode"
}

# Check dependencies
command -v mktemp >/dev/null 2>&1 || die "mktemp is required"
command -v tar >/dev/null 2>&1 || die "tar is required"

main
