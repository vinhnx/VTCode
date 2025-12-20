#!/bin/bash

# VTCode Binary Build and Upload Script
# This script builds binaries for macOS and uploads them to GitHub Releases

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

BUILD_TOOL="cargo"
TARGET_ENV_ASSIGNMENTS=()

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

    # Verify correct GitHub account is active
    local expected_account="vinhnx"
    if ! gh auth status >/dev/null 2>&1; then
        print_error "GitHub CLI is not authenticated. Please run: gh auth login"
        exit 1
    fi
    
    # Check if we're logged in to the expected account and it's active
    if ! gh auth status 2>&1 | grep -q "Logged in to github.com account $expected_account"; then
        print_error "Not logged in to GitHub account: $expected_account"
        print_info "Run: gh auth login --hostname github.com"
        exit 1
    fi
    
    if ! gh auth status 2>&1 | grep -A 5 "account $expected_account" | grep -q "Active account: true"; then
        print_error "GitHub account '$expected_account' is not active"
        print_info "Run: gh auth switch --hostname github.com --user $expected_account"
        exit 1
    fi
    
    print_success "GitHub CLI authenticated with correct account: $expected_account"

    # Check if cross is available and should be used
    if command -v cross &> /dev/null; then
        BUILD_TOOL="cross"
        print_success "Detected cross – using it for reproducible cross-compilation builds"
        # Set default container engine if not set
        if [[ -z "${CROSS_CONTAINER_ENGINE:-}" ]]; then
            export CROSS_CONTAINER_ENGINE="docker"
        fi
    else
        BUILD_TOOL="cargo"
        print_warning "cross not found. Install with 'cargo install cross' for faster, sandboxed cross-compilation."
        print_info "Falling back to cargo with native compilation"
    fi

    print_success "All required tools are available"
}

# Function to get version from Cargo.toml
get_version() {
    grep '^version = ' Cargo.toml | head -1 | sed 's/version = \"\(.*\)\"/\1/'
}

# Function to install Rust targets if needed
install_rust_targets() {
    print_info "Checking and installing required Rust targets..."

    # Check if targets are installed
    local targets=$(rustc --print target-list)

    # macOS targets
    if ! echo "$targets" | grep -q "x86_64-apple-darwin"; then
        print_info "Installing x86_64-apple-darwin target..."
        rustup target add x86_64-apple-darwin
    fi

    if ! echo "$targets" | grep -q "aarch64-apple-darwin"; then
        print_info "Installing aarch64-apple-darwin target..."
        rustup target add aarch64-apple-darwin
    fi

    # Linux targets
    if ! echo "$targets" | grep -q "x86_64-unknown-linux-gnu"; then
        print_info "Installing x86_64-unknown-linux-gnu target..."
        rustup target add x86_64-unknown-linux-gnu
    fi

    if ! echo "$targets" | grep -q "aarch64-unknown-linux-gnu"; then
        print_info "Installing aarch64-unknown-linux-gnu target..."
        rustup target add aarch64-unknown-linux-gnu
    fi

    print_success "Required Rust targets are installed"
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
            TARGET_ENV_ASSIGNMENTS+=("PKG_CONFIG_ALLOW_CROSS=1")
        else
            print_warning "Homebrew OpenSSL not found. Install with 'brew install openssl@3' for reliable macOS builds."
        fi

        if command -v xcrun &> /dev/null; then
            local sdkroot
            sdkroot=$(xcrun --sdk macosx --show-sdk-path 2>/dev/null || true)
            if [[ -n "$sdkroot" ]]; then
                TARGET_ENV_ASSIGNMENTS+=("SDKROOT=$sdkroot")
            fi
        fi

        TARGET_ENV_ASSIGNMENTS+=("MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}")

        case "$target" in
            x86_64-apple-darwin)
                local cflags="-arch x86_64"
                local combined_cflags="${CFLAGS:-}"
                if [[ -n "$combined_cflags" ]]; then
                    cflags="$combined_cflags $cflags"
                fi
                TARGET_ENV_ASSIGNMENTS+=("CFLAGS=$cflags")

                local cxxflags="-arch x86_64"
                local combined_cxxflags="${CXXFLAGS:-}"
                if [[ -n "$combined_cxxflags" ]]; then
                    cxxflags="$combined_cxxflags $cxxflags"
                fi
                TARGET_ENV_ASSIGNMENTS+=("CXXFLAGS=$cxxflags")

                local ldflags="-arch x86_64"
                local combined_ldflags="${LDFLAGS:-}"
                if [[ -n "$combined_ldflags" ]]; then
                    ldflags="$combined_ldflags $ldflags"
                fi
                TARGET_ENV_ASSIGNMENTS+=("LDFLAGS=$ldflags")
                ;;
            aarch64-apple-darwin)
                local cflags="-arch arm64"
                if [[ -n "${CFLAGS:-}" ]]; then
                    cflags="${CFLAGS} $cflags"
                fi
                TARGET_ENV_ASSIGNMENTS+=("CFLAGS=$cflags")

                local cxxflags="-arch arm64"
                if [[ -n "${CXXFLAGS:-}" ]]; then
                    cxxflags="${CXXFLAGS} $cxxflags"
                fi
                TARGET_ENV_ASSIGNMENTS+=("CXXFLAGS=$cxxflags")

                local ldflags="-arch arm64"
                if [[ -n "${LDFLAGS:-}" ]]; then
                    ldflags="${LDFLAGS} $ldflags"
                fi
                TARGET_ENV_ASSIGNMENTS+=("LDFLAGS=$ldflags")
                ;;
        esac
    fi
}

