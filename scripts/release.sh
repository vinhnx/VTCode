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
        # Extract commit hash from entry (format: "... (commit_hash)")
        if [[ $entry =~ \(([a-f0-9]+)\)$ ]]; then
            local full_hash="${BASH_REMATCH[1]}"
            # Find username from the temporary file
            local username=""
            local found=0

            while IFS= read -r mapping_line; do
                if [[ -n "$mapping_line" && $found -eq 0 ]]; then
                    local map_hash map_username
                    map_hash=$(echo "$mapping_line" | cut -d'|' -f1)
                    map_username=$(echo "$mapping_line" | cut -d'|' -f2)
                    # Check if the full hash starts with the map hash (to match short vs full hashes)
                    if [[ ${full_hash} == ${map_hash}* || ${map_hash} == ${full_hash}* ]]; then
                        username="$map_username"
                        found=1
                    fi
                fi
            done < "$temp_mapping_file"

            if [[ -n "$username" ]]; then
                # Append @username to the entry if not already present
                if [[ $entry != *"@$username"* ]]; then
                    entry="$entry (@$username)"
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

# Parse commit type from conventional commit message
parse_commit_type() {
    local message="$1"
    # Extract type from conventional commit format: type(scope): message or type: message
    # Use sed to extract the type prefix
    local type=$(echo "$message" | sed -E 's/^([a-z]+)(\([^)]+\))?:.*/\1/')
    if [[ "$type" == "$message" ]]; then
        echo "other"
    else
        echo "$type"
    fi
}

# Get prefix indicator for commit type (text-based, no emoji)
get_type_prefix() {
    local type="$1"
    case "$type" in
        feat) echo "[FEAT]" ;;
        fix) echo "[FIX]" ;;
        perf) echo "[PERF]" ;;
        refactor) echo "[REFACTOR]" ;;
        docs) echo "[DOCS]" ;;
        test) echo "[TEST]" ;;
        build) echo "[BUILD]" ;;
        ci) echo "[CI]" ;;
        chore) echo "[CHORE]" ;;
        security) echo "[SECURITY]" ;;
        deps) echo "[DEPS]" ;;
        *) echo "" ;;
    esac
}

# Get human-readable title for commit type
get_type_title() {
    local type="$1"
    case "$type" in
        feat) echo "Features" ;;
        fix) echo "Bug Fixes" ;;
        perf) echo "Performance" ;;
        refactor) echo "Refactors" ;;
        docs) echo "Documentation" ;;
        test) echo "Tests" ;;
        build) echo "Build" ;;
        ci) echo "CI" ;;
        chore) echo "Chores" ;;
        security) echo "Security" ;;
        deps) echo "Dependencies" ;;
        *) echo "Other" ;;
    esac
}

# Clean commit message by removing the type prefix
clean_commit_message() {
    local message="$1"
    # Remove conventional commit prefix (type(scope): or type:)
    echo "$message" | sed -E 's/^[a-z]+(\([^)]+\))?:[[:space:]]*//'
}

