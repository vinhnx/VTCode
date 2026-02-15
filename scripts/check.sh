#!/bin/bash

# vtcode Code Quality Check Script
# This script runs the same checks as our CI pipeline

set -e

echo "Running vtcode Code Quality Checks..."
echo "========================================"

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

# Check if rustfmt is installed
check_rustfmt() {
    print_status "Checking rustfmt..."
    if ! command -v rustfmt &> /dev/null; then
        print_warning "rustfmt not found. Installing..."
        rustup component add rustfmt
    fi

    if cargo fmt --version &> /dev/null; then
        print_success "rustfmt is available"
        return 0
    else
        print_error "Failed to install/find rustfmt"
        return 1
    fi
}

# Check if clippy is installed
check_clippy() {
    print_status "Checking clippy..."
    if ! command -v cargo-clippy &> /dev/null; then
        print_warning "clippy not found. Installing..."
        rustup component add clippy
    fi

    if cargo clippy --version &> /dev/null; then
        print_success "clippy is available"
        return 0
    else
        print_error "Failed to install/find clippy"
        return 1
    fi
}

# Run rustfmt check
run_rustfmt() {
    print_status "Running rustfmt check..."
    if cargo fmt --all -- --check; then
        print_success "Code formatting is correct!"
        return 0
    else
        print_error "Code formatting issues found. Run 'cargo fmt --all' to fix."
        return 1
    fi
}

# Run clippy
run_clippy() {
    print_status "Running clippy..."
    if cargo clippy --workspace --all-targets --all-features -- -D warnings; then
        print_success "No clippy warnings found!"
        return 0
    else
        print_error "Clippy found issues. Please fix them."
        return 1
    fi
}

# Run tests
run_tests() {
    print_status "Running tests..."
    if cargo test --workspace; then
        print_success "All tests passed!"
        return 0
    else
        print_error "Some tests failed."
        return 1
    fi
}

# Run build
run_build() {
    print_status "Building project..."
    if cargo build --workspace; then
        print_success "Build successful!"
        return 0
    else
        print_error "Build failed."
        return 1
    fi
}

# Check documentation
run_docs() {
    print_status "Checking documentation..."
    if cargo doc --workspace --no-deps --document-private-items; then
        print_success "Documentation generated successfully!"
        return 0
    else
        print_error "Documentation generation failed."
        return 1
    fi
}

# Run structured logging lint
run_structured_logging_lint() {
    print_status "Running structured logging lint..."
    if ./scripts/lint_structured_logging.sh; then
        print_success "Structured logging is correct!"
        return 0
    else
        print_error "Structured logging violations found. See ARCHITECTURAL_INVARIANTS.md #4."
        return 1
    fi
}

# Main function
main() {
    local failed_checks=0

    echo ""
    echo "Starting comprehensive code quality checks..."
    echo ""

    # Check prerequisites
    check_rustfmt || ((failed_checks++))
    check_clippy || ((failed_checks++))

    if [ $failed_checks -gt 0 ]; then
        print_error "Prerequisites not met. Exiting."
        exit 1
    fi

    echo ""
    echo "Running checks..."
    echo ""

    # Run all checks
    run_rustfmt || ((failed_checks++))
    run_structured_logging_lint || ((failed_checks++))
    run_clippy || ((failed_checks++))
    run_build || ((failed_checks++))
    run_tests || ((failed_checks++))
    run_docs || ((failed_checks++))

    echo ""
    echo "========================================"

    if [ $failed_checks -eq 0 ]; then
        print_success "All checks passed! Your code is ready for commit."
        echo ""
        echo "Tips:"
        echo "  • Run 'cargo fmt --all' to auto-format your code"
        echo "  • Run 'cargo clippy' to see clippy suggestions"
        echo "  • Run 'cargo doc --open' to view documentation"
        echo ""
        exit 0
    else
        print_error "$failed_checks check(s) failed. Please fix the issues above."
        echo ""
        echo "Quick fixes:"
        echo "  • Format code: cargo fmt --all"
        echo "  • Fix clippy: cargo clippy --fix"
        echo "  • Run again: ./scripts/check.sh"
        echo ""
        exit 1
    fi
}

# Parse command line arguments
case "${1:-}" in
    "fmt"|"format")
        check_rustfmt && run_rustfmt
        ;;
    "clippy"|"lint")
        check_clippy && run_clippy
        ;;
    "test")
        run_tests
        ;;
    "build")
        run_build
        ;;
    "docs"|"doc")
        run_docs
        ;;
    "help"|"-h"|"--help")
        echo "vtcode Code Quality Check Script"
        echo ""
        echo "Usage: $0 [COMMAND]"
        echo ""
        echo "Commands:"
        echo "  fmt     - Check code formatting with rustfmt"
        echo "  clippy  - Run clippy lints"
        echo "  test    - Run tests"
        echo "  build   - Build the project"
        echo "  docs    - Generate documentation"
        echo "  help    - Show this help message"
        echo ""
        echo "If no command is specified, runs all checks."
        ;;
    *)
        main
        ;;
esac
