#!/usr/bin/env bash

# VTCode Release Script powered by cargo-release
#
# Changelog Generation:
# - This script uses cargo-release to manage versioning and tagging
# - Changelog generation is handled by changelogithub in GitHub Actions
# - When a tag is pushed, the release.yml workflow automatically generates
#   the changelog using conventional commit messages from .github/changelogithub.config.js
# - The generated changelog updates CHANGELOG.md and creates GitHub Releases

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

  --skip-docs         Skip docs.rs rebuild trigger
  --enable-homebrew   Build and upload Homebrew binaries after release
  -h, --help          Show this help message
USAGE
}

update_changelog_from_commits() {
    local version=$1
    local dry_run_flag=$2

    if [[ "$dry_run_flag" == 'true' ]]; then
        print_info "Dry run - would generate changelog for version $version from commits"
        return 0
    fi

    print_info "Generating changelog for version $version from commits..."

    # Get the tag for the previous version to compare against
    local previous_tag
    previous_tag=$(git tag --sort=-v:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -n 2 | tail -n 1)
    
    if [[ -n "$previous_tag" ]]; then
        print_info "Generating changelog from $previous_tag to HEAD"
        
        # Get commits from the previous tag to HEAD
        local commits
        commits=$(git log "$previous_tag..HEAD" --oneline --no-merges --pretty=format:"%s")
    else
        # If no previous tag, get all commits
        print_info "No previous tag found, getting all commits"
        local commits
        commits=$(git log --oneline --no-merges --pretty=format:"%s")
    fi

    # Group commits by conventional commit types
    local feat_commits=$(echo "$commits" | grep -E '^(feat|feature)' | sed 's/^/    - /' | sed 's/):/:/')
    local fix_commits=$(echo "$commits" | grep -E '^(fix|bug)' | sed 's/^/    - /' | sed 's/):/:/')
    local perf_commits=$(echo "$commits" | grep -E '^(perf|performance)' | sed 's/^/    - /' | sed 's/):/:/')
    local refactor_commits=$(echo "$commits" | grep -E '^(refactor)' | sed 's/^/    - /' | sed 's/):/:/')
    local docs_commits=$(echo "$commits" | grep -E '^(docs|documentation)' | sed 's/^/    - /' | sed 's/):/:/')
    local style_commits=$(echo "$commits" | grep -E '^(style)' | sed 's/^/    - /' | sed 's/):/:/')
    local test_commits=$(echo "$commits" | grep -E '^(test)' | sed 's/^/    - /' | sed 's/):/:/')
    local chore_commits=$(echo "$commits" | grep -E '^(chore)' | sed 's/^/    - /' | sed 's/):/:/')
    local build_commits=$(echo "$commits" | grep -E '^(build)' | sed 's/^/    - /' | sed 's/):/:/')
    local ci_commits=$(echo "$commits" | grep -E '^(ci)' | sed 's/^/    - /' | sed 's/):/:/')
    
    # Prepare new changelog entry
    local new_entry=""
    new_entry+="# [Version $version] - $(date +%Y-%m-%d)$'\n\n'"
    
    if [[ -n "$feat_commits" ]]; then
        new_entry+="### Features$'\n'$feat_commits$'\n\n'"
    fi
    
    if [[ -n "$fix_commits" ]]; then
        new_entry+="### Bug Fixes$'\n'$fix_commits$'\n\n'"
    fi
    
    if [[ -n "$perf_commits" ]]; then
        new_entry+="### Performance Improvements$'\n'$perf_commits$'\n\n'"
    fi
    
    if [[ -n "$refactor_commits" ]]; then
        new_entry+="### Refactors$'\n'$refactor_commits$'\n\n'"
    fi
    
    if [[ -n "$docs_commits" ]]; then
        new_entry+="### Documentation$'\n'$docs_commits$'\n\n'"
    fi
    
    if [[ -n "$style_commits" ]]; then
        new_entry+="### Style Changes$'\n'$style_commits$'\n\n'"
    fi
    
    if [[ -n "$test_commits" ]]; then
        new_entry+="### Tests$'\n'$test_commits$'\n\n'"
    fi
    
    if [[ -n "$build_commits" ]]; then
        new_entry+="### Build System$'\n'$build_commits$'\n\n'"
    fi
    
    if [[ -n "$ci_commits" ]]; then
        new_entry+="### CI Changes$'\n'$ci_commits$'\n\n'"
    fi
    
    if [[ -n "$chore_commits" ]]; then
        new_entry+="### Chores$'\n'$chore_commits$'\n\n'"
    fi

    # Insert the new entry at the beginning of the changelog, right after the initial header
    if [[ -f CHANGELOG.md ]]; then
        # Create a temporary file with the new content
        local temp_changelog
        temp_changelog=$(mktemp)
        
        # Copy the header (first few lines) to the temp file
        {
            head -n 5 CHANGELOG.md
            printf "%b" "$new_entry"
            echo ""
            tail -n +6 CHANGELOG.md
        } > "$temp_changelog"
        
        # Replace the original file
        mv "$temp_changelog" CHANGELOG.md
        
        print_success "Updated CHANGELOG.md with entries for version $version"
    else
        # Create a new changelog file
        {
            echo "# Changelog - vtcode"
            echo ""
            echo "All notable changes to vtcode will be documented in this file."
            echo ""
            printf "%b" "$new_entry"
        } > CHANGELOG.md
        
        print_success "Created new CHANGELOG.md with entries for version $version"
    fi
    
    # Stage the changelog file for commit
    git add CHANGELOG.md
    
    # Create a commit for the changelog update
    git commit -m "docs: update changelog for v$version [skip ci]"
    
    print_success "Changelog generation completed for version $version"
}

