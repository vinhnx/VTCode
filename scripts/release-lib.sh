#!/usr/bin/env bash

# VT Code Release Library — shared helpers for release.sh
# Source: source "$(dirname "${BASH_SOURCE[0]}")/release-lib.sh"

[ -z "${SCRIPT_DIR:-}" ] && SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

# ── Helpers ──────────────────────────────────────────────────────────

print_distribution() {
    printf '%b\n' "${PURPLE}DISTRIBUTION:${NC} $1"
}

package_release_archive_with_ghostty() {
    local target=$1
    local binary_name=$2
    local archive_path=$3
    local release_dir="target/$target/release"
    bash "$SCRIPT_DIR/prepare-ghostty-vt-release-assets.sh" "$target" "$release_dir"
    tar -C "$release_dir" -czf "$archive_path" "$binary_name" ghostty-vt
}

get_github_username() {
    local email=$1
    case "$email" in
        vinhnguyen*) echo "vinhnx" ;;
        noreply@vtcode.com) echo "vtcode-release-bot" ;;
        *)
            local username="${email%%@*}"
            echo "$username"
            ;;
    esac
}

# ── Changelog ────────────────────────────────────────────────────────

add_username_tags() {
    local changelog=$1
    local commits_range=$2
    local temp_mapping_file
    temp_mapping_file=$(mktemp)
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
    local result=""
    while IFS= read -r entry; do
        if [[ $entry =~ \(([a-f0-9]+)\)$ ]]; then
            local full_hash="${BASH_REMATCH[1]}"
            local username=""
            local found=0
            while IFS= read -r mapping_line; do
                if [[ -n "$mapping_line" && $found -eq 0 ]]; then
                    local map_hash map_username
                    map_hash=$(echo "$mapping_line" | cut -d'|' -f1)
                    map_username=$(echo "$mapping_line" | cut -d'|' -f2)
                    if [[ ${full_hash} == ${map_hash}* || ${map_hash} == ${full_hash}* ]]; then
                        username="$map_username"
                        found=1
                    fi
                fi
            done < "$temp_mapping_file"
            if [[ -n "$username" ]]; then
                if [[ $entry != *"@$username"* ]]; then
                    entry="$entry (@$username)"
                fi
            fi
        fi
        result+="$entry"$'\n'
    done <<< "$changelog"
    rm -f "$temp_mapping_file"
    echo "$result"
}

parse_commit_type() {
    local message="$1"
    local type=$(echo "$message" | sed -E 's/^([a-z]+)(\([^)]+\))?:.*/\1/')
    if [[ "$type" == "$message" ]]; then
        echo "other"
    else
        echo "$type"
    fi
}

get_type_prefix() {
    local type="$1"
    case "$type" in
        feat) echo "  ✨ " ;;
        fix) echo "  🐛 " ;;
        docs) echo "  📝 " ;;
        style) echo "  💎 " ;;
        refactor) echo "  ♻️ " ;;
        perf) echo "  ⚡️ " ;;
        test) echo "  ✅ " ;;
        build) echo "  📦️ " ;;
        ci) echo "  🎡 " ;;
        chore) echo "  🔧 " ;;
        revert) echo "  ⏪️ " ;;
        *) echo "  • " ;;
    esac
}

get_type_title() {
    local type="$1"
    case "$type" in
        feat) echo "New Features" ;;
        fix) echo "Bug Fixes" ;;
        docs) echo "Documentation" ;;
        style) echo "Code Style" ;;
        refactor) echo "Refactoring" ;;
        perf) echo "Performance Improvements" ;;
        test) echo "Testing" ;;
        build) echo "Build System" ;;
        ci) echo "Continuous Integration" ;;
        chore) echo "Chores" ;;
        revert) echo "Reverts" ;;
        *) echo "Other Changes" ;;
    esac
}

clean_commit_message() {
    local message="$1"
    echo "$message" | sed -E 's/^[a-z]+(\([^)]+\))?:\s*//' | sed -E 's/\[skip ci\]|\[ci skip\]//g' | sed 's/^ *//' | sed 's/ *$//'
}

generate_contributors_section() {
    local commits_range=$1
    local contributors
    contributors=$(git log "$commits_range" --no-merges --format="%aN" 2>/dev/null | sort -u | grep -v "^$" | grep -v "vtcode-release-bot" || true)
    if [[ -n "$contributors" ]]; then
        echo ""
        echo "**Contributors**"
        echo ""
        echo "$contributors" | while IFS= read -r name; do
            [[ -n "$name" ]] && echo "  - $name"
        done
        echo ""
    fi
}

