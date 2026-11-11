#!/usr/bin/env bash

# VT Code Native Installer
# Downloads and installs the latest VT Code binary from GitHub Releases
# Supports: macOS (Intel/Apple Silicon), Linux, Windows (WSL/Git Bash)

set -euo pipefail

# Check for curl existence
if ! command -v curl >/dev/null 2>&1; then
    printf '✗ curl is required for installation\n' >&2
    exit 1
fi

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Configuration
REPO="vinhnx/vtcode"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BIN_NAME="vtcode"
GITHUB_API="https://api.github.com/repos/$REPO/releases?per_page=10"
GITHUB_RELEASES="https://github.com/$REPO/releases/download"
WITH_COMPLETIONS=0
WITH_DESKTOP=1
WITH_DESKTOP_EXPLICIT=0
DESKTOP_APP_NAME="VT Code.app"
DESKTOP_INSTALL_DIR="${DESKTOP_INSTALL_DIR:-}"
DESKTOP_NEEDS_SUDO=0
# Set by main() before the EXIT trap is installed. The trap handler cannot use
# main()'s locals: they are unset by the time EXIT fires.
VTCODE_WORK_DIR=""

# Ensure INSTALL_DIR exists
mkdir -p "$INSTALL_DIR"

# Hide/show cursor
show_cursor() { printf "\033[?25h"; }
hide_cursor() { printf "\033[?25l"; }
trap show_cursor EXIT INT TERM

# Logging functions (all output to stderr to avoid interfering with command output)
log_info() {
    printf '%b\n' "${BLUE}INFO:${NC} $1" >&2
}

log_success() {
    printf '%b\n' "${GREEN}✓${NC} $1" >&2
}

log_error() {
    printf '%b\n' "${RED}✗${NC} $1" >&2
}

log_warning() {
    printf '%b\n' "${YELLOW}⚠${NC} $1" >&2
}

# Spinner for long running tasks
show_spinner() {
    local pid=$1
    local msg=$2
    local frames='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
    local n=10
    local i=0
    
    # Don't show spinner if not in a TTY
    if [[ ! -t 2 ]]; then
        return 0
    fi

    hide_cursor
    while kill -0 "$pid" 2>/dev/null; do
        local frame="${frames:i:1}"
        printf "\r${BLUE}%s${NC} %s" "$frame" "$msg" >&2
        i=$(( (i + 1) % n ))
        sleep 0.1
    done
    printf "\r\033[K" >&2 # Clear line
    show_cursor
}

# Detect OS and architecture
detect_platform() {
    local os
    local arch
    local uname_s
    local uname_m
    
    uname_s=$(uname -s)
    uname_m=$(uname -m)
    
    case "$uname_s" in
        Darwin)
            os="apple-darwin"
            if [[ "$uname_m" == "arm64" ]]; then
                arch="aarch64"
            else
                arch="x86_64"
            fi
            ;;
        Linux)
            os="unknown-linux-musl"
            if [[ "$uname_m" == "x86_64" ]]; then
                arch="x86_64"
            elif [[ "$uname_m" == "aarch64" || "$uname_m" == "arm64" ]]; then
                arch="aarch64"
            else
                log_error "Unsupported Linux architecture: $uname_m"
                exit 1
            fi
            ;;
        MINGW*|MSYS*)
            log_error "Windows native is not supported. Please use WSL or Git Bash."
            exit 1
            ;;
        *)
            log_error "Unsupported OS: $uname_s"
            exit 1
            ;;
    esac
    
    echo "${arch}-${os}"
}

# Candidate platforms by preference for current host
get_candidate_platforms() {
    local uname_s
    local uname_m
    uname_s=$(uname -s)
    uname_m=$(uname -m)

    case "$uname_s-$uname_m" in
        Linux-x86_64)
            # Prefer musl for broad compatibility; fall back to gnu for older releases.
            echo "x86_64-unknown-linux-musl x86_64-unknown-linux-gnu"
            ;;
        Linux-aarch64|Linux-arm64)
            echo "aarch64-unknown-linux-musl aarch64-unknown-linux-gnu"
            ;;
        Darwin-arm64)
            echo "aarch64-apple-darwin x86_64-apple-darwin"
            ;;
        Darwin-x86_64)
            echo "x86_64-apple-darwin"
            ;;
        *)
            detect_platform
            ;;
    esac
}

