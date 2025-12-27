#!/usr/bin/env bash

# VT Code Release Script powered by cargo-release
#
# This script handles releases for the main VT Code Rust binary and related components.
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
  --skip-binaries     Skip building and uploading binaries
  --skip-docs         Skip docs.rs rebuild trigger
  --skip-release      Skip GitHub release verification (for CI/CD or manual releases)
  --trusted-publishers-info  Show information about trusted publishers setup
  --verify-trusted-publishing  Verify trusted publishing setup
  -h, --help          Show this help message

Note: For CI/CD environments, this script supports npm trusted publishers (OIDC) for secure publishing.
      Ensure your CI/CD workflow is configured for trusted publishing: https://docs.npmjs.com/trusted-publishers
USAGE
}

# Explain trusted publishers setup
show_trusted_publishers_info() {
    cat <<'INFO'
Trusted Publishers Setup Information:

Trusted publishing allows secure publishing from CI/CD workflows to npmjs.com using OpenID Connect (OIDC),
eliminating the need for long-lived npm tokens.

To configure trusted publishers for this project:

1. For npmjs.com:
   - Go to https://www.npmjs.com/settings/{username}/tokens
   - Set up trusted publishing for your CI/CD provider (GitHub Actions or GitLab CI/CD)
   - Provide your GitHub/GitLab organization, repository, and workflow filename

2. For GitHub Actions, add this to your workflow:
   ```yaml
   permissions:
     id-token: write  # Required for trusted publishing
     contents: read
   ```

3. Your CI/CD workflow should use npm CLI version 11.5.1 or later

Learn more: https://docs.npmjs.com/trusted-publishers
INFO
}