generate_structured_changelog() {
    local commits_range=$1
    if [[ -z "$commits_range" ]]; then
        commits_range="HEAD"
    fi
    local all_commits
    all_commits=$(git log "$commits_range" --no-merges --oneline 2>/dev/null || git log --oneline 2>/dev/null || true)
    if [[ -z "$all_commits" ]]; then
        echo "No commits found."
        return
    fi
    local grouped
    grouped=$(echo "$all_commits" | while IFS= read -r line; do
        if [[ -n "$line" ]]; then
            local hash message
            hash=$(echo "$line" | awk '{print $1}')
            message=$(echo "$line" | cut -d' ' -f2-)
            local type
            type=$(parse_commit_type "$message")
            local clean_msg
            clean_msg=$(clean_commit_message "$message")
            echo "$type|$hash|$clean_msg"
        fi
    done)
    local types_order=("feat" "fix" "docs" "style" "refactor" "perf" "test" "build" "ci" "chore" "revert" "other")
    for type in "${types_order[@]}"; do
        local type_commits
        type_commits=$(echo "$grouped" | grep "^$type|" || true)
        if [[ -n "$type_commits" ]]; then
            local title
            title=$(get_type_title "$type")
            local prefix
            prefix=$(get_type_prefix "$type")
            echo "### $title"
            echo ""
            echo "$type_commits" | while IFS='|' read -r t hash msg; do
                echo "${prefix}${msg} (${hash})"
            done
            echo ""
        fi
    done
}

