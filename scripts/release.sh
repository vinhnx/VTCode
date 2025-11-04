#!/usr/bin/env bash

# VTCode Release Script powered by cargo-release
#
# This script handles releases for the main VTCode Rust binary and related components.
# For VSCode extension releases, use: cd vscode-extension && ./release.sh
#
# Changelog Generation:
# - This script uses cargo-release to manage versioning and tagging
# - Changelog generation is handled by changelogithub in GitHub Actions
# - When a tag is pushed, the release.yml workflow automatically generates
#   the changelog using conventional commit messages from .github/changelogithub.config.js
# - The generated changelog updates CHANGELOG.md and creates GitHub Releases
#
# Version Tagging:
# - Main binary uses: v0.39.0, v0.39.1, etc.
# - VSCode extension uses: vscode-v0.1.0, vscode-v0.1.1, etc. (separate versioning)

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
NC='\033[0m'

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
  <version>-<suffix>  Release with pre-release suffix (e.g. 1.2.3-alpha.1)
  --patch             Increment patch version (default)
  --minor             Increment minor version
  --major             Increment major version
  --pre-release       Create pre-release version (default: alpha.0)
  --pre-release-suffix <suffix>  Specify custom pre-release suffix (e.g. alpha, beta, rc)

Options:
  --dry-run           Run cargo-release in dry-run mode
  --skip-crates       Skip publishing crates to crates.io (pass --no-publish)
  --skip-npm          Skip npm publish step
  --skip-github-packages  Skip publishing to GitHub Packages (pass --no-publish)
  --skip-binaries     Skip building and uploading binaries
  --skip-docs         Skip docs.rs rebuild trigger
  --skip-zed-checksums Skip updating Zed extension checksums (default behavior)
  --enable-zed-checksums Enable updating Zed extension checksums (overrides skip)
  -h, --help          Show this help message
USAGE
}

