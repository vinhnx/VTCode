#!/bin/bash

# VT Code Code Quality Check Script
# This script runs local quality checks with nextest-first test execution

set -e

# Source common utilities
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

echo "Running VT Code Quality Checks..."
echo "========================================"

# Check rustfmt availability
check_rustfmt() {
    if cargo fmt --version > /dev/null 2>&1; then
        return 0
    else
        print_error "rustfmt is not available. Install it with 'rustup component add rustfmt'."
        return 1
    fi
}

# Check clippy availability
check_clippy() {
    if cargo clippy --version > /dev/null 2>&1; then
        return 0
    else
        print_error "clippy is not available. Install it with 'rustup component add clippy'."
        return 1
    fi
}

find_ast_grep() {
    if command -v ast-grep > /dev/null 2>&1; then
        command -v ast-grep
        return 0
    fi

    if command -v sg > /dev/null 2>&1; then
        command -v sg
        return 0
    fi

    return 1
}

run_vtcode_command() {
    if command -v vtcode > /dev/null 2>&1; then
        vtcode "$@"
        return $?
    fi

    if [ -x "./target/debug/vtcode" ]; then
        ./target/debug/vtcode "$@"
        return $?
    fi

    if command -v cargo > /dev/null 2>&1 && [ -f "Cargo.toml" ]; then
        cargo run --quiet --bin vtcode -- "$@"
        return $?
    fi

    print_error "VT Code is not available. Install it or build the local binary before running '$0 ast-grep'."
    return 1
}

# Function to run rustfmt check
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
    local test_exit=0

    if cargo nextest --version &> /dev/null; then
        print_status "Using cargo-nextest..."
        cargo nextest run || test_exit=$?
    else
        print_warning "cargo-nextest not found. Falling back to cargo test."
        cargo test --workspace || test_exit=$?
    fi

    if [ $test_exit -eq 0 ]; then
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

run_ast_grep_scan() {
    local require_binary="${1:-optional}"
    if [ "$require_binary" != "required" ]; then
        if ! find_ast_grep > /dev/null 2>&1; then
            print_warning "ast-grep is not available. Skipping repository scan. Install it with 'vtcode dependencies install ast-grep'."
            return 0
        fi
    fi

    print_status "Running ast-grep rule tests and scan via VT Code..."
    if run_vtcode_command check ast-grep; then
        print_success "ast-grep rules passed!"
        return 0
    fi

    print_error "VT Code ast-grep check failed."
    return 1
}

# Run Zen governance checks (unwrap/expect enforced, other checks warning-only)
run_zen_governance() {
    print_status "Running Zen governance checks (unwrap/expect enforce mode)..."
    if python3 scripts/check_rust_file_length.py --mode warn --max-lines 500 \
        && python3 scripts/check_no_unwrap_expect_prod.py --mode enforce --allowlist scripts/zen_allowlist.txt \
        && python3 scripts/check_zen_allowlist.py --mode warn --allowlist scripts/zen_allowlist.txt; then
        print_success "Zen governance checks completed."
        return 0
    else
        print_error "Zen governance checks failed unexpectedly."
        return 1
    fi
}

# Run Miri (detect undefined behavior)
run_miri() {
    print_status "Running Miri (detecting Undefined Behavior/aliasing issues)..."
    print_warning "Miri can be slow as it interprets the code. Running a subset by default."
    if cargo miri test --locked; then
        print_success "Miri found no Undefined Behavior!"
        return 0
    else
        print_error "Miri detected issues! Check output for Stacked Borrows/aliasing violations."
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
    run_zen_governance || ((failed_checks++))
    run_ast_grep_scan || ((failed_checks++))
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
        echo "VT Code Code Quality Check Script"
        echo ""
        echo "Usage: $0 [COMMAND]"
        echo ""
        echo "Commands:"
        echo "  fmt     - Check code formatting with rustfmt"
        echo "  clippy  - Run clippy lints"
        echo "  ast-grep - Run repo ast-grep rule tests and scan via 'vtcode check ast-grep'"
        echo "  test    - Run tests"
        echo "  build   - Build the project"
        echo "  docs    - Generate documentation"
        echo "  zen     - Run Zen governance checks (warn mode)"
        echo "  miri    - Run Miri to detect Undefined Behavior (slow)"
        echo "  help    - Show this help message"
        echo ""
        echo "If no command is specified, runs all checks."
        ;;
    "zen")
        run_zen_governance
        ;;
    "ast-grep"|"astgrep")
        run_ast_grep_scan required
        ;;
    "miri")
        run_miri
        ;;
    *)
        main
        ;;
esac
