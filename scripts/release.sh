#!/usr/bin/env bash

# VT Code Release Script
#
# This script handles local releases for VT Code:
# 1. Builds binaries locally (Sanity Check)
# 2. Runs cargo-release to version, tag, and publish to crates.io
# 3. Uploads pre-built binaries to GitHub Releases
# 4. Updates Homebrew formula
#
# Usage: ./scripts/release.sh [version|level] [options]

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
NC='\033[0m'

# Temporary file to store release notes
RELEASE_NOTES_FILE=$(mktemp)
trap 'rm -f "$RELEASE_NOTES_FILE"' EXIT

print_info() {
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

print_distribution() {
    printf '%b\n' "${PURPLE}DISTRIBUTION:${NC} $1"
}

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
  --skip-crates       Skip publishing crates to crates.io
  --skip-binaries     Skip building and uploading binaries (and Homebrew update)
  --skip-docs         Skip docs.rs rebuild trigger
  -h, --help          Show this help message
USAGE
}

# Changelog generation from commits
update_changelog_from_commits() {
    local version=$1
    local dry_run_flag=$2

    print_info "Generating changelog for version v$version from commits..."

    local previous_tag
    previous_tag=$(git tag --sort=-v:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -n 1)

    local commits_range="HEAD"
    if [[ -n "$previous_tag" ]]; then
        print_info "Generating changelog from $previous_tag to HEAD"
        commits_range="$previous_tag..HEAD"
    else
        print_info "No previous tag found, getting all commits"
    fi

    local date_str
    date_str=$(date +%Y-%m-%d)

    local changelog_content
    if command -v npx >/dev/null 2>&1; then
        print_info "Using npx changelogithub for formatting..."
        # Capture changelogithub output
        local full_output
        if [[ -n "$previous_tag" ]]; then
            full_output=$(npx changelogithub --dry --from "$previous_tag" --to HEAD 2>/dev/null || echo "")
        else
            full_output=$(npx changelogithub --dry 2>/dev/null || echo "")
        fi

        # Extract content between separators (14 dashes)
        changelog_content=$(echo "$full_output" | sed -n '/^--------------$/,/^--------------$/p' | sed '1d;$d')

        # Replace ...HEAD with ...v$version in the comparison link
        changelog_content=$(echo "$changelog_content" | sed "s/\.\.\.HEAD/...v$version/g")

        # If extraction failed or empty, fallback to simple log
        if [[ -z "$(echo "$changelog_content" | tr -d '[:space:]')" ]]; then
            changelog_content=$(git log "$commits_range" --no-merges --pretty=format:"* %s (%h)")
        fi
    else
        # Fallback if npx is missing
        changelog_content=$(git log "$commits_range" --no-merges --pretty=format:"* %s (%h)")
    fi

    # Save to global variable for release notes use (GitHub Release body)
    echo -e "$changelog_content" > "$RELEASE_NOTES_FILE"

    if [[ "$dry_run_flag" == 'true' ]]; then
        print_info "Dry run - would update CHANGELOG.md"
        return 0
    fi

    # Format for CHANGELOG.md (with version header)
    local changelog_entry
    changelog_entry="## v$version - $date_str\n\n$changelog_content\n"

    if [[ -f CHANGELOG.md ]]; then
        # Check if file has enough lines
        local line_count
        line_count=$(wc -l < CHANGELOG.md)
        if [[ $line_count -gt 4 ]]; then
            local header
            header=$(head -n 4 CHANGELOG.md)
            local remainder
            remainder=$(tail -n +5 CHANGELOG.md)
            {
                printf '%s\n' "$header"
                printf '%b\n' "$changelog_entry"
                printf '%s\n' "$remainder"
            } > CHANGELOG.md
        else
            printf '%b\n' "$changelog_entry" >> CHANGELOG.md
        fi
    else
        {
            printf '%s\n' "# Changelog - vtcode"
            printf '%s\n' ""
            printf '%s\n' "All notable changes to vtcode will be documented in this file."
            printf '%s\n' ""
            printf '%b\n' "$changelog_entry"
        } > CHANGELOG.md
    fi

    git add CHANGELOG.md
    GIT_AUTHOR_NAME="vtcode-release-bot" \
    GIT_AUTHOR_EMAIL="noreply@vtcode.com" \
    GIT_COMMITTER_NAME="vtcode-release-bot" \
    GIT_COMMITTER_EMAIL="noreply@vtcode.com" \
    git commit -m "docs: update changelog for v$version [skip ci]"
    print_success "Changelog updated and committed for version v$version"
}

get_current_version() {
    local line
    line=$(grep '^version = ' Cargo.toml | head -1)
    echo "${line#*\"}" | sed 's/\".*//'
}

check_branch() {
    local current_branch
    current_branch=$(git branch --show-current)
    if [[ "$current_branch" != 'main' ]]; then
        print_error 'You must be on the main branch to create a release'
        exit 1
    fi
}

check_clean_tree() {
    if [[ -n "$(git status --porcelain)" ]]; then
        print_error 'Working tree is not clean. Please commit or stash your changes.'
        git status --short
        exit 1
    fi
}

ensure_cargo_release() {
    if ! command -v cargo-release >/dev/null 2>&1; then
        print_error 'cargo-release is not installed. Install it with `cargo install cargo-release`.'
        exit 1
    fi
}

trigger_docs_rs_rebuild() {
    local version=$1
    local dry_run_flag=$2

    if [[ "$dry_run_flag" == 'true' ]]; then
        print_info "Dry run - would trigger docs.rs rebuild for version $version"
        return 0
    fi

    print_distribution "Triggering docs.rs rebuild for version $version..."
    curl -s -o /dev/null "https://docs.rs/vtcode/$version" || true
    curl -s -o /dev/null "https://docs.rs/vtcode-core/$version" || true
}

