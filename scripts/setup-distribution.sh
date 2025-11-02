#!/bin/bash

# VTCode Distribution Setup Script
# This script helps set up and test the distribution configuration

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
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

print_step() {
    echo -e "${PURPLE}STEP: $1${NC}"
}

# Function to check Cargo setup
setup_cargo() {
    print_step "Setting up Cargo (crates.io) distribution"

    if ! command -v cargo &> /dev/null; then
        print_error "Cargo is not installed"
        return 1
    fi

    print_info "Checking Cargo login status..."
    if cargo login --help &> /dev/null && cargo publish --dry-run &> /dev/null 2>&1; then
        print_success "Cargo is logged in to crates.io"
    else
        print_warning "Cargo is not logged in to crates.io"
        echo
        print_info "To set up Cargo publishing:"
        echo "1. Go to https://crates.io/me"
        echo "2. Generate an API token"
        echo "3. Run: cargo login"
        echo "4. Paste your token when prompted"
        echo
        print_info "For GitHub Actions, add CRATES_IO_TOKEN secret in repository settings"
    fi
}

# Function to check npm setup
setup_npm() {
    print_step "Setting up npm distribution"

    if ! command -v npm &> /dev/null; then
        print_warning "npm is not installed - skipping npm setup"
        return 0
    fi

    print_info "Checking npm login status..."
    if npm whoami &> /dev/null; then
        print_success "npm is logged in"
    else
        print_warning "npm is not logged in"
        echo
        print_info "To set up npm publishing:"
        echo "1. Create an npm account at https://www.npmjs.com"
        echo "2. Run: npm login"
        echo "3. Enter your credentials"
    fi
}

# Function to validate package files
validate_packages() {
    print_step "Validating package configuration"

    # Check main Cargo.toml
    if [[ ! -f "Cargo.toml" ]]; then
        print_error "Cargo.toml not found"
        return 1
    fi

    if ! grep -q '^description = ' Cargo.toml; then
        print_error "Missing description in Cargo.toml"
        return 1
    fi

    if ! grep -q '^repository = ' Cargo.toml; then
        print_error "Missing repository in Cargo.toml"
        return 1
    fi

    print_success "Main Cargo.toml is valid"

    # Check vtcode-core Cargo.toml
    if [[ ! -f "vtcode-core/Cargo.toml" ]]; then
        print_error "vtcode-core/Cargo.toml not found"
        return 1
    fi

    print_success "vtcode-core Cargo.toml exists"

    # Check npm package
    if [[ -d "npm" ]]; then
        if [[ ! -f "npm/package.json" ]]; then
            print_error "npm/package.json not found"
            return 1
        fi
        if [[ ! -f "npm/README.md" ]]; then
            print_warning "npm/README.md not found - npm package will not have a README on npmjs.com"
        fi
        print_success "npm package is configured"
    else
        print_warning "npm directory not found - npm distribution not set up"
    fi
}

# Function to test build
test_build() {
    print_step "Testing build process"

    print_info "Running cargo check..."
    if ! cargo check; then
        print_error "cargo check failed"
        return 1
    fi

    print_info "Running cargo build --release..."
    if ! cargo build --release; then
        print_error "cargo build --release failed"
        return 1
    fi

    print_success "Build test passed"
}

# Function to show next steps
show_next_steps() {
    echo
    print_info "=== NEXT STEPS ==="
    echo
    print_info "1. Complete authentication setup:"
    echo "   - Run: cargo login (for crates.io)"
    echo "   - Run: npm login (for npm, if desired)"
    echo
    print_info "2. Set up GitHub Actions secrets:"
    echo "   - Add CRATES_IO_TOKEN in repository settings"
    echo
    print_info "3. Test the release process:"
    echo "   - Run: ./scripts/release.sh --patch --dry-run"
    echo
    print_info "4. Create your first release:"
    echo "   - Run: ./scripts/release.sh --patch"
    echo
    print_info "Documentation:"
    print_info "   - Setup Guide: docs/project/DISTRIBUTION_SETUP_GUIDE.md"
    print_info "   - Distribution Overview: docs/project/DISTRIBUTION_SETUP.md"
}

# Main function
main() {
    echo "========================================"
    echo "  VTCode Distribution Setup Script"
    echo "========================================"
    echo

    print_info "This script will help you set up VTCode distribution"
    echo

    setup_cargo
    echo
    setup_npm
    echo
    validate_packages
    echo
    test_build
    echo

    show_next_steps
}

# Run main function
main "$@"
