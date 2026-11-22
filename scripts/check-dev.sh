#!/bin/bash

# VT Code Fast Development Check Script
# Optimized for rapid development iterations (typically 10-30s vs 2-5m for full check)
#
# Default checks (fast):
#   - rustfmt (formatting)
#   - clippy (linting, default-members only; its --all-targets run surfaces Cargo's
#     built-in `unused_dependencies` manifest lint, configured in Cargo.toml [lints.cargo])
#   - cargo check (compilation verification)
#   - file length gate (hard ceiling: 1000 lines per tracked text file)
#   - Rust file length report (soft: src 500 / tests 800; check.sh enforces)
#
# Optional checks (via flags):
#   - Tests (--test)
#   - Structured logging and Markdown lint (--lints)
#   - Workspace-wide checks (--workspace)
#   - CodeRabbit AI review (--review): opt-in, network-bound, minutes-long, and
#     limited per hour by the CodeRabbit plan, so it stays out of the default gate
#   - Quiet mode (--quiet): suppress progress banners and cargo noise; keep
#     errors/warnings and the final summary visible. Useful for agents and CI.

set -e

# Source common utilities
# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# Configuration
SCRIPT_START=$(date +%s)
FAILED_CHECKS=0
RUN_TESTS=false
RUN_WORKSPACE=false
RUN_EXTRA_LINTS=false
RUN_CHANGED=false
QUIET=false
RUN_DENY=false
RUN_REVIEW=${VTCODE_SELF_VERIFY_REVIEW:-false}
SKIP_CARGO_CACHE=false
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
	local end
	end=$(date +%s)
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

DEFAULT_PACKAGE_NAMES=("vtcode" "vtcode-core" "vtcode-skills" "vtcode-tui-runtime" "vtcode-ui")
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

# True when a compiler wrapper that rejects `-C incremental` is configured.
# Cargo accepts the wrapper through RUSTC_WRAPPER (sccache's own docs) or the
# build.rustc-wrapper config key (CARGO_BUILD_RUSTC_WRAPPER). Checking only the
# former makes `check-dev.sh --test` force CARGO_INCREMENTAL=1 for contributors
# who configure sccache the Cargo-native way, which sccache then rejects.
uses_sccache_wrapper() {
	local wrapper="${RUSTC_WRAPPER:-} ${CARGO_BUILD_RUSTC_WRAPPER:-}"
	[[ "$wrapper" == *sccache* ]]
}

# Fingerprint of everything that can change cargo's verdict: tracked Rust and
# manifest sources plus their current working-tree state. Used to skip the
# minutes-long cargo stages when a repeated run has nothing new to compile,
# which is the common case in a self-verification loop. Stored in the real git
# dir so it never appears in `git status` and therefore never perturbs its own
# key. `--git-dir` resolves correctly in a linked worktree, where `.git` is a file.
GIT_DIR_PATH=$(git rev-parse --git-dir 2>/dev/null || printf '.git')
RUST_STATE_FILE=${RUST_STATE_FILE:-${GIT_DIR_PATH}/check-dev-rust-state}

rust_state_key() {
	# Hash pathname/content pairs, not bare content hashes. A bare hash stream does
	# not encode the path, so staging a rename would leave the fingerprint
	# unchanged while `Cargo.toml` still points at the old module name.
	{
		git ls-files -z -- '*.rs' 'Cargo.toml' '*/Cargo.toml' 'rust-toolchain.toml' 'Cargo.lock' \
			'.cargo/config.toml' '.cargo/config' 2>/dev/null |
			tr '\0' '\n' |
			while IFS= read -r path; do
				[ -n "$path" ] || continue
				printf '%s %s\n' "$(git hash-object -- "$path" 2>/dev/null)" "$path"
			done
		# An untracked Rust source changes what cargo compiles, so include both its
		# path and its contents: a filename-only entry would miss an edit to it.
		git ls-files -z --others --exclude-standard -- '*.rs' 2>/dev/null |
			tr '\0' '\n' |
			sort |
			while IFS= read -r path; do
				[ -n "$path" ] || continue
				printf '%s %s\n' "$(git hash-object -- "$path" 2>/dev/null)" "$path"
			done
		# Flags and environment that change what cargo does, so a scope or compiler
		# flag change never reuses a verdict produced under different settings.
		printf 'scope=%s\n' "$SCOPE_LABEL"
		printf 'rustflags=%s\n' "${RUSTFLAGS:-}"
	} | git hash-object --stdin 2>/dev/null
}