main() {
    local release_argument=''
    local increment_type=''
    local dry_run=false
    local skip_crates=false
    local skip_binaries=false
    local skip_docs=false

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
            *)
                if [[ -n "$release_argument" ]]; then
                    print_error 'Multiple versions specified'
                    exit 1
                fi
                release_argument=$1
                shift
                ;;
        esac
    done

    if [[ -z "$increment_type" && -z "$release_argument" ]]; then
        increment_type='patch'
    fi

    if [[ -n "$increment_type" ]]; then
        release_argument=$increment_type
    fi

    check_branch
    check_clean_tree
    ensure_cargo_release

    # Pre-flight GitHub scope check
    if command -v gh >/dev/null 2>&1; then
        if ! gh auth status --show-token 2>&1 | grep -q "workflow"; then
            print_warning "GitHub CLI lacks 'workflow' scope. This may cause Step 4 to fail."
            print_info "Recommendation: run 'gh auth refresh -s workflow' before proceeding."
        fi

        # Check current GitHub user and warn if not the expected one
        current_user=$(gh api user --jq '.login' 2>/dev/null || echo "unknown")
        if [[ "$current_user" != "vinhnx" ]]; then
            print_warning "Current GitHub user is '$current_user', expected 'vinhnx'. Will attempt to switch in Step 3.5."
        fi
    fi

    local current_version
    current_version=$(get_current_version)
    print_info "Current version: $current_version"

    # Calculate next version
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

    if [[ "$dry_run" == 'true' ]]; then
        print_warning "Running in dry-run mode for v$next_version"
    else
        print_warning "Releasing version: $next_version"
    fi

    # 1. Local Build (both macOS architectures for Homebrew, or current platform on Linux)
    if [[ "$skip_binaries" == 'false' ]]; then
        print_info "Step 1: Local binary build (macOS: both architectures, Linux: current platform)..."
        local build_args=(-v "$next_version" --only-build-local)
        if [[ "$dry_run" == 'true' ]]; then
            build_args+=(--dry-run)
        fi
        ./scripts/build-and-upload-binaries.sh "${build_args[@]}"
    fi

    # 2. Changelog Update & capture for Release Notes
    print_info "Step 2: Generating changelog and release notes..."
    update_changelog_from_commits "$next_version" "$dry_run"

    # 3. Cargo Release (Publish to crates.io, tag and push)
    print_info "Step 3: Running cargo release (publish to crates.io, tag and push)..."
    local command=(cargo release "$release_argument" --workspace --config release.toml --execute --no-confirm)
    if [[ "$skip_crates" == 'true' ]]; then
        command+=(--no-publish)
    fi

    if [[ "$dry_run" == 'true' ]]; then
        print_info "Dry run - would run: ${command[*]}"
    else
            "${command[@]}"
    fi

    if [[ "$dry_run" == 'true' ]]; then
        print_success 'Dry run completed'
        exit 0
    fi

    # Confirm version after cargo-release
    local released_version
    released_version=$(get_current_version)
    if [[ "$released_version" != "$next_version" ]]; then
        print_warning "Released version $released_version differs from expected $next_version"
    fi

    # 3.5 GitHub Release Creation via changelogithub
    # This matches the workflow used in GitHub Actions and ensures consistent formatting
    print_info "Step 3.5: Creating GitHub Release via npx changelogithub..."

    # Switch to the correct GitHub account before creating the release
    if command -v gh >/dev/null 2>&1; then
        print_info "Switching to GitHub account vinhnx..."
        if ! gh auth switch -u vinhnx; then
            print_warning "Could not switch to GitHub account vinhnx, continuing with current account..."
        fi
    else
        print_warning "GitHub CLI not found, skipping account switch..."
    fi

    if command -v npx >/dev/null 2>&1; then
        # Ensure GITHUB_TOKEN is available for changelogithub
        if [[ -z "${GITHUB_TOKEN:-}" ]] && command -v gh >/dev/null 2>&1; then
            export GITHUB_TOKEN=$(gh auth token)
        fi

        # Run changelogithub to create the release on GitHub
        if npx changelogithub; then
            print_success "GitHub Release v$released_version created successfully"
        else
            print_warning "changelogithub failed, falling back to manual release creation in Step 4"
        fi
    else
        print_warning "npx not found, skipping changelogithub step"
    fi

    # 4. Upload Binaries to GitHub with the captured Release Notes
    if [[ "$skip_binaries" == 'false' ]]; then
        print_info "Step 4: Uploading binaries to GitHub Release v$released_version..."
        if ! ./scripts/build-and-upload-binaries.sh -v "$released_version" --only-upload --notes-file "$RELEASE_NOTES_FILE"; then
            print_error "Step 4 failed locally."
            print_info "You can trigger the release via GitHub Actions instead:"
            print_info "gh workflow run release.yml -f tag=v$released_version"
            print_info "Alternatively, refresh your token: gh auth refresh -h github.com -s workflow"
            exit 1
        fi
    fi

    # 5. Update Homebrew
    if [[ "$skip_binaries" == 'false' ]]; then
        print_info "Step 5: Updating Homebrew formula..."
        ./scripts/build-and-upload-binaries.sh -v "$released_version" --only-homebrew
    fi

    # 6. Handle docs.rs rebuild
    if [[ "$skip_crates" == 'false' && "$skip_docs" == 'false' ]]; then
        trigger_docs_rs_rebuild "$released_version" false
    fi

    print_success "Release process finished for v$released_version"
    print_info "Distribution: Cargo (crates.io), GitHub Releases, and Homebrew (vinhnx/tap/vtcode)"
}

main "$@"