build_with_tool() {
    local target=$1
    TARGET_ENV_ASSIGNMENTS=()
    configure_target_env "$target"

    local cmd=("${BUILD_TOOL:-cargo}" build --release --target "$target")

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

    print_info "Building binaries for version $version..."

    # Create dist directory
    mkdir -p "$dist_dir"

    # macOS builds
    # Build for x86_64 macOS
    print_info "Building for x86_64 macOS..."
    build_with_tool x86_64-apple-darwin

    # Package x86_64 binary
    print_info "Packaging x86_64 macOS binary..."
    cp "target/x86_64-apple-darwin/release/vtcode" "$dist_dir/"
    cd "$dist_dir"
    tar -czf "vtcode-v$version-x86_64-apple-darwin.tar.gz" vtcode
    cd ..

    # Build for aarch64 macOS
    print_info "Building for aarch64 macOS..."
    build_with_tool aarch64-apple-darwin

    # Package aarch64 binary
    print_info "Packaging aarch64 macOS binary..."
    cp "target/aarch64-apple-darwin/release/vtcode" "$dist_dir/"
    cd "$dist_dir"
    tar -czf "vtcode-v$version-aarch64-apple-darwin.tar.gz" vtcode
    cd ..

    # Linux builds
    # Build for x86_64 Linux
    print_info "Building for x86_64 Linux..."
    build_with_tool x86_64-unknown-linux-gnu

    # Package x86_64 Linux binary
    print_info "Packaging x86_64 Linux binary..."
    cp "target/x86_64-unknown-linux-gnu/release/vtcode" "$dist_dir/"
    cd "$dist_dir"
    tar -czf "vtcode-v$version-x86_64-unknown-linux-gnu.tar.gz" vtcode
    cd ..

    # Build for aarch64 Linux
    print_info "Building for aarch64 Linux..."
    build_with_tool aarch64-unknown-linux-gnu

    # Package aarch64 Linux binary
    print_info "Packaging aarch64 Linux binary..."
    cp "target/aarch64-unknown-linux-gnu/release/vtcode" "$dist_dir/"
    cd "$dist_dir"
    tar -czf "vtcode-v$version-aarch64-unknown-linux-gnu.tar.gz" vtcode
    cd ..

    print_success "Binaries built and packaged successfully"
}