# True when the last passing run had the same Rust inputs and the same scope.
rust_inputs_unchanged() {
	[ -f "$RUST_STATE_FILE" ] || return 1
	local recorded current
	recorded=$(cat "$RUST_STATE_FILE" 2>/dev/null)
	current=$(rust_state_key) || return 1
	[ -n "$current" ] && [ "$recorded" = "$current" ]
}

record_rust_state() {
	mkdir -p "$(dirname "$RUST_STATE_FILE")" 2>/dev/null || return 0
	rust_state_key >"$RUST_STATE_FILE" 2>/dev/null || true
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
	# Unquoted on purpose: cargo_verbosity prints one flag or nothing (which must vanish, not become "").
	# shellcheck disable=SC2046
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
	# Unquoted on purpose: see run_clippy above.
	# shellcheck disable=SC2046
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
	local changed_package_args=()

	if cargo nextest --version &>/dev/null; then
		# Full-workspace runs need the load-insensitive default profile: quick's
		# 10s slow-timeout trips on heavy tests under parallel load, and its
		# fail-fast then SIGTERMs/SIGKILLs a moving set of siblings (see
		# .vtcode/memory/issues.md). Keep quick for fast local iteration.
		if [ "$RUN_WORKSPACE" = true ]; then
			test_profile="default"
		elif nextest_profile_available quick; then
			test_profile="quick"
		else
			print_warning "nextest profile 'quick' is unavailable; using the default profile."
		fi
	else
		print_error "cargo-nextest is required for tests. Install it with 'cargo install cargo-nextest --locked'."
		return 1
	fi
	nextest_args+=("--profile" "$test_profile")

	# Machine-tuned concurrency for local runs. This suite is latency-bound
	# (child processes, sleeps, socket mocks), so 2x cores outperforms num-cpus
	# on this Apple M5 (10 cores): vtcode-core 41s -> 32s, full workspace
	# 157s -> 131s, quick gate 80s -> 58s. Cap at 20 to bound memory; hosts with
	# < 4 cores keep nextest's default. The quick profile's 20s slow ceiling is
	# load-aware (see .config/nextest.toml), so it tolerates the boost.
	# NEXTEST_TEST_THREADS overrides the computed value when set explicitly.
	if [ -n "${NEXTEST_TEST_THREADS:-}" ]; then
		nextest_args+=("--test-threads" "$NEXTEST_TEST_THREADS")
	else
		local cpu_count=""
		if command -v sysctl >/dev/null 2>&1; then
			cpu_count="$(sysctl -n hw.ncpu 2>/dev/null || true)"
		fi
		if [[ ! "$cpu_count" =~ ^[0-9]+$ ]] && command -v nproc >/dev/null 2>&1; then
			cpu_count="$(nproc 2>/dev/null || true)"
		fi
		if [[ "$cpu_count" =~ ^[0-9]+$ ]] && [ "$cpu_count" -ge 4 ]; then
			local tuned_threads=$((cpu_count * 2))
			if [ "$tuned_threads" -gt 20 ]; then
				tuned_threads=20
			fi
			nextest_args+=("--test-threads" "$tuned_threads")
		fi
	fi

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

changed_files = subprocess.check_output(["git", "diff", "--no-renames", "--name-only", "-z", "HEAD~1", "--"]).split(b"\0")
changed_files += subprocess.check_output(["git", "ls-files", "-z", "--others", "--exclude-standard"]).split(b"\0")
changed_paths = [os.path.abspath(os.fsdecode(path)) for path in changed_files if path]

packages_by_root = {
    os.path.dirname(os.path.abspath(package["manifest_path"])): package["name"]
    for package in json.load(sys.stdin)["packages"]
}
changed_packages = set()
# The workspace root is also a package, so stop at the closest package root.
for changed_path in changed_paths:
    directory = os.path.dirname(changed_path)
    while directory not in packages_by_root:
        parent = os.path.dirname(directory)
        if parent == directory:
            break
        directory = parent
    if directory in packages_by_root:
        changed_packages.add(packages_by_root[directory])

for package_name in sorted(changed_packages):
    print(package_name)
'
			); then
				print_error "Failed to detect changed packages; test selection is unavailable."
				return 1
			fi

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

	if [ "$RUN_WORKSPACE" = true ] && [ "${#changed_package_args[@]}" -eq 0 ]; then
		nextest_args+=("--workspace")
	fi

	print_status "Running tests ($SCOPE_LABEL)..."
	local test_exit=0

	# Enable incremental compilation for local test builds to avoid full
	# recompiles on every test run. Skip this when a compiler wrapper such as
	# sccache (the repo's recommended dev setup, which sets `incremental =
	# false` in the dev profile) is active: sccache rejects `-C incremental`,
	# so forcing `CARGO_INCREMENTAL=1` makes `check-dev.sh --test` fail for
	# sccache users. The helper checks both supported wrapper env vars.
	local nextest_env=()
	if ! uses_sccache_wrapper; then
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