update_npm_package_version() {
    local release_arg=$1
    local is_pre_release=$2
    local pre_release_suffix=$3

    if [[ ! -f "npm/package.json" ]]; then
        print_warning "npm/package.json not found - skipping npm version update"
        return 0
    fi

    local current_version
    current_version=$(get_current_version)
    
    # Calculate the next version based on the release type
    local next_version
    
    if [[ "$is_pre_release" == "true" ]]; then
        # For pre-release, increment the patch version and add the pre-release suffix
        IFS='.' read -ra version_parts <<< "$current_version"
        local major=${version_parts[0]}
        local minor=${version_parts[1]}
        local patch=${version_parts[2]}
        
        # Extract the numeric part of the patch if it contains additional info after the number
        patch=$(echo "$patch" | sed 's/[^0-9]*$//')
        
        if [[ "$pre_release_suffix" == "alpha.0" ]]; then
            # Default to alpha.1
            next_version="${major}.${minor}.$((patch + 1))-alpha.1"
        else
            next_version="${major}.${minor}.$((patch + 1))-${pre_release_suffix}"
        fi
    else
        # For regular releases (patch, minor, major)
        IFS='.' read -ra version_parts <<< "$current_version"
        local major=${version_parts[0]}
        local minor=${version_parts[1]}
        local patch=${version_parts[2]}
        
        # Extract the numeric part of the patch if needed
        patch=$(echo "$patch" | sed 's/[^0-9]*$//')
        
        case "$release_arg" in
            "major")
                next_version="$((major + 1)).0.0"
                ;;
            "minor")
                next_version="${major}.$((minor + 1)).0"
                ;;
            "patch")
                next_version="${major}.${minor}.$((patch + 1))"
                ;;
            *)
                # If a specific version was provided
                if [[ "$release_arg" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]]; then
                    next_version="$release_arg"
                else
                    # Default fallback - should not happen in normal usage
                    next_version="${major}.${minor}.$((patch + 1))"
                fi
                ;;
        esac
    fi
    
    print_info "Updating npm/package.json version from $current_version to $next_version"

    # Update the version in npm/package.json
    if [[ "$OSTYPE" == "darwin"* ]]; then
        # macOS
        sed -i '' "s/\"version\": \"[^\"]*\"/\"version\": \"$next_version\"/" npm/package.json
    else
        # Linux and other platforms
        sed -i "s/\"version\": \"[^\"]*\"/\"version\": \"$next_version\"/" npm/package.json
    fi

    if [[ $? -eq 0 ]]; then
        print_success "Updated npm/package.json to version $next_version"
        # Change is made but not committed - will be committed after cargo-release runs
    else
        print_error "Failed to update npm/package.json version"
        return 1
    fi
}

