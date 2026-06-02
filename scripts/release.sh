#!/usr/bin/env bash

# VT Code Release Script
#
# Orchestrator for the full release pipeline.
# Default: macOS local, Linux/Windows via zigbuild (saves ~30-60min CI wait).
# Falls back to CI if zig unavailable.
#
# Usage: ./scripts/release.sh [version|level] [options]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"
source "$SCRIPT_DIR/release-lib.sh"

RELEASE_NOTES_FILE=$(mktemp)
trap 'rm -f "$RELEASE_NOTES_FILE"' EXIT

# ── Help ─────────────────────────────────────────────────────────────

show_usage() {
    cat <<'USAGE'
Usage: ./scripts/release.sh [version|level] [options]

Version or level:
  <version>           Release the specified semantic version (e.g. 1.2.3)
  --patch             Increment patch version (default)
  --minor             Increment minor version
  --major             Increment major version

Options:
  --dry-run           Run in dry-run mode
  --skip-crates       Skip the crates.io publish handoff
  --skip-binaries     Skip building and uploading binaries (and Homebrew update)
  --skip-docs         Skip docs.rs rebuild trigger
  --full-ci           Build ALL platforms on GitHub Actions
  --ci-only           Trigger CI for Linux/Windows only (skip local build)
  --force-ci          Use CI even if zig is available locally
  -h, --help          Show this help message

Default mode:
   1. macOS (native) + Linux/Windows (via zigbuild — no Docker needed)
      Falls back to GitHub Actions CI if Zig unavailable
USAGE
}
ensure_gh_auth() {
    local dry_run=$1
    if ! command -v gh >/dev/null 2>&1; then
        [[ "$dry_run" == 'true' ]] && return 0
        print_error "GitHub CLI not found"
        exit 1
    fi
    if gh auth status >/dev/null 2>&1; then
        local user
        user=$(gh api user --jq '.login' 2>/dev/null || true)
        if [[ -n "$user" && "$user" != "vinhnx" ]]; then
            gh auth switch -u vinhnx >/dev/null 2>&1 || [[ "$dry_run" == 'true' ]] || {
                print_error "Could not switch to vinhnx account"
                exit 1
            }
        fi
    elif [[ "$dry_run" == 'false' ]]; then
        print_error "GitHub CLI not authenticated"
        exit 1
    fi
}

# ── Main ─────────────────────────────────────────────────────────────

