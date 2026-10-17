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
#   - Quiet mode (--quiet): suppress progress banners and cargo noise; keep
#     errors/warnings and the final summary visible. Useful for agents and CI.

set -e

# Source common utilities
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# Configuration
SCRIPT_START=$(date +%s)
FAILED_CHECKS=0
RUN_TESTS=false
RUN_WORKSPACE=false
RUN_EXTRA_LINTS=false
RUN_CHANGED=false
QUIET=false
SCOPE_LABEL="default-members"

# Quiet-aware status printer: progress banners are suppressed in quiet mode.
# print_error/print_warning (from common.sh) always print — they carry the
# failure detail an agent or CI run needs.
print_status() {
	[ "$QUIET" = true ] && return 0
	printf '%b\n' "${BLUE}INFO:${NC} $1"
}

# Always-print status for outcome evidence (timing, final result) that must
# survive quiet mode.
print_always() {
	printf '%b\n' "${BLUE}INFO:${NC} $1"
}

# Timing helper
print_timing() {
	local end=$(date +%s)
	local duration=$((end - SCRIPT_START))
	if [ $duration -lt 60 ]; then
		echo ""
		print_always "Completed in ${duration}s"
	else
		local minutes=$((duration / 60))
		local seconds=$((duration % 60))
		echo ""
		print_always "Completed in ${minutes}m ${seconds}s"
	fi
}

# Check rustfmt availability
check_rustfmt() {
	if cargo fmt --version >/dev/null 2>&1; then
		return 0
	else
		print_error "rustfmt is not available. Install it with 'rustup component add rustfmt'."
		return 1
	fi
}