# Verify trusted publishing setup
verify_trusted_publishing_setup() {
    cat <<'VERIFY'
Trusted Publishing Verification:

1. Check if you're properly set up for trusted publishing:

   # Check npm version (should be >= 11.5.1)
   npm --version

   # Check if running in CI environment
   echo $CI
   echo $GITHUB_ACTIONS

   # Check if npm registry is properly configured
   npm config get registry

2. For testing trusted publishing locally, you can configure a test setup:
   # Login to npm if needed
   npm login

   # Verify your account
   npm whoami

3. For CI/CD verification, ensure your workflow includes:
   ```yaml
   permissions:
     id-token: write  # Required for trusted publishing
     contents: read
   ```

4. If you have already configured trusted publishing on npmjs.com, simply run this script
   in your CI/CD environment and publishing should work automatically without tokens.

VERIFY
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
update_npm_package_version() {
    local version=$1
    local npm_package_file="npm/package.json"

    if [[ ! -f "$npm_package_file" ]]; then
        print_warning "npm package.json not found at $npm_package_file - skipping npm version update"
        return 0
    fi

    print_distribution "Updating npm package version to $version"

    if command -v jq >/dev/null 2>&1; then
        jq --arg new_version "$version" '.version = $new_version' "$npm_package_file" > "$npm_package_file.tmp" && mv "$npm_package_file.tmp" "$npm_package_file"
        print_info "npm package version updated to $version"
        git add "$npm_package_file"
    else
        print_warning 'jq not found; skipping npm package version update'
    fi
}

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

# Run all authentication checks (with trusted publishers info)
check_all_auth() {
    local skip_npm=$1

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

    # Check npm authentication
    if [[ "$skip_npm" == 'false' ]]; then
        if command -v npm >/dev/null 2>&1; then
            print_info 'npm detected - since you have trusted publishing configured, no token required in CI/CD'
            print_info 'Trusted publishing uses OIDC tokens provided by the CI/CD environment'
            print_info 'In local environments, ensure npm is authenticated: npm whoami'

            # Check if we're in a CI environment
            if [[ -n "${CI:-}" || -n "${GITHUB_ACTIONS:-}" ]]; then
                print_info 'Running in CI/CD environment - trusted publishing will be used automatically'
            else
                print_info 'Running locally - trusted publishing may not be available, fallback to token auth'
            fi

            # Check if npm is logged in locally (useful for local testing)
            if ! npm whoami >/dev/null 2>&1; then
                print_info 'npm not logged in locally - this is OK if running in CI/CD with trusted publishing'
            else
                print_info 'npm is logged in locally - authentication check passed'
            fi
        else
            print_warning 'npm is not available (required for npm publishing)'
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

publish_npm_package() {
    local version=$1

    print_distribution "Publishing npm package v$version to npmjs.com..."

    if [[ ! -f 'npm/package.json' ]]; then
        print_warning 'npm package.json not found - skipping npm publish'
        return 0
    fi

    if ! command -v npm >/dev/null 2>&1; then
        print_warning 'npm not available - skipping npm publish'
        return 0
    fi

    # Ensure npm CLI version 11.5.1 or later for trusted publishing support
    local npm_version
    npm_version=$(npm --version)
    print_info "Current npm version: $npm_version"

    # Extract major.minor.patch from version string to compare
    local npm_major npm_minor npm_patch
    IFS='.' read -ra version_parts <<< "$npm_version"
    npm_major=${version_parts[0]:-0}
    npm_minor=${version_parts[1]:-0}
    npm_patch=${version_parts[2]:-0}

    # Compare versions - trusted publishing requires npm v11.5.1 or later
    if [[ $npm_major -lt 11 ]] ||
       [[ $npm_major -eq 11 && $npm_minor -lt 5 ]] ||
       [[ $npm_major -eq 11 && $npm_minor -eq 5 && $npm_patch -lt 1 ]]; then
        print_warning "npm version $npm_version is too old for trusted publishing (requires v11.5.1+)"
        print_info "Updating npm to latest version for trusted publishing support..."
        npm install -g npm@latest || {
            print_error "Failed to update npm - trusted publishing will not work properly"
            return 1
        }
    fi

    # Check if in a CI/CD environment (detected by environment variables)
    local in_ci=false
    if [[ -n "${CI:-}" || -n "${GITHUB_ACTIONS:-}" || -n "${GITLAB_CI:-}" || -n "${CIRCLECI:-}" || -n "${TRAVIS:-}" ]]; then
        in_ci=true
        print_info "Running in CI/CD environment - trusted publishing should work automatically"
    else
        print_info "Running in local environment - trusted publishing may not be available"
        print_info "If publishing fails, you may need to configure npm tokens locally or run in CI/CD environment"
    fi

    # If running locally without npm login, skip npm publish to avoid 401s
    local npm_logged_in=true
    if ! npm whoami >/dev/null 2>&1; then
        npm_logged_in=false
    fi

    if [[ "$in_ci" == false && "$npm_logged_in" == false ]]; then
        print_warning "npm auth not configured locally; skipping npm publish (use --skip-npm or npm login)"
        return 0
    fi

    # Ensure npm package has required bin directory structure
    if [[ ! -f "npm/bin/vtcode" ]]; then
        print_warning "npm/bin/vtcode stub not found, creating placeholder..."
        mkdir -p npm/bin
        cat > "npm/bin/vtcode" << 'EOF'
#!/usr/bin/env node
console.error('VT Code binary not downloaded yet. Please run: npm install');
console.error('If you see this after installation, reinstall with: npm uninstall -g @vinhnx/vtcode && npm install -g @vinhnx/vtcode');
process.exit(1);
EOF
        chmod +x npm/bin/vtcode
        print_success "Created npm/bin/vtcode stub for publish validation"
    fi

    # Publish to npmjs.com with different package name using trusted publishing (OIDC)
    print_distribution "Publishing npm package v$version to npmjs.com using trusted publishers (OIDC)..."

    # Create a temporary copy of package.json with different name for npmjs.com
    local temp_npm_dir=$(mktemp -d)
    cp -r npm/* "$temp_npm_dir/"

    # Modify the package name for npmjs.com (unscoped vtcode)
    jq --arg new_name "vtcode" '.name = $new_name' "$temp_npm_dir/package.json" > "$temp_npm_dir/package.json.tmp" && mv "$temp_npm_dir/package.json.tmp" "$temp_npm_dir/package.json"

    # Remove the scoped registry config for npmjs.com publish
    if jq 'del(.publishConfig)' "$temp_npm_dir/package.json" > "$temp_npm_dir/package.json.tmp" 2>/dev/null; then
        mv "$temp_npm_dir/package.json.tmp" "$temp_npm_dir/package.json"
    fi

    (
        cd "$temp_npm_dir" || return 1

        # For trusted publishing in CI/CD, no token is required
        # npm will automatically use OIDC tokens provided by the CI/CD environment
        # In local environments, this may fallback to configured tokens
        if npm publish; then
            print_success "npm package v$version published to npmjs.com as vtcode using trusted publishers"
            return 0
        else
            print_warning "npm publish to npmjs.com failed"

            # Provide more specific guidance based on environment
            if [[ "$in_ci" == true ]]; then
                print_warning "Running in CI/CD - check trusted publishing configuration at npmjs.com"
                print_info "Ensure your workflow is configured with proper permissions and trusted publishers are set up"
                print_info "Visit: https://docs.npmjs.com/trusted-publishers to configure trusted publishers"
            else
                print_info "For local publishing, you may need to run: npm login"
                print_info "Or ensure you have proper token configured in ~/.npmrc"
            fi

            return 1
        fi
    )

    # Clean up temporary directory
    rm -rf "$temp_npm_dir"

    return 0
}

build_and_upload_binaries() {
    local version=$1

    print_distribution 'Building and distributing binaries...'

    if [[ ! -f 'scripts/build-and-upload-binaries.sh' ]]; then
        print_warning 'Binary build script not found - skipping binary distribution'
        return 0
    fi

    # Enable cross-compilation by default for multi-platform builds
    export VTCODE_DISABLE_CROSS=0
    print_info "Cross-compilation enabled for multi-platform builds"

    if ! ./scripts/build-and-upload-binaries.sh -v "$version"; then
        print_warning 'Binary build/distribution failed'
        return 1
    fi

    print_success 'Binaries built and distributed successfully'
}

update_extensions_version() {
    local version=$1

    print_info "Syncing extension versions with main project version $version..."

    # Update VSCode extension version (Zed extension release removed)
    update_vscode_extension_version "$version"
}

# Zed extension functionality removed
# update_zed_extension_version() {
#     local version=$1
#     local manifest="zed-extension/extension.toml"
#
#     if [[ ! -f "$manifest" ]]; then
#         print_warning "Zed extension manifest not found at $manifest; skipping version update"
#         return 0
#     fi
#
#     print_distribution "Updating Zed extension version to $version"
#
#     # Use Python to update the version and URLs in extension.toml
#     python3 -c "
# import re
# from pathlib import Path
#
# manifest_path = Path('$manifest')
# content = manifest_path.read_text()
#
# # Update the main version field
# content = re.sub(r'^version = \".*\"', f'version = \"{version}\"', content, flags=re.MULTILINE)
#
# # Update URLs to use the new version
# content = re.sub(
#     r'https://github.com/vinhnx/vtcode/releases/download/v[0-9.]+/vtcode-v[0-9.]+-(aarch64-apple-darwin|x86_64-apple-darwin)\.tar\.gz',
#     f'https://github.com/vinhnx/vtcode/releases/download/v{version}/vtcode-v{version}-\\\\1.tar.gz',
#     content
# )
#
# manifest_path.write_text(content)
# print(f'INFO: Zed extension version updated to {version}')
# "
# }

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

        # Stage and commit the updated package.json file
        git add "$package_json"
        git commit -m "chore: update VSCode extension package.json to v$version [skip ci]" --no-verify
        print_info "Committed VSCode extension package.json with version $version"
    else
        print_warning 'jq not found; skipping VSCode extension version update'
    fi
}

# Zed extension functionality removed
# update_zed_extension_checksums() {
#     local version=$1
#     local manifest="zed-extension/extension.toml"
#     local dist_dir="dist"
#
#     if [[ ! -f "$manifest" ]]; then
#         print_warning "Zed extension manifest not found at $manifest; skipping checksum update"
#         return 0
#     fi
#
#     if [[ ! -d "$dist_dir" ]]; then
#         print_warning "Distribution directory $dist_dir missing; skipping checksum update"
#         return 0
#     fi
#
#     print_distribution "Updating Zed extension checksums from $dist_dir"
#
#     # Create a more efficient Python script for checksum updates
#     cat > /tmp/zed_checksum_update.py << 'PYTHON_EOF'
# import re
# import subprocess
# import sys
# from pathlib import Path
#
# def main(version, manifest_path, dist_dir):
#     manifest_path = Path(manifest_path)
#     dist_dir = Path(dist_dir)
#
#     targets = {
#         "darwin-aarch64": f"vtcode-v{version}-aarch64-apple-darwin.tar.gz",
#         "darwin-x86_64": f"vtcode-v{version}-x86_64-apple-darwin.tar.gz",
#     }
#
#     text = manifest_path.read_text()
#     updated = False
#
#     for target, filename in targets.items():
#         archive = dist_dir / filename
#         if not archive.exists():
#             print(f"WARNING: Archive {archive} not found; leaving sha256 unchanged for {target}", file=sys.stderr)
#             continue
#
#         try:
#             result = subprocess.run(["shasum", "-a", "256", str(archive)], capture_output=True, text=True, check=True)
#             sha = result.stdout.split()[0]
#
#             pattern = re.compile(rf"(\[agent_servers\.vtcode\.targets\.{re.escape(target)}\][^\[]*?sha256 = \")([^\"]*)(\")", re.DOTALL)
#             new_text, count = pattern.subn("\\g<1>" + sha + "\\g<3>", text, count=1)
#
#             if count == 0:
#                 print(f"WARNING: sha256 entry not found for target {target}", file=sys.stderr)
#             else:
#                 text = new_text
#                 updated = True
#                 print(f"INFO: Updated {target} checksum to {sha}")
#
#         except subprocess.CalledProcessError as e:
#             print(f"ERROR: Failed to compute checksum for {archive}: {e}", file=sys.stderr)
#
#     if updated:
#         manifest_path.write_text(text)
#         print(f"INFO: Zed extension checksums updated in {manifest_path}")
#     else:
#         print("WARNING: No sha256 fields updated in Zed extension manifest", file=sys.stderr)
#
# if __name__ == "__main__":
#     if len(sys.argv) != 4:
#         print("Usage: python script.py <version> <manifest_path> <dist_dir>", file=sys.stderr)
#         sys.exit(1)
#     main(sys.argv[1], sys.argv[2], sys.argv[3])
# PYTHON_EOF
#
#     python3 /tmp/zed_checksum_update.py "$version" "$manifest" "$dist_dir"
#     rm -f /tmp/zed_checksum_update.py
# }

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
    local skip_binaries=false
    local skip_docs=false
    local skip_release_check=false
    local pre_release=false
    local pre_release_suffix='alpha.0'
    local show_trusted_publishers=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                show_usage
                exit 0
                ;;
            --trusted-publishers-info)
                show_trusted_publishers_info
                exit 0
                ;;
            --verify-trusted-publishing)
                verify_trusted_publishing_setup
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
            --skip-binaries)
                skip_binaries=true
                shift
                ;;
            --skip-docs)
                skip_docs=true
                shift
                ;;
            --skip-release)
                skip_release_check=true
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
    check_all_auth "$skip_npm"

    local current_version
    current_version=$(get_current_version)
    print_info "Current version: $current_version"

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

    # Update npm package version to match the released version and commit the change
    if [[ "$skip_npm" == 'false' ]]; then
        update_npm_package_version "$released_version"
        # Commit the npm package.json change
        if [[ "$dry_run" != 'true' ]]; then
            if [[ -n "$(git status --porcelain npm/package.json)" ]]; then
                git add npm/package.json
                git commit -m "chore: update npm package.json to v$released_version [skip ci]" --no-verify
                print_info "Committed npm package.json with version $released_version"
            fi
        fi
    fi

    # Push operations combined into one step - also add a timeout for large pushes
    print_info "Pushing commits and tags to remote..."
    git push origin main && git push --tags origin
    print_success "Commits and tags pushed successfully"

    # Inform user about GitHub Actions workflows that will run
    print_info "GitHub Actions workflows triggered:"
    print_info "  - Release workflow: Creates GitHub release and changelog"
    print_info "  - Release-on-tag workflow: Builds and uploads binaries"
    print_info "These workflows may take 5-15 minutes to complete depending on platform builds"

    # Check if we should skip GitHub release verification
    if [[ "$skip_release_check" == 'true' ]]; then
        print_info "Skipping GitHub release verification as requested"
    else
        # Detect if we're in CI environment
        local in_ci=false
        if [[ -n "${CI:-}" || -n "${GITHUB_ACTIONS:-}" || -n "${GITLAB_CI:-}" || -n "${CIRCLECI:-}" || -n "${TRAVIS:-}" ]]; then
            in_ci=true
            print_info "Running in CI environment - skipping GitHub release wait (GitHub Actions handles this)"
            skip_release_check=true
        fi

        if [[ "$skip_release_check" != 'true' ]]; then
            # Wait for GitHub release to be created by GitHub Actions
            print_info "Waiting for GitHub release v$released_version to be created by GitHub Actions..."
            local retry_count=0
            local max_retries=90  # Increased to 3 minutes (90 attempts × 2 seconds) to accommodate GitHub Actions
            while ! gh release view "v$released_version" >/dev/null 2>&1; do
                retry_count=$((retry_count + 1))
                if [[ $retry_count -gt $max_retries ]]; then
                    print_warning "GitHub release v$released_version not found after $max_retries attempts"
                    print_warning "GitHub Actions may still be running or the release may need to be created manually"
                    print_info "You can skip this check in the future with --skip-release flag"
                    if [[ "$in_ci" == false ]]; then
                        read -p "Continue anyway? (y/N): " -n 1 -r
                        echo
                        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                            exit 1
                        fi
                    fi
                    break
                fi
                if [[ $retry_count -eq 1 ]]; then
                    print_info "Release not found yet, will keep checking... (GitHub Actions may take a few minutes)"
                fi
                # Show progress every 10 attempts
                if [[ $((retry_count % 10)) -eq 0 ]]; then
                    print_info "Still waiting... (attempt $retry_count/$max_retries) - GitHub Actions may still be building"
                fi
                sleep 2
            done

            print_success "GitHub release v$released_version confirmed created"
        fi
    fi

    # Verify GitHub CLI authentication before starting uploads
    print_info "Verifying GitHub CLI authentication..."
    if ! command -v gh &> /dev/null; then
        print_error "GitHub CLI (gh) is required for binary uploads but not installed"
        print_info "Install from: https://cli.github.com/"
        exit 1
    fi

    local expected_account="vinhnx"
    if ! gh auth status >/dev/null 2>&1; then
        print_error "GitHub CLI is not authenticated. Please run: gh auth login"
        exit 1
    fi

    if ! gh auth status 2>&1 | grep -q "Logged in to github.com account $expected_account"; then
        print_error "Not logged in to GitHub account: $expected_account"
        print_info "Run: gh auth login --hostname github.com"
        exit 1
    fi

    if ! gh auth status 2>&1 | grep -A 5 "account $expected_account" | grep -q "Active account: true"; then
        print_error "GitHub account '$expected_account' is not active"
        print_info "Run: gh auth switch --hostname github.com --user $expected_account"
        exit 1
    fi

    print_success "GitHub CLI authenticated with correct account: $expected_account"

    # Perform post-release operations
    # 1. Publish npm package (DISABLED - avoids GitHub Actions build costs)
    if [[ "$skip_npm" == 'false' ]]; then
        print_warning "npm publishing is disabled to avoid build costs"
        print_info "Install VT Code via: cargo install vtcode or brew install vinhnx/tap/vtcode"
    fi

    # 2. Build and upload binaries
    if [[ "$skip_binaries" == 'false' ]]; then
        # Enable cross-compilation for multi-platform builds
        export VTCODE_DISABLE_CROSS=0
        print_info "Cross-compilation enabled for multi-platform builds"

        if ! build_and_upload_binaries "$released_version"; then
            print_error "Binary build/upload failed"
            # In CI environments, don't exit on binary build failure as GitHub Actions handles this
            if [[ "$in_ci" == true ]]; then
                print_warning "Continuing in CI environment despite binary build failure (GitHub Actions handles binary builds)"
            else
                exit 1
            fi
        fi
    fi

    # 3. Handle docs.rs rebuild (background)
    local pid_docs=""
    if [[ "$skip_crates" == 'false' && "$skip_docs" == 'false' ]]; then
        trigger_docs_rs_rebuild "$released_version" false &
        pid_docs=$!
    fi

    # Wait for background docs.rs process if it was started
    if [[ -n "$pid_docs" ]]; then
        print_info 'Waiting for docs.rs rebuild trigger to complete...'
        if wait "$pid_docs"; then
            print_success 'docs.rs rebuild triggered'
        else
            print_warning 'docs.rs rebuild trigger may have failed'
        fi
    fi

    # Update extension versions to match main project version
    update_extensions_version "$released_version"

    # Zed extension checksum update removed
    print_info "Zed extension checksum update functionality has been removed from this release"

    print_info 'VSCode extension publishing skipped'
    print_info 'To release the VSCode extension separately, use: cd vscode-extension && ./release.sh'

    print_success 'Release process finished'
    print_info "GitHub Release should now contain changelog notes generated by cargo-release"
    print_info "All commits, tags, and releases have been pushed to the remote repository"
    if [[ "$skip_binaries" == 'false' && "$in_ci" != 'true' ]]; then
        print_info "Binary assets have been uploaded to the GitHub release"
    else
        print_info "Binary assets will be uploaded by GitHub Actions (release-on-tag workflow)"
    fi
    print_info "Install VT Code via: cargo install vtcode or brew install vinhnx/tap/vtcode"
    print_info ""
    print_info "GitHub Actions workflows status:"
    print_info "  Check: https://github.com/vinhnx/vtcode/actions for workflow progress"
    print_info "  Release assets will be available at: https://github.com/vinhnx/vtcode/releases/tag/v$released_version"
}

main "$@"