# Ultra-optimized changelog generation using awk for everything
update_changelog_from_commits() {
    local version=$1
    local dry_run_flag=$2

    if [[ "$dry_run_flag" == 'true' ]]; then
        print_info "Dry run - would generate changelog for version $version from commits"
        return 0
    fi

    print_info "Generating changelog for version $version from commits..."

    # Get all commits and categorize them in a single awk operation
    local previous_tag
    previous_tag=$(git tag --sort=-v:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -n 2 | tail -n 1)

    local commits_range="HEAD"
    if [[ -n "$previous_tag" ]]; then
        print_info "Generating changelog from $previous_tag to HEAD"
        commits_range="$previous_tag..HEAD"
    else
        print_info "No previous tag found, getting all commits"
    fi

    # Use awk to categorize commits, but handle date separately since strftime may not be available
    local changelog_content
    local date_str
    date_str=$(date +%Y-%m-%d)
    
    changelog_content=$(git log "$commits_range" --no-merges --pretty=format:"%s" | awk -v vers="$version" -v date="$date_str" '
    {
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
    }')

    # Update changelog efficiently in one write operation
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

    # Stage and commit efficiently
    git add CHANGELOG.md
    # Use environment variables to avoid git hanging for editor
    GIT_AUTHOR_NAME="vtcode-release-bot" \
    GIT_AUTHOR_EMAIL="noreply@vtcode.com" \
    GIT_COMMITTER_NAME="vtcode-release-bot" \
    GIT_COMMITTER_EMAIL="noreply@vtcode.com" \
    git commit -m "docs: update changelog for v$version [skip ci]"
    print_success "Changelog generation completed for version $version"
}

# Ultra-fast version parsing using bash parameter expansion
get_current_version() {
    local line
    line=$(grep '^version = ' Cargo.toml)
    echo "${line#*\"}" | sed 's/\".*//'
}

# Optimized npm package version update




load_env_file() {
    if [[ -f '.env' ]]; then
        print_info 'Loading environment from .env'
        set -a
        # shellcheck disable=SC1091
        source .env
        set +a
    fi
}

check_branch() {
    local current_branch
    current_branch=$(git branch --show-current)
    if [[ "$current_branch" != 'main' ]]; then
        print_error 'You must be on the main branch to create a release'
        print_info "Current branch: $current_branch"
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

# Run all authentication checks (without npm)
check_all_auth() {
    local skip_npm=$1
    local skip_github_packages=$2

    # Check cargo auth
    if command -v cargo >/dev/null 2>&1; then
        local credentials_file="$HOME/.cargo/credentials.toml"
        if [[ -f "$credentials_file" && -s "$credentials_file" ]]; then
            print_success 'Cargo authentication verified' 
        else
            print_warning 'Cargo credentials not found or empty. Run `cargo login` before releasing.'
        fi
    else
        print_error 'Cargo is not available'
    fi

    # Skip npm authentication as npm release workflow has been removed
    if [[ "$skip_npm" == 'false' ]]; then
        print_info "npm release workflow has been removed - skipping npm authentication checks"
    fi

    if [[ "$skip_github_packages" == 'false' ]]; then
        # GitHub Packages authentication still requires npm but we'll provide info
        if command -v npm >/dev/null 2>&1; then
            if [[ -n "${GITHUB_TOKEN:-}" ]]; then
                local token_config
                token_config=$(npm config get //npm.pkg.github.com/:_authToken 2>/dev/null || echo "")
                
                if [[ -n "$token_config" && "$token_config" != "null" ]]; then
                    print_success 'GitHub Packages authentication verified'
                else
                    print_warning 'GITHUB_TOKEN may not be properly configured for GitHub Packages.'
                    print_info 'Make sure your GitHub token has write:packages, read:packages, and repo scopes.'
                fi
            else
                print_warning 'GITHUB_TOKEN environment variable not set. Set it before releasing to GitHub Packages.'
                print_info 'Make sure your GitHub token has write:packages, read:packages, and repo scopes.'
            fi
        else
            print_warning 'npm is not available (required for GitHub Packages authentication)'
        fi
    fi
}

ensure_cargo_release() {
    if ! command -v cargo-release >/dev/null 2>&1; then
        print_error 'cargo-release is not installed. Install it with `cargo install cargo-release`.'
        exit 1
    fi
}

ensure_cross_support() {
    if command -v cross >/dev/null 2>&1; then
        print_success 'cross detected – binary packaging will use reproducible cross-compilation builds'
        return 0
    fi

    if [[ -n "${VTCODE_SKIP_AUTO_CROSS:-}" ]]; then
        print_warning 'cross not found and automatic installation disabled (VTCODE_SKIP_AUTO_CROSS set). Builds will fall back to native cargo.'
        return 1
    fi

    print_warning 'cross not found. Attempting to install with `cargo install cross` for faster cross-compilation builds.'

    if ! command -v cargo >/dev/null 2>&1; then
        print_warning 'cargo not available; cannot install cross automatically. Builds will fall back to native cargo.'
        return 1
    fi

    if cargo install cross --quiet; then
        print_success 'cross installed successfully'
        return 0
    fi

    print_warning 'Unable to install cross automatically. Binary builds will fall back to cargo; cross-compilation may require manual OpenSSL setup.'
    return 1
}

# Optimized docs.rs trigger with correct HTTP status handling
trigger_docs_rs_rebuild() {
    local version=$1
    local dry_run_flag=$2

    if [[ "$dry_run_flag" == 'true' ]]; then
        print_info "Dry run - would trigger docs.rs rebuild for version $version"
        return 0
    fi

    if [[ -z "${CRATES_IO_TOKEN:-}" ]]; then
        print_warning 'CRATES_IO_TOKEN not set - skipping docs.rs rebuild trigger'
        return 0
    fi

    print_distribution "Triggering docs.rs rebuild for version $version..."

    # Define a function for triggering docs.rs that we can run in background
    _trigger_docs() {
        local crate_name=$1
        local version=$2
        local url="https://docs.rs/crates/$crate_name/$version"
        
        # Try GET request to check if the crate exists
        local response
        response=$(curl -s -o /dev/null -w "%{http_code}" --connect-timeout 10 "$url" 2>/dev/null || echo "0")
        
        if [[ "$response" == "200" ]]; then
            print_info "Crate $crate_name v$version exists on docs.rs - documentation will update on next publish"
        elif [[ "$response" == "404" ]]; then
            print_info "Crate $crate_name v$version not found yet on docs.rs - will be built when published"
        elif [[ "$response" == "405" ]]; then
            print_info "Crate $crate_name v$version - docs.rs update happens automatically on publish"
        else
            print_warning "Could not check docs.rs status for $crate_name v$version (HTTP $response)"
        fi
    }

    # Run both checks in parallel
    _trigger_docs "vtcode-core" "$version" &
    local pid_core=$!
    
    _trigger_docs "vtcode" "$version" &
    local pid_main=$!

    # Wait for both to complete
    wait "$pid_core"
    wait "$pid_main"
}





build_and_upload_binaries() {
    local version=$1

    print_distribution 'Building and distributing binaries...'

    if [[ ! -f 'scripts/build-and-upload-binaries.sh' ]]; then
        print_warning 'Binary build script not found - skipping binary distribution'
        return 0
    fi

    if ! ./scripts/build-and-upload-binaries.sh -v "$version"; then
        print_warning 'Binary build/distribution failed'
        return 1
    fi

    print_success 'Binaries built and distributed successfully'
}

update_extensions_version() {
    local version=$1

    print_info "Syncing extension versions with main project version $version..."

    # Update Zed extension version
    update_zed_extension_version "$version"

    # Update VSCode extension version
    update_vscode_extension_version "$version"
}

update_zed_extension_version() {
    local version=$1
    local manifest="zed-extension/extension.toml"

    if [[ ! -f "$manifest" ]]; then
        print_warning "Zed extension manifest not found at $manifest; skipping version update"
        return 0
    fi

    print_distribution "Updating Zed extension version to $version"

    # Use Python to update the version and URLs in extension.toml
    python3 -c "
import re
from pathlib import Path

manifest_path = Path('$manifest')
content = manifest_path.read_text()

# Update the main version field
content = re.sub(r'^version = \".*\"', f'version = \"{version}\"', content, flags=re.MULTILINE)

# Update URLs to use the new version
content = re.sub(
    r'https://github.com/vinhnx/vtcode/releases/download/v[0-9.]+/vtcode-v[0-9.]+-(aarch64-apple-darwin|x86_64-apple-darwin)\.tar\.gz',
    f'https://github.com/vinhnx/vtcode/releases/download/v{version}/vtcode-v{version}-\\\\1.tar.gz',
    content
)

manifest_path.write_text(content)
print(f'INFO: Zed extension version updated to {version}')
"
}

update_vscode_extension_version() {
    local version=$1
    local package_json="vscode-extension/package.json"

    if [[ ! -f "$package_json" ]]; then
        print_warning 'VSCode extension package.json not found; skipping version update'
        return 0
    fi

    print_distribution "Updating VSCode extension version to $version"

    # Use jq to update the version in package.json
    if command -v jq >/dev/null 2>&1; then
        jq --arg new_version "$version" '.version = $new_version' "$package_json" > "$package_json.tmp" && mv "$package_json.tmp" "$package_json"
        print_info "VSCode extension version updated to $version"
    else
        print_warning 'jq not found; skipping VSCode extension version update'
    fi
}

update_zed_extension_checksums() {
    local version=$1
    local manifest="zed-extension/extension.toml"
    local dist_dir="dist"

    if [[ ! -f "$manifest" ]]; then
        print_warning "Zed extension manifest not found at $manifest; skipping checksum update"
        return 0
    fi

    if [[ ! -d "$dist_dir" ]]; then
        print_warning "Distribution directory $dist_dir missing; skipping checksum update"
        return 0
    fi

    print_distribution "Updating Zed extension checksums from $dist_dir"

    # Create a more efficient Python script for checksum updates
    cat > /tmp/zed_checksum_update.py << 'PYTHON_EOF'
import re
import subprocess
import sys
from pathlib import Path

def main(version, manifest_path, dist_dir):
    manifest_path = Path(manifest_path)
    dist_dir = Path(dist_dir)

    targets = {
        "darwin-aarch64": f"vtcode-v{version}-aarch64-apple-darwin.tar.gz",
        "darwin-x86_64": f"vtcode-v{version}-x86_64-apple-darwin.tar.gz",
    }

    text = manifest_path.read_text()
    updated = False

    for target, filename in targets.items():
        archive = dist_dir / filename
        if not archive.exists():
            print(f"WARNING: Archive {archive} not found; leaving sha256 unchanged for {target}", file=sys.stderr)
            continue

        try:
            result = subprocess.run(["shasum", "-a", "256", str(archive)], capture_output=True, text=True, check=True)
            sha = result.stdout.split()[0]
            
            pattern = re.compile(rf"(\[agent_servers\.vtcode\.targets\.{re.escape(target)}\][^\[]*?sha256 = \")([^\"]*)(\")", re.DOTALL)
            new_text, count = pattern.subn("\\g<1>" + sha + "\\g<3>", text, count=1)
            
            if count == 0:
                print(f"WARNING: sha256 entry not found for target {target}", file=sys.stderr)
            else:
                text = new_text
                updated = True
                print(f"INFO: Updated {target} checksum to {sha}")

        except subprocess.CalledProcessError as e:
            print(f"ERROR: Failed to compute checksum for {archive}: {e}", file=sys.stderr)

    if updated:
        manifest_path.write_text(text)
        print(f"INFO: Zed extension checksums updated in {manifest_path}")
    else:
        print("WARNING: No sha256 fields updated in Zed extension manifest", file=sys.stderr)

if __name__ == "__main__":
    if len(sys.argv) != 4:
        print("Usage: python script.py <version> <manifest_path> <dist_dir>", file=sys.stderr)
        sys.exit(1)
    main(sys.argv[1], sys.argv[2], sys.argv[3])
PYTHON_EOF

    python3 /tmp/zed_checksum_update.py "$version" "$manifest" "$dist_dir"
    rm -f /tmp/zed_checksum_update.py
}

run_release() {
    local release_argument=$1
    local dry_run_flag=$2
    local skip_crates_flag=$3

    if [[ "$dry_run_flag" != 'true' ]]; then
        local version
        if [[ "$release_argument" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
            version="$release_argument"
        else
            local current_version
            current_version=$(get_current_version)
            IFS='.' read -ra version_parts <<< "$current_version"
            local major=${version_parts[0]}
            local minor=${version_parts[1]}
            local patch=${version_parts[2]}
            case "$release_argument" in
                "major") version="$((major + 1)).0.0" ;;
                "minor") version="${major}.$((minor + 1)).0" ;;
                "patch") version="${major}.${minor}.$((patch + 1))" ;;
                *) version="${major}.${minor}.$((patch + 1))" ;;
            esac
        fi
        update_changelog_from_commits "$version" "$dry_run_flag"
    fi

    local command=(cargo release "$release_argument" --workspace --config release.toml)

    if [[ "$skip_crates_flag" == 'true' ]]; then
        command+=(--no-publish)
    fi

    if [[ "$dry_run_flag" == 'true' ]]; then
        command+=(--dry-run --no-confirm)
    else
        command+=(--execute --no-confirm)
    fi

    print_info "Running: ${command[*]}"
    "${command[@]}"
}

run_prerelease() {
    local pre_release_suffix=$1
    local dry_run_flag=$2
    local skip_crates_flag=$3

    if [[ "$dry_run_flag" != 'true' ]]; then
        local current_version
        current_version=$(get_current_version)
        IFS='.' read -ra version_parts <<< "$current_version"
        local major=${version_parts[0]}
        local minor=${version_parts[1]}
        local patch_part=${version_parts[2]}
        local patch=$(echo "$patch_part" | sed 's/[^0-9]*$//')

        local version
        if [[ "$pre_release_suffix" == "alpha.0" ]]; then
            version="${major}.${minor}.$((patch + 1))-alpha.1"
        else
            version="${major}.${minor}.$((patch + 1))-${pre_release_suffix}"
        fi
        update_changelog_from_commits "$version" "$dry_run_flag"
    fi

    case "$pre_release_suffix" in
        alpha|beta|rc|release)
            local command=(cargo release "$pre_release_suffix" --workspace --config release.toml)
            ;;
        alpha.*|beta.*|rc.*)
            local base_suffix
            base_suffix=$(echo "$pre_release_suffix" | cut -d. -f1)
            local command=(cargo release "$base_suffix" --workspace --config release.toml)
            ;;
        *)
            print_warning "Using custom suffix '$pre_release_suffix' may result in duplicate pre-release markers"
            local command=(cargo release alpha --workspace --config release.toml -m "$pre_release_suffix")
            ;;
    esac

    if [[ "$skip_crates_flag" == 'true' ]]; then
        command+=(--no-publish)
    fi

    if [[ "$dry_run_flag" == 'true' ]]; then
        command+=(--dry-run --no-confirm)
    else
        command+=(--execute --no-confirm)
    fi

    print_info "Running: ${command[*]}"
    "${command[@]}"
}