# Check clippy availability
check_clippy() {
	if cargo clippy --version >/dev/null 2>&1; then
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

DEFAULT_PACKAGE_NAMES=("vtcode" "vtcode-core" "vtcode-ui")
DEFAULT_MEMBERS=()
for package_name in "${DEFAULT_PACKAGE_NAMES[@]}"; do
	DEFAULT_MEMBERS+=("-p" "$package_name")
done

# Cargo verbosity flags for quiet mode: suppress tool noise while keeping
# errors (cargo -q keeps diagnostics on stderr for failures).
cargo_verbosity() {
	if [ "$QUIET" = true ]; then
		echo "-q"
	fi
}

run_clippy() {
	local scope_args=()
	local label="default-members"
	if [ "$RUN_WORKSPACE" = true ]; then
		scope_args=("--workspace")
		label="workspace"
	else
		scope_args=("${DEFAULT_MEMBERS[@]}")
	fi
	SCOPE_LABEL="$label"

	print_status "Running clippy ($SCOPE_LABEL)..."
	if cargo clippy --locked $(cargo_verbosity) "${scope_args[@]}" --all-targets --all-features -- -D warnings; then
		print_success "No clippy warnings found!"
		return 0
	else
		print_error "Clippy found issues. Please fix them."
		return 1
	fi
}

run_check() {
	local scope_args=()
	local label="default-members"
	if [ "$RUN_WORKSPACE" = true ]; then
		scope_args=("--workspace")
		label="workspace"
	else
		scope_args=("${DEFAULT_MEMBERS[@]}")
	fi
	SCOPE_LABEL="$label"

	print_status "Running cargo check ($SCOPE_LABEL)..."
	if cargo check --locked $(cargo_verbosity) "${scope_args[@]}"; then
		print_success "Compilation successful!"
		return 0
	else
		print_error "Compilation failed."
		return 1
	fi
}

run_tests() {
	local nextest_args=()
	local test_profile="default"

	if [ "$RUN_WORKSPACE" = true ]; then
		nextest_args+=("--workspace")
	fi

	if cargo nextest --version &>/dev/null; then
		if nextest_profile_available quick; then
			test_profile="quick"
		else
			print_warning "nextest profile 'quick' is unavailable; using the default profile."
		fi
	else
		print_error "cargo-nextest is required for tests. Install it with 'cargo install cargo-nextest --locked'."
		return 1
	fi
	nextest_args+=("--profile" "$test_profile")

	# Changed-crate mode: only test crates with changes since HEAD~1. nextest
	# does not provide cargo's `--changed --since` flags, so resolve changed
	# packages from Cargo metadata and pass explicit package filters instead.
	if [ "$RUN_CHANGED" = true ]; then
		if git rev-parse --git-dir >/dev/null 2>&1; then
			print_status "Detecting changed crates since HEAD~1..."
			local changed_package_names
			if ! changed_package_names=$(
				set -o pipefail
				cargo metadata --locked --no-deps --format-version 1 | python3 -c '
import json
import os
import subprocess
import sys

changed_files = subprocess.check_output(["git", "diff", "--name-only", "HEAD~1", "--"], text=True).splitlines()
changed_files += subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard"], text=True).splitlines()
changed_paths = [os.path.abspath(path) for path in changed_files if path.strip()]

for package in json.load(sys.stdin)["packages"]:
    package_root = os.path.dirname(os.path.abspath(package["manifest_path"]))
    for changed_path in changed_paths:
        try:
            if os.path.commonpath([package_root, changed_path]) == package_root:
                print(package["name"])
                break
        except ValueError:
            continue
'
			); then
				print_error "Failed to detect changed packages; test selection is unavailable."
				return 1
			fi

			local changed_package_args=()
			while IFS= read -r package_name; do
				[ -z "$package_name" ] && continue
				if [ "$RUN_WORKSPACE" = true ]; then
					changed_package_args+=("-p" "$package_name")
					continue
				fi
				for default_package in "${DEFAULT_PACKAGE_NAMES[@]}"; do
					if [ "$package_name" = "$default_package" ]; then
						changed_package_args+=("-p" "$package_name")
						break
					fi
				done
			done <<<"$changed_package_names"

			if [ ${#changed_package_args[@]} -eq 0 ]; then
				print_always "No changed packages in the selected test scope; skipping tests."
				return 0
			fi
			nextest_args+=("${changed_package_args[@]}")
		else
			print_warning "Not a git repo; ignoring --changed flag."
		fi
	fi

	print_status "Running tests ($SCOPE_LABEL)..."
	local test_exit=0

	# Enable incremental compilation for local test builds to avoid full
	# recompiles on every test run. Skip this when a distributed compiler
	# wrapper such as sccache (the repo's recommended dev setup, which sets
	# `incremental = false` in the dev profile) is active: sccache forbids
	# incremental compilation, so forcing `CARGO_INCREMENTAL=1` makes
	# `check-dev.sh --test` fail for sccache users.
	local nextest_env=()
	if [[ -z "${RUSTC_WRAPPER:-}" || "${RUSTC_WRAPPER}" != *sccache* ]]; then
		nextest_env=(CARGO_INCREMENTAL=1)
	fi
	local nextest_verbosity=()
	if [ "$QUIET" = true ]; then
		nextest_verbosity=("--status-level" "fail")
	fi
	env "${nextest_env[@]}" cargo nextest run --locked "${nextest_args[@]}" "${nextest_verbosity[@]}" --no-tests=fail || test_exit=$?

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

run_agent_legibility_lint() {
	print_status "Running agent legibility lint..."
	if python3 scripts/check_agent_legibility.py --mode warn; then
		print_success "Agent legibility lint completed."
		return 0
	else
		print_error "Agent legibility lint failed unexpectedly."
		return 1
	fi
}

run_shell_scripts_lint() {
	print_status "Running shell script lint (bash -n + nfo typo check)..."
	local failed=0
	local script
	for script in scripts/*.sh scripts/**/*.sh; do
		[ -f "$script" ] || continue
		if ! bash -n "$script" 2>&1; then
			print_error "bash -n failed for $script"
			failed=1
		fi
		if grep -Eq '^[[:space:]]*nfo\b' "$script"; then
			print_error "Found truncated 'nfo' command in $script (should be print_info):"
			grep -n '^[[:space:]]*nfo\b' "$script" >&2
			failed=1
		fi
		if command -v shellcheck >/dev/null 2>&1; then
			if ! shellcheck "$script" >/dev/null 2>&1; then
				if [ "$QUIET" = false ]; then
					print_warning "shellcheck warnings in $script (run shellcheck $script for details)"
				fi
			fi
		fi
	done
	if [ $failed -eq 0 ]; then
		print_success "Shell scripts passed lint!"
		return 0
	else
		print_error "Shell script lint failed."
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
	echo "Cargo checks, metadata, and tests use --locked to preserve Cargo.lock."
	echo ""
	echo "Options:"
	echo "  --test, -t          Also run tests (uses nextest quick profile when available)"
	echo "  --workspace, -w     Run checks on full workspace (default: default-members only)"
	echo "  --lints, -l         Run extra lints (structured logging, agent legibility)"
	echo "  --changed, -c       Only run tests in crates changed since HEAD~1 (implies --test)"
	echo "  --quiet, -q         Suppress progress banners and cargo noise; errors and"
	echo "                      the final summary still print (agent/CI friendly)"
	echo "  --help, -h          Show this help message"
	echo ""
	echo "Examples:"
	echo "  $0                  # Fast check (default-members only)"
	echo "  $0 --test           # Fast check + tests (quick profile when configured)"
	echo "  $0 --changed        # Tests only for crates changed since last commit"
	echo "  $0 --workspace      # Workspace-wide fast check"
	echo "  $0 -t -w -l         # Full dev check with tests, workspace, and lints"
	echo ""
	echo "For release/PR quality gate, run: ./scripts/check.sh"
}

main() {
	# Parse arguments
	while [[ $# -gt 0 ]]; do
		case "$1" in
		--test | -t)
			RUN_TESTS=true
			shift
			;;
		--workspace | -w)
			RUN_WORKSPACE=true
			shift
			;;
		--lints | -l)
			RUN_EXTRA_LINTS=true
			shift
			;;
		--changed | -c)
			RUN_CHANGED=true
			RUN_TESTS=true # --changed implies --test
			shift
			;;
		--quiet | -q)
			QUIET=true
			shift
			;;
		--help | -h)
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
	if [ "$RUN_CHANGED" = true ]; then
		print_status "Test mode: Changed crates only (since HEAD~1)"
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
		run_agent_legibility_lint || ((FAILED_CHECKS++))
		run_shell_scripts_lint || ((FAILED_CHECKS++))
	fi

	# Always run shell script syntax check (fast, no extra deps)
	run_shell_scripts_lint || ((FAILED_CHECKS++))

	# Run tests if requested (always last, as they're the slowest)
	if [ "$RUN_TESTS" = true ]; then
		run_tests || ((FAILED_CHECKS++))
	fi

	# Summary (always printed — this is the outcome evidence, even in quiet mode)
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
		if [ "$RUN_CHANGED" = true ]; then
			echo "Tip: Run without --changed for full test suite."
		fi
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
