#!/usr/bin/env bash

# Common utilities and configuration for VT Code scripts

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
# Palette entries consumed by scripts that source this file (e.g. release-preflight.sh, check-release-status.sh).
# shellcheck disable=SC2034
PURPLE='\033[0;35m'
# shellcheck disable=SC2034
MAGENTA='\033[0;35m'
# shellcheck disable=SC2034
CYAN='\033[0;36m'
NC='\033[0m'

# Cargo configuration to prevent timeouts
export CARGO_HTTP_TIMEOUT=${CARGO_HTTP_TIMEOUT:-300}
export CARGO_NET_RETRY=${CARGO_NET_RETRY:-5}

# Disable OS keyring access during checks/tests so test binaries do not trigger
# repeated macOS Keychain authorization prompts. Credential code falls back to
# encrypted-file storage. Override by exporting VTCODE_DISABLE_KEYRING=0 first.
export VTCODE_DISABLE_KEYRING=${VTCODE_DISABLE_KEYRING:-1}

# Logging functions
print_info() {
    printf '%b\n' "${BLUE}INFO:${NC} $1"
}

print_status() {
    printf '%b\n' "${BLUE}INFO:${NC} $1"
}

print_success() {
    printf '%b\n' "${GREEN}SUCCESS:${NC} $1"
}

print_warning() {
    printf '%b\n' "${YELLOW}WARNING:${NC} $1"
}

print_error() {
    printf '%b\n' "${RED}ERROR:${NC} $1"
}

# Return success when a named profile is explicitly defined in the workspace
# Nextest configuration. Local `.config/nextest.toml` files are intentionally
# ignored, so scripts must not assume optional profiles exist. Avoid invoking
# test discovery here: `show-config test-groups` can compile the workspace.
nextest_profile_available() {
    local profile_name="${1:?profile name is required}"
    local script_root
    script_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
    local config_file="${script_root}/.config/nextest.toml"

    [ -f "$config_file" ] || return 1
    grep -Eq "^[[:space:]]*\\[profile\\.${profile_name}(\\]|\\.)" "$config_file"
}

# Compatibility logging functions (from various scripts)
log_info() {
    printf '%b\n' "${BLUE}ℹ${NC} $1" >&2
}

log_success() {
    printf '%b\n' "${GREEN}✓${NC} $1" >&2
}

log_warning() {
    printf '%b\n' "${YELLOW}⚠${NC} $1" >&2
}

log_error() {
    printf '%b\n' "${RED}✗${NC} $1" >&2
}

# Version detection
get_current_version() {
    local line
    # Try current directory first
    if [[ -f "Cargo.toml" ]]; then
        line=$(grep '^version = ' Cargo.toml | head -1 2>/dev/null || echo "")
    fi
    
    if [[ -z "$line" ]]; then
        # Try to find version in workspace members if not in root
        local root_toml
        root_toml="$(dirname "${BASH_SOURCE[0]}")/../Cargo.toml"
        if [[ -f "$root_toml" ]]; then
            line=$(grep '^version = ' "$root_toml" | head -1 2>/dev/null || echo "")
        fi
    fi
    
    if [[ -z "$line" ]]; then
        # Last resort: find any Cargo.toml
        line=$(find . -maxdepth 2 -name Cargo.toml -exec grep '^version = ' {} + | head -1 2>/dev/null || echo "")
    fi
    
    if [[ -z "$line" ]]; then
        echo "0.82.1" # Fallback to latest known
    else
        local stripped="${line#*\"}"
        echo "${stripped%%\"*}"
    fi
}

# Audit dependencies against the RustSec Advisory Database plus license, ban,
# and source policy via cargo-deny (policy in deny.toml). CI runs the same
# policy in .github/workflows/deny.yml. Needs network access on first run to
# fetch the advisory DB, then uses the local cache. Shared by check.sh (full
# gate) and check-dev.sh (--deny opt-in); print_status/print_success come from
# this file or the caller's quiet-aware overrides.
# Pass "required" to fail instead of skipping when cargo-deny is not installed
# (explicit `deny` subcommand / --deny flag), mirroring run_ast_grep_scan.
# Repository governance gates shared by check-dev.sh and check.sh, so the fast
# gate and the release gate cannot drift apart. Each check is a sub-second
# Python script that exits non-zero with its own remediation text.
run_python_gate() {
    local success_message=$1 failure_message=$2
    shift 2
    if python3 "$@"; then
        print_success "$success_message"
        return 0
    fi
    print_error "$failure_message"
    return 1
}

run_file_length_gate() {
    print_status "Running file length gate (max 1000 lines)..."
    run_python_gate "File lengths within budget!" "File length violations found." \
        scripts/check_file_length.py --mode enforce --max-lines 1000 --quiet
}

run_repo_governance_gates() {
    local failed=0
    run_file_length_gate || failed=1
    print_status "Checking local links in docs and AGENTS.md files..."
    run_python_gate "Docs and AGENTS.md links resolve!" \
        "Broken docs links or overlong module AGENTS.md lines found." \
        scripts/check_docs_links.py || failed=1
    print_status "Checking top-level docs placement..."
    run_python_gate "Top-level docs placement is valid!" "Unexpected top-level docs files found." \
        scripts/check_markdown_location.py || failed=1
    print_status "Checking tracked file sizes..."
    run_python_gate "Tracked file sizes within budget!" "Oversized tracked files found." \
        scripts/check_large_files.py || failed=1
    return "$failed"
}

# Markdown lint needs Node (`npm install`), so it runs in the release gate and
# `check-dev.sh --lints` rather than the node-free fast gate.
run_markdown_lint_gate() {
    print_status "Running Markdown lint..."
    run_python_gate "Markdown lint passed!" "Markdown lint failed (exit 2 means the Node toolchain is missing)." \
        scripts/check_markdown.py
}

run_cargo_deny() {
    local require_binary="${1:-optional}"
    if [ "$require_binary" != "required" ]; then
        if ! command -v cargo-deny > /dev/null 2>&1; then
            print_warning "cargo-deny is not available. Skipping dependency audit (CI enforces the same policy via .github/workflows/deny.yml). Install it with 'brew install cargo-deny' or 'cargo install cargo-deny --locked'."
            return 0
        fi
    fi
    print_status "Running cargo-deny (RustSec advisories, licenses, bans, sources)..."
    # --locked is a global cargo-deny flag and must precede the subcommand.
    if cargo deny --locked check; then
        print_success "cargo-deny checks passed!"
        return 0
    else
        print_error "cargo-deny found policy violations. Bump the offending dependency or justify an ignore in deny.toml."
        return 1
    fi
}

run_cargo_vet() {
    local require_binary="${1:-optional}"
    if [ "$require_binary" != "required" ]; then
        if ! command -v cargo-vet > /dev/null 2>&1; then
            print_warning "cargo-vet is not available. Skipping audit-provenance check (CI enforces the same policy via .github/workflows/deny.yml). Install it with 'cargo binstall cargo-vet' or 'cargo install cargo-vet --locked'."
            return 0
        fi
    fi
    print_status "Running cargo-vet (third-party audit provenance)..."
    # --locked reuses imports.lock without fetching new upstream audits, so the
    # check is deterministic. Refresh imports explicitly with plain `cargo vet`.
    if cargo vet --locked; then
        print_success "cargo-vet checks passed!"
        return 0
    else
        print_error "cargo-vet found unaudited dependencies. Audit them with 'cargo vet certify' or record an exemption, then shrink supply-chain/config.toml over time."
        return 1
    fi
}