# Function to calculate SHA256 checksums
calculate_checksums() {
    local version=$1
    local dist_dir="dist"

    print_info "Calculating SHA256 checksums..."

    cd "$dist_dir"

    # macOS checksums
    local x86_64_macos_sha256=$(shasum -a 256 "vtcode-v$version-x86_64-apple-darwin.tar.gz" | cut -d' ' -f1)
    local aarch64_macos_sha256=$(shasum -a 256 "vtcode-v$version-aarch64-apple-darwin.tar.gz" | cut -d' ' -f1)
    
    # Linux checksums
    local x86_64_linux_sha256=$(shasum -a 256 "vtcode-v$version-x86_64-unknown-linux-gnu.tar.gz" | cut -d' ' -f1)
    local aarch64_linux_sha256=$(shasum -a 256 "vtcode-v$version-aarch64-unknown-linux-gnu.tar.gz" | cut -d' ' -f1)

    cd ..

    # Write checksum files
    echo "$x86_64_macos_sha256" > "$dist_dir/vtcode-v$version-x86_64-apple-darwin.sha256"
    echo "$aarch64_macos_sha256" > "$dist_dir/vtcode-v$version-aarch64-apple-darwin.sha256"
    echo "$x86_64_linux_sha256" > "$dist_dir/vtcode-v$version-x86_64-unknown-linux-gnu.sha256"
    echo "$aarch64_linux_sha256" > "$dist_dir/vtcode-v$version-aarch64-unknown-linux-gnu.sha256"

    print_info "x86_64 macOS SHA256: $x86_64_macos_sha256"
    print_info "aarch64 macOS SHA256: $aarch64_macos_sha256"
    print_info "x86_64 Linux SHA256: $x86_64_linux_sha256"
    print_info "aarch64 Linux SHA256: $aarch64_linux_sha256"

    print_success "SHA256 checksums calculated"
}