main() {
    local release_argument=''
    local increment_type=''
    local dry_run=false
    local skip_crates=false
    local skip_npm=false
    local skip_github_packages=false
    local skip_binaries=false
    local skip_docs=false
    local skip_zed_checksums=true  # Changed to true to disable by default
    local enable_zed_checksums=false
    local pre_release=false
    local pre_release_suffix='alpha.0'

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                show_usage
                exit 0
                ;;
            -p|--patch)
                increment_type='patch'
                shift
                ;;
            -m|--minor)
                increment_type='minor'
                shift
                ;;
            -M|--major)
                increment_type='major'
                shift
                ;;
            --pre-release)
                pre_release=true
                increment_type='prerelease'
                shift
                ;;
            --pre-release-suffix)
                pre_release=true
                increment_type='prerelease'
                pre_release_suffix="${2:-alpha.0}"
                shift 2
                ;;
            --dry-run)
                dry_run=true
                shift
                ;;
            --skip-crates)
                skip_crates=true
                shift
                ;;
            --skip-npm)
                skip_npm=true
                shift
                ;;
            --skip-github-packages)
                skip_github_packages=true
                shift
                ;;
            --skip-binaries)
                skip_binaries=true
                shift
                ;;
            --skip-docs)
                skip_docs=true
                shift
                ;;
            --skip-zed-checksums)
                skip_zed_checksums=true
                shift
                ;;
            --enable-zed-checksums)
                skip_zed_checksums=false
                enable_zed_checksums=true
                shift
                ;;
            -*)
                print_error "Unknown option: $1"
                show_usage
                exit 1
                ;;
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

    if [[ -n "$increment_type" && -n "$release_argument" ]]; then
        print_error 'Cannot specify both increment type and explicit version'
        exit 1
    fi

    if [[ -z "$increment_type" && -z "$release_argument" ]]; then
        increment_type='patch'
    fi

    if [[ -n "$increment_type" ]]; then
        if [[ "$increment_type" != "prerelease" ]]; then
            release_argument=$increment_type
        fi
    fi

    load_env_file
    check_branch
    check_clean_tree
    ensure_cargo_release
    
    # Only install cross if we're not in dry-run mode and not skipping binaries
    if [[ "$dry_run" != 'true' ]] && [[ "$skip_binaries" == 'false' ]]; then
        ensure_cross_support || true
    elif [[ "$dry_run" == 'true' ]]; then
        print_info 'Dry run - skipping automatic cross installation check'
    fi
    
    # Run all auth checks together (they are fast and don't block each other)
    check_all_auth "$skip_npm" "$skip_github_packages"

    local current_version
    current_version=$(get_current_version)
    print_info "Current version: $current_version"

    # Skip npm package.json update as npm release workflow has been removed
    if [[ "$skip_npm" == 'false' ]]; then
        print_info "npm release workflow has been removed - skipping npm package updates"
    fi

    if [[ "$dry_run" == 'true' ]]; then
        print_warning 'Running in dry-run mode'
    else
        print_warning 'Releasing with cargo-release (this will modify and push tags)'
    fi

    if [[ "$pre_release" == 'true' ]]; then
        run_prerelease "$pre_release_suffix" "$dry_run" "$skip_crates"
    else
        run_release "$release_argument" "$dry_run" "$skip_crates"
    fi

    if [[ "$dry_run" == 'true' ]]; then
        print_success 'Dry run completed'
        exit 0
    fi

    local released_version
    released_version=$(get_current_version)
    print_success "Release completed for version $released_version"

    # Push operations combined into one step - also add a timeout for large pushes
    print_info "Pushing commits and tags to remote..."
    git push origin main && git push --tags origin
    print_success "Commits and tags pushed successfully"

    # Perform post-release operations in parallel with proper dependency management
    local pid_docs=""
    local pid_npm=""
    local pid_github=""
    local pid_binaries=""

    # Trigger docs.rs rebuild in background if not skipped
    if [[ "$skip_crates" == 'false' && "$skip_docs" == 'false' ]]; then
        trigger_docs_rs_rebuild "$released_version" false &
        pid_docs=$!
    fi

    # Skip npm publishing as npm release workflow has been removed
    if [[ "$skip_npm" == 'false' ]]; then
        print_info "npm publishing skipped - npm release workflow has been removed"
    fi

    # Skip GitHub Packages publishing as npm release workflow has been completely removed
    if [[ "$skip_github_packages" == 'false' ]]; then
        print_info "GitHub Packages publishing skipped - npm/GitHub Packages release workflow has been completely removed"
    fi

    # Build binaries in background if not skipped
    local binaries_completed=false
    if [[ "$skip_binaries" == 'false' ]]; then
        # Disable cross compilation by default to avoid Docker dependency
        # Users can override with VTCODE_DISABLE_CROSS=0 to use cross
        export VTCODE_DISABLE_CROSS=${VTCODE_DISABLE_CROSS:-1}
        build_and_upload_binaries "$released_version" &
        pid_binaries=$!
        binaries_completed=true
    fi

    # Wait for binaries to complete before updating Zed checksums
    if [[ $binaries_completed == true ]]; then
        wait "$pid_binaries" || print_error "Binary build failed"
        # Only update Zed checksums if explicitly enabled (disabled by default)
        if [[ "$skip_zed_checksums" == 'false' ]]; then
            # Only run Zed checksum update if dist directory exists and has files
            if [[ -d "dist" ]] && [[ "$(ls -A dist 2>/dev/null)" ]]; then
                update_zed_extension_checksums "$released_version"
            else
                print_warning "dist directory is empty or doesn't exist - skipping Zed extension checksum update"
            fi
        fi
    fi

    # Update extension versions to match main project version
    update_extensions_version "$released_version"

    # Wait for all other background processes to complete
    for pid in $pid_docs; do
        if [[ -n "$pid" ]]; then
            wait "$pid" || print_warning "Background process $pid failed"
        fi
    done

    print_info 'VSCode extension publishing skipped'
    print_info 'To release the VSCode extension separately, use: cd vscode-extension && ./release.sh'

    print_success 'Release process finished'
    print_info "GitHub Release should now contain changelog notes generated by cargo-release"
    print_info "All commits, tags, and releases have been pushed to the remote repository"
}

main "$@"