commit_npm_package_update() {
    local version=$1

    if [[ ! -f "npm/package.json" ]]; then
        print_warning "npm/package.json not found - skipping npm commit"
        return 0
    fi

    # Check if npm/package.json has been modified
    if git diff --quiet npm/package.json; then
        print_info "npm/package.json is already up to date"
        return 0
    fi

    print_info "Committing npm/package.json version update to $version"

    git add npm/package.json
    git commit -m "chore: update npm package to v$version"

    print_success "Committed npm/package.json update (will be pushed with other changes)"
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

check_cargo_auth() {
    if ! command -v cargo >/dev/null 2>&1; then
        print_error 'Cargo is not available'
        return 1
    fi

    local credentials_file="$HOME/.cargo/credentials.toml"
    if [[ ! -f "$credentials_file" || ! -s "$credentials_file" ]]; then
        print_warning 'Cargo credentials not found or empty. Run `cargo login` before releasing.'
        return 1
    fi

    print_success 'Cargo authentication verified'
}

check_npm_auth() {
    if ! command -v npm >/dev/null 2>&1; then
        print_warning 'npm is not available'
        return 1
    fi

    if ! npm whoami >/dev/null 2>&1; then
        print_warning 'Not logged in to npm. Run `npm login` before releasing.'
        return 1
    fi

    print_success 'npm authentication verified'
}

check_github_packages_auth() {
    if ! command -v npm >/dev/null 2>&1; then
        print_warning 'npm is not available'
        return 1
    fi

    if [[ -z "${GITHUB_TOKEN:-}" ]]; then
        print_warning 'GITHUB_TOKEN environment variable not set. Set it before releasing to GitHub Packages.'
        print_info 'Make sure your GitHub token has write:packages, read:packages, and repo scopes.'
        return 1
    fi

    # Test that the token is properly configured in npm
    local token_config
    token_config=$(npm config get //npm.pkg.github.com/:_authToken 2>/dev/null || echo "")
    
    if [[ -z "$token_config" || "$token_config" == "null" ]]; then
        print_warning 'GITHUB_TOKEN may not be properly configured for GitHub Packages. Ensure your .npmrc is set up correctly.'
        print_info 'Make sure your GitHub token has write:packages, read:packages, and repo scopes.'
        return 1
    fi

    print_success 'GitHub Packages authentication verified'
}

ensure_cargo_release() {
    if ! command -v cargo-release >/dev/null 2>&1; then
        print_error 'cargo-release is not installed. Install it with `cargo install cargo-release`.'
        exit 1
    fi
}

get_current_version() {
    grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/'
}

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

    print_info "Triggering docs.rs rebuild for vtcode-core v$version..."
    local core_response
    # Use POST to trigger rebuild; docs.rs doesn't require Authorization header for this endpoint
    core_response=$(curl -X POST "https://docs.rs/crate/vtcode-core/$version/builds" \
        -H "Content-Type: application/json" \
        -w '%{http_code}' \
        --silent --output /dev/null)
    if [[ "$core_response" == '200' || "$core_response" == '202' ]]; then
        print_success "Triggered docs.rs rebuild for vtcode-core v$version (HTTP $core_response)"
    elif [[ "$core_response" == '404' ]]; then
        print_info "vtcode-core v$version not found on docs.rs yet - will be built when available"
    else
        print_warning "Failed to trigger docs.rs rebuild for vtcode-core v$version (HTTP $core_response)"
        print_info "Note: Documentation will be built automatically when the crate is published to crates.io"
    fi

    print_info "Triggering docs.rs rebuild for vtcode v$version..."
    local main_response
    # Use POST to trigger rebuild; docs.rs doesn't require Authorization header for this endpoint
    main_response=$(curl -X POST "https://docs.rs/crate/vtcode/$version/builds" \
        -H "Content-Type: application/json" \
        -w '%{http_code}' \
        --silent --output /dev/null)
    if [[ "$main_response" == '200' || "$main_response" == '202' ]]; then
        print_success "Triggered docs.rs rebuild for vtcode v$version (HTTP $main_response)"
    elif [[ "$main_response" == '404' ]]; then
        print_info "vtcode v$version not found on docs.rs yet - will be built when available"
    else
        print_warning "Failed to trigger docs.rs rebuild for vtcode v$version (HTTP $main_response)"
        print_info "Note: Documentation will be built automatically when the crate is published to crates.io"
    fi
}

publish_to_npm() {
    local version=$1

    print_distribution 'Publishing to npm...'

    local original_dir
    original_dir=$(pwd)

    if [[ ! -d 'npm' ]]; then
        print_warning 'npm directory not found - skipping npm publish'
        return 0
    fi

    cd npm || {
        print_error 'Failed to change to npm directory'
        cd "$original_dir"
        return 1
    }

    if [[ ! -f 'package.json' ]]; then
        print_warning 'package.json not found - skipping npm publish'
        cd "$original_dir"
        return 0
    fi

    if ! npm publish --access public; then
        print_error 'Failed to publish to npm'
        cd "$original_dir"
        return 1
    fi

    cd "$original_dir"
    print_success "Published npm package version $version"
}



publish_to_github_packages() {
    local version=$1

    print_distribution 'Publishing to GitHub Packages...'

    local original_dir
    original_dir=$(pwd)

    if [[ ! -d 'npm' ]]; then
        print_warning 'npm directory not found - skipping GitHub Packages publish'
        return 0
    fi

    cd npm || {
        print_error 'Failed to change to npm directory'
        cd "$original_dir"
        return 1
    }

    if [[ ! -f 'package.json' ]]; then
        print_warning 'package.json not found - skipping GitHub Packages publish'
        cd "$original_dir"
        return 0
    fi

    # Check if GITHUB_TOKEN is set
    if [[ -z "${GITHUB_TOKEN:-}" ]]; then
        print_error 'GITHUB_TOKEN environment variable not set - skipping GitHub Packages publish'
        print_info 'Set GITHUB_TOKEN to publish to GitHub Packages'
        cd "$original_dir"
        return 1
    fi

    # Ensure .npmrc is properly configured for GitHub Packages
    if [[ ! -f '.npmrc' ]]; then
        print_error '.npmrc file not found - skipping GitHub Packages publish'
        print_info 'Create .npmrc file with GitHub Packages configuration'
        cd "$original_dir"
        return 1
    fi

    # Verify .npmrc contains GitHub registry configuration
    if ! grep -q "npm.pkg.github.com" .npmrc; then
        print_error '.npmrc does not contain GitHub Packages registry - skipping GitHub Packages publish'
        print_info 'Ensure .npmrc contains authentication for https://npm.pkg.github.com'
        cd "$original_dir"
        return 1
    fi

    # For GitHub Packages, we need to temporarily modify package.json to have a scoped name
    # Create a backup of the original package.json
    cp package.json package.json.backup

    # Create a temporary package.json with the scoped name for GitHub Packages
    # Using a temporary approach to avoid permanently changing the package name
    if command -v jq >/dev/null 2>&1; then
        # Using jq if available to properly modify JSON
        jq '.name = "@vinhnx/" + .name' package.json.backup > package.json.temp
        mv package.json.temp package.json
    else
        # Fallback using sed if jq is not available
        # This is a simple replacement that assumes the name line format
        sed 's/"name": "\([^"]*\)"/"name": "@vinhnx\/\1"/' package.json.backup > package.json
    fi

    # Use the GitHub registry for this publish
    if ! npm publish --registry=https://npm.pkg.github.com --access=public; then
        # Restore the original package.json before exiting with error
        mv package.json.backup package.json
        print_error 'Failed to publish to GitHub Packages'
        cd "$original_dir"
        return 1
    fi

    # Restore the original package.json after successful publish
    mv package.json.backup package.json

    cd "$original_dir"
    print_success "Published npm package version $version to GitHub Packages"
}

build_and_upload_binaries() {
    local version=$1
    local skip_homebrew_flag=$2

    print_distribution 'Building and distributing binaries...'
    
    # Check if we have the binary build script
    if [[ ! -f 'scripts/build-and-upload-binaries.sh' ]]; then
        print_warning 'Binary build script not found - skipping binary distribution'
        return 0
    fi

    if [[ "$skip_homebrew_flag" == 'true' ]]; then
        if ! ./scripts/build-and-upload-binaries.sh -v "$version" --skip-homebrew; then
            print_warning 'Binary build/distribution failed (Homebrew skipped)'
            return 1
        fi
    else
        if ! ./scripts/build-and-upload-binaries.sh -v "$version"; then
            print_warning 'Binary build/distribution failed'
            return 1
        fi
    fi

    print_success 'Binaries built and distributed successfully'
}

run_release() {
    local release_argument=$1
    local dry_run_flag=$2
    local skip_crates_flag=$3

    # Generate changelog from commits before running the release
    if [[ "$dry_run_flag" != 'true' ]]; then
        local version
        # Extract the version from the release argument or compute it
        if [[ "$release_argument" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
            version="$release_argument"
        else
            # Compute the next version based on current version and increment type
            local current_version
            current_version=$(get_current_version)
            IFS='.' read -ra version_parts <<< "$current_version"
            local major=${version_parts[0]}
            local minor=${version_parts[1]}
            local patch=${version_parts[2]}
            
            case "$release_argument" in
                "major")
                    version="$((major + 1)).0.0"
                    ;;
                "minor")
                    version="${major}.$((minor + 1)).0"
                    ;;
                "patch")
                    version="${major}.${minor}.$((patch + 1))"
                    ;;
                *)
                    # If it's neither a specific version nor an increment type, 
                    # keep the computed version (or use current +1 patch)
                    version="${major}.${minor}.$((patch + 1))"
                    ;;
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
        command+=(--execute)
    fi

    print_info "Running: ${command[*]}"
    "${command[@]}"
}