main() {
    local release_argument=''
    local increment_type=''
    local dry_run=false
    local skip_crates=false
    local skip_binaries=false
    local skip_docs=false
    local full_ci=false
    local ci_only=false
    local force_ci=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help) show_usage; exit 0 ;;
            -p|--patch) increment_type='patch'; shift ;;
            -m|--minor) increment_type='minor'; shift ;;
            -M|--major) increment_type='major'; shift ;;
            --dry-run) dry_run=true; shift ;;
            --skip-crates) skip_crates=true; shift ;;
            --skip-binaries) skip_binaries=true; shift ;;
            --skip-docs) skip_docs=true; shift ;;
            --full-ci) full_ci=true; shift ;;
            --ci-only) ci_only=true; shift ;;
            --force-ci) force_ci=true; shift ;;
            *)
                [[ -z "$release_argument" ]] || { print_error 'Multiple versions specified'; exit 1; }
                release_argument=$1; shift ;;
        esac
    done

    [[ -z "$increment_type" && -z "$release_argument" ]] && increment_type='patch'
    [[ -n "$increment_type" ]] && release_argument=$increment_type

    check_branch
    check_clean_tree
    ensure_cargo_release
    ensure_gh_auth "$dry_run"

    # Zig available = all targets local (macOS + Linux + Windows via zigbuild)
    local zig_available=false
    if command -v zig &>/dev/null; then
        zig_available=true
        print_info "Zig found: building all targets locally via zigbuild"
    else
        print_info "Zig not found: building macOS only, CI handles Linux/Windows"
        print_info "  Install Zig: https://ziglang.org/download/"
    fi

    # Version calculation
    local current_version
    current_version=$(get_current_version)
    print_info "Current version: $current_version"

    local next_version
    if [[ "$release_argument" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        next_version="$release_argument"
    else
        IFS='.' read -ra v <<< "$current_version"
        case "$release_argument" in
            major) next_version="$((v[0] + 1)).0.0" ;;
            minor) next_version="${v[0]}.$((v[1] + 1)).0" ;;
            patch) next_version="${v[0]}.${v[1]}.$((v[2] + 1))" ;;
        esac
    fi
    print_info "Releasing$( [[ "$dry_run" == 'true' ]] && echo ' (dry-run)' ): $next_version"

    # Full CI / CI-only shortcuts
    if [[ "$full_ci" == 'true' ]]; then
        [[ "$dry_run" == 'false' ]] && gh workflow run release.yml --field tag="$next_version"
        skip_binaries=true
    fi
    if [[ "$ci_only" == 'true' ]]; then
        [[ "$dry_run" == 'false' ]] && gh workflow run build-linux-windows.yml --field tag="$next_version"
        skip_binaries=true
    fi

    # ── Step 0.5: Docs map ──
    print_info "Step 0.5: Regenerating documentation map..."
    if [[ "$dry_run" == 'false' ]]; then
        python3 scripts/generate_docs_map.py && python3 scripts/sync_embedded_assets.py || true
        git add docs/modules/vtcode_docs_map.md vtcode-core/embedded_assets_source/docs/modules/vtcode_docs_map.md
        if ! git diff --cached --quiet; then
            GIT_AUTHOR_NAME="vtcode-release-bot" GIT_AUTHOR_EMAIL="noreply@vtcode.com" \
            GIT_COMMITTER_NAME="vtcode-release-bot" GIT_COMMITTER_EMAIL="noreply@vtcode.com" \
            git commit -m "docs: update documentation map [skip ci]"
        fi
    fi

    # ── Step 1: Build binaries ──
    if [[ "$skip_binaries" == 'false' ]]; then
        local build_flag="--only-build"
        [[ "$zig_available" == 'false' ]] && build_flag="--only-build-local"
        [[ "$force_ci" == 'true' ]] && build_flag="--only-build-local"
        print_info "Step 1: Building binaries ($([[ "$zig_available" == 'true' && "$force_ci" == 'false' ]] && echo 'all targets via zigbuild' || echo 'macOS only, CI for rest'))..."
        env CARGO_BUILD_RUSTC_WRAPPER= RUSTC_WRAPPER= \
            ./scripts/build-and-upload-binaries.sh -v "$next_version" $build_flag
    fi

    # ── Step 2: Changelog ──
    print_info "Step 2: Generating changelog..."
    update_changelog_from_commits "$next_version" "$dry_run"

    # ── Step 3: Cargo release ──
    print_info "Step 3: Running cargo release..."
    if [[ "$dry_run" == 'true' ]]; then
        print_info "Dry run - would run cargo release and publish crates"
    else
        env CARGO_BUILD_RUSTC_WRAPPER= RUSTC_WRAPPER= \
            cargo release "$release_argument" --workspace --config release.toml --execute --no-confirm --no-publish
        if [[ "$skip_crates" == 'false' ]]; then
            print_distribution "Publishing crates..."
            ./scripts/publish_extracted_crates.sh --skip-tests --skip-tags --skip-follow-up
        fi
    fi

    [[ "$dry_run" == 'true' ]] && { print_success 'Dry run completed'; exit 0; }

    local released_version
    released_version=$(get_current_version)

    # ── Step 3.5: CI (only if zig not available or forced) ──
    local CI_RUN_ID=""
    if [[ "$skip_binaries" == 'false' && "$zig_available" == 'false' ]]; then
        CI_RUN_ID=$(trigger_and_wait_ci "$released_version" "$dry_run")
    elif [[ "$skip_binaries" == 'false' && "$force_ci" == 'true' ]]; then
        CI_RUN_ID=$(trigger_and_wait_ci "$released_version" "$dry_run")
    fi

    # ── Step 4: GitHub Release + upload ──
    local sha_output
    sha_output=$(create_and_upload_release "$released_version" "$dry_run" "$CI_RUN_ID")
    IFS='|' read -r x86_sha arm_sha arm_linux_sha <<< "$sha_output"

    # ── Step 5: Homebrew ──
    if [[ "$skip_binaries" == 'false' ]]; then
        print_info "Step 5: Publishing Homebrew formula..."
        publish_homebrew_tap "$released_version" "${x86_sha:-}" "${arm_sha:-}" "${arm_linux_sha:-}"
    fi

    # ── Step 6: Docs.rs ──
    [[ "$skip_crates" == 'false' && "$skip_docs" == 'false' ]] && \
        trigger_docs_rs_rebuild "$released_version" false

    print_success "Release $released_version complete"
    print_info "Distribution: crates.io + GitHub Releases + Homebrew"
    if [[ "$zig_available" == 'true' && "$force_ci" == 'false' ]]; then
        print_info "  All platforms built locally via zigbuild — no CI wait"
    fi
}

main "$@"