# Group commits by type and generate structured changelog
# Note: Uses simple arrays instead of associative arrays for bash 3.2 compatibility (macOS)
generate_structured_changelog() {
    local commits_range="$1"

    # Define type order (priority)
    local type_order="feat fix perf refactor security docs test build ci deps chore other"

    # Initialize storage for each type (using prefix variables instead of associative arrays)
    local feat_commits=""
    local fix_commits=""
    local perf_commits=""
    local refactor_commits=""
    local security_commits=""
    local docs_commits=""
    local test_commits=""
    local build_commits=""
    local ci_commits=""
    local deps_commits=""
    local chore_commits=""
    local other_commits=""

    # Get commits with their hashes
    while IFS='|' read -r hash message; do
        [[ -z "$hash" ]] && continue

        local type=$(parse_commit_type "$message")
        local clean_msg=$(clean_commit_message "$message")

        # Skip excluded patterns
        if [[ "$message" =~ (chore\(release\):|bump version|update version|version bump|release v[0-9]+\.[0-9]+\.[0-9]+|chore.*version|chore.*release|build.*version|update.*version.*number|bump.*version.*to|update homebrew|update changelog) ]]; then
            continue
        fi

        # Get author for this commit
        local author_email=$(git log -1 --pretty=format:"%ae" "$hash" 2>/dev/null || echo "")
        local username=""
        if [[ -n "$author_email" ]]; then
            username=$(get_github_username "$author_email")
        fi

        # Build entry
        local entry="- $clean_msg ($hash)"
        if [[ -n "$username" && "$username" != "vtcode-release-bot" ]]; then
            entry="$entry (@$username)"
        fi

        # Add to appropriate group using prefix variables
        case "$type" in
            feat) feat_commits="${feat_commits}${entry}"$'\n' ;;
            fix) fix_commits="${fix_commits}${entry}"$'\n' ;;
            perf) perf_commits="${perf_commits}${entry}"$'\n' ;;
            refactor) refactor_commits="${refactor_commits}${entry}"$'\n' ;;
            security) security_commits="${security_commits}${entry}"$'\n' ;;
            docs) docs_commits="${docs_commits}${entry}"$'\n' ;;
            test) test_commits="${test_commits}${entry}"$'\n' ;;
            build) build_commits="${build_commits}${entry}"$'\n' ;;
            ci) ci_commits="${ci_commits}${entry}"$'\n' ;;
            deps) deps_commits="${deps_commits}${entry}"$'\n' ;;
            chore) chore_commits="${chore_commits}${entry}"$'\n' ;;
            *) other_commits="${other_commits}${entry}"$'\n' ;;
        esac
    done < <(git log "$commits_range" --no-merges --pretty=format:"%h|%s")

    # Generate structured output
    local output=""
    local has_content=false

    # Process each type in order (using case for bash 3.2 compatibility)
    for type in $type_order; do
        local commits=""
        case "$type" in
            feat) commits="$feat_commits" ;;
            fix) commits="$fix_commits" ;;
            perf) commits="$perf_commits" ;;
            refactor) commits="$refactor_commits" ;;
            security) commits="$security_commits" ;;
            docs) commits="$docs_commits" ;;
            test) commits="$test_commits" ;;
            build) commits="$build_commits" ;;
            ci) commits="$ci_commits" ;;
            deps) commits="$deps_commits" ;;
            chore) commits="$chore_commits" ;;
            other) commits="$other_commits" ;;
        esac

        if [[ -n "$commits" ]]; then
            local title=$(get_type_title "$type")

            output="${output}### ${title}"$'\n\n'
            output="${output}${commits}"$'\n'
            has_content=true
        fi
    done

    if [[ "$has_content" == false ]]; then
        output="*No significant changes*"$'\n'
    fi

    echo "$output"
}

