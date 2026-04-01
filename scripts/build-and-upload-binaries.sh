#!/bin/bash

# VT Code Binary Build and Upload Script
# This script builds binaries locally and uploads them to GitHub Releases

set -e

# Source common utilities
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

BUILD_TOOL="cargo"
TARGET_ENV_ASSIGNMENTS=()
DRY_RUN=false

package_release_archive_with_ghostty() {
    local target=$1
    local dist_dir=$2
    local archive_name=$3
    local binary_name="${4:-vtcode}"
    local release_dir="target/$target/release"

    bash "$SCRIPT_DIR/prepare-ghostty-vt-release-assets.sh" "$target" "$release_dir"

    rm -rf "$dist_dir/ghostty-vt"
    cp "$release_dir/$binary_name" "$dist_dir/$binary_name"
    cp -R "$release_dir/ghostty-vt" "$dist_dir/ghostty-vt"
    (
        cd "$dist_dir"
        tar -czf "$archive_name" "$binary_name" ghostty-vt
        rm -rf "$binary_name" ghostty-vt
    )
}

# Function to check if required tools are available
check_dependencies() {
    local missing_tools=()

    if ! command -v cargo &> /dev/null; then
        missing_tools+=("cargo")
    fi

    if ! command -v rustc &> /dev/null; then
        missing_tools+=("rustc")
    fi

    if ! command -v zig &> /dev/null; then
        missing_tools+=("zig")
    fi

    if ! command -v curl &> /dev/null; then
        missing_tools+=("curl")
    fi

    if ! command -v gh &> /dev/null; then
        missing_tools+=("gh (GitHub CLI)")
    fi

    if ! command -v shasum &> /dev/null && ! command -v sha256sum &> /dev/null; then
        missing_tools+=("shasum or sha256sum")
    fi

    if [ ${#missing_tools[@]} -ne 0 ]; then
        print_error "Missing required tools: ${missing_tools[*]}"
        print_info "Please install the missing tools and try again"
        exit 1
    fi

    # Verify GitHub CLI authentication and attempt to switch to vinhnx account
    if ! gh auth status >/dev/null 2>&1; then
        print_info "Attempting to authenticate with GitHub CLI (vinhnx account)..."
        if gh auth switch -u vinhnx 2>/dev/null; then
            print_success "Switched to GitHub account vinhnx"
        else
            print_error "GitHub CLI is not authenticated. Please run: gh auth login"
            exit 1
        fi
    fi

    # Skip the refresh step that causes hangs, assuming user has proper scopes
    print_warning "Skipping GitHub CLI scopes refresh (may need manual refresh if issues occur)"

    print_success "All required tools are available"
}

install_rust_targets() {
    print_info "Checking and installing required Rust targets..."

    local targets=$(rustup target list --installed)

    # macOS targets
    if ! echo "$targets" | grep -q "x86_64-apple-darwin"; then
        print_info "Installing x86_64-apple-darwin target..."
        rustup target add x86_64-apple-darwin
    fi

    if ! echo "$targets" | grep -q "aarch64-apple-darwin"; then
        print_info "Installing aarch64-apple-darwin target..."
        rustup target add aarch64-apple-darwin
    fi

    # Linux targets (optional)
    if ! echo "$targets" | grep -q "x86_64-unknown-linux-gnu"; then
        print_info "Attempting to install x86_64-unknown-linux-gnu target..."
        rustup target add x86_64-unknown-linux-gnu || print_warning "Failed to add Linux target"
    fi

    print_success "Rust targets check completed"
}

configure_target_env() {
    local target=$1
    TARGET_ENV_ASSIGNMENTS=()

    if [[ "$target" == *"-apple-"* ]]; then
        # macOS targets
        local openssl_prefix=""
        if command -v brew &> /dev/null; then
            openssl_prefix=$(brew --prefix openssl@3 2>/dev/null || true)
        fi

        if [[ -n "$openssl_prefix" ]]; then
            TARGET_ENV_ASSIGNMENTS+=("OPENSSL_DIR=$openssl_prefix")
            TARGET_ENV_ASSIGNMENTS+=("OPENSSL_LIB_DIR=$openssl_prefix/lib")
            TARGET_ENV_ASSIGNMENTS+=("OPENSSL_INCLUDE_DIR=$openssl_prefix/include")

            local pkg_config_path="$openssl_prefix/lib/pkgconfig"
            if [[ -n "${PKG_CONFIG_PATH:-}" ]]; then
                pkg_config_path+=":${PKG_CONFIG_PATH}"
            fi
            TARGET_ENV_ASSIGNMENTS+=("PKG_CONFIG_PATH=$pkg_config_path")
        fi

        TARGET_ENV_ASSIGNMENTS+=("MACOSX_DEPLOYMENT_TARGET=11.0")
    elif [[ "$target" == *"-linux-"* ]]; then
        # Linux targets - don't set host-specific OpenSSL paths for cross-compilation
        # Cross compilation should handle this automatically
        if command -v cross &>/dev/null; then
            # When using cross, we don't need to set these variables
            :
        else
            # For direct cross-compilation, we might need to set different variables
            local openssl_prefix=""
            if command -v brew &> /dev/null; then
                openssl_prefix=$(brew --prefix openssl@3 2>/dev/null || true)
            fi

            if [[ -n "$openssl_prefix" ]]; then
                # Only set these if we're not using cross
                TARGET_ENV_ASSIGNMENTS+=("OPENSSL_DIR=$openssl_prefix")
                TARGET_ENV_ASSIGNMENTS+=("OPENSSL_LIB_DIR=$openssl_prefix/lib")
                TARGET_ENV_ASSIGNMENTS+=("OPENSSL_INCLUDE_DIR=$openssl_prefix/include")
            fi
        fi
    fi
}

build_with_tool() {
    local target=$1
    TARGET_ENV_ASSIGNMENTS=()
    configure_target_env "$target"

    print_info "Building for $target using $BUILD_TOOL..."
    
    if [ "$DRY_RUN" = true ]; then
        print_info "Dry run: would build $target"
        return 0
    fi

    local cmd=("$BUILD_TOOL" build --release --target "$target")

    if [[ ${#TARGET_ENV_ASSIGNMENTS[@]} -gt 0 ]]; then
        env "${TARGET_ENV_ASSIGNMENTS[@]}" "${cmd[@]}"
    else
        "${cmd[@]}"
    fi
}

# Function to build binaries for all platforms
build_binaries() {
    local version=$1
    local dist_dir="dist"

    print_info "Building binaries for all platforms for version $version..."

    if [ "$DRY_RUN" = false ]; then
        rm -rf "$dist_dir"
        mkdir -p "$dist_dir"
    fi

    # Build targets in parallel where possible (background jobs)
    local pids=()

    # macOS x86_64
    build_with_tool x86_64-apple-darwin &
    pids+=($!)

    # macOS aarch64
    build_with_tool aarch64-apple-darwin &
    pids+=($!)

    # Linux x86_64 (only if on Linux or have cross setup)
    local build_linux=false
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        # On Linux, build directly
        build_linux=true
        print_info "Building Linux binary natively..."
        ( $BUILD_TOOL build --release --target x86_64-unknown-linux-gnu || print_warning "Linux build failed" ) &
        pids+=($!)
    elif command -v cross &>/dev/null; then
        # Use cross for cross-compilation which handles dependencies better
        build_linux=true
        print_info "Attempting Linux build using cross..."
        ( cross build --release --target x86_64-unknown-linux-gnu || print_warning "Linux build failed" ) &
        pids+=($!)
    else
        print_warning "Skipping Linux build - not on Linux and 'cross' tool not available"
        print_info "To enable Linux builds, install cross: cargo install cross"
    fi

    # Wait for all builds to complete
    print_info "Waiting for all parallel builds to finish..."
    for pid in "${pids[@]}"; do
        wait "$pid"
    done

    if [ "$DRY_RUN" = true ]; then
        print_success "Dry run: Build process simulation complete"
        return 0
    fi

    # Packaging
    print_info "Packaging binaries..."

    # macOS x86_64
    package_release_archive_with_ghostty \
        "x86_64-apple-darwin" \
        "$dist_dir" \
        "vtcode-$version-x86_64-apple-darwin.tar.gz"

    # macOS aarch64
    package_release_archive_with_ghostty \
        "aarch64-apple-darwin" \
        "$dist_dir" \
        "vtcode-$version-aarch64-apple-darwin.tar.gz"

    # Linux
    if [ "$build_linux" = true ] && [ -f "target/x86_64-unknown-linux-gnu/release/vtcode" ]; then
        package_release_archive_with_ghostty \
            "x86_64-unknown-linux-gnu" \
            "$dist_dir" \
            "vtcode-$version-x86_64-unknown-linux-gnu.tar.gz"
    fi

    print_success "Binaries build and packaging process completed"
}

# Function to build binaries for current platform only (for local sanity check)
# On macOS, this builds both x86_64 and aarch64 architectures for Homebrew compatibility
build_binaries_local() {
    local version=$1
    local dist_dir="dist"

    print_info "Building binaries for local platform(s) for version $version..."

    if [ "$DRY_RUN" = false ]; then
        rm -rf "$dist_dir"
        mkdir -p "$dist_dir"
    fi

    # On macOS, build both architectures for Homebrew formula updates
    if [[ "$OSTYPE" == "darwin"* ]]; then
        print_info "Building both macOS architectures (x86_64 and aarch64)..."
        
        # Build x86_64
        print_info "Building x86_64-apple-darwin..."
        build_with_tool "x86_64-apple-darwin" || print_warning "x86_64 build failed"
        
        # Build aarch64
        print_info "Building aarch64-apple-darwin..."
        build_with_tool "aarch64-apple-darwin" || print_warning "aarch64 build failed"
        
        if [ "$DRY_RUN" = true ]; then
            print_success "Dry run: Local macOS build process simulation complete"
            return 0
        fi
        
        # Package both architectures
        print_info "Packaging macOS binaries..."
        
        # Package x86_64
        if [ -f "target/x86_64-apple-darwin/release/vtcode" ]; then
            package_release_archive_with_ghostty \
                "x86_64-apple-darwin" \
                "$dist_dir" \
                "vtcode-$version-x86_64-apple-darwin.tar.gz"
            print_success "x86_64 binary packaged"
        else
            print_warning "x86_64 binary not found"
        fi
        
        # Package aarch64
        if [ -f "target/aarch64-apple-darwin/release/vtcode" ]; then
            package_release_archive_with_ghostty \
                "aarch64-apple-darwin" \
                "$dist_dir" \
                "vtcode-$version-aarch64-apple-darwin.tar.gz"
            print_success "aarch64 binary packaged"
        else
            print_warning "aarch64 binary not found"
        fi
        
        print_success "Local macOS build completed (x86_64 + aarch64)"
    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
        # Linux: build only current platform
        print_info "Building x86_64-unknown-linux-gnu..."
        build_with_tool "x86_64-unknown-linux-gnu"
        
        if [ "$DRY_RUN" = true ]; then
            print_success "Dry run: Local Linux build process simulation complete"
            return 0
        fi
        
        # Package Linux binary
        print_info "Packaging Linux binary..."
        package_release_archive_with_ghostty \
            "x86_64-unknown-linux-gnu" \
            "$dist_dir" \
            "vtcode-$version-x86_64-unknown-linux-gnu.tar.gz"
        print_success "Local Linux build completed"
    else
        print_error "Unsupported platform: $OSTYPE"
        exit 1
    fi
}

# Function to calculate SHA256 checksums
calculate_checksums() {
    local version=$1
    local dist_dir="dist"

    if [ "$DRY_RUN" = true ]; then
        print_info "Dry run: would calculate checksums"
        return 0
    fi

    print_info "Calculating SHA256 checksums..."
    
    # Use a subshell to avoid changing directory in the main script
    (
        cd "$dist_dir"
        # Clear/create checksums.txt
        rm -f checksums.txt
        touch checksums.txt

        # Determine available checksum tool
        local shacmd=""
        if command -v sha256sum &> /dev/null; then
            shacmd="sha256sum"
        elif command -v shasum &> /dev/null; then
            shacmd="shasum -a 256"
        else
            print_error "Neither sha256sum nor shasum found"
            exit 1
        fi

        for f in *; do
            # Process common archive formats
            if [[ "$f" == *.tar.gz ]] || [[ "$f" == *.zip ]] || [[ "$f" == *.tar.xz ]]; then
                if [ -f "$f" ]; then
                    # Standard format: hash  filename
                    $shacmd "$f" >> checksums.txt
                    # Keep individual files for Homebrew formula update (use original filename with .sha256 suffix)
                    # We keep the old naming convention for .tar.gz for compatibility
                    local shafile=""
                    if [[ "$f" == *.tar.gz ]]; then
                        shafile="${f%.tar.gz}.sha256"
                    else
                        shafile="${f}.sha256"
                    fi
                    $shacmd "$f" | cut -d' ' -f1 > "$shafile"
                    print_info "Checksum for $f: $(cat "$shafile")"
                fi
            fi
        done
        
        if [ -s checksums.txt ]; then
            print_info "Generated checksums.txt:"
            cat checksums.txt
        else
            print_warning "No binaries found to checksum"
        fi
    )

    print_success "SHA256 checksums calculated"
}

# Function to poll for GitHub release existence
poll_github_release() {
    local tag=$1
    local max_attempts=3
    local wait_seconds=5
    local attempt=1

    print_info "Polling GitHub for release $tag (short-circuiting as we create it if missing)..."
    while [ $attempt -le $max_attempts ]; do
        if gh release view "$tag" >/dev/null 2>&1; then
            print_success "GitHub release $tag is available"
            return 0
        fi
        print_info "Attempt $attempt/$max_attempts: Release $tag not found yet. Waiting ${wait_seconds}s..."
        sleep $wait_seconds
        attempt=$((attempt + 1))
    done

    print_warning "Timed out waiting for GitHub release $tag"
    return 1
}

# Function to upload binaries to GitHub Release
upload_binaries() {
    local version=$1
    local dist_dir="dist"
    local tag="$version"
    local notes_file="$2"

    if [ "$DRY_RUN" = true ]; then
        print_info "Dry run: would upload binaries to $tag"
        return 0
    fi

    print_info "Checking GitHub Release $tag..."

    # Check if release exists, if not poll then create
    if ! gh release view "$tag" >/dev/null 2>&1; then
        if ! poll_github_release "$tag"; then
            print_info "Creating GitHub release $tag..."
            local notes_args=("--title" "$version")
            if [ -n "$notes_file" ] && [ -f "$notes_file" ]; then
                notes_args+=("--notes-file" "$notes_file")
            else
                notes_args+=("--notes" "Release $version")
            fi
            gh release create "$tag" "${notes_args[@]}"
        fi
    fi

    # Upload all files
    cd "$dist_dir"
    local files=(*)
    if [ ${#files[@]} -gt 0 ]; then
        print_info "Uploading ${#files[@]} assets to $tag..."
        gh release upload "$tag" "${files[@]}" --clobber
        print_success "Uploaded assets to $tag"
    else
        print_error "No assets found in $dist_dir to upload"
        cd ..
        return 1
    fi
    cd ..
}

update_homebrew_formula_file() {
    local formula_path=$1
    local version=$2
    local x86_64_macos_sha=$3
    local aarch64_macos_sha=$4

    FORMULA_PATH="$formula_path" \
    FORMULA_VERSION="$version" \
    FORMULA_X86_64_MACOS_SHA="$x86_64_macos_sha" \
    FORMULA_AARCH64_MACOS_SHA="$aarch64_macos_sha" \
        python3 <<'PYTHON_SCRIPT'
import os
import re
from pathlib import Path

formula_path = Path(os.environ["FORMULA_PATH"])
version = os.environ["FORMULA_VERSION"]
x86_64_macos_sha = os.environ["FORMULA_X86_64_MACOS_SHA"]
aarch64_macos_sha = os.environ["FORMULA_AARCH64_MACOS_SHA"]

content = formula_path.read_text()
content = re.sub(r'version\s+"[^"]*"', f'version "{version}"', content)
content = re.sub(
    r'(aarch64-apple-darwin\.tar\.gz"\s+sha256\s+")([^"]*)(")',
    lambda match: f'{match.group(1)}{aarch64_macos_sha}{match.group(3)}',
    content,
)
content = re.sub(
    r'(x86_64-apple-darwin\.tar\.gz"\s+sha256\s+")([^"]*)(")',
    lambda match: f'{match.group(1)}{x86_64_macos_sha}{match.group(3)}',
    content,
)

formula_path.write_text(content)
PYTHON_SCRIPT
}

publish_homebrew_tap() {
    local version=$1
    local x86_64_macos_sha=$2
    local aarch64_macos_sha=$3

    print_info "Publishing Homebrew formula to vinhnx/homebrew-tap..."

    local temp_dir
    temp_dir=$(mktemp -d 2>/dev/null || mktemp -d -t 'vtcode-homebrew')

    if ! (
        trap 'rm -rf "$temp_dir"' EXIT

        if ! gh repo clone vinhnx/homebrew-tap "$temp_dir" >/dev/null 2>&1; then
            print_error "Failed to clone vinhnx/homebrew-tap"
            exit 1
        fi

        if ! update_homebrew_formula_file "$temp_dir/vtcode.rb" "$version" "$x86_64_macos_sha" "$aarch64_macos_sha"; then
            print_error "Failed to update Homebrew tap formula"
            exit 1
        fi

        if git -C "$temp_dir" diff --quiet -- vtcode.rb; then
            print_info "Homebrew tap formula is already up to date"
            exit 0
        fi

        git -C "$temp_dir" add vtcode.rb

        if ! GIT_AUTHOR_NAME="vtcode-release-bot" \
            GIT_AUTHOR_EMAIL="noreply@vtcode.com" \
            GIT_COMMITTER_NAME="vtcode-release-bot" \
            GIT_COMMITTER_EMAIL="noreply@vtcode.com" \
            git -C "$temp_dir" commit -m "Update vtcode formula to v$version"; then
            print_error "Failed to commit Homebrew tap update"
            exit 1
        fi

        if ! git -C "$temp_dir" -c credential.helper='!gh auth git-credential' push https://github.com/vinhnx/homebrew-tap.git HEAD:main; then
            print_error "Failed to push Homebrew tap update"
            exit 1
        fi

        print_success "Published vtcode formula to vinhnx/homebrew-tap"
    ); then
        return 1
    fi
}

# Function to update Homebrew formula
update_homebrew_formula() {
    local version=$1
    local formula_path="homebrew/vtcode.rb"

    if [ ! -f "$formula_path" ]; then
        print_warning "Homebrew formula not found at $formula_path"
        return 0
    fi

    if [ "$DRY_RUN" = true ]; then
        print_info "Dry run: would update Homebrew formula to $version"
        return 0
    fi

    print_info "Updating Homebrew formula at $formula_path..."

    local x86_64_macos_sha
    x86_64_macos_sha=$(cat "dist/vtcode-$version-x86_64-apple-darwin.sha256" 2>/dev/null || echo "")
    local aarch64_macos_sha
    aarch64_macos_sha=$(cat "dist/vtcode-$version-aarch64-apple-darwin.sha256" 2>/dev/null || echo "")
    if [ -z "$x86_64_macos_sha" ] || [ -z "$aarch64_macos_sha" ]; then
        print_error "Missing macOS checksums, cannot update Homebrew formula"
        return 1
    fi

    if ! update_homebrew_formula_file "$formula_path" "$version" "$x86_64_macos_sha" "$aarch64_macos_sha"; then
        print_error "Failed to update local Homebrew formula"
        return 1
    fi

    print_success "Homebrew formula updated locally"

    if ! git diff --quiet -- "$formula_path"; then
        git add "$formula_path"

        if ! GIT_AUTHOR_NAME="vtcode-release-bot" \
            GIT_AUTHOR_EMAIL="noreply@vtcode.com" \
            GIT_COMMITTER_NAME="vtcode-release-bot" \
            GIT_COMMITTER_EMAIL="noreply@vtcode.com" \
            git commit -m "chore: update homebrew formula to $version"; then
            print_warning "Failed to commit local Homebrew formula update"
        elif ! git -c credential.helper='!gh auth git-credential' push https://github.com/vinhnx/vtcode.git main; then
            print_warning "Failed to push Homebrew update"
        fi
    else
        print_info "Local Homebrew formula is already up to date"
    fi

    publish_homebrew_tap "$version" "$x86_64_macos_sha" "$aarch64_macos_sha"
}

# Main function
main() {
    local version=""
    local only_build=false
    local only_build_local=false
    local only_upload=false
    local only_homebrew=false
    local notes_file=""

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            -v|--version) version="$2"; shift 2 ;;
            --only-build) only_build=true; shift ;;
            --only-build-local) only_build_local=true; shift ;;
            --only-upload) only_upload=true; shift ;;
            --only-homebrew) only_homebrew=true; shift ;;
            --notes-file) notes_file="$2"; shift 2 ;;
            --dry-run) DRY_RUN=true; shift ;;
            *) shift ;;
        esac
    done

    if [ -z "$version" ]; then
        version=$(get_current_version)
    fi

    check_dependencies

    # If no specific flags are set, run everything
    if [ "$only_build" = false ] && [ "$only_build_local" = false ] && [ "$only_upload" = false ] && [ "$only_homebrew" = false ]; then
        install_rust_targets
        build_binaries "$version"
        calculate_checksums "$version"
        upload_binaries "$version" "$notes_file"
        update_homebrew_formula "$version"
    else
        if [ "$only_build" = true ]; then
            install_rust_targets
            build_binaries "$version"
            calculate_checksums "$version"
        fi
        if [ "$only_build_local" = true ]; then
            install_rust_targets
            build_binaries_local "$version"
            calculate_checksums "$version"
        fi
        if [ "$only_upload" = true ]; then
            calculate_checksums "$version"
            upload_binaries "$version" "$notes_file"
        fi
        if [ "$only_homebrew" = true ]; then
            calculate_checksums "$version"
            update_homebrew_formula "$version"
        fi
    fi

    print_success "Process completed for $version"
}

# Run main function
main "$@"
