#!/bin/bash

# VTCode Binary Build and Upload Script
# This script builds binaries for macOS, Linux, and Windows targets and uploads them to GitHub Releases

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

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

# Function to detect cross availability
has_cross() {
    command -v cross >/dev/null 2>&1
}

# Function to ensure a Rust target is installed when using native cargo builds
ensure_rust_target() {
    local target=$1
    if rustup target list --installed | grep -qx "$target"; then
        return 0
    fi

    print_info "Installing Rust target $target..."
    rustup target add "$target"
}

# Function to select builder and compile a specific target
build_target() {
    local target=$1
    local builder="cargo"

    if has_cross && [[ "$target" != *"apple-darwin"* ]]; then
        builder="cross"
    fi

    if [[ "$builder" == "cargo" ]]; then
        ensure_rust_target "$target"
    fi

    print_info "Building for $target using $builder..."
    if ! $builder build --release --target "$target"; then
        print_error "Failed to build target $target"
        exit 1
    fi
}

# Function to package a built target into an archive
package_target() {
    local target=$1
    local version=$2
    local dist_dir=$3
    local release_dir="target/$target/release"
    local binary_name="vtcode"
    local archive_ext="tar.gz"

    if [[ "$target" == *"windows"* ]]; then
        binary_name="vtcode.exe"
        if command -v zip >/dev/null 2>&1; then
            archive_ext="zip"
        else
            print_warning "zip not found; packaging Windows binary as tar.gz"
        fi
    fi

    if [[ ! -f "$release_dir/$binary_name" ]]; then
        print_error "Binary $release_dir/$binary_name not found"
        exit 1
    fi

    local archive_name="vtcode-v${version}-${target}"
    local archive_path

    case "$archive_ext" in
        tar.gz)
            archive_path="$dist_dir/${archive_name}.tar.gz"
            tar -czf "$archive_path" -C "$release_dir" "$binary_name"
            ;;
        zip)
            archive_path="$dist_dir/${archive_name}.zip"
            zip -j -q "$archive_path" "$release_dir/$binary_name"
            ;;
    esac

    print_success "Packaged $target into $(basename "$archive_path")"
}

# Determine which checksum tool to use
get_checksum_tool() {
    if command -v shasum >/dev/null 2>&1; then
        echo "shasum -a 256"
    elif command -v sha256sum >/dev/null 2>&1; then
        echo "sha256sum"
    else
        return 1
    fi
}

# Determine the checksum filename for a given archive name
checksum_output_name() {
    local archive_name=$1
    if [[ "$archive_name" == *.tar.gz ]]; then
        echo "${archive_name%.tar.gz}.sha256"
    elif [[ "$archive_name" == *.zip ]]; then
        echo "${archive_name%.zip}.sha256"
    else
        echo "${archive_name}.sha256"
    fi
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
    
    print_success "All required tools are available"
}

# Function to get version from Cargo.toml
get_version() {
    grep '^version = ' Cargo.toml | head -1 | sed 's/version = \"\(.*\)\"/\1/'
}

# Function to build binaries
build_binaries() {
    local version=$1
    local dist_dir="dist"
    local targets=("x86_64-apple-darwin" "aarch64-apple-darwin")

    print_info "Building binaries for version $version..."

    # Create dist directory
    mkdir -p "$dist_dir"

    if has_cross; then
        print_success "Detected cross: $(cross --version)"
        targets+=(
            "x86_64-unknown-linux-gnu"
            "aarch64-unknown-linux-gnu"
            "x86_64-pc-windows-gnu"
        )
    else
        print_warning "cross not found; skipping Linux and Windows release artifacts"
    fi

    for target in "${targets[@]}"; do
        build_target "$target"
        package_target "$target" "$version" "$dist_dir"
    done

    print_success "Binaries built and packaged successfully"
}

# Function to calculate SHA256 checksums
calculate_checksums() {
    local version=$1
    local dist_dir="dist"
    local checksum_tool

    print_info "Calculating SHA256 checksums..."

    checksum_tool=$(get_checksum_tool) || {
        print_error "No SHA256 checksum tool available (install shasum or sha256sum)"
        exit 1
    }

    pushd "$dist_dir" >/dev/null
    shopt -s nullglob

    local archives=(vtcode-v$version-*.tar.gz vtcode-v$version-*.zip)

    if [ ${#archives[@]} -eq 0 ]; then
        print_warning "No release archives found for version $version"
    fi

    for archive_name in "${archives[@]}"; do
        local checksum
        checksum=$($checksum_tool "$archive_name" | awk '{print $1}')
        local checksum_file
        checksum_file=$(checksum_output_name "$archive_name")
        echo "$checksum" > "$checksum_file"
        print_info "$archive_name SHA256: $checksum"
    done

    shopt -u nullglob
    popd >/dev/null

    print_success "SHA256 checksums calculated"
}

# Function to upload binaries to GitHub Release
upload_binaries() {
    local version=$1
    local dist_dir="dist"
    local tag="v$version"

    print_info "Uploading binaries to GitHub Release $tag..."

    pushd "$dist_dir" >/dev/null
    shopt -s nullglob

    local archives=(vtcode-v$version-*.tar.gz vtcode-v$version-*.zip)

    for artifact in "${archives[@]}"; do
        print_info "Uploading $artifact..."
        if ! gh release upload "$tag" "$artifact" --clobber; then
            print_warning "Failed to upload $artifact - it may already exist or there might be permission issues"
        fi

        local checksum_file
        checksum_file=$(checksum_output_name "$artifact")
        if [[ -f "$checksum_file" ]]; then
            print_info "Uploading $checksum_file..."
            if ! gh release upload "$tag" "$checksum_file" --clobber; then
                print_warning "Failed to upload $checksum_file - it may already exist or there might be permission issues"
            fi
        fi
    done

    shopt -u nullglob
    popd >/dev/null

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
    
    # Update aarch64 SHA256 (find the line with aarch64 and update the SHA256 on the next line)
    sed -i.bak "/aarch64-apple-darwin/,+1{s|sha256 \"[a-f0-9]*\"|sha256 \"$aarch64_sha256\"|g};" "$formula_path"
    
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
            --skip-homebrew)
                skip_homebrew=true
                shift
                ;;
            -h|--help)
                echo "Usage: $0 [OPTIONS]"
                echo ""
                echo "Options:"
                echo "  -v, --version VERSION    Specify the version to build (default: read from Cargo.toml)"
                echo "  --skip-upload            Skip uploading binaries to GitHub Release"
                echo "  --skip-homebrew          Skip updating Homebrew formula"
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
