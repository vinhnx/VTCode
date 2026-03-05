#!/bin/bash

# VT Code Development Environment Setup Script
# This script sets up the development environment with baseline tooling

set -e

# Source common utilities
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

echo "Setting up VT Code Development Environment..."
echo "=============================================="

# Check if Rust is installed
check_rust() {
    print_status "Checking Rust installation..."
    if ! command -v cargo &> /dev/null; then
        print_error "Rust/Cargo not found. Please install Rust first:"
        echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        echo "  source ~/.cargo/env"
        exit 1
    fi

    print_success "Rust is installed: $(cargo --version)"
    print_success "Cargo is available: $(cargo --version)"
}

# Update Rust toolchain
update_rust() {
    print_status "Updating Rust toolchain..."
    rustup update
    print_success "Rust toolchain updated"
}

# Install required components
install_components() {
    print_status "Installing Rust components..."

    # List of components to install
    local components=("rustfmt" "clippy")

    for component in "${components[@]}"; do
        if rustup component list | grep -q "$component.*installed"; then
            print_success "$component is already installed"
        else
            print_status "Installing $component..."
            rustup component add "$component"
            print_success "$component installed"
        fi
    done
}

# Install baseline development tools
install_baseline_tools() {
    print_status "Checking baseline development tools..."

    if cargo nextest --version &> /dev/null; then
        print_success "cargo-nextest is already installed: $(cargo nextest --version)"
    else
        print_status "Installing cargo-nextest..."
        if cargo install cargo-nextest --locked; then
            print_success "cargo-nextest installed successfully"
        else
            print_warning "Failed to install cargo-nextest. Falling back to 'cargo test' remains available."
        fi
    fi
}

# Setup git hooks (optional)
setup_git_hooks() {
    if [ "${1:-}" = "--with-hooks" ]; then
        print_status "Setting up git hooks..."

        # Create pre-commit hook
        local hook_dir=".git/hooks"
        local pre_commit_hook="$hook_dir/pre-commit"

        if [ -d "$hook_dir" ]; then
            cat > "$pre_commit_hook" << 'EOF'
#!/bin/bash
# Pre-commit hook to run code quality checks

echo "Running pre-commit checks..."

# Run format check
if ! cargo fmt --all -- --check; then
    echo "Code formatting issues found. Run 'cargo fmt --all' to fix."
    exit 1
fi

# Run clippy
if ! cargo clippy -- -D warnings; then
    echo "Clippy found issues. Please fix them."
    exit 1
fi

echo "Pre-commit checks passed!"
EOF

            chmod +x "$pre_commit_hook"
            print_success "Pre-commit hook created"
        else
            print_warning "Git repository not found, skipping git hooks setup"
        fi
    fi
}

# Verify installation
verify_installation() {
    print_status "Verifying installation..."

    # Check rustfmt
    if cargo fmt --version &> /dev/null; then
        print_success "rustfmt: $(cargo fmt --version)"
    else
        print_error "rustfmt not working properly"
    fi

    # Check clippy
    if cargo clippy --version &> /dev/null; then
        print_success "clippy: $(cargo clippy --version)"
    else
        print_error "clippy not working properly"
    fi

    # Check cargo-nextest (optional but recommended)
    if cargo nextest --version &> /dev/null; then
        print_success "cargo-nextest: $(cargo nextest --version)"
    else
        print_warning "cargo-nextest not detected. 'cargo test --workspace' can be used as fallback."
    fi

    # Test build
    print_status "Testing project build..."
    if cargo check; then
        print_success "Project builds successfully"
    else
        print_error "Project build failed"
        exit 1
    fi
}

# Main function
main() {
    echo ""
    echo "This script will set up your development environment for VT Code."
    echo ""

    # Parse arguments
    local with_hooks=false
    if [ "${1:-}" = "--with-hooks" ]; then
        with_hooks=true
    fi

    # Run setup steps
    check_rust
    update_rust
    install_components
    install_baseline_tools
    setup_git_hooks "$with_hooks"
    verify_installation

    echo ""
    echo "=============================================="
    print_success "Development environment setup complete!"
    echo ""
    echo "Next steps:"
    echo "  • Create a .env file with your API keys (copy .env.example)"
    echo "  • Run './scripts/check.sh' to verify everything works"
    echo "  • Use 'cargo fmt --all' to format your code"
    echo "  • Use 'cargo clippy' to lint your code"
    echo "  • Use 'cargo nextest run' to run tests quickly"
    echo ""
    echo "Useful commands:"
    echo "  • Format code: cargo fmt --all"
    echo "  • Lint code: cargo clippy -- -D warnings"
    echo "  • Run tests: cargo nextest run (fallback: cargo test --workspace)"
    echo "  • Build docs: cargo doc --workspace --open"
    echo "  • Check everything: ./scripts/check.sh"
    echo ""
    if [ "$with_hooks" = true ]; then
        echo "Git hooks have been set up to run checks before commits."
        echo ""
    fi
    exit 0
}

# Help function
show_help() {
    cat << EOF
VT Code Development Environment Setup Script

Usage: $0 [OPTIONS]

Options:
  --with-hooks    Set up git hooks for pre-commit checks
  --help, -h      Show this help message

This script will:
  • Check Rust installation
  • Update Rust toolchain
  • Install rustfmt and clippy components
  • Install baseline development tools (cargo-nextest)
  • Optionally set up git hooks
  • Verify everything works

After running this script, you can use:
  • ./scripts/check.sh - Run comprehensive code quality checks
  • cargo fmt --all - Format code
  • cargo clippy - Lint code
  • cargo nextest run - Run tests (fallback: cargo test --workspace)

To configure API keys:
  • Copy .env.example to .env and add your actual API keys
  • Or set environment variables directly
  • Or configure in vtcode.toml (less secure)

EOF
}

# Parse command line arguments
case "${1:-}" in
    "--help"|"-h")
        show_help
        ;;
    "--with-hooks")
        main --with-hooks
        ;;
    "")
        main
        ;;
    *)
        print_error "Unknown option: $1"
        echo "Use '$0 --help' for usage information."
        exit 1
        ;;
esac
