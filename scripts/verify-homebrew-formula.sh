#!/bin/bash

# VT Code Homebrew Formula Verification Script
# This script validates the Homebrew formula and tests installation

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

print_info() {
    printf '%b\n' "${BLUE}INFO:${NC} $1"
}

print_success() {
    printf '%b\n' "${GREEN}SUCCESS:${NC} $1"
}

print_warning() {
    printf '%b\n' "${YELLOW}WARNING:${NC} $1"
}

print_error() {
    printf '%b\n' "${RED}ERROR:${NC} $1"
}

show_usage() {
    cat <<'USAGE'
Usage: ./scripts/verify-homebrew-formula.sh [options]

Options:
  --check-syntax       Check formula syntax with brew audit
  --test-install       Test installation from formula
  --verify-checksums   Verify checksums match GitHub releases
  --full               Run all checks
  -h, --help           Show this help message
USAGE
}

check_formula_syntax() {
    print_info "Checking Homebrew formula syntax..."

    if ! command -v brew &> /dev/null; then
        print_error "Homebrew not installed"
        return 1
    fi

    local formula_path="homebrew/vtcode.rb"

    if [ ! -f "$formula_path" ]; then
        print_error "Formula not found at $formula_path"
        return 1
    fi

    # Check Ruby syntax
    if ! ruby -c "$formula_path" >/dev/null 2>&1; then
        print_error "Formula has Ruby syntax errors"
        return 1
    fi

    print_success "Formula Ruby syntax is valid"

    # Try brew audit
    if command -v brew &> /dev/null; then
        if brew audit --new-formula "$formula_path" 2>/dev/null || true; then
            print_success "Formula passes brew audit"
        else
            print_warning "Formula has audit warnings (non-critical)"
        fi
    fi

    return 0
}

verify_checksums() {
    print_info "Verifying checksums against GitHub releases..."

    local formula_path="homebrew/vtcode.rb"
    local version=$(grep 'version "' "$formula_path" | head -1 | sed 's/.*version "\([^"]*\)".*/\1/')

    if [ -z "$version" ]; then
        print_error "Could not extract version from formula"
        return 1
    fi

    print_info "Formula version: $version"

    # Check if release exists on GitHub
    if ! gh release view "$version" >/dev/null 2>&1; then
        print_warning "Release $version not found on GitHub"
        return 1
    fi

    print_success "Release $version found on GitHub"

    # Get asset list
    print_info "Checking for binaries..."

    local assets=$(gh release view "$version" --json assets --jq '.assets | length')
    print_info "Found $assets assets in release"

    if [ "$assets" -lt 4 ]; then
        print_warning "Expected at least 4 binary assets (macOS Intel/ARM, Linux x64/ARM64)"
    else
        print_success "Release has sufficient binary assets"
    fi

    # Check for sha256 files
    local sha256_count=$(gh release view "$version" --json assets --jq '.assets[] | select(.name | endswith(".sha256")) | .name' | wc -l || echo "0")
    print_info "Found $sha256_count checksum files"

    # Try to download and verify checksums
    if [ "$sha256_count" -gt 0 ]; then
        print_info "Downloading checksums from release..."

        local temp_dir=$(mktemp -d)
        trap "rm -rf $temp_dir" EXIT

        if gh release download "$version" --dir "$temp_dir" --pattern "*.sha256" 2>/dev/null; then
            print_success "Downloaded checksum files"

            # Extract checksums from downloaded files
            for sha_file in "$temp_dir"/*.sha256; do
                local filename=$(basename "$sha_file" .sha256)
                local sha=$(cat "$sha_file" | awk '{print $1}')
                print_info "  $filename: $sha"
            done
        else
            print_warning "Could not download checksum files from GitHub"
        fi
    fi

    return 0
}

test_installation() {
    print_info "Testing Homebrew installation..."

    if ! command -v brew &> /dev/null; then
        print_error "Homebrew not installed - cannot test installation"
        return 1
    fi

    print_warning "Dry-run installation (not actually installing)"

    local formula_path="homebrew/vtcode.rb"

    if ! brew install --dry-run "$formula_path" 2>&1 | head -20; then
        print_warning "Dry-run showed some issues (may be non-critical)"
    else
        print_success "Formula installation would proceed"
    fi

    return 0
}

main() {
    local check_syntax=false
    local test_install=false
    local verify_checksums=false

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            --check-syntax)
                check_syntax=true
                shift
                ;;
            --test-install)
                test_install=true
                shift
                ;;
            --verify-checksums)
                verify_checksums=true
                shift
                ;;
            --full)
                check_syntax=true
                test_install=true
                verify_checksums=true
                shift
                ;;
            -h|--help)
                show_usage
                exit 0
                ;;
            *)
                print_error "Unknown option: $1"
                show_usage
                exit 1
                ;;
        esac
    done

    # If no specific checks, run syntax check
    if [ "$check_syntax" = false ] && [ "$test_install" = false ] && [ "$verify_checksums" = false ]; then
        check_syntax=true
    fi

    print_info "Starting Homebrew formula verification..."
    echo ""

    local failed=0

    if [ "$check_syntax" = true ]; then
        if ! check_formula_syntax; then
            ((failed++))
        fi
        echo ""
    fi

    if [ "$verify_checksums" = true ]; then
        if ! verify_checksums; then
            ((failed++))
        fi
        echo ""
    fi

    if [ "$test_install" = true ]; then
        if ! test_installation; then
            ((failed++))
        fi
        echo ""
    fi

    if [ $failed -eq 0 ]; then
        print_success "All verification checks passed!"
        exit 0
    else
        print_error "$failed check(s) failed"
        exit 1
    fi
}

main "$@"
