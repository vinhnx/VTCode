#!/bin/bash

# vtcode Development Environment Setup Script
# This script sets up the development environment with all necessary tools

set -e

echo "Setting up vtcode Development Environment..."
echo "=============================================="

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print status messages
print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

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

# Install development tools
install_dev_tools() {
    print_status "Installing development tools..."

    # List of tools to install
    local tools=(
        "cargo-audit:Security auditing"
        "cargo-outdated:Check for outdated dependencies"
        "cargo-udeps:Find unused dependencies"
        "cargo-msrv:Find minimum supported Rust version"
        "cargo-license:Check dependency licenses"
        "cargo-tarpaulin:Code coverage"
        "cargo-bench:Performance benchmarking"
        "ripgrep:Fast text search tool"
        )

    for tool_info in "${tools[@]}"; do
        local tool=$(echo "$tool_info" | cut -d: -f1)
        local description=$(echo "$tool_info" | cut -d: -f2)

        print_status "Installing $tool ($description)..."
        if cargo install "$tool" --locked; then
            print_success "$tool installed successfully"
        else
            print_warning "Failed to install $tool (non-critical)"
        fi
    done
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
    echo "This script will set up your development environment for vtcode."
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
    install_dev_tools
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
    echo "  • Use 'cargo nextest run' to run tests (or 'cargo test' as fallback)"
    echo ""
    echo "Useful commands:"
    echo "  • Format code: cargo fmt --all"
    echo "  • Lint code: cargo clippy -- -D warnings"
    echo "  • Run tests: cargo nextest run --workspace (preferred) or cargo test --workspace"
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
vtcode Development Environment Setup Script

Usage: $0 [OPTIONS]

Options:
  --with-hooks    Set up git hooks for pre-commit checks
  --help, -h      Show this help message

This script will:
  • Check Rust installation
  • Update Rust toolchain
  • Install rustfmt and clippy components
  • Install development tools (cargo-audit, cargo-outdated, etc.)
  • Optionally set up git hooks
  • Verify everything works

After running this script, you can use:
  • ./scripts/check.sh - Run comprehensive code quality checks
  • cargo fmt --all - Format code
  • cargo clippy - Lint code
  • cargo test - Run tests

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