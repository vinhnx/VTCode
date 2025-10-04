#!/usr/bin/env bash

# VTCode Release Script powered by cargo-release

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
  --skip-crates       Skip publishing crates to crates.io (pass --skip-publish)
  --skip-npm          Skip npm publish step
  --skip-docs         Skip docs.rs rebuild trigger
  --enable-homebrew   Build and upload Homebrew binaries after release
  -h, --help          Show this help message
USAGE
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
    core_response=$(curl -X POST "https://docs.rs/crate/vtcode-core/$version/builds" \
        -H "Authorization: Bearer $CRATES_IO_TOKEN" \
        -H "Content-Type: application/json" \
        -w '%{http_code}' \
        --silent --output /dev/null)
    if [[ "$core_response" == '200' || "$core_response" == '202' ]]; then
        print_success "Triggered docs.rs rebuild for vtcode-core v$version (HTTP $core_response)"
    else
        print_warning "Failed to trigger docs.rs rebuild for vtcode-core v$version (HTTP $core_response)"
    fi

    print_info "Triggering docs.rs rebuild for vtcode v$version..."
    local main_response
    main_response=$(curl -X POST "https://docs.rs/crate/vtcode/$version/builds" \
        -H "Authorization: Bearer $CRATES_IO_TOKEN" \
        -H "Content-Type: application/json" \
        -w '%{http_code}' \
        --silent --output /dev/null)
    if [[ "$main_response" == '200' || "$main_response" == '202' ]]; then
        print_success "Triggered docs.rs rebuild for vtcode v$version (HTTP $main_response)"
    else
        print_warning "Failed to trigger docs.rs rebuild for vtcode v$version (HTTP $main_response)"
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

build_and_upload_binaries() {
    local version=$1
    local skip_homebrew_flag=$2

    print_distribution 'Building and uploading binaries...'
    if [[ ! -f 'scripts/build-and-upload-binaries.sh' ]]; then
        print_warning 'Binary build script not found - skipping binary build/upload'
        return 0
    fi

    if [[ "$skip_homebrew_flag" == 'true' ]]; then
        if ! ./scripts/build-and-upload-binaries.sh -v "$version" --skip-homebrew; then
            print_warning 'Binary build/upload failed (skip-homebrew)'
            return 1
        fi
    else
        if ! ./scripts/build-and-upload-binaries.sh -v "$version"; then
            print_warning 'Binary build/upload failed'
            return 1
        fi
    fi

    print_success 'Binaries built and uploaded successfully'
}

run_release() {
    local release_argument=$1
    local dry_run_flag=$2
    local skip_crates_flag=$3

    local command=(cargo release "$release_argument" --workspace --config release.toml)

    if [[ "$skip_crates_flag" == 'true' ]]; then
        command+=(--skip-publish)
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

    # Check if the pre_release_suffix is one of the recognized types
    case "$pre_release_suffix" in
        alpha|beta|rc|release)
            # Use the suffix as a level argument
            local command=(cargo release "$pre_release_suffix" --workspace --config release.toml)
            ;;
        *)
            # For custom alpha/beta/rc suffixes like alpha.0, beta.1, etc.
            # Use the -m option to append metadata
            local command=(cargo release alpha --workspace --config release.toml -m "$pre_release_suffix")
            ;;
    esac

    if [[ "$skip_crates_flag" == 'true' ]]; then
        command+=(--skip-publish)
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

    build_and_upload_binaries "$released_version" "$skip_homebrew"

    print_success 'Release process finished'
    print_info "GitHub Release should now contain changelog notes generated by cargo-release"
}

main "$@"
