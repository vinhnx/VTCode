#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# fuzz-test.sh — Weekly fuzz testing routine for vtcode
#
# Runs proptest-based fuzz tests (fast, stable) and, if nightly toolchain is
# available, also runs cargo-fuzz targets (slow, LLVM coverage).
#
# Usage:
#   ./scripts/fuzz-test.sh            # proptest fuzz only (fast ~1min)
#   ./scripts/fuzz-test.sh --all      # proptest + cargo-fuzz (~20-30min)
#   ./scripts/fuzz-test.sh --cargo-fuzz  # cargo-fuzz only
#
# Schedule: Run weekly (CRON: 0 9 * * 1  cd /repo && ./scripts/fuzz-test.sh)
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color
PASS=0
FAIL=0

cleanup() {
    exit_code=$?
    echo ""
    echo "=============================="
    echo -e "${GREEN}Passed:${NC} $PASS"
    echo -e "${RED}Failed:${NC} $FAIL"
    echo "=============================="
    exit "$exit_code"
}
trap cleanup EXIT

run_test() {
    local desc="$1"
    shift
    echo ""
    echo "──────────────────────────────"
    echo -e "${YELLOW}[fuzz]${NC} $desc"
    echo "──────────────────────────────"
    if "$@" 2>&1; then
        echo -e "${GREEN}  ✓ PASS${NC}"
        PASS=$((PASS + 1))
    else
        echo -e "${RED}  ✗ FAIL${NC}"
        FAIL=$((FAIL + 1))
    fi
}

# ─── Phase 1: proptest-based fuzz tests (fast, works on stable) ────

run_test "proptest: core_fuzz_props (dangerous_commands, policy, paths, parser)" \
    cargo test -p vtcode-core --test core_fuzz_props

run_test "proptest: loop_detector_props" \
    cargo test -p vtcode-core --test loop_detector_props

run_test "unit: dangerous_commands" \
    cargo test -p vtcode-core command_safety::dangerous_commands::tests

run_test "unit: shell_parser" \
    cargo test -p vtcode-core command_safety::shell_parser::tests

run_test "unit: exec_policy::policy" \
    cargo test -p vtcode-core exec_policy::policy::tests

run_test "unit: exec_policy::parser" \
    cargo test -p vtcode-core exec_policy::parser::tests

run_test "unit: tools::validation::paths" \
    cargo test -p vtcode-core tools::validation::paths::tests

# ─── Phase 2: cargo-fuzz (slow, nightly-only, LLVM coverage) ────

if [ $# -eq 0 ] || [ "$1" = "--cargo-fuzz" ] || [ "$1" = "--all" ]; then
    if command -v cargo-fuzz &>/dev/null && rustc +nightly --version &>/dev/null; then
        echo ""
        echo "=============================================="
        echo -e "${YELLOW}Phase 2: cargo-fuzz targets (nightly)${NC}"
        echo "=============================================="

        run_test "cargo-fuzz: shell_parser (30s)" \
            bash -c "cd fuzz && RUSTC_WRAPPER='' CARGO_BUILD_RUSTC_WRAPPER='' CARGO_INCREMENTAL=1 cargo +nightly fuzz run shell_parser -- -max_total_time=30 -runs=50000 2>&1"

        run_test "cargo-fuzz: exec_policy_parser (30s)" \
            bash -c "cd fuzz && RUSTC_WRAPPER='' CARGO_BUILD_RUSTC_WRAPPER='' CARGO_INCREMENTAL=1 cargo +nightly fuzz run exec_policy_parser -- -max_total_time=30 -runs=50000 2>&1"

        run_test "cargo-fuzz: unified_path_validation (30s)" \
            bash -c "cd fuzz && RUSTC_WRAPPER='' CARGO_BUILD_RUSTC_WRAPPER='' CARGO_INCREMENTAL=1 cargo +nightly fuzz run unified_path_validation -- -max_total_time=30 -runs=50000 2>&1"

        run_test "cargo-fuzz: dangerous_commands (30s)" \
            bash -c "cd fuzz && RUSTC_WRAPPER='' CARGO_BUILD_RUSTC_WRAPPER='' CARGO_INCREMENTAL=1 cargo +nightly fuzz run dangerous_commands -- -max_total_time=30 -runs=50000 2>&1"

        run_test "cargo-fuzz: exec_policy_command_validation (30s)" \
            bash -c "cd fuzz && RUSTC_WRAPPER='' CARGO_BUILD_RUSTC_WRAPPER='' CARGO_INCREMENTAL=1 cargo +nightly fuzz run exec_policy_command_validation -- -max_total_time=30 -runs=50000 2>&1"
    else
        echo ""
        echo -e "${YELLOW}[skip] cargo-fuzz: nightly toolchain or cargo-fuzz not available${NC}"
    fi
fi

echo ""
echo -e "${GREEN}All fuzz tests completed.${NC}"
