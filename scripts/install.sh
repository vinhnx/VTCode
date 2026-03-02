#!/bin/sh
set -eu

# VT Code Installer - macOS & Linux
# Usage: curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
#        curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash -s 0.85.0

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
    url="$1"
    output="$2"
    
    if command -v curl >/dev/null 2>&1; then
        # Use -f to fail on HTTP errors, -L to follow redirects
        if curl -fSL --connect-timeout 10 --max-time 120 "$url" -o "$output" 2>/dev/null; then
            return 0
        fi
    elif command -v wget >/dev/null 2>&1; then
        if wget -q --timeout=10 "$url" -O "$output" 2>/dev/null; then
            return 0
        fi
    else
        die "curl or wget is required"
    fi
    
    # Check if URL returns 404 (invalid version or platform)
    if curl -fsIL --connect-timeout 10 "$url" 2>/dev/null | grep -q "404"; then
        die "Version not found: $resolved_version for platform $platform\nCheck available releases: https://github.com/vinhnx/vtcode/releases"
    fi
    
    die "Failed to download from $url"
}

fetch_latest_version() {
    # Try to fetch from GitHub API with retry logic
    attempt=1
    max_attempts=3
    
    while [ $attempt -le $max_attempts ]; do
        # Use GitHub API to get latest release
        release_json=$(curl -fsSL --connect-timeout 10 --max-time 30 \
            "https://api.github.com/repos/vinhnx/VTCode/releases/latest" 2>/dev/null) || true
        
        # Try multiple parsing methods (jq preferred, then grep/sed fallback)
        if command -v jq >/dev/null 2>&1; then
            version=$(echo "$release_json" | jq -r '.tag_name // empty' 2>/dev/null | sed 's/^v//')
        else
            # Fallback: grep for tag_name field, extract version
            version=$(echo "$release_json" | grep -o '"tag_name"[[:space:]]*:[[:space:]]*"[^"]*"' \
                | head -1 | sed 's/.*://' | tr -d '"' | tr -d ' ' | sed 's/^v//')
        fi
        
        # Validate version format (should be like "0.85.2")
        if [ -n "$version" ] && echo "$version" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+'; then
            echo "$version"
            return 0
        fi
        
        # Check for rate limiting
        if echo "$release_json" | grep -q "rate limit"; then
            [ $attempt -lt $max_attempts ] && step "Rate limited, retrying in 2s... ($attempt/$max_attempts)"
        else
            [ $attempt -lt $max_attempts ] && step "Retrying... ($attempt/$max_attempts)"
        fi
        
        attempt=$((attempt + 1))
        [ $attempt -le $max_attempts ] && sleep 2
    done
    
    die "Failed to fetch latest version from GitHub API. Try specifying a version: bash install.sh 0.85.2"
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
            if [ "$os_name" = "apple-darwin" ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = "1" ]; then
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
    url="https://github.com/vinhnx/VTCode/releases/download/${resolved_version}/${asset}"
    
    # Create temp directory
    tmp_dir=$(mktemp -d)
    trap 'rm -rf "$tmp_dir"' EXIT INT TERM
    
    # Download and extract
    step "Downloading..."
    download "$url" "$tmp_dir/$asset"
    
    step "Extracting..."
    if ! tar -xzf "$tmp_dir/$asset" -C "$tmp_dir" 2>/dev/null; then
        die "Failed to extract archive. File may be corrupted."
    fi

    # Verify binary was extracted
    if [ ! -f "$tmp_dir/vtcode" ]; then
        die "Binary not found in archive. Expected: vtcode"
    fi

    # Install
    step "Installing to $INSTALL_DIR"
    mkdir -p "$INSTALL_DIR"
    if ! cp "$tmp_dir/vtcode" "$INSTALL_DIR/vtcode" 2>/dev/null; then
        die "Failed to copy binary to $INSTALL_DIR"
    fi
    chmod 0755 "$INSTALL_DIR/vtcode"
    
    # PATH
    add_to_path

    # Verify installation
    if "$INSTALL_DIR/vtcode" --version >/dev/null 2>&1; then
        version=$("$INSTALL_DIR/vtcode" --version 2>&1 | head -1)
        step "Installation successful! $version"
    else
        step "Installation complete (verification skipped)"
    fi
    
    step "Run: vtcode"
}

# Check dependencies
command -v mktemp >/dev/null 2>&1 || die "mktemp is required"
command -v tar >/dev/null 2>&1 || die "tar is required"

main