run_prerelease() {
    local pre_release_suffix=$1
    local dry_run_flag=$2
    local skip_crates_flag=$3

    # Generate changelog from commits before running the pre-release
    if [[ "$dry_run_flag" != 'true' ]]; then
        # For pre-release, we'll use the next patch version with the pre-release suffix
        local current_version
        current_version=$(get_current_version)
        IFS='.' read -ra version_parts <<< "$current_version"
        local major=${version_parts[0]}
        local minor=${version_parts[1]}
        local patch=${version_parts[2]}
        
        # Extract the numeric part of the patch if needed
        patch=$(echo "$patch" | sed 's/[^0-9]*$//')
        
        local version
        if [[ "$pre_release_suffix" == "alpha.0" ]]; then
            # Default to alpha.1
            version="${major}.${minor}.$((patch + 1))-alpha.1"
        else
            version="${major}.${minor}.$((patch + 1))-${pre_release_suffix}"
        fi
        update_changelog_from_commits "$version" "$dry_run_flag"
    fi

    # For pre-release versions, cargo-release has specific commands:
    # - `alpha` creates alpha.1, alpha.2, etc.
    # - `beta` creates beta.1, beta.2, etc.
    # - `rc` creates rc.1, rc.2, etc.
    # - `release` removes pre-release markers
    case "$pre_release_suffix" in
        alpha|beta|rc|release)
            # Use the suffix as a level argument directly
            local command=(cargo release "$pre_release_suffix" --workspace --config release.toml)
            ;;
        alpha.*|beta.*|rc.*)
            # For custom suffixes like alpha.1, beta.2, etc., 
            # we need to use the specific part (alpha, beta, rc) 
            # and let cargo-release increment the number
            local base_suffix
            base_suffix=$(echo "$pre_release_suffix" | cut -d. -f1)
            local command=(cargo release "$base_suffix" --workspace --config release.toml)
            ;;
        *)
            # For completely custom suffixes, default to alpha with metadata
            # NOTE: This might create duplicate format, so warn user
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
        command+=(--execute)
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
    local skip_docs=false
    local skip_homebrew=true
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

            --skip-docs)
                skip_docs=true
                shift
                ;;
            --enable-homebrew)
                skip_homebrew=false
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
        # For prerelease, we handle it differently in the main function directly
    fi

    load_env_file
    check_branch
    check_clean_tree
    ensure_cargo_release
    check_cargo_auth || true
    if [[ "$skip_npm" == 'false' ]]; then
        check_npm_auth || true
    fi
    if [[ "$skip_github_packages" == 'false' ]]; then
        check_github_packages_auth || true
    fi

    local current_version
    current_version=$(get_current_version)
    print_info "Current version: $current_version"

    # Update npm package.json before starting the cargo release process
    if [[ "$skip_npm" == 'false' ]]; then
        update_npm_package_version "$release_argument" "$pre_release" "$pre_release_suffix"
        # Commit the npm package.json version bump immediately to ensure it's included in the release process
        # Get the version that was just set by parsing the updated package.json
        if [[ -f "npm/package.json" ]]; then
            local npm_version
            npm_version=$(grep -o '"version": *"[^"]*"' npm/package.json | sed 's/"version": *"\([^"]*\)"/\1/')
            if [[ -n "$npm_version" ]]; then
                commit_npm_package_update "$npm_version"
            else
                print_warning "Could not determine npm package version"
            fi
        fi
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

    # Explicitly push commits and tags to ensure they are properly synchronized
    print_info "Pushing commits and tags to remote..."
    if [[ "$dry_run" != 'true' ]]; then
        # Push commits to main branch
        git push origin main
        
        # Push tags (cargo-release with push=true should have created the tag, 
        # but we explicitly push to make sure)
        git push --tags origin
        
        print_success "Commits and tags pushed successfully"
    else
        print_info "Dry run - would push commits and tags"
    fi

    if [[ "$skip_crates" == 'false' ]]; then
        print_info 'Waiting for crates.io to propagate...'
        sleep 10
        if [[ "$skip_docs" == 'false' ]]; then
            trigger_docs_rs_rebuild "$released_version" false
        else
            print_info 'Docs.rs rebuild skipped'
        fi
    else
        print_info 'Crates.io publishing skipped'
    fi

    if [[ "$skip_npm" == 'false' ]]; then
        publish_to_npm "$released_version"
    else
        print_info 'npm publishing skipped'
    fi

    print_info 'VSCode extension publishing skipped'

    if [[ "$skip_github_packages" == 'false' ]]; then
        publish_to_github_packages "$released_version"
    else
        print_info 'GitHub Packages publishing skipped'
    fi

    build_and_upload_binaries "$released_version" "$skip_homebrew"

    print_success 'Release process finished'
    print_info "GitHub Release should now contain changelog notes generated by cargo-release"
    print_info "All commits, tags, and releases have been pushed to the remote repository"
}

main "$@"