# Fetch recent releases info from GitHub API
fetch_recent_releases() {
    local response_file
    response_file=$(mktemp)
    
    # Try to fetch releases with a 10s timeout
    (curl -fsSL --connect-timeout 10 "$GITHUB_API" > "$response_file" 2>/dev/null) &
    local pid=$!
    show_spinner "$pid" "Fetching recent releases..."
    wait "$pid" || true

    local response
    response=$(cat "$response_file")
    rm -f "$response_file"

    if [[ -n "$response" && "$response" != "[]" ]]; then
        echo "$response"
        return 0
    fi

    return 1
}

fetch_latest_release_tag_fallback() {
    local location
    location=$(curl -fsSIL --connect-timeout 10 "https://github.com/$REPO/releases/latest" \
        | tr -d '\r' \
        | awk '/^location:/ {print $2}' \
        | tail -n1)

    if [[ -z "$location" ]]; then
        return 1
    fi

    local tag
    tag=$(echo "$location" | sed -nE 's|.*/tag/([^/]+)$|\1|p')
    if [[ -z "$tag" ]]; then
        return 1
    fi

    echo "$tag"
}

# Extract download URL for the detected platform
get_download_url() {
    local release_tag="$1"
    local platform="$2"

    # Determine file extension based on platform
    local file_ext
    if [[ "$platform" == *"darwin"* ]] || [[ "$platform" == *"linux"* ]]; then
        file_ext="tar.gz"
    else
        file_ext="zip"
    fi

    # Strip 'v' prefix from tag for filename only
    local version_tag="${release_tag#v}"
    local filename="vtcode-${version_tag}-${platform}.${file_ext}"
    echo "${GITHUB_RELEASES}/${release_tag}/${filename}"
}

# Check if a specific version is available for the platform
check_version_available() {
    local version="$1"
    local platform="$2"

    local download_url
    download_url=$(get_download_url "$version" "$platform")

    # Use HEAD request to check availability with a timeout
    if curl -fIsL --connect-timeout 5 "$download_url" > /dev/null 2>&1; then
        return 0
    else
        return 1
    fi
}

# Find and download the most recent release with assets for the given platform
find_latest_release_tag() {
    local all_releases="$1"
    local platform="$2"

    # Extract all tag names using a more robust pattern
    local tags
    tags=$(echo "$all_releases" | grep -oE '"tag_name":\s*"[^"]+"' | cut -d'"' -f4)

    if [[ -z "$tags" ]]; then
        return 1
    fi

    # Iterate through tags to find one with assets
    for tag in $tags; do
        if [[ -n "$tag" ]]; then
            if check_version_available "$tag" "$platform"; then
                echo "$tag"
                return 0
            fi
        fi
    done

    return 1
}

# Download binary with progress
download_binary() {
    local url="$1"
    local output_file="$2"
    
    log_info "Downloading binary..."
    
    # Use curl -# for a simple progress bar
    if ! curl -fSL -# -o "$output_file" "$url"; then
        log_error "Failed to download binary from $url"
        exit 1
    fi
}

