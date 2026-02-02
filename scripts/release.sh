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

# Get GitHub username from commit author email
get_github_username() {
    local email=$1
    # Common email-to-username mappings
    case "$email" in
        vinhnguyen*) echo "vinhnx" ;;
        noreply@vtcode.com) echo "vtcode-release-bot" ;;
        *)
            # Extract username from email (before @)
            local username="${email%%@*}"
            echo "$username"
            ;;
    esac
}

# Add @username tags to changelog entries
add_username_tags() {
    local changelog=$1
    local commits_range=$2

    # Create a temporary file to store the mapping of commit hashes to usernames
    local temp_mapping_file
    temp_mapping_file=$(mktemp)

    # Populate the mapping - use a subshell to avoid variable scoping issues
    (
        git log "$commits_range" --no-merges --pretty=format:"%h|%ae"
    ) | while IFS= read -r line; do
        if [[ -n "$line" ]]; then
            local hash author_email
            hash=$(echo "$line" | cut -d'|' -f1)
            author_email=$(echo "$line" | cut -d'|' -f2)
            local username=$(get_github_username "$author_email")
            echo "$hash|$username"
        fi
    done > "$temp_mapping_file"

    # Process changelog and add @username tags
    local result=""
    while IFS= read -r entry; do
        # Extract commit hash from entry (e.g., "[<samp>(f533a)</samp>]")
        if [[ $entry =~ \[.*\(([a-f0-9]+)\) ]]; then
            local short_hash="${BASH_REMATCH[1]}"
            # Find username from the temporary file
            local username=""
            local found=0

            while IFS= read -r mapping_line; do
                if [[ -n "$mapping_line" && $found -eq 0 ]]; then
                    local map_hash map_username
                    map_hash=$(echo "$mapping_line" | cut -d'|' -f1)
                    map_username=$(echo "$mapping_line" | cut -d'|' -f2)
                    if [[ $map_hash == ${short_hash}* ]]; then
                        username="$map_username"
                        found=1
                    fi
                fi
            done < "$temp_mapping_file"

            if [[ -n "$username" ]]; then
                # Add @username before the closing bracket if not already present
                if [[ $entry != *"@$username"* ]]; then
                    entry=$(echo "$entry" | sed "s/by \*\*\([^*]*\)\*\*/by [@\2](&) **\1**/")
                fi
            fi
        fi
        result+="$entry"$'\n'
    done <<< "$changelog"

    # Clean up
    rm -f "$temp_mapping_file"

    echo "${result%$'\n'}"
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

        # Strip ANSI color codes from output
        full_output=$(echo "$full_output" | sed 's/\x1b\[[0-9;]*m//g')

        # Extract changelog content between separators
        # Format: version line, then content, then "Dry run. Release skipped."
        # We want lines between first and second separator
        local first_sep_line
        first_sep_line=$(echo "$full_output" | grep -n '^--------------$' | head -1 | cut -d: -f1)
        local second_sep_line
        second_sep_line=$(echo "$full_output" | grep -n '^--------------$' | head -2 | tail -1 | cut -d: -f1)
        
        if [[ -n "$first_sep_line" && -n "$second_sep_line" ]]; then
            # Ensure variables are treated as integers for arithmetic
            local start_line=$((first_sep_line + 1))
            local end_line=$((second_sep_line - 1))
            # Extract lines between separators (exclusive)
            changelog_content=$(echo "$full_output" | sed -n "${start_line},${end_line}p")
            # Remove the version/tag line if present (e.g., "v0.74.6 -> v0.74.7 (4 commits)")
            changelog_content=$(echo "$changelog_content" | sed '/^v[0-9]\+\.[0-9]\+\.[0-9]\+.*$/d')
            # Replace ...HEAD with ...v$version in the comparison link
            changelog_content=$(echo "$changelog_content" | sed "s/\.\.\.HEAD/...v$version/g")
        else
            print_warning "Could not find separators in changelogithub output"
            changelog_content=""
        fi

        # If extraction failed or empty, fallback to simple log
        if [[ -z "$(echo "$changelog_content" | tr -d '[:space:]')" ]]; then
            print_warning "changelogithub extraction failed, using git log fallback"
            changelog_content=$(git log "$commits_range" --no-merges --pretty=format:"* %s (%h)")
        fi
    else
        # Fallback if npx is missing
        print_warning "changelogithub not found, using git log fallback"
        changelog_content=$(git log "$commits_range" --no-merges --pretty=format:"* %s (%h)")
    fi

    # Add @username tags to changelog entries
    if [[ -n "$commits_range" && "$commits_range" != "HEAD" ]]; then
        changelog_content=$(add_username_tags "$changelog_content" "$commits_range")
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

    # GitHub CLI authentication setup
    if command -v gh >/dev/null 2>&1; then
        print_info "Checking GitHub CLI authentication..."

        # Check current GitHub user
        current_user=$(gh api user --jq '.login' 2>/dev/null || echo "")
        print_info "Currently logged in as: $current_user"

        if [[ "$current_user" != "vinhnx" ]]; then
            print_info "Switching to GitHub account vinhnx..."
            if gh auth switch -u vinhnx 2>/dev/null; then
                print_success "Switched to GitHub account vinhnx"
            else
                print_warning "Could not switch to GitHub account vinhnx"
            fi
        fi

        # Skip the refresh step that causes hangs, assuming user has proper scopes
        print_warning "Skipping GitHub CLI scopes refresh (may need manual refresh if issues occur)"
    else
        print_warning "GitHub CLI not found. Release will continue but binary uploads may fail."
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
            major)
                local major_num=$((v[0] + 1))
                next_version="${major_num}.0.0" ;;
            minor)
                local minor_num=$((v[1] + 1))
                next_version="${v[0]}.${minor_num}.0" ;;
            patch)
                local patch_num=$((v[2] + 1))
                next_version="${v[0]}.${v[1]}.${patch_num}" ;;
        esac
    fi

    if [[ "$dry_run" == 'true' ]]; then
        print_warning "Running in dry-run mode for v$next_version"
    else
        print_warning "Releasing version: $next_version"
    fi

    # 1. Local Build (both macOS architectures for Homebrew, or current platform on Linux)
    if [[ "$skip_binaries" == 'false' ]]; then
        if [[ "$dry_run" == 'true' ]]; then
            print_info "Step 1 (dry-run): Would build binaries for x86_64-apple-darwin and aarch64-apple-darwin"
        else
            print_info "Step 1: Local binary build (macOS: both architectures, Linux: current platform)..."
            local build_args=(-v "$next_version" --only-build-local)
            ./scripts/build-and-upload-binaries.sh "${build_args[@]}"
        fi
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

    # 3.5 GitHub Release Creation and Binary Upload via gh
     print_info "Step 3.5: Creating GitHub Release with binaries..."
    
     # Ensure GITHUB_TOKEN is available
     if [[ -z "${GITHUB_TOKEN:-}" ]] && command -v gh >/dev/null 2>&1; then
         export GITHUB_TOKEN=$(gh auth token)
     fi
    
     # Check if release already exists
     if gh release view "v$released_version" &>/dev/null; then
         print_warning "Release v$released_version already exists"
     else
         # Read release notes from file
         local release_body=""
         if [[ -f "$RELEASE_NOTES_FILE" ]]; then
             release_body=$(cat "$RELEASE_NOTES_FILE")
         fi
    
         # Create GitHub release with release notes
         if gh release create "v$released_version" \
             --title "VT Code v$released_version" \
             --notes "$release_body" \
             --draft=false \
             --prerelease=false; then
             print_success "GitHub Release v$released_version created successfully"
         else
             print_error "Failed to create GitHub Release"
             exit 1
         fi
     fi

    # 4. Upload Binaries to GitHub Release
    if [[ "$skip_binaries" == 'false' ]]; then
        print_info "Step 4: Packaging and uploading binaries to GitHub Release v$released_version..."
        
        local binaries_dir="/tmp/vtcode-release-$released_version"
        mkdir -p "$binaries_dir"
        
        # Build and package binaries
        print_info "Building binaries for macOS architectures..."
        
        # x86_64-apple-darwin
        if cargo build --release --target x86_64-apple-darwin &>/dev/null; then
            tar -C target/x86_64-apple-darwin/release -czf "$binaries_dir/vtcode-v$released_version-x86_64-apple-darwin.tar.gz" vtcode
            shasum -a 256 "$binaries_dir/vtcode-v$released_version-x86_64-apple-darwin.tar.gz" > "$binaries_dir/vtcode-v$released_version-x86_64-apple-darwin.sha256"
            print_success "Built x86_64-apple-darwin"
        else
            print_warning "Failed to build x86_64-apple-darwin"
        fi
        
        # aarch64-apple-darwin
        if cargo build --release --target aarch64-apple-darwin &>/dev/null; then
            tar -C target/aarch64-apple-darwin/release -czf "$binaries_dir/vtcode-v$released_version-aarch64-apple-darwin.tar.gz" vtcode
            shasum -a 256 "$binaries_dir/vtcode-v$released_version-aarch64-apple-darwin.tar.gz" > "$binaries_dir/vtcode-v$released_version-aarch64-apple-darwin.sha256"
            print_success "Built aarch64-apple-darwin"
        else
            print_warning "Failed to build aarch64-apple-darwin"
        fi
        
        # Upload binaries to GitHub Release
        print_info "Uploading binaries to GitHub Release..."
        if gh release upload "v$released_version" "$binaries_dir"/*.tar.gz "$binaries_dir"/*.sha256 --clobber; then
            print_success "Binaries uploaded successfully"
        else
            print_error "Failed to upload binaries to GitHub Release"
            exit 1
        fi
        
        # Cleanup
        rm -rf "$binaries_dir"
    fi

    # 5. Trigger GitHub Actions Release Workflow for full cross-platform builds
     if [[ "$skip_binaries" == 'false' ]]; then
         print_info "Step 5: Triggering GitHub Actions release workflow for cross-platform builds (Linux, Windows)..."
         if command -v gh >/dev/null 2>&1; then
             # Trigger the release workflow using gh CLI
             if gh workflow run release.yml -f tag="v$released_version"; then
                 print_success "GitHub Actions release workflow triggered"
                 print_info "Monitor progress at: https://github.com/vinhnx/vtcode/actions/workflows/release.yml"
             else
                 print_warning "Failed to trigger GitHub Actions release workflow via gh CLI"
             fi
         else
             print_warning "GitHub CLI not available. GitHub Actions release workflow must be triggered manually."
         fi
     fi

     # 6. Update Homebrew
     if [[ "$skip_binaries" == 'false' ]]; then
         print_info "Step 6: Updating Homebrew formula..."
         ./scripts/build-and-upload-binaries.sh -v "$released_version" --only-homebrew
     fi

     # 7. Handle docs.rs rebuild
     if [[ "$skip_crates" == 'false' && "$skip_docs" == 'false' ]]; then
         trigger_docs_rs_rebuild "$released_version" false
     fi

     print_success "Release process finished for v$released_version"
     print_info "Distribution:"
     print_info "  ✓ Cargo (crates.io)"
     print_info "  ✓ GitHub Releases (macOS built locally + Linux/Windows via Actions)"
     print_info "  ✓ Homebrew (vinhnx/tap/vtcode)"
     print_info "Monitor GitHub Actions: https://github.com/vinhnx/vtcode/actions/workflows/release.yml"
}

main "$@"