# run_cargo_deny is provided by scripts/common.sh (shared with check.sh).

print_usage() {
	echo "VT Code Fast Development Check Script"
	echo ""
	echo "Usage: $0 [OPTIONS]"
	echo ""
	echo "Optimized for rapid development iterations. By default runs only:"
	echo "  - rustfmt (formatting)"
	echo "  - clippy (linting, default-members; its --all-targets run surfaces Cargo's"
	echo "     built-in unused_dependencies lint)"
	echo "  - cargo check (compilation)"
	echo "Cargo checks, metadata, and tests use --locked to preserve Cargo.lock."
	echo ""
	echo "Options:"
	echo "  --test, -t          Also run tests (uses nextest quick profile when available)"
	echo "  --workspace, -w     Run checks on full workspace (default: default-members only)"
	echo "  --lints, -l         Run extra lints (structured logging, agent legibility, Markdown)"
	echo "  --changed, -c       Only run tests in crates changed since HEAD~1 (implies --test)"
	echo "  --quiet, -q         Suppress progress banners and cargo noise; errors and"
	echo "                      the final summary still print (agent/CI friendly)"
	echo "  --deny, -d          Also run dependency audit (cargo-deny advisories/licenses/bans/"
	echo "                      sources + cargo-vet audit provenance; network-bound on first run)"
	echo "  --review, -r        Also run an opt-in CodeRabbit AI review of the current diff"
	echo "                      (network-bound, minutes-long, plan-limited; see"
	echo "                      scripts/coderabbit-review.sh --help). Also enabled by"
	echo "                      VTCODE_SELF_VERIFY_REVIEW=true, which is how the"
	echo "                      self-verification loop opts in."
	echo "  --no-cache          Force the cargo stages even when no Rust input changed"
	echo "                      since the last passing run (the skip is keyed on tracked"
	echo "                      sources, manifests, and their working-tree state)."
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

# Opt-in CodeRabbit review. Exit 2 means "gate blocked by a finding" and is
# reported distinctly so the summary does not read like a tool failure. The
# wrapper owns the throttle and grace period, so this stays a thin delegate.
run_coderabbit_review() {
	print_status "Running opt-in CodeRabbit review..."
	local status=0
	"$(dirname "${BASH_SOURCE[0]}")/coderabbit-review.sh" || status=$?
	case "$status" in
	0) print_success "CodeRabbit review passed." ;;
	2)
		print_error "CodeRabbit review blocked: findings at or above the gate severity."
		FAILED_CHECKS=$((FAILED_CHECKS + 1))
		;;
	*)
		print_error "CodeRabbit review could not complete."
		FAILED_CHECKS=$((FAILED_CHECKS + 1))
		;;
	esac
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
		--deny | -d)
			RUN_DENY=true
			shift
			;;
		--review | -r)
			RUN_REVIEW=true
			shift
			;;
		--no-cache)
			SKIP_CARGO_CACHE=true
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
	check_rustfmt || FAILED_CHECKS=$((FAILED_CHECKS + 1))
	check_clippy || FAILED_CHECKS=$((FAILED_CHECKS + 1))

	if [ $FAILED_CHECKS -gt 0 ]; then
		print_error "Prerequisites not met. Exiting."
		exit 1
	fi

	# Order by cost, cheapest first, so a self-verification loop learns about a
	# failure in ~1s instead of after the slowest cargo stage. Sub-second
	# governance gates and shell lint run before the minutes-long cargo work, and
	# rustfmt (seconds) runs before clippy (the slowest stage).
	run_shell_scripts_lint || FAILED_CHECKS=$((FAILED_CHECKS + 1))
	run_repo_governance_gates || FAILED_CHECKS=$((FAILED_CHECKS + 1))

	# Cargo stages: rustfmt is seconds, clippy dominates, check only verifies
	# feature combinations clippy does not cover. Keep rustfmt first so a
	# formatting-only mistake fails before paying for a full clippy pass.
	#
	# Skip the cargo block entirely when nothing it consumes has changed since the
	# last passing run. Cargo's own fingerprint cache still rebuilds on an actual
	# dependency change; this only avoids re-paying process startup and metadata
	# resolution for an unchanged tree, which is the normal self-verification
	# rerun. --no-cache forces the full path.
	local cargo_cached=false
	# Capture the fingerprint *before* the cargo decision. Recording it later (after
	# tests and an optional minutes-long review) would let an edit made during the
	# run be stored as passing, and the next run would skip checks that would catch
	# that exact edit.
	local checked_key=""
	if [ "$SKIP_CARGO_CACHE" != true ] && [ "$RUN_WORKSPACE" != true ]; then
		# Failure-tolerant: outside a git checkout the fingerprint cannot be built,
		# which simply means the fast path is unavailable rather than an error.
		checked_key=$(rust_state_key 2>/dev/null || true)
		if [ -n "$checked_key" ] && rust_inputs_unchanged; then
			cargo_cached=true
			print_always "Cargo unchanged since the last passing run; skipping rustfmt/clippy/check."
		fi
	fi

	if [ "$cargo_cached" = true ]; then
		:
	else
		run_rustfmt || FAILED_CHECKS=$((FAILED_CHECKS + 1))
		run_clippy || FAILED_CHECKS=$((FAILED_CHECKS + 1))
		run_check || FAILED_CHECKS=$((FAILED_CHECKS + 1))
	fi

	# Run extra lints if requested (Markdown lint needs Bun and is slow)
	if [ "$RUN_EXTRA_LINTS" = true ]; then
		run_structured_logging_lint || FAILED_CHECKS=$((FAILED_CHECKS + 1))
		run_markdown_lint_gate || FAILED_CHECKS=$((FAILED_CHECKS + 1))
		run_agent_legibility_lint || FAILED_CHECKS=$((FAILED_CHECKS + 1))
	fi

	# Run dependency audit if requested (network-bound on first run); the
	# explicit flag hard-requires the tool rather than skipping.
	if [ "$RUN_DENY" = true ]; then
		run_cargo_deny required || FAILED_CHECKS=$((FAILED_CHECKS + 1))
		run_cargo_vet required || FAILED_CHECKS=$((FAILED_CHECKS + 1))
	fi

	# Run tests if requested (always last, as they're the slowest)
	if [ "$RUN_TESTS" = true ]; then
		run_tests || FAILED_CHECKS=$((FAILED_CHECKS + 1))
	fi

	# Opt-in AI review. Deliberately outside the default gate: it needs network
	# access and a CodeRabbit login, takes minutes rather than seconds, and its
	# hourly allowance is small. Runs last so deterministic checks report first.
	if [ "$RUN_REVIEW" = true ]; then
		run_coderabbit_review
	fi

	# Summary (always printed — this is the outcome evidence, even in quiet mode)
	echo ""
	echo "========================================"
	print_timing

	if [ $FAILED_CHECKS -eq 0 ]; then
		# Only a fully green run may seed the fast path: caching after a failure
		# would let the next run skip the stage that just broke. Record the
		# fingerprint captured before the run, and only when the inputs still match
		# it, so an edit made during the run is not stored as verified.
		if [ -n "$checked_key" ] && [ "$(rust_state_key)" = "$checked_key" ]; then
			printf '%s\n' "$checked_key" >"$RUST_STATE_FILE" 2>/dev/null || true
		fi
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

# Run only when executed directly: sourcing lets tests exercise the helpers
# (rust_state_key, gate ordering) without starting a full check run.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
	main "$@"
fi
