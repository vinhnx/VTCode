#!/usr/bin/env bash
# Compile Time Profiling Script for vtcode
# Usage: ./scripts/compile-profile.sh [command]
#
# Commands:
#   timings   - Generate cargo build timing report
#   llvm      - Analyze LLVM IR line counts (requires cargo-llvm-lines)
#   macros    - Show macro expansion stats (requires nightly)
#   clean     - Clean build with time measurement
#   all       - Run all profiling commands

set -e

# Source common utilities
# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_ROOT"

# Function to run cargo build timing report
run_timings() {
    log_info "Generating cargo build timing report..."
    cargo build --timings
    
    # Find and display the latest timing file (portable newest-file scan; macOS find has no -printf).
    TIMING_FILE=""
    for timing_candidate in target/cargo-timings/cargo-timing-*.html; do
        [[ -f "$timing_candidate" ]] || continue
        if [[ -z "$TIMING_FILE" || "$timing_candidate" -nt "$TIMING_FILE" ]]; then
            TIMING_FILE="$timing_candidate"
        fi
    done
    if [ -n "$TIMING_FILE" ]; then
        log_info "Timing report: $TIMING_FILE"
        if command -v open &> /dev/null; then
            open "$TIMING_FILE"
        fi
    fi
}

run_llvm_lines() {
    if ! command -v cargo-llvm-lines &> /dev/null; then
        log_warn "cargo-llvm-lines not installed. Installing..."
        cargo install cargo-llvm-lines
    fi
    
    log_info "Analyzing LLVM IR line counts for vtcode-core..."
    cargo llvm-lines --lib -p vtcode-core 2>&1 | head -60
}

run_macro_stats() {
    if ! rustup run nightly rustc --version &> /dev/null; then
        log_warn "Nightly toolchain not available. Install with: rustup install nightly"
        return 1
    fi
    
    log_info "Analyzing macro expansion stats (nightly)..."
    RUSTFLAGS="-Zmacro-stats" cargo +nightly build 2>&1 | grep -E "(macro-stats|Macro)" | head -50
}

run_clean_build() {
    log_info "Running clean build with timing..."
    cargo clean
    time cargo build
}

case "${1:-all}" in
    timings)
        run_timings
        ;;
    llvm)
        run_llvm_lines
        ;;
    macros)
        run_macro_stats
        ;;
    clean)
        run_clean_build
        ;;
    all)
        run_timings
        echo ""
        run_llvm_lines
        echo ""
        run_macro_stats || true
        ;;
    *)
        echo "Usage: $0 [timings|llvm|macros|clean|all]"
        exit 1
        ;;
esac