# Changelog generation from commits
update_changelog_from_commits() {
    local version=$1
    local dry_run_flag=$2

    print_info "Generating changelog for version $version from commits..."

    local previous_tag
    previous_tag=$(git tag --sort=-v:refname | grep -E '^v?[0-9]+\.[0-9]+\.[0-9]+$' | head -n 1)

    local commits_range="HEAD"
    if [[ -n "$previous_tag" ]]; then
        print_info "Generating changelog from $previous_tag to HEAD"
        commits_range="$previous_tag..HEAD"
    else
        print_info "No previous tag found, getting all commits"
    fi

    local date_str
    date_str=$(date +%Y-%m-%d)

    # Generate structured changelog
    print_info "Generating structured changelog from commits..."
    local structured_changelog
    structured_changelog=$(generate_structured_changelog "$commits_range")

    # Save to global variable for release notes use (GitHub Release body)
    {
        echo "## What's Changed"
        echo ""
        echo "$structured_changelog"
        echo ""
        echo "**Full Changelog**: https://github.com/vinhnx/vtcode/compare/${previous_tag}...${version}"
    } > "$RELEASE_NOTES_FILE"

    if [[ "$dry_run_flag" == 'true' ]]; then
        print_info "Dry run - would update CHANGELOG.md"
        print_info "Release notes preview:"
        cat "$RELEASE_NOTES_FILE"
        return 0
    fi

    # Format for CHANGELOG.md (with version header)
    local changelog_entry
    changelog_entry="## $version - $date_str"$'\n\n'
    changelog_entry="${changelog_entry}${structured_changelog}"$'\n'

    if [[ -f CHANGELOG.md ]]; then
        # Check if this version already exists in the changelog
        if grep -q "^## $version " CHANGELOG.md; then
            print_warning "Version $version already exists in CHANGELOG.md, skipping update"
        else
            # Insert new entry after the header
            local header
            header=$(head -n 4 CHANGELOG.md)
            local remainder
            remainder=$(tail -n +5 CHANGELOG.md)
            {
                printf '%s\n' "$header"
                printf '%b\n' "$changelog_entry"
                printf '%s\n' "$remainder"
            } > CHANGELOG.md
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
    if ! git diff --cached --quiet; then
        GIT_AUTHOR_NAME="vtcode-release-bot" \
        GIT_AUTHOR_EMAIL="noreply@vtcode.com" \
        GIT_COMMITTER_NAME="vtcode-release-bot" \
        GIT_COMMITTER_EMAIL="noreply@vtcode.com" \
        git commit -m "docs: update changelog for $version [skip ci]"
        print_success "Changelog updated and committed for version $version"
    else
        print_info "No changes to CHANGELOG.md to commit."
    fi
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
        print_warning "Running in dry-run mode for $next_version"
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
     if gh release view "$released_version" &>/dev/null; then
         print_warning "Release $released_version already exists"
     else
         # Read release notes from file
         local release_body=""
         if [[ -f "$RELEASE_NOTES_FILE" ]]; then
             release_body=$(cat "$RELEASE_NOTES_FILE")
         fi

         # Create GitHub release with release notes
         if gh release create "$released_version" \
             --title "$released_version" \
             --notes "$release_body" \
             --draft=false \
             --prerelease=false; then
             print_success "GitHub Release $released_version created successfully"
         else
             print_error "Failed to create GitHub Release"
             exit 1
         fi
     fi

    # 4. Upload Binaries to GitHub Release
    if [[ "$skip_binaries" == 'false' ]]; then
        print_info "Step 4: Packaging and uploading binaries to GitHub Release $released_version..."

        local binaries_dir="/tmp/vtcode-release-$released_version"
        mkdir -p "$binaries_dir"

        # Build and package binaries
        print_info "Building binaries for macOS architectures..."

        # x86_64-apple-darwin
        if cargo build --release --target x86_64-apple-darwin &>/dev/null; then
            tar -C target/x86_64-apple-darwin/release -czf "$binaries_dir/vtcode-$released_version-x86_64-apple-darwin.tar.gz" vtcode
            shasum -a 256 "$binaries_dir/vtcode-$released_version-x86_64-apple-darwin.tar.gz" > "$binaries_dir/vtcode-$released_version-x86_64-apple-darwin.sha256"
            print_success "Built x86_64-apple-darwin"
        else
            print_warning "Failed to build x86_64-apple-darwin"
        fi

        # aarch64-apple-darwin
        if cargo build --release --target aarch64-apple-darwin &>/dev/null; then
            tar -C target/aarch64-apple-darwin/release -czf "$binaries_dir/vtcode-$released_version-aarch64-apple-darwin.tar.gz" vtcode
            shasum -a 256 "$binaries_dir/vtcode-$released_version-aarch64-apple-darwin.tar.gz" > "$binaries_dir/vtcode-$released_version-aarch64-apple-darwin.sha256"
            print_success "Built aarch64-apple-darwin"
        else
            print_warning "Failed to build aarch64-apple-darwin"
        fi

        # Upload binaries to GitHub Release
        print_info "Uploading binaries to GitHub Release..."
        if gh release upload "$released_version" "$binaries_dir"/*.tar.gz "$binaries_dir"/*.sha256 --clobber; then
            print_success "Binaries uploaded successfully"
        else
            print_error "Failed to upload binaries to GitHub Release"
            exit 1
        fi

        # Cleanup
        rm -rf "$binaries_dir"
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

     print_success "Release process finished for $released_version"
     print_info "Distribution:"
     print_info "  ✓ Cargo (crates.io)"
     print_info "  ✓ GitHub Releases (macOS built locally)"
     print_info "  ✓ Homebrew (vinhnx/tap/vtcode)"
}

main "$@"