# Verify checksum if available
verify_checksum() {
    local binary_file="$1"
    local release_tag="$2"
    local release_filename="${3:-}"
    local strict="${4:-0}"

    log_info "Verifying binary integrity..."

    local basename_file
    if [[ -n "$release_filename" ]]; then
        basename_file="$release_filename"
    else
        basename_file=$(basename "$binary_file")
    fi

    local temp_checksums
    temp_checksums=$(mktemp)

    # Try to download checksums.txt first (aggregated file from release)
    local checksums_url="${GITHUB_RELEASES}/${release_tag}/checksums.txt"
    local expected_checksum=""

    if curl -fsSL -o "$temp_checksums" "$checksums_url" 2>/dev/null; then
        # checksums.txt format: <hash>  <filename> (two spaces, from sha256sum/shasum)
        # Use grep with fixed string match and extract the hash (first field)
        expected_checksum=$(grep -F "$basename_file" "$temp_checksums" 2>/dev/null | awk '{print $1}' | head -n1 || true)
    fi

    # If not found in checksums.txt, try individual .sha256 file (backwards compat)
    if [[ -z "$expected_checksum" ]]; then
        local sha_url="${GITHUB_RELEASES}/${release_tag}/${basename_file%.tar.gz}.sha256"
        if curl -fsSL -o "$temp_checksums" "$sha_url" 2>/dev/null; then
            expected_checksum=$(awk '{print $1}' "$temp_checksums" | head -n1)
        fi
    fi

    rm -f "$temp_checksums"
    
    if [[ -z "$expected_checksum" ]]; then
        [[ "$strict" -eq 1 ]] && log_error "Checksum not found for $basename_file; refusing to install it unverified" && return 1
        log_warning "Checksum not found for $basename_file, skipping verification"
        return 0
    fi
    
    # Compute actual checksum
    local actual_checksum=""
    if command -v sha256sum &> /dev/null; then
        actual_checksum=$(sha256sum "$binary_file" | awk '{print $1}')
    elif command -v shasum &> /dev/null; then
        actual_checksum=$(shasum -a 256 "$binary_file" | awk '{print $1}')
    elif command -v sha256 &> /dev/null; then
        actual_checksum=$(sha256 -q "$binary_file")
    else
        [[ "$strict" -eq 1 ]] && log_error "No checksum tool (sha256sum/shasum/sha256) found; refusing to install unverified" && return 1
        log_warning "No checksum tool (sha256sum/shasum/sha256) found, skipping verification"
        return 0
    fi
    
    if [[ "$actual_checksum" != "$expected_checksum" ]]; then
        log_error "Checksum mismatch for $basename_file!"
        log_error "Expected: $expected_checksum"
        log_error "Got:      $actual_checksum"
        exit 1
    fi
    
    log_success "Checksum verified: $expected_checksum"
}

# Extract binary from archive.
# Sets EXTRACTED_BINARY_PATH and EXTRACTED_TEMP_DIR as global variables.
extract_binary() {
    local archive="$1"
    local platform="$2"
    local temp_dir
    temp_dir=$(mktemp -d)
    
    log_info "Extracting binary..."
    
    if [[ "$platform" == *"darwin"* ]] || [[ "$platform" == *"linux"* ]]; then
        tar -xzf "$archive" -C "$temp_dir"
    else
        # Windows/MSVC - requires 7z or unzip
        if command -v 7z &> /dev/null; then
            7z x "$archive" -o"$temp_dir" > /dev/null
        elif command -v unzip &> /dev/null; then
            unzip -q "$archive" -d "$temp_dir"
        else
            log_error "Neither 7z nor unzip found. Cannot extract Windows binary."
            exit 1
        fi
    fi
    
    # Find the binary
    local binary_path
    binary_path=$(find "$temp_dir" -type f \( -name "$BIN_NAME" -o -name "$BIN_NAME.exe" \) | head -1)
    
    if [[ -z "$binary_path" ]]; then
        log_error "Binary not found in archive"
        exit 1
    fi
    
    EXTRACTED_BINARY_PATH="$binary_path"
    EXTRACTED_TEMP_DIR="$temp_dir"
}

# Install binary to target directory
install_binary() {
    local source="$1"
    local target="$2"
    
    log_info "Installing to $target..."
    
    # Make source executable
    chmod +x "$source"
    
    # Copy to installation directory
    if ! cp "$source" "$target"; then
        log_error "Failed to install binary to $target"
        log_info "You may need to use: sudo cp $source $target"
        exit 1
    fi
    
    chmod +x "$target"
    log_success "Binary installed to $target"
}