update_changelog_from_commits() {
    local version=$1
    local dry_run_flag=$2
    print_info "Generating changelog for version $version using git-cliff..."
    local previous_version
    previous_version=$(git tag | grep -E '^[vV]?[0-9]+\.[0-9]+\.[0-9]+$' | sed 's/^[vV]//' | sort -t. -k1,1rn -k2,2rn -k3,3rn | awk -v ver="$version" '$0 != ver {print; exit}')
    if [[ -n "$previous_version" ]]; then
        print_info "Previous version tag: $previous_version"
    else
        print_info "No previous semver tag found"
    fi
    if command -v git-cliff >/dev/null 2>&1; then
        print_info "Using git-cliff for changelog generation"
        local github_token=""
        if command -v gh >/dev/null 2>&1; then
            github_token=$(gh auth token 2>/dev/null || true)
        fi
        local cliff_args=("--config" "cliff.toml" "--tag" "$version")
        if [[ -n "$github_token" ]]; then
            export GITHUB_TOKEN="$github_token"
        fi
        if [[ "$dry_run_flag" == 'true' ]]; then
            print_info "Dry run - would generate changelog with git-cliff"
            if [[ -n "$previous_version" ]]; then
                git-cliff "${cliff_args[@]}" --unreleased "${previous_version}..HEAD" 2>/dev/null || true
            else
                git-cliff "${cliff_args[@]}" --unreleased 2>/dev/null || true
            fi
            return 0
        fi
        local temp_changelog
        temp_changelog=$(mktemp)
        if [[ -n "$previous_version" ]]; then
            print_info "Generating changelog from $previous_version to $version"
            git-cliff "${cliff_args[@]}" --output "$temp_changelog" "${previous_version}..HEAD" 2>/dev/null || \
            git-cliff "${cliff_args[@]}" --output "$temp_changelog" 2>/dev/null || true
        else
            git-cliff "${cliff_args[@]}" --output "$temp_changelog" 2>/dev/null || true
        fi
        if [[ -s "$temp_changelog" ]]; then
            local changelog_content
            changelog_content=$(cat "$temp_changelog")
            local version_section
            version_section=$(echo "$changelog_content" | awk '/^## /{if(++n==2)exit} n==1' 2>/dev/null || true)
            local full_changelog_url
            if [[ -n "$previous_version" ]]; then
                full_changelog_url="https://github.com/vinhnx/vtcode/compare/${previous_version}...${version}"
            else
                full_changelog_url="https://github.com/vinhnx/vtcode/releases/tag/${version}"
            fi
            {
                echo "## What's Changed"
                echo ""
                if [[ -n "$version_section" ]]; then
                    echo "$version_section"
                else
                    echo "$changelog_content"
                fi
                echo ""
                echo "**Full Changelog**: ${full_changelog_url}"
            } > "$RELEASE_NOTES_FILE"
            if [[ -f CHANGELOG.md ]]; then
                if grep -q "^## $version " CHANGELOG.md; then
                    print_warning "Version $version already exists in CHANGELOG.md, skipping update"
                else
                    local header
                    header=$(head -n 4 CHANGELOG.md)
                    local remainder
                    remainder=$(tail -n +5 CHANGELOG.md)
                    {
                        printf '%s\n' "$header"
                        if [[ -n "$version_section" ]]; then
                            printf '%s\n' "$version_section"
                        else
                            printf '%s\n' "$changelog_content"
                        fi
                        printf '%s\n' "$remainder"
                    } > CHANGELOG.md
                fi
            else
                cp "$temp_changelog" CHANGELOG.md
            fi
            rm -f "$temp_changelog"
        else
            print_warning "git-cliff failed, falling back to built-in changelog generator"
            update_changelog_builtin "$version" "$dry_run_flag"
            return $?
        fi
    else
        print_warning "git-cliff not found, using built-in changelog generator"
        print_info "Install with: cargo install git-cliff"
        update_changelog_builtin "$version" "$dry_run_flag"
        return $?
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

update_changelog_builtin() {
    local version=$1
    local dry_run_flag=$2
    print_info "Generating changelog for version $version from commits (builtin)..."
    local previous_tag
    previous_tag=$(git log --tags --simplify-by-decoration --pretty="format:%D" | grep -oE "tag: v?[0-9]+\.[0-9]+\.[0-9]+" | sed 's/tag: //;s/,.*//' | grep -vE "^(v)?${version}$" | head -n 1)
    local commits_range="HEAD"
    if [[ -n "$previous_tag" ]]; then
        print_info "Generating changelog from $previous_tag to HEAD"
        commits_range="$previous_tag..HEAD"
    else
        print_info "No previous tag found, getting all commits"
    fi
    local date_str
    date_str=$(date +%Y-%m-%d)
    print_info "Generating structured changelog from commits..."
    local structured_changelog
    structured_changelog=$(generate_structured_changelog "$commits_range")
    {
        echo "## What's Changed"
        echo ""
        echo "$structured_changelog"
        echo ""
        if [[ -n "$previous_tag" ]]; then
            echo "**Full Changelog**: https://github.com/vinhnx/vtcode/compare/${previous_tag}...${version}"
        else
            echo "**Full Changelog**: https://github.com/vinhnx/vtcode/releases/tag/${version}"
        fi
    } > "$RELEASE_NOTES_FILE"
    if [[ "$dry_run_flag" == 'true' ]]; then
        print_info "Dry run - would update CHANGELOG.md"
        print_info "Release notes preview:"
        cat "$RELEASE_NOTES_FILE"
        return 0
    fi
    local changelog_entry
    changelog_entry="## $version - $date_str"$'\n\n'
    changelog_entry="${changelog_entry}${structured_changelog}"$'\n'
    if [[ -f CHANGELOG.md ]]; then
        if grep -q "^## $version " CHANGELOG.md; then
            print_warning "Version $version already exists in CHANGELOG.md, skipping update"
        else
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

# ── Checks ───────────────────────────────────────────────────────────

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

# ── Docs.rs ──────────────────────────────────────────────────────────

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

# ── Homebrew ─────────────────────────────────────────────────────────

update_homebrew_formula_file() {
    local formula_path=$1
    local version=$2
    local x86_64_macos_sha=$3
    local aarch64_macos_sha=$4
    local aarch64_linux_sha=${5:-}
    FORMULA_PATH="$formula_path" \
    FORMULA_VERSION="$version" \
    FORMULA_X86_64_MACOS_SHA="$x86_64_macos_sha" \
    FORMULA_AARCH64_MACOS_SHA="$aarch64_macos_sha" \
    FORMULA_AARCH64_LINUX_SHA="$aarch64_linux_sha" \
        python3 <<'PYTHON_SCRIPT'
import os
import re
from pathlib import Path

formula_path = Path(os.environ["FORMULA_PATH"])
version = os.environ["FORMULA_VERSION"]
x86_64_macos_sha = os.environ["FORMULA_X86_64_MACOS_SHA"]
aarch64_macos_sha = os.environ["FORMULA_AARCH64_MACOS_SHA"]
aarch64_linux_sha = os.environ.get("FORMULA_AARCH64_LINUX_SHA", "")

content = formula_path.read_text()
content = re.sub(r'version\s+"[^"]*"', f'version "{version}"', content)
content = re.sub(
    r'(aarch64-apple-darwin\.tar\.gz"\s+sha256\s+")([^"]*)(")',
    lambda match: f'{match.group(1)}{aarch64_macos_sha}{match.group(3)}',
    content,
)
content = re.sub(
    r'(x86_64-apple-darwin\.tar\.gz"\s+sha256\s+")([^"]*)(")',
    lambda match: f'{match.group(1)}{x86_64_macos_sha}{match.group(3)}',
    content,
)
if aarch64_linux_sha:
    content = re.sub(
        r'(aarch64-unknown-linux-gnu\.tar\.gz"\s+sha256\s+")([^"]*)(")',
        lambda match: f'{match.group(1)}{aarch64_linux_sha}{match.group(3)}',
        content,
    )

formula_path.write_text(content)
PYTHON_SCRIPT
}

publish_homebrew_tap() {
    local version=$1
    local x86_64_macos_sha=${2:-}
    local aarch64_macos_sha=${3:-}
    local aarch64_linux_sha=${4:-}
    local formula_path="homebrew/vtcode.rb"

    if [[ -z "$x86_64_macos_sha" ]]; then
        x86_64_macos_sha=$(cat "dist/vtcode-$version-x86_64-apple-darwin.sha256" 2>/dev/null | awk '{print $1}' || echo "")
    fi
    if [[ -z "$aarch64_macos_sha" ]]; then
        aarch64_macos_sha=$(cat "dist/vtcode-$version-aarch64-apple-darwin.sha256" 2>/dev/null | awk '{print $1}' || echo "")
    fi
    if [[ -z "$aarch64_linux_sha" ]]; then
        aarch64_linux_sha=$(cat "dist/vtcode-$version-aarch64-unknown-linux-gnu.sha256" 2>/dev/null | awk '{print $1}' || echo "")
    fi
    if [[ -z "$x86_64_macos_sha" || -z "$aarch64_macos_sha" ]]; then
        print_info "Fetching checksums from GitHub release $version..."
        local sha_tmp
        sha_tmp=$(mktemp -d)
        if gh release download "$version" --dir "$sha_tmp" --pattern "*.sha256" 2>/dev/null; then
            if [[ -z "$x86_64_macos_sha" ]]; then
                x86_64_macos_sha=$(cat "$sha_tmp/vtcode-$version-x86_64-apple-darwin.sha256" 2>/dev/null | awk '{print $1}' || echo "")
            fi
            if [[ -z "$aarch64_macos_sha" ]]; then
                aarch64_macos_sha=$(cat "$sha_tmp/vtcode-$version-aarch64-apple-darwin.sha256" 2>/dev/null | awk '{print $1}' || echo "")
            fi
            if [[ -z "$aarch64_linux_sha" ]]; then
                aarch64_linux_sha=$(cat "$sha_tmp/vtcode-$version-aarch64-unknown-linux-gnu.sha256" 2>/dev/null | awk '{print $1}' || echo "")
            fi
        fi
        rm -rf "$sha_tmp"
    fi

    if [[ -z "$x86_64_macos_sha" || -z "$aarch64_macos_sha" ]]; then
        print_error "Missing macOS checksums, cannot publish Homebrew tap"
        return 1
    fi

    print_info "Updating local Homebrew formula at $formula_path..."
    if ! update_homebrew_formula_file "$formula_path" "$version" "$x86_64_macos_sha" "$aarch64_macos_sha" "$aarch64_linux_sha"; then
        print_error "Failed to update local Homebrew formula"
        return 1
    fi

    if git diff --quiet -- "$formula_path"; then
        print_info "Local Homebrew formula is already up to date"
    else
        git add "$formula_path"
        if GIT_AUTHOR_NAME="vtcode-release-bot" \
            GIT_AUTHOR_EMAIL="noreply@vtcode.com" \
            GIT_COMMITTER_NAME="vtcode-release-bot" \
            GIT_COMMITTER_EMAIL="noreply@vtcode.com" \
            git commit -m "chore: update homebrew formula to $version [skip ci]"; then
            print_success "Local Homebrew formula updated and committed"
            if git push origin main --no-verify; then
                print_success "Homebrew formula commit pushed to origin"
            else
                print_warning "Failed to push Homebrew formula commit to origin"
            fi
        else
            print_warning "Failed to commit local Homebrew formula update"
        fi
    fi

    print_info "Publishing Homebrew formula to vinhnx/homebrew-tap..."

    local temp_dir
    temp_dir=$(mktemp -d 2>/dev/null || mktemp -d -t 'vtcode-homebrew')

    if ! (
        trap 'rm -rf "$temp_dir"' EXIT
        if ! gh repo clone vinhnx/homebrew-tap "$temp_dir" >/dev/null 2>&1; then
            print_error "Failed to clone vinhnx/homebrew-tap"
            exit 1
        fi
        cp "$formula_path" "$temp_dir/vtcode.rb"
        if git -C "$temp_dir" diff --quiet -- vtcode.rb; then
            print_info "Homebrew tap formula is already up to date"
            exit 0
        fi
        git -C "$temp_dir" add vtcode.rb
        if ! GIT_AUTHOR_NAME="vtcode-release-bot" \
            GIT_AUTHOR_EMAIL="noreply@vtcode.com" \
            GIT_COMMITTER_NAME="vtcode-release-bot" \
            GIT_COMMITTER_EMAIL="noreply@vtcode.com" \
            git -C "$temp_dir" commit -m "Update vtcode formula to v$version"; then
            print_error "Failed to commit Homebrew tap update"
            exit 1
        fi
        if ! git -C "$temp_dir" -c credential.helper='!gh auth git-credential' push https://github.com/vinhnx/homebrew-tap.git HEAD:main; then
            print_error "Failed to push Homebrew tap update"
            exit 1
        fi
        print_success "Published vtcode formula to vinhnx/homebrew-tap"
    ); then
        return 1
    fi
}

# ── CI ───────────────────────────────────────────────────────────────

trigger_and_wait_ci() {
    local released_version=$1
    local dry_run=$2

    if [[ "$dry_run" == 'true' ]]; then
        print_info "Dry run - would trigger CI workflow for $released_version"
        echo ""
        return
    fi

    print_info "Step 3.5: Triggering CI for Linux/Windows..."
    git push origin --tags --no-verify

    if ! gh workflow run build-linux-windows.yml --field tag="$released_version"; then
        print_warning "Failed to trigger CI workflow"
        echo ""
        return
    fi

    print_success "CI workflow triggered for $released_version"
    print_info "Waiting for CI builds to complete (timeout: 60 minutes)..."

    local wait_start=$(date +%s)
    local timeout=3600
    local run_id=""
    local attempts=0

    while [[ -z "$run_id" && $attempts -lt 24 ]]; do
        sleep 5
        run_id=$(gh run list --workflow build-linux-windows.yml --limit 1 --json databaseId --jq '.[0].databaseId' 2>/dev/null || true)
        attempts=$((attempts + 1))
    done

    if [[ -z "$run_id" ]]; then
        print_warning "Could not find CI workflow run"
        echo ""
        return
    fi

    local status="in_progress"
    while [[ "$status" == "in_progress" || "$status" == "queued" ]]; do
        sleep 30
        status=$(gh run view "$run_id" --json status --jq '.status' 2>/dev/null || echo "failed")
        local now=$(date +%s)
        local elapsed=$((now - wait_start))
        if [[ $elapsed -gt $timeout ]]; then
            print_warning "CI build timeout after $timeout seconds"
            echo ""
            return
        fi
        print_info "CI status: $status (${elapsed}s elapsed)"
    done

    if [[ "$status" == "completed" || "$status" == "success" ]]; then
        print_success "CI builds completed successfully"
        echo "$run_id"
    else
        print_warning "CI build failed with status: $status"
        gh run view "$run_id" --log || true
        echo ""
    fi
}

# ── GitHub Release ───────────────────────────────────────────────────

create_and_upload_release() {
    local released_version=$1
    local dry_run=$2
    local CI_RUN_ID=$3

    if [[ "$dry_run" == 'true' ]]; then
        print_info "Dry run - would create GitHub Release for $released_version"
        return
    fi

    if [[ -z "${GITHUB_TOKEN:-}" ]] && command -v gh >/dev/null 2>&1; then
        export GITHUB_TOKEN=$(gh auth token)
    fi

    if ! gh release view "$released_version" &>/dev/null; then
        local release_body=""
        [[ -f "$RELEASE_NOTES_FILE" ]] && release_body=$(cat "$RELEASE_NOTES_FILE")
        gh release create "$released_version" \
            --title "$released_version" \
            --notes "$release_body" \
            --draft=false \
            --prerelease=false
        print_success "GitHub Release $released_version created"
    fi

    local binaries_dir="/tmp/vtcode-release-$released_version"
    mkdir -p "$binaries_dir"

    # Use pre-built artifacts from dist/ (step 1 built & packaged everything there)
    print_info "Collecting pre-built artifacts from dist/..."
    if ls dist/vtcode-*.tar.gz dist/vtcode-*.zip 2>/dev/null >/dev/null; then
        cp dist/vtcode-*.tar.gz "$binaries_dir/" 2>/dev/null || true
        cp dist/vtcode-*.zip "$binaries_dir/" 2>/dev/null || true
        for archive in "$binaries_dir"/*.tar.gz "$binaries_dir"/*.zip; do
            if [[ -f "$archive" && ! -f "$archive.sha256" ]]; then
                shasum -a 256 "$archive" > "$archive.sha256"
            fi
        done
    fi

    # Download CI artifacts if zig wasn't available locally
    if [[ -n "$CI_RUN_ID" ]]; then
        print_info "Downloading CI-built binaries (run #$CI_RUN_ID)..."
        local ci_dir="/tmp/vtcode-ci-$released_version"
        mkdir -p "$ci_dir"
        for target in x86_64-unknown-linux-gnu x86_64-unknown-linux-musl aarch64-unknown-linux-gnu x86_64-pc-windows-msvc aarch64-pc-windows-msvc; do
            if gh run download "$CI_RUN_ID" --name "vtcode-${released_version}-${target}" --dir "$ci_dir" 2>/dev/null; then
                mv "$ci_dir"/*.tar.gz "$binaries_dir/" 2>/dev/null || true
                mv "$ci_dir"/*.zip "$binaries_dir/" 2>/dev/null || true
                mv "$ci_dir"/*.sha256 "$binaries_dir/" 2>/dev/null || true
            fi
        done
        rm -rf "$ci_dir"
    fi

    # Generate consolidated checksums.txt
    (
        cd "$binaries_dir"
        local shacmd=""
        if command -v sha256sum &>/dev/null; then
            shacmd="sha256sum"
        else
            shacmd="shasum -a 256"
        fi
        rm -f checksums.txt
        for f in *.tar.gz *.zip; do
            if [[ -f "$f" ]]; then
                $shacmd "$f" >> checksums.txt
            fi
        done
        touch checksums.txt
    )

    shopt -s nullglob
    local release_files=(
        "$binaries_dir"/*.tar.gz
        "$binaries_dir"/*.zip
        "$binaries_dir"/*.sha256
        "$binaries_dir"/checksums.txt
        "$SCRIPT_DIR/install.sh"
        "$SCRIPT_DIR/install.ps1"
    )
    shopt -u nullglob

    chmod +x "$SCRIPT_DIR/install.sh" "$SCRIPT_DIR/install.ps1" 2>/dev/null || true

    if gh release upload "$released_version" "${release_files[@]}" --clobber; then
        print_success "All binaries, checksums, and install scripts uploaded"
    else
        print_warning "Failed to upload binaries to GitHub Release"
        print_info "You can upload manually: gh release upload $released_version ${release_files[*]} --clobber"
    fi

    # Extract SHAs for Homebrew
    local x86_sha=""
    local arm_sha=""
    local arm_linux_sha=""
    [[ -f "$binaries_dir/vtcode-$released_version-x86_64-apple-darwin.sha256" ]] && \
        x86_sha=$(awk '{print $1}' "$binaries_dir/vtcode-$released_version-x86_64-apple-darwin.sha256")
    [[ -f "$binaries_dir/vtcode-$released_version-aarch64-apple-darwin.sha256" ]] && \
        arm_sha=$(awk '{print $1}' "$binaries_dir/vtcode-$released_version-aarch64-apple-darwin.sha256")
    [[ -f "$binaries_dir/vtcode-$released_version-aarch64-unknown-linux-gnu.sha256" ]] && \
        arm_linux_sha=$(awk '{print $1}' "$binaries_dir/vtcode-$released_version-aarch64-unknown-linux-gnu.sha256")

    rm -rf "$binaries_dir"

    # Export for Homebrew step
    echo "${x86_sha:-}|${arm_sha:-}|${arm_linux_sha:-}"
}
