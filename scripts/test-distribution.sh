#!/bin/bash

# VTCode Distribution Test Script
# This script helps test the distribution setup before releasing

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

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

# Function to check if cross is available
check_cross() {
    if command -v cross &> /dev/null; then
        print_success "cross is available: $(cross --version)"
        return 0
    fi

    print_warning "cross is not available - skipping cross-compilation smoke tests"
    return 1
}

# Function to check if cargo is available
check_cargo() {
    if ! command -v cargo &> /dev/null; then
        print_error "Cargo is not installed or not in PATH"
        return 1
    fi
    print_success "Cargo is available"
}

# Function to check if npm is available
check_npm() {
    if ! command -v npm &> /dev/null; then
        print_warning "npm is not available - npm distribution won't work"
        return 1
    fi
    print_success "npm is available"
}

# Function to check if brew is available (macOS only)
check_brew() {
    if [[ "$OSTYPE" == "darwin"* ]]; then
        if ! command -v brew &> /dev/null; then
            print_warning "Homebrew is not available - Homebrew distribution won't work on macOS"
            return 1
        fi
        print_success "Homebrew is available"
    else
        print_info "Not on macOS - skipping Homebrew check"
    fi
}

# Function to validate Cargo.toml metadata
validate_cargo_toml() {
    print_info "Validating Cargo.toml metadata..."

    if ! grep -q '^description = ' Cargo.toml; then
        print_error "Missing description in Cargo.toml"
        return 1
    fi

    if ! grep -q '^repository = ' Cargo.toml; then
        print_error "Missing repository in Cargo.toml"
        return 1
    fi

    if ! grep -q '^license = ' Cargo.toml; then
        print_error "Missing license in Cargo.toml"
        return 1
    fi

    if ! grep -q '^keywords = ' Cargo.toml; then
        print_error "Missing keywords in Cargo.toml"
        return 1
    fi

    print_success "Cargo.toml metadata is valid"
}

# Function to validate vtcode-core Cargo.toml
validate_vtcode_core_toml() {
    print_info "Validating vtcode-core/Cargo.toml metadata..."

    if ! grep -q '^description = ' vtcode-core/Cargo.toml; then
        print_error "Missing description in vtcode-core/Cargo.toml"
        return 1
    fi

    print_success "vtcode-core/Cargo.toml metadata is valid"
}

# Function to check if binary builds successfully
test_build() {
    print_info "Testing build..."

    if ! cargo check; then
        print_error "Build check failed"
        return 1
    fi

    if ! cargo build --release; then
        print_error "Release build failed"
        return 1
    fi

    if check_cross; then
        local cross_targets=(
            "x86_64-unknown-linux-gnu"
            "aarch64-unknown-linux-gnu"
            "x86_64-pc-windows-gnu"
        )

        for target in "${cross_targets[@]}"; do
            print_info "Cross-compiling release binary for $target..."
            if ! cross build --release --target "$target"; then
                print_error "cross build failed for target $target"
                return 1
            fi
        done
    fi

    print_success "Build successful"
}

# Function to validate Homebrew formula
validate_homebrew_formula() {
    print_info "Validating Homebrew formula..."

    if [[ ! -f "homebrew/vtcode.rb" ]]; then
        print_error "Homebrew formula not found at homebrew/vtcode.rb"
        return 1
    fi

    # Check if formula has required fields
    if ! grep -q 'desc "' homebrew/vtcode.rb; then
        print_error "Missing description in Homebrew formula"
        return 1
    fi

    if ! grep -q 'homepage "' homebrew/vtcode.rb; then
        print_error "Missing homepage in Homebrew formula"
        return 1
    fi

    print_success "Homebrew formula is valid"
}

# Function to validate npm package
validate_npm_package() {
    print_info "Validating npm package..."

    if [[ ! -f "npm/package.json" ]]; then
        print_error "npm package.json not found"
        return 1
    fi

    if [[ ! -f "npm/README.md" ]]; then
        print_error "npm README.md not found"
        return 1
    fi

    if [[ ! -f "npm/scripts/postinstall.js" ]]; then
        print_error "npm postinstall script not found"
        return 1
    fi

    if [[ ! -f "npm/bin/vtcode" ]]; then
        print_error "npm bin script not found"
        return 1
    fi

    # Test npm package structure
    local original_dir=$(pwd)
    cd npm || {
        print_error "Failed to change to npm directory"
        return 1
    }
    
    # Validate package.json structure (not version)
    if ! node -e "const pkg = require('./package.json'); if (!pkg.name || !pkg.description) throw new Error('Invalid package.json');" &>/dev/null; then
        print_error "npm package.json structure invalid"
        cd "$original_dir"
        return 1
    fi
    
    cd "$original_dir"
    print_success "npm package structure is valid"
}

# Function to check GitHub Actions workflows
validate_workflows() {
    print_info "Validating GitHub Actions workflows..."

    if [[ ! -f ".github/workflows/release.yml" ]]; then
        print_error "Release workflow not found"
        return 1
    fi

    if [[ ! -f ".github/workflows/build-release.yml" ]]; then
        print_error "Build release workflow not found"
        return 1
    fi

    if [[ ! -f ".github/workflows/publish-crates.yml" ]]; then
        print_error "Publish crates workflow not found"
        return 1
    fi

    print_success "GitHub Actions workflows are present"
}

# Main test function
main() {
    print_info "Starting VTCode distribution validation..."

    local errors=0

    check_cargo || ((errors++))
    check_npm || true  # Don't fail if npm not available
    check_brew || true # Don't fail if brew not available

    validate_cargo_toml || ((errors++))
    validate_vtcode_core_toml || ((errors++))

    test_build || ((errors++))

    validate_homebrew_formula || ((errors++))
    validate_npm_package || ((errors++))
    validate_workflows || ((errors++))

    echo
    if [[ $errors -eq 0 ]]; then
        print_success "All distribution validation checks passed!"
        print_info "You can now create a release using: ./scripts/release.sh"
    else
        print_error "$errors validation check(s) failed"
        print_info "Please fix the issues above before creating a release"
        exit 1
    fi
}

# Run main function
main "$@"