# Function to upload binaries to GitHub Release
upload_binaries() {
    local version=$1
    local dist_dir="dist"
    local tag="v$version"

    print_info "Uploading binaries to GitHub Release $tag..."

    # Verify the release exists before attempting upload
    if ! gh release view "$tag" >/dev/null 2>&1; then
        print_error "GitHub release '$tag' does not exist. Please ensure the release is created first."
        print_info "You may need to wait a moment for the release to be created by cargo-release."
        return 1
    fi

    cd "$dist_dir"

    # Upload all files in batch for better performance
    print_info "Uploading assets to GitHub release..."
    
    local files_to_upload=()
    # macOS files
    files_to_upload+=("vtcode-v$version-x86_64-apple-darwin.tar.gz")
    files_to_upload+=("vtcode-v$version-x86_64-apple-darwin.sha256")
    files_to_upload+=("vtcode-v$version-aarch64-apple-darwin.tar.gz")
    files_to_upload+=("vtcode-v$version-aarch64-apple-darwin.sha256")
    # Linux files
    files_to_upload+=("vtcode-v$version-x86_64-unknown-linux-gnu.tar.gz")
    files_to_upload+=("vtcode-v$version-x86_64-unknown-linux-gnu.sha256")
    files_to_upload+=("vtcode-v$version-aarch64-unknown-linux-gnu.tar.gz")
    files_to_upload+=("vtcode-v$version-aarch64-unknown-linux-gnu.sha256")
    
    # Verify all files exist before uploading
    for file in "${files_to_upload[@]}"; do
        if [[ ! -f "$file" ]]; then
            print_error "Required file not found: $file"
            cd ..
            return 1
        fi
    done
    
    # Upload all files
    print_info "Uploading ${#files_to_upload[@]} files to release $tag..."
    if ! gh release upload "$tag" "${files_to_upload[@]}" --clobber; then
        print_error "Failed to upload assets to GitHub release"
        cd ..
        return 1
    fi
    
    # Verify upload was successful
    print_info "Verifying uploaded assets..."
    sleep 2  # Give GitHub a moment to process the upload
    local asset_count=$(gh release view "$tag" --json assets --jq '.assets | length' 2>/dev/null || echo "0")
    if [[ $asset_count -lt ${#files_to_upload[@]} ]]; then
        print_warning "Expected ${#files_to_upload[@]} assets, but found $asset_count in release"
        print_info "This may be temporary - check the release on GitHub to confirm all assets uploaded"
    else
        print_success "All $asset_count assets uploaded successfully"
    fi
    
    cd ..

    print_success "Binary upload process completed"
}

# Function to update Homebrew formula
update_homebrew_formula() {
    local version=$1

    print_info "Updating Homebrew formula..."

    # Calculate SHA256 checksums (we already have them, but let's recalculate to be sure)
    local x86_64_sha256=$(cat "dist/vtcode-v$version-x86_64-apple-darwin.sha256")
    local aarch64_sha256=$(cat "dist/vtcode-v$version-aarch64-apple-darwin.sha256")

    # Update the formula
    local formula_path="homebrew/vtcode.rb"

    if [ ! -f "$formula_path" ]; then
        print_warning "Homebrew formula not found at $formula_path"
        return 1
    fi

    # Update version
    sed -i.bak "s|version \"[0-9.]*\"|version \"$version\"|g" "$formula_path"

    # Update x86_64 SHA256
    sed -i.bak "s|sha256 \"[a-f0-9]*\"|sha256 \"$x86_64_sha256\"|g" "$formula_path"

    # Update aarch64 SHA256 (find the line with aarch64 and update the next SHA256 line)
    # Using a more portable approach with Python for cross-platform compatibility
    python3 -c "
import re
with open('$formula_path', 'r') as f:
    content = f.read()

# Replace x86_64 SHA256 (first occurrence after x86_64 url)
content = re.sub(r'(x86_64-apple-darwin.*?sha256\s+\")([a-f0-9]+)(\")', r'\g<1>${x86_64_sha256}\g<3>', content, 1, re.DOTALL)

# Replace aarch64 SHA256 (first occurrence after aarch64 url)
content = re.sub(r'(aarch64-apple-darwin.*?sha256\s+\")([a-f0-9]+)(\")', r'\g<1>${aarch64_sha256}\g<3>', content, 1, re.DOTALL)

with open('$formula_path', 'w') as f:
    f.write(content)
"

    # Clean up backup files
    rm "$formula_path.bak"

    print_success "Homebrew formula updated"

    # Commit and push the formula update
    git add "$formula_path"
    git commit -m "Update Homebrew formula to version $version" || true
    git push || true

    print_success "Homebrew formula committed and pushed"
}

# Main function
main() {
    local version=""
    local skip_upload=false
    local skip_homebrew=false

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            -v|--version)
                version="$2"
                shift 2
                ;;
            --skip-upload)
                skip_upload=true
                shift
                ;;
            -h|--help)
                echo "Usage: $0 [OPTIONS]"
                echo ""
                echo "Options:"
                echo "  -v, --version VERSION    Specify the version to build (default: read from Cargo.toml)"
                echo "  --skip-upload            Skip uploading binaries to GitHub Release"
                echo "  -h, --help               Show this help message"
                exit 0
                ;;
            *)
                print_error "Unknown option: $1"
                exit 1
                ;;
        esac
    done

    # Get version if not specified
    if [ -z "$version" ]; then
        version=$(get_version)
        print_info "Using version from Cargo.toml: $version"
    fi

    # Check dependencies
    check_dependencies

    # Install Rust targets
    install_rust_targets

    # Build binaries
    build_binaries "$version"

    # Calculate checksums
    calculate_checksums "$version"

    # Upload binaries (unless skipped)
    if [ "$skip_upload" = false ]; then
        upload_binaries "$version"
    else
        print_info "Skipping binary upload as requested"
    fi

    # Update Homebrew formula (unless skipped)
    if [ "$skip_homebrew" = false ]; then
        update_homebrew_formula "$version"
    else
        print_info "Skipping Homebrew formula update as requested"
    fi

    print_success "Binary build and upload process completed for version $version"
}

# Run main function
main "$@"
