#!/bin/bash

# VT Code Fast Development Check Script
# Optimized for rapid development iterations (typically 10-30s vs 2-5m for full check)
#
# Default checks (fast):
#   - rustfmt (formatting)
#   - clippy (linting, default-members only)
#   - cargo check (compilation verification)
#
# Optional checks (via flags):
#   - Tests (--test)
#   - Structured logging lint (--lints)
#   - Workspace-wide checks (--workspace)

set -e

# Source common utilities
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# Configuration
SCRIPT_START=$(date +%s)
FAILED_CHECKS=0
RUN_TESTS=false
RUN_WORKSPACE=false
RUN_EXTRA_LINTS=false
SCOPE_LABEL="default-members"

# Timing helper
print_timing() {
    local end=$(date +%s)
    local duration=$((end - SCRIPT_START))
    if [ $duration -lt 60 ]; then
        echo ""
        print_status "Completed in ${duration}s"
    else
        local minutes=$((duration / 60))
        local seconds=$((duration % 60))
        echo ""
        print_status "Completed in ${minutes}m ${seconds}s"
    fi
}

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

run_clippy() {
    local scope_args=""
    if [ "$RUN_WORKSPACE" = true ]; then
        scope_args="--workspace"
        SCOPE_LABEL="workspace"
    else
        scope_args=""
        SCOPE_LABEL="default-members"
    fi

    print_status "Running clippy ($SCOPE_LABEL)..."
    if cargo clippy $scope_args --all-targets --all-features -- -D warnings; then
        print_success "No clippy warnings found!"
        return 0
    else
        print_error "Clippy found issues. Please fix them."
        return 1
    fi
}

run_check() {
    local scope_args=""
    if [ "$RUN_WORKSPACE" = true ]; then
        scope_args="--workspace"
        SCOPE_LABEL="workspace"
    else
        scope_args=""
        SCOPE_LABEL="default-members"
    fi

    print_status "Running cargo check ($SCOPE_LABEL)..."
    if cargo check $scope_args; then
        print_success "Compilation successful!"
        return 0
    else
        print_error "Compilation failed."
        return 1
    fi
}

run_tests() {
    local scope_args=""
    if [ "$RUN_WORKSPACE" = true ]; then
        scope_args="--workspace"
    else
        scope_args=""
    fi

    print_status "Running tests ($SCOPE_LABEL)..."
    local test_exit=0

    if cargo nextest --version &> /dev/null; then
        cargo nextest run $scope_args || test_exit=$?
    else
        print_warning "cargo-nextest not found. Falling back to cargo test."
        cargo test $scope_args || test_exit=$?
    fi

    if [ $test_exit -eq 0 ]; then
        print_success "All tests passed!"
        return 0
    else
        print_error "Some tests failed."
        return 1
    fi
}

run_structured_logging_lint() {
    print_status "Running structured logging lint..."
    if ./scripts/lint_structured_logging.sh; then
        print_success "Structured logging is correct!"
        return 0
    else
        print_error "Structured logging violations found."
        return 1
    fi
}

print_usage() {
    echo "VT Code Fast Development Check Script"
    echo ""
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Optimized for rapid development iterations. By default runs only:"
    echo "  - rustfmt (formatting)"
    echo "  - clippy (linting, default-members)"
    echo "  - cargo check (compilation)"
    echo ""
    echo "Options:"
    echo "  --test, -t          Also run tests (slower)"
    echo "  --workspace, -w     Run checks on full workspace (default: default-members only)"
    echo "  --lints, -l         Run extra lints (structured logging, etc)"
    echo "  --help, -h          Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0                  # Fast check (default-members only)"
    echo "  $0 --test           # Fast check + tests"
    echo "  $0 --workspace      # Workspace-wide fast check"
    echo "  $0 -t -w -l         # Full dev check with tests, workspace, and lints"
    echo ""
    echo "For release/PR quality gate, run: ./scripts/check.sh"
}

main() {
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --test|-t)
                RUN_TESTS=true
                shift
                ;;
            --workspace|-w)
                RUN_WORKSPACE=true
                shift
                ;;
            --lints|-l)
                RUN_EXTRA_LINTS=true
                shift
                ;;
            --help|-h)
                print_usage
                exit 0
                ;;
            *)
                print_error "Unknown argument: $1"
                echo "Run '$0 --help' for usage information."
                exit 1
                ;;
        esac
    done

    echo ""
    echo "Starting fast development checks..."
    if [ "$RUN_WORKSPACE" = true ]; then
        print_status "Scope: Full workspace"
    else
        print_status "Scope: Default members only"
    fi
    echo ""

    # Check prerequisites
    check_rustfmt || ((FAILED_CHECKS++))
    check_clippy || ((FAILED_CHECKS++))

    if [ $FAILED_CHECKS -gt 0 ]; then
        print_error "Prerequisites not met. Exiting."
        exit 1
    fi

    # Run essential checks (parallelizable in theory, but sequential for clear error messages)
    run_rustfmt || ((FAILED_CHECKS++))
    run_clippy || ((FAILED_CHECKS++))
    run_check || ((FAILED_CHECKS++))

    # Run extra lints if requested
    if [ "$RUN_EXTRA_LINTS" = true ]; then
        run_structured_logging_lint || ((FAILED_CHECKS++))
    fi

    # Run tests if requested (always last, as they're the slowest)
    if [ "$RUN_TESTS" = true ]; then
        run_tests || ((FAILED_CHECKS++))
    fi

    # Summary
    echo ""
    echo "========================================"
    print_timing

    if [ $FAILED_CHECKS -eq 0 ]; then
        print_success "All checks passed! Your code is ready for commit."
        echo ""
        if [ "$RUN_TESTS" = false ]; then
            echo "Note: Tests were not run. Add --test to include them."
        fi
        echo "For full release quality gate, run: ./scripts/check.sh"
        echo ""
        exit 0
    else
        print_error "$FAILED_CHECKS check(s) failed. Please fix the issues above."
        echo ""
        echo "Quick fixes:"
        echo "  • Format code: cargo fmt --all"
        echo "  • Fix clippy: cargo clippy --fix"
        echo "  • Run again: $0"
        echo ""
        exit 1
    fi
}

main "$@"
