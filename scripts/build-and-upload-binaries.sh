#!/bin/bash

# VT Code Binary Build and Upload Script
# This script builds binaries locally and uploads them to GitHub Releases

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

BUILD_TOOL="cargo"
TARGET_ENV_ASSIGNMENTS=()
DRY_RUN=false

# Function to print colored output
print_info() {
    echo -e "${BLUE}INFO: $1${NC}"
}

print_success() {
    echo -e "${GREEN}SUCCESS: $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}WARNING: $1${NC}"
}

print_error() {
    echo -e "${RED}ERROR: $1${NC}"
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

    if ! command -v gh &> /dev/null; then
        missing_tools+=("gh (GitHub CLI)")
    fi

    if [ ${#missing_tools[@]} -ne 0 ]; then
        print_error "Missing required tools: ${missing_tools[*]}"
        print_info "Please install the missing tools and try again"
        exit 1
    fi

    # Verify GitHub CLI authentication and scopes
    if ! gh auth status >/dev/null 2>&1; then
        print_error "GitHub CLI is not authenticated. Please run: gh auth login"
        exit 1
    fi

    # Check for required 'workflow' scope which is often needed for release actions
    if ! gh auth status --show-token 2>&1 | grep -q "workflow"; then
        print_warning "GitHub CLI may lack 'workflow' scope required for some release operations."
        print_info "If release creation fails, run: gh auth refresh -s workflow"
    fi

    print_success "All required tools are available"
}

# Function to get version from Cargo.toml
get_version() {
    grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"//'
}

# Function to install Rust targets if needed
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

    if [[ "$OSTYPE" == "darwin"* ]]; then
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

# Function to build binaries
build_binaries() {
    local version=$1
    local dist_dir="dist"
    
    print_info "Building binaries locally for version $version..."
    
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
    if [[ "$OSTYPE" == "linux-gnu"* ]] || command -v cross &>/dev/null; then
        build_linux=true
        local tool=$BUILD_TOOL
        if command -v cross &>/dev/null; then tool="cross"; fi
        
        print_info "Attempting Linux build using $tool..."
        ( $tool build --release --target x86_64-unknown-linux-gnu || print_warning "Linux build failed" ) &
        pids+=($!)
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
    cp "target/x86_64-apple-darwin/release/vtcode" "$dist_dir/vtcode"
    (cd "$dist_dir" && tar -czf "vtcode-v$version-x86_64-apple-darwin.tar.gz" vtcode && rm vtcode)

    # macOS aarch64
    cp "target/aarch64-apple-darwin/release/vtcode" "$dist_dir/vtcode"
    (cd "$dist_dir" && tar -czf "vtcode-v$version-aarch64-apple-darwin.tar.gz" vtcode && rm vtcode)

    # Create macOS Universal Binary
    if [ -f "target/x86_64-apple-darwin/release/vtcode" ] && [ -f "target/aarch64-apple-darwin/release/vtcode" ]; then
        print_info "Creating macOS Universal Binary using lipo..."
        lipo -create \
            "target/x86_64-apple-darwin/release/vtcode" \
            "target/aarch64-apple-darwin/release/vtcode" \
            -output "$dist_dir/vtcode-universal"
        
        (cd "$dist_dir" && tar -czf "vtcode-v$version-universal-apple-darwin.tar.gz" vtcode-universal && rm vtcode-universal)
        print_success "macOS Universal Binary created"
    fi

    # Linux
    if [ "$build_linux" = true ] && [ -f "target/x86_64-unknown-linux-gnu/release/vtcode" ]; then
        cp "target/x86_64-unknown-linux-gnu/release/vtcode" "$dist_dir/vtcode"
        (cd "$dist_dir" && tar -czf "vtcode-v$version-x86_64-unknown-linux-gnu.tar.gz" vtcode && rm vtcode)
    fi

    print_success "Binaries build and packaging process completed"
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
    cd "$dist_dir"

    for f in *.tar.gz; do
        if [ -f "$f" ]; then
            shasum -a 256 "$f" | cut -d' ' -f1 > "${f%.tar.gz}.sha256"
            print_info "Checksum for $f: $(cat ${f%.tar.gz}.sha256)"
        fi
    done

    cd ..
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
    local tag="v$version"
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
            local notes_args=("--title" "VT Code v$version")
            if [ -n "$notes_file" ] && [ -f "$notes_file" ]; then
                notes_args+=("--notes-file" "$notes_file")
            else
                notes_args+=("--notes" "Release v$version")
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

# Function to update Homebrew formula
update_homebrew_formula() {
    local version=$1
    local formula_path="homebrew/vtcode.rb"

    if [ ! -f "$formula_path" ]; then
        print_warning "Homebrew formula not found at $formula_path"
        return 0
    fi

    if [ "$DRY_RUN" = true ]; then
        print_info "Dry run: would update Homebrew formula to v$version"
        return 0
    fi

    print_info "Updating Homebrew formula at $formula_path..."

    local x86_64_macos_sha=$(cat "dist/vtcode-v$version-x86_64-apple-darwin.sha256" 2>/dev/null || echo "")
    local aarch64_macos_sha=$(cat "dist/vtcode-v$version-aarch64-apple-darwin.sha256" 2>/dev/null || echo "")
    local universal_macos_sha=$(cat "dist/vtcode-v$version-universal-apple-darwin.sha256" 2>/dev/null || echo "")

    if [ -z "$x86_64_macos_sha" ] || [ -z "$aarch64_macos_sha" ]; then
        print_error "Missing macOS checksums, cannot update Homebrew formula"
        return 1
    fi

    python3 << PYTHON_SCRIPT
import re
with open('$formula_path', 'r') as f:
    content = f.read()

content = re.sub(r'version\s+"[^\"]*"', 'version "$version"', content)
content = re.sub(
    r'(aarch64-apple-darwin.tar.gz"\s+sha256\s+")[^\"]*(")', 
    r'\g<1>$aarch64_macos_sha\g<2>',
    content
)
content = re.sub(
    r'(x86_64-apple-darwin.tar.gz"\s+sha256\s+")[^\"]*(")', 
    r'\g<1>$x86_64_macos_sha\g<2>',
    content
)

# If universal SHA is available, we could update that too if the formula supports it
# For now, we update the primary architecture-specific ones

with open('$formula_path', 'w') as f:
    f.write(content)
PYTHON_SCRIPT

    print_success "Homebrew formula updated locally"
    
    # Commit and push
    git add "$formula_path"
    git commit -m "chore: update homebrew formula to v$version" || true
    git push origin main || print_warning "Failed to push Homebrew update"
}

# Main function
main() {
    local version=""
    local only_build=false
    local only_upload=false
    local only_homebrew=false
    local notes_file=""

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            -v|--version) version="$2"; shift 2 ;;
            --only-build) only_build=true; shift ;;
            --only-upload) only_upload=true; shift ;;
            --only-homebrew) only_homebrew=true; shift ;;
            --notes-file) notes_file="$2"; shift 2 ;;
            --dry-run) DRY_RUN=true; shift ;;
            *) shift ;;
        esac
    done

    if [ -z "$version" ]; then
        version=$(get_version)
    fi

    check_dependencies

    # If no specific flags are set, run everything
    if [ "$only_build" = false ] && [ "$only_upload" = false ] && [ "$only_homebrew" = false ]; then
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
        if [ "$only_upload" = true ]; then
            upload_binaries "$version" "$notes_file"
        fi
        if [ "$only_homebrew" = true ]; then
            update_homebrew_formula "$version"
        fi
    fi

    print_success "Process completed for v$version"
}

# Run main function
main "$@"