install_man_and_completions() {
    local archive_source="$1"
    local install_dir="$2"

    local man_source_dir="$archive_source/man/man1"
    local completions_source="$archive_source/completions"

    if [[ ! -d "$man_source_dir" && ! -d "$completions_source" ]]; then
        log_warning "Archive does not include man page or shell completions"
        return 0
    fi

    local share_dir
    share_dir="$(dirname "$install_dir")/share"

    if [[ -d "$man_source_dir" ]]; then
        local man_dir="$share_dir/man/man1"
        mkdir -p "$man_dir"
        local man_page
        local man_count=0
        for man_page in "$man_source_dir"/*.1; do
            [[ -f "$man_page" ]] || continue
            cp "$man_page" "$man_dir/"
            man_count=$((man_count + 1))
        done
        log_success "Installed $man_count man pages to $man_dir"
    fi

    if [[ -d "$completions_source" ]]; then
        local spec label src_rel dest_rel dest_name src_file dest_dir
        for spec in "Bash|bash/vtcode|bash-completion/completions|vtcode" \
            "Zsh|zsh/_vtcode|zsh/site-functions|_vtcode" \
            "Fish|fish/vtcode.fish|fish/vendor_completions.d|vtcode.fish"; do
            IFS='|' read -r label src_rel dest_rel dest_name <<<"$spec"
            src_file="$completions_source/$src_rel"
            [[ -f "$src_file" ]] || continue
            dest_dir="$share_dir/$dest_rel"
            mkdir -p "$dest_dir"
            cp "$src_file" "$dest_dir/$dest_name"
            log_success "$label completions installed to $dest_dir/$dest_name"
        done
    fi

    echo ""
    log_info "To enable completions, add to your shell config:"
    log_info "  Bash:  source $share_dir/bash-completion/completions/vtcode"
    log_info "  Zsh:   fpath=($share_dir/zsh/site-functions \$fpath)"
    log_info "  Fish:  fish_add_path $share_dir/fish/vendor_completions.d"
}

# Print the installed desktop bundle path when present.
find_installed_desktop_app() {
    local candidate
    for candidate in "$HOME/Applications/$DESKTOP_APP_NAME" "/Applications/$DESKTOP_APP_NAME"; do
        if [[ -d "$candidate" && -f "$candidate/Contents/Info.plist" ]]; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

# OS gate for the desktop app. Applies to both install paths: the prebuilt
# release asset and the from-source build both require macOS 15+.
check_desktop_os() {
    if [[ "$(uname -s)" != "Darwin" ]]; then
        log_error "The VT Code desktop app requires macOS."
        return 1
    fi
    local macos_version macos_major
    macos_version="$(sw_vers -productVersion 2>/dev/null || echo "")"
    macos_major="${macos_version%%.*}"
    if [[ ! "$macos_major" =~ ^[0-9]+$ ]] || (( macos_major < 15 )); then
        log_error "VT Code Desktop requires macOS 15 or newer; found ${macos_version:-unknown}."
        return 1
    fi
}

# Fail-fast checks for a from-source desktop build. Returns nonzero (never
# exits) so the caller can report context.
check_desktop_prerequisites() {
    check_desktop_os || return 1
    local missing=()
    local tool
    for tool in git cargo codesign ditto sips iconutil plutil; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            missing+=("$tool")
        fi
    done
    if (( ${#missing[@]} > 0 )); then
        log_error "Missing required tools for the desktop build: ${missing[*]}"
        log_info "Install the Xcode command line tools (xcode-select --install), Rust (https://rustup.rs), and git, then retry."
        return 1
    fi
    return 0
}

# Print the first published desktop asset for the tag (nonzero when none). ARM probes native then Intel; Intel probes native only.
find_desktop_release_asset() {
    local release_tag="$1"
    local version="${release_tag#v}"
    local targets=("x86_64-apple-darwin")
    if [[ "$(uname -m)" == "arm64" ]]; then
        targets=("aarch64-apple-darwin" "x86_64-apple-darwin")
    fi
    local target asset
    for target in "${targets[@]}"; do
        asset="vtcode-desktop-${version}-${target}.zip"
        if curl -fIsL --connect-timeout 5 "${GITHUB_RELEASES}/${release_tag}/${asset}" >/dev/null 2>&1; then
            echo "$asset"
            return 0
        fi
    done
    return 1
}

# Install from a prebuilt release asset (curl, shasum, and ditto only).
install_desktop_release_asset() {
    local release_tag="$1" asset_name="$2" work_dir="$3"
    local archive_file="$work_dir/vtcode-desktop.zip"
    # Inline download: download_binary exits the installer, but desktop
    # failure must stay best-effort (explicit --with-desktop fails via caller).
    log_info "Downloading prebuilt desktop app: $asset_name"
    if ! curl -fSL -# -o "$archive_file" "${GITHUB_RELEASES}/${release_tag}/${asset_name}"; then
        log_error "Failed to download the desktop app ($asset_name)"
        return 1
    fi
    # Subshell: verify_checksum exits on mismatch instead of returning. Strict
    # (4th arg): a missing checksum or checksum tool fails instead of warning.
    if ! (verify_checksum "$archive_file" "$release_tag" "$asset_name" 1); then
        return 1
    fi

    local extract_dir="$work_dir/desktop-extract"
    # Start clean: a shared work dir may hold a previous extraction whose
    # FIRST-LAUNCH.txt would wrongly mark this build as unsigned.
    rm -rf "$extract_dir"
    mkdir -p "$extract_dir"
    log_info "Extracting desktop app..."
    if ! ditto -x -k "$archive_file" "$extract_dir"; then
        log_error "Failed to extract the desktop app archive"
        return 1
    fi

    local staged_app="$extract_dir/$DESKTOP_APP_NAME"
    if [[ ! -f "$staged_app/Contents/Info.plist" || ! -f "$staged_app/Contents/MacOS/vtcode-desktop" ]]; then
        log_error "Desktop archive is not a valid app bundle"
        return 1
    fi
    install_staged_desktop_app "$staged_app" || return 1
    # Notarized builds omit FIRST-LAUNCH.txt and need no approval steps.
    if [[ -f "$extract_dir/FIRST-LAUNCH.txt" ]]; then
        print_gatekeeper_note
    fi
    log_info "Open it anytime with: vtcode desktop"
    return 0
}

# One-time macOS security approval for ad hoc builds (mirrors FIRST-LAUNCH.txt).
# macOS 15+ has no right-click > Open bypass, and Open Anyway only appears for
# about an hour after a blocked launch; both facts belong in the steps.
print_gatekeeper_note() {
    log_warning "ONE-TIME macOS SECURITY STEP REQUIRED. This build is not notarized, so macOS blocks the first launch.\n  1. Open the app from Applications so macOS records the block\n  2. Click Open Anyway in System Settings > Privacy & Security\n  3. Open the app again: vtcode desktop\nOpen Anyway only appears for about an hour after a blocked launch."
}

# Copy a staged app bundle to the desktop location and verify it. Shared by
# the prebuilt-asset and from-source install paths. Elevates with sudo when
# the destination is not user-writable (system-wide /Applications install).
install_staged_desktop_app() {
    local staged_app="$1" applications_dir="${DESKTOP_INSTALL_DIR:-$HOME/Applications}"
    local target="$applications_dir/$DESKTOP_APP_NAME"
    [[ -w "$applications_dir" ]] || [[ ! -e "$applications_dir" && -w "$(dirname "$applications_dir")" ]] \
        || DESKTOP_NEEDS_SUDO=1
    log_info "Installing to $target..."
    run_desktop_install_step mkdir -p -- "$applications_dir" || return 1
    # Remove any previous install first so stale bundle files never survive.
    run_desktop_install_step rm -rf -- "$target" || return 1
    if ! run_desktop_install_step ditto "$staged_app" "$target"; then
        log_error "Failed to install the desktop app to $target"
        return 1
    fi
    if [[ ! -f "$target/Contents/Info.plist" || ! -f "$target/Contents/MacOS/vtcode-desktop" ]]; then
        log_error "Desktop install verification failed: $target is not a valid app bundle"
        return 1
    fi
    log_success "Desktop app installed to $target"
}

run_desktop_install_step() { if (( DESKTOP_NEEDS_SUDO )); then sudo "$@"; else "$@"; fi; }

# Install at the release tag (prebuilt asset first, from-source fallback). Never prompts.
install_desktop_app() {
    local release_tag="$1"
    local work_dir="$2"

    if ! check_desktop_os; then
        return 1
    fi

    local existing
    if existing=$(find_installed_desktop_app); then
        log_success "Desktop app already installed at $existing"
        log_info "Open it with: vtcode desktop"
        return 0
    fi

    local desktop_asset
    if desktop_asset=$(find_desktop_release_asset "$release_tag"); then
        install_desktop_release_asset "$release_tag" "$desktop_asset" "$work_dir" || return 1
        return 0
    fi

    if ! check_desktop_prerequisites; then
        return 1
    fi

    log_warning "No prebuilt desktop app found for this host; falling back to a from-source build."
    log_info "Installing VT Code Desktop ($release_tag)..."
    log_warning "The desktop build compiles from source and can take several minutes."

    local source_dir="$work_dir/vtcode-desktop-src"
    log_info "Cloning $REPO at $release_tag..."
    if ! git clone --depth 1 --branch "$release_tag" "https://github.com/$REPO.git" "$source_dir"; then
        log_error "Failed to clone $REPO at $release_tag"
        return 1
    fi

    local staged_app="$work_dir/$DESKTOP_APP_NAME"
    log_info "Building and packaging the desktop app (this takes a while)..."
    if ! bash "$source_dir/scripts/package-desktop-macos.sh" "$staged_app"; then
        log_error "Desktop packaging failed"
        return 1
    fi

    install_staged_desktop_app "$staged_app" || return 1
    print_gatekeeper_note
    log_info "After that, open it anytime with: vtcode desktop"
    return 0
}

# Read a yes/no answer (0 = yes); $1 prompt, $2 default; stdin wired by caller; empty/EOF takes default.
ask_yes_no() {
    local prompt_text="$1" default="$2" reply="" hint="[Y/n]"
    [[ "$default" == "n" ]] && hint="[y/N]"
    printf '%s %s ' "$prompt_text" "$hint" >&2
    IFS= read -r reply || true

    reply=$(printf '%s' "$reply" | tr '[:upper:]' '[:lower:]' | tr -d '[:space:]')
    [[ -z "$reply" ]] && reply="$default"
    [[ "$reply" == "y" || "$reply" == "yes" ]]
}

# Ask yes/no on the best input (stdin TTY, /dev/tty for curl|bash, else default).
ask_interactive() {
    if [[ -t 0 ]]; then
        ask_yes_no "$1" "$2"
    elif [[ -r /dev/tty && -w /dev/tty ]]; then
        ask_yes_no "$1" "$2" </dev/tty
    else
        [[ "$2" == "y" ]]
    fi
}

resolve_desktop_choice() { # 0=install, 1=declined, 2=no prompt needed.
    [[ "$WITH_DESKTOP" -eq 1 && "$WITH_DESKTOP_EXPLICIT" -eq 0 && "$(uname -s)" == "Darwin" ]] || return 2
    ! find_installed_desktop_app >/dev/null 2>&1 || return 2
    ask_interactive "Install the VT Code desktop app?" "y"
}

resolve_desktop_location() { # Presets and explicit flags skip the question.
    [[ -z "$DESKTOP_INSTALL_DIR" ]] || return 0
    DESKTOP_INSTALL_DIR="$HOME/Applications"
    [[ "$WITH_DESKTOP_EXPLICIT" -eq 0 ]] && ask_interactive "Install for all users (/Applications, may need sudo)?" "n" && DESKTOP_INSTALL_DIR="/Applications" || true
}

# Install the desktop app when wanted. Explicit --with-desktop fails hard when the
# app is unavailable; the macOS default stays best-effort so a missing asset or
# toolchain never breaks the CLI install. Returns nonzero only for a failed explicit request.
maybe_install_desktop_app() {
    local release_tag="$1"
    local work_dir="$2"

    if [[ "$(uname -s)" != "Darwin" ]]; then
        if [[ "$WITH_DESKTOP_EXPLICIT" -eq 1 ]]; then
            log_error "--with-desktop requires macOS; the desktop app is not available on this platform."
            return 1
        fi
        return 0
    fi
    if install_desktop_app "$release_tag" "$work_dir"; then
        return 0
    fi
    if [[ "$WITH_DESKTOP_EXPLICIT" -eq 1 ]]; then
        return 1
    fi
    log_warning "Continuing without the desktop app (pass --without-desktop to silence this, or re-run with --with-desktop to retry)."
    return 0
}

# Check if install directory is in PATH
check_path() {
    local install_path="$1"
    
    if [[ ":$PATH:" == *":$install_path:"* ]]; then
        return 0
    fi
    
    return 1
}

# Add install directory to PATH (for common shells)
add_to_path() {
    local install_path="$1"
    local shell_name
    shell_name=$(basename "$SHELL")
    
    log_warning "Installation directory is not in PATH"
    log_info "Add the following to your shell configuration file:"
    echo ""
    echo "  export PATH=\"$install_path:\$PATH\""
    echo ""
    
    case "$shell_name" in
        bash) echo "Add to: ~/.bashrc or ~/.bash_profile" ;;
        zsh) echo "Add to: ~/.zshrc" ;;
        fish) echo "Add to: ~/.config/fish/config.fish (using: set -gx PATH $install_path \$PATH)" ;;
        *) echo "Add to: ~/.${shell_name}rc or equivalent" ;;
    esac
}

# Cleanup temporary files (EXIT/INT/TERM handler; registered by name so the work
# dir is quoted at cleanup time instead of expanded into the trap command).
# Only the recorded work dir is removed: /tmp/vtcode-* also matches live temp
# dirs owned by other sessions, so it must never be glob-deleted here.
cleanup() {
    if [[ -n "${VTCODE_WORK_DIR:-}" ]]; then
        rm -rf -- "${VTCODE_WORK_DIR:?}" || true
    fi
    show_cursor
}

# Main installation flow
main() {
    if [[ "$WITH_DESKTOP_EXPLICIT" -eq 1 && "$(uname -s)" != "Darwin" ]]; then
        log_error "--with-desktop requires macOS; the desktop app is not available on this platform."
        exit 1
    fi

    log_info "VT Code Native Installer"
    echo ""

    # Detect preferred platform
    local current_platform
    current_platform=$(detect_platform)
    log_info "Detected platform: $current_platform"

    # Create temporary directory for downloads
    local temp_dir
    temp_dir=$(mktemp -d 2>/dev/null || mktemp -d -t 'vtcode')
    VTCODE_WORK_DIR="$temp_dir"
    trap cleanup EXIT INT TERM

    # Fetch recent releases
    local all_releases
    if all_releases=$(fetch_recent_releases); then
        log_info "Fetched release metadata from GitHub API"
    else
        log_warning "Failed to fetch release metadata from GitHub API"
        log_info "Falling back to the latest release redirect endpoint"
        local fallback_tag=""
        fallback_tag=$(fetch_latest_release_tag_fallback || true)
        if [[ -z "$fallback_tag" ]]; then
            log_error "Could not resolve a release tag from GitHub"
            log_info "Ensure github.com is reachable, or install via: cargo install vtcode"
            exit 1
        fi
        all_releases="[{\"tag_name\":\"$fallback_tag\"}]"
    fi

    # Find the most recent release with assets for this platform
    local release_tag=""
    local selected_platform=""
    local candidate_platforms
    candidate_platforms=$(get_candidate_platforms)

    for candidate in $candidate_platforms; do
        local tag_file
        tag_file="$temp_dir/tag_$candidate"
        (find_latest_release_tag "$all_releases" "$candidate" > "$tag_file" 2>/dev/null) &
        local pid=$!
        show_spinner "$pid" "Checking for compatible binaries for $candidate..."
        wait "$pid" || true

        local candidate_tag
        candidate_tag=$(cat "$tag_file" 2>/dev/null || true)
        if [[ -n "$candidate_tag" ]]; then
            release_tag="$candidate_tag"
            selected_platform="$candidate"
            break
        fi
    done

    if [[ -z "$release_tag" || -z "$selected_platform" ]]; then
        log_error "No releases with compatible binaries found for platform: $current_platform"
        log_info "Available platforms for this host were: $candidate_platforms"
        exit 1
    fi

    platform="$selected_platform"
    log_success "Found compatible version: $release_tag ($platform)"

    # Download binary
    local archive_file="$temp_dir/vtcode-binary.tar.gz"
    local download_url
    download_url=$(get_download_url "$release_tag" "$platform")
    
    download_binary "$download_url" "$archive_file"
    log_success "Downloaded successfully"

    # Verify checksum
    verify_checksum "$archive_file" "$release_tag" "$(basename "$download_url")"

    # Extract binary
    extract_binary "$archive_file" "$platform"
    local binary_path="$EXTRACTED_BINARY_PATH"
    local extract_dir="$EXTRACTED_TEMP_DIR"

    # Install binary
    local target_path="$INSTALL_DIR/$BIN_NAME"
    install_binary "$binary_path" "$target_path"

    if [[ "$WITH_COMPLETIONS" -eq 1 ]]; then
        install_man_and_completions "$extract_dir" "$INSTALL_DIR"
    fi

    # Check if in PATH
    if ! check_path "$INSTALL_DIR"; then
        add_to_path "$INSTALL_DIR"
    fi

    echo ""
    log_success "Installation complete!"
    log_info "VT Code is ready to use"
    echo ""

    # Test installation
    if "$target_path" --version &>/dev/null; then
        log_success "Version check passed: $($target_path --version)"
    else
        log_warning "Could not verify installation, but binary appears to be installed"
    fi

    local desktop_choice=0; resolve_desktop_choice || desktop_choice=$?
    if (( desktop_choice == 1 )); then
        WITH_DESKTOP=0
        log_info "Skipping the desktop app (re-run with --with-desktop to install it later)."
    fi

    if [[ "$WITH_DESKTOP" -eq 1 ]]; then
        echo ""
        if ! find_installed_desktop_app >/dev/null 2>&1; then
            resolve_desktop_location
        fi
        if ! maybe_install_desktop_app "$release_tag" "$temp_dir"; then
            log_error "Desktop installation failed, but the CLI is installed and ready at $target_path"
            exit 1
        fi
    fi

    echo ""
    log_success "Next steps:"
    log_info "  vtcode init          # scaffold project config and AGENTS.md"
    log_info "  vtcode               # launch interactive TUI"
    log_info "  vtcode --help        # see all commands"
    echo ""
    log_info "Docs: https://vtcode.dev/docs"
}

# Show usage
show_usage() {
    cat <<'USAGE'
VT Code Native Installer

Usage: ./install.sh [options]

Options:
  -d, --dir DIR      Installation directory (default: ~/.local/bin)
  --with-completions Install man page and shell completions (bash/zsh/fish)
  --with-desktop     Install the macOS desktop app without prompting (fails if unavailable)
  --without-desktop  Skip the macOS desktop app without prompting
  -h, --help         Show this help message

Without either desktop flag, the installer asks when interactive (default:
install). The app installs from a prebuilt release asset (no toolchain);
releases without one fall back to a from-source build.

Examples:
  ./install.sh                           # Install VT Code (asks about the desktop app on macOS)
  ./install.sh --with-completions        # Install with man page and shell completions
  ./install.sh --with-desktop            # Require the macOS desktop app (fail if unavailable)
  ./install.sh --without-desktop         # Skip the macOS desktop app
  ./install.sh --dir /usr/local/bin    # Install to /usr/local/bin (may need sudo)

Environment variables:
  INSTALL_DIR        Set installation directory
USAGE
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -d|--dir)
            if [[ $# -lt 2 ]]; then
                log_error "-d/--dir requires a directory argument"
                show_usage
                exit 1
            fi
            INSTALL_DIR="$2"
            shift 2
            ;;
        -h|--help)
            show_usage
            exit 0
            ;;
        --with-completions)
            WITH_COMPLETIONS=1
            shift
            ;;
        --with-desktop)
            WITH_DESKTOP=1
            WITH_DESKTOP_EXPLICIT=1
            shift
            ;;
        --without-desktop)
            WITH_DESKTOP=0
            WITH_DESKTOP_EXPLICIT=0
            shift
            ;;
        *)
            log_error "Unknown option: $1"
            show_usage
            exit 1
            ;;
    esac
done

# Allow tests to source this file for its helper functions without installing.
# BASH_SOURCE is unset when run via `curl ... | bash` (stdin); that is a real
# install, not a source, so main must still run. The `:-` default keeps this
# safe under `set -u`.
if [[ -z "${BASH_SOURCE[0]:-}" || "${BASH_SOURCE[0]:-}" == "$0" ]]; then
    main
fi
