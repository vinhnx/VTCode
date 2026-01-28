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

    print_info "Generating changelog for version $version from commits..."

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

    # Generate the categorization logic once and use it for both CHANGELOG.md and GitHub Release
    local raw_commits
    raw_commits=$(git log "$commits_range" --no-merges --pretty=format:"%s")
    
    local changelog_content
    changelog_content=$(echo "$raw_commits" | awk -v vers="$version" -v date="$date_str" \
    '{
        line = $0
        if (match(line, /^(feat|feature)/)) feat = feat "    - " line "\n"
        else if (match(line, /^(fix|bug)/)) fix = fix "    - " line "\n"
        else if (match(line, /^(perf|performance)/)) perf = perf "    - " line "\n"
        else if (match(line, /^(refactor)/)) refactor = refactor "    - " line "\n"
        else if (match(line, /^(docs|documentation)/)) docs = docs "    - " line "\n"
        else if (match(line, /^(style)/)) style = style "    - " line "\n"
        else if (match(line, /^(test)/)) test = test "    - " line "\n"
        else if (match(line, /^(build)/)) build = build "    - " line "\n"
        else if (match(line, /^(ci)/)) ci = ci "    - " line "\n"
        else if (match(line, /^(chore)/)) chore = chore "    - " line "\n"
        else other = other "    - " line "\n"
    }
    END {
        print "# [Version " vers "] - " date "\n"
        print ""
        if (feat != "") print "### Features\n" feat "\n"
        if (fix != "") print "### Bug Fixes\n" fix "\n"
        if (perf != "") print "### Performance Improvements\n" perf "\n"
        if (refactor != "") print "### Refactors\n" refactor "\n"
        if (docs != "") print "### Documentation\n" docs "\n"
        if (style != "") print "### Style Changes\n" style "\n"
        if (test != "") print "### Tests\n" test "\n"
        if (build != "") print "### Build System\n" build "\n"
        if (ci != "") print "### CI Changes\n" ci "\n"
        if (chore != "") print "### Chores\n" chore "\n"
        if (other != "") print "### Other Changes\n" other "\n"
    }')

    # Save to global variable for release notes use
    echo "$changelog_content" > "$RELEASE_NOTES_FILE"

    if [[ "$dry_run_flag" == 'true' ]]; then
        print_info "Dry run - would update CHANGELOG.md"
        return 0
    fi

    if [[ -f CHANGELOG.md ]]; then
        local header
        header=$(head -n 5 CHANGELOG.md)
        local remainder
        remainder=$(tail -n +6 CHANGELOG.md)
        {
            printf '%s\n' "$header"
            printf '%s\n' "$changelog_content"
            printf '%s\n' "$remainder"
        } > CHANGELOG.md
    else
        {
            printf '%s\n' "# Changelog - vtcode"
            printf '%s\n' ""
            printf '%s\n' "All notable changes to vtcode will be documented in this file."
            printf '%s\n' ""
            printf '%s\n' "$changelog_content"
        } > CHANGELOG.md
    fi

    git add CHANGELOG.md
    GIT_AUTHOR_NAME="vtcode-release-bot" \
    GIT_AUTHOR_EMAIL="noreply@vtcode.com" \
    GIT_COMMITTER_NAME="vtcode-release-bot" \
    GIT_COMMITTER_EMAIL="noreply@vtcode.com" \
    git commit -m "docs: update changelog for v$version [skip ci]"
    print_success "Changelog updated and committed for version $version"
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

    # 1. Local Build Sanity Check (using background parallel builds)
    if [[ "$skip_binaries" == 'false' ]]; then
        print_info "Step 1: Local binary build sanity check..."
        local build_args=(-v "$next_version" --only-build)
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

    # 4. Upload Binaries to GitHub with the captured Release Notes
    if [[ "$skip_binaries" == 'false' ]]; then
        print_info "Step 4: Uploading binaries to GitHub Release v$released_version..."
        if ! ./scripts/build-and-upload-binaries.sh -v "$released_version" --only-upload --notes-file "$RELEASE_NOTES_FILE"; then
            print_error "Step 4 failed. If this is a permission error, try:"
            print_info "gh auth refresh -h github.com -s workflow"
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
