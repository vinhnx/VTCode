#!/bin/bash

# VTCode Release Script
# Fixed sequence: commit first, then publish, rollback on failure

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
NC='\033[0m' # No Color

# Function to print colored output
print_info() {
    echo -e "${BLUE}INFO: $1${NC}"
}

print_success() {
    echo -e "${GREEN}SUCCESS: $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}WARNING: $1${NC}"
}

print_error() {
    echo -e "${RED}ERROR: $1${NC}"
}

print_distribution() {
    echo -e "${PURPLE}DISTRIBUTION: $1${NC}"
}

# Function to check if we're on main branch
check_branch() {
    local current_branch=$(git branch --show-current)
    if [ "$current_branch" != "main" ]; then
        print_error "You must be on the main branch to create a release"
        print_info "Current branch: $current_branch"
        print_info "Please switch to main branch: git checkout main"
        exit 1
    fi
}

# Function to check if working tree is clean
check_clean_tree() {
    if [ -n "$(git status --porcelain)" ]; then
        print_error "Working tree is not clean. Please commit or stash your changes."
        git status --short
        exit 1
    fi
}

# Function to check Cargo authentication
check_cargo_auth() {
    if ! command -v cargo &> /dev/null; then
        print_error "Cargo is not available"
        return 1
    fi

    # Check if credentials file exists
    local credentials_file="$HOME/.cargo/credentials.toml"
    if [[ ! -f "$credentials_file" ]]; then
        print_warning "Not logged in to crates.io"
        print_info "Run: cargo login"
        print_info "Get your API token from: https://crates.io/me"
        return 1
    fi

    # Check if credentials file has content (not empty)
    if [[ ! -s "$credentials_file" ]]; then
        print_warning "Cargo credentials file is empty"
        print_info "Run: cargo login"
        print_info "Get your API token from: https://crates.io/me"
        return 1
    fi

    print_success "Cargo authentication verified"
}

# Function to check npm authentication
check_npm_auth() {
    if ! command -v npm &> /dev/null; then
        print_warning "npm is not available"
        return 1
    fi

    # Check if user is logged in to npm
    if ! npm whoami &> /dev/null; then
        print_warning "Not logged in to npm"
        print_info "Run: npm login"
        return 1
    fi

    print_success "npm authentication verified"
}

# Function to trigger docs.rs rebuild
trigger_docs_rs_rebuild() {
    local version=$1
    local dry_run=$2

    print_distribution "Triggering docs.rs rebuild for version $version..."

    if [[ "$dry_run" == "true" ]]; then
        print_info "Dry run - would trigger docs.rs rebuild for vtcode and vtcode-core v$version"
        return 0
    fi

    # Check if CRATES_IO_TOKEN is available
    if [[ -z "$CRATES_IO_TOKEN" ]]; then
        print_warning "CRATES_IO_TOKEN not set - skipping docs.rs rebuild trigger"
        print_info "Note: docs.rs rebuild is usually automatic after crates.io publishing"
        return 0
    fi

    # Trigger docs.rs rebuild for vtcode-core
    print_info "Triggering docs.rs rebuild for vtcode-core v$version..."
    local core_response=$(curl -X POST "https://docs.rs/crate/vtcode-core/$version/builds" \
             -H "Authorization: Bearer $CRATES_IO_TOKEN" \
             -H "Content-Type: application/json" \
             -w "%{http_code}" \
             --silent --output /dev/null)
    if [[ "$core_response" == "200" || "$core_response" == "202" ]]; then
        print_success "Triggered docs.rs rebuild for vtcode-core v$version (HTTP $core_response)"
    else
        print_warning "Failed to trigger docs.rs rebuild for vtcode-core v$version (HTTP $core_response)"
        print_info "This may be normal - docs.rs usually rebuilds automatically after publishing"
    fi

    # Trigger docs.rs rebuild for vtcode
    print_info "Triggering docs.rs rebuild for vtcode v$version..."
    local main_response=$(curl -X POST "https://docs.rs/crate/vtcode/$version/builds" \
             -H "Authorization: Bearer $CRATES_IO_TOKEN" \
             -H "Content-Type: application/json" \
             -w "%{http_code}" \
             --silent --output /dev/null)
    if [[ "$main_response" == "200" || "$main_response" == "202" ]]; then
        print_success "Triggered docs.rs rebuild for vtcode v$version (HTTP $main_response)"
    else
        print_warning "Failed to trigger docs.rs rebuild for vtcode v$version (HTTP $main_response)"
        print_info "This may be normal - docs.rs usually rebuilds automatically after publishing"
    fi

    print_info "Note: docs.rs rebuild is usually automatic after crates.io publishing"
    print_info "Check https://docs.rs/vtcode/$version and https://docs.rs/vtcode-core/$version for status"
}

# Function to get current version from Cargo.toml
get_current_version() {
    grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/'
}

# Function to get current version from vtcode-core/Cargo.toml
get_core_version() {
    grep '^version = ' vtcode-core/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/'
}

# Function to update version in Cargo.toml files
update_version() {
    local new_version=$1

    # Update main Cargo.toml package version only
    local line_num_main=$(grep -n "^version = " Cargo.toml | head -1 | cut -d: -f1)
    if [ -n "$line_num_main" ]; then
        sed -i.bak "${line_num_main}s/version = \".*\"/version = \"$new_version\"/" Cargo.toml
        rm Cargo.toml.bak
    fi

    # Update vtcode-core dependency version requirement in main Cargo.toml
    local line_num_dep=$(grep -n "vtcode-core = " Cargo.toml | head -1 | cut -d: -f1)
    if [ -n "$line_num_dep" ]; then
        sed -i.bak "${line_num_dep}s/version = \".*\"/version = \"$new_version\"/" Cargo.toml
        rm Cargo.toml.bak
    fi

    # Update vtcode-core Cargo.toml package version only
    local line_num_core=$(grep -n "^version = " vtcode-core/Cargo.toml | head -1 | cut -d: -f1)
    if [ -n "$line_num_core" ]; then
        sed -i.bak "${line_num_core}s/version = \".*\"/version = \"$new_version\"/" vtcode-core/Cargo.toml
        rm vtcode-core/Cargo.toml.bak
    fi

    print_success "Updated version to $new_version in all package files"
}

# Function to update vtcode-core version only
update_core_version() {
    local new_version=$1

    # Update only the package version line in vtcode-core/Cargo.toml
    # Find the line number of the package version and update only that line
    local line_num=$(grep -n "^version = " vtcode-core/Cargo.toml | head -1 | cut -d: -f1)
    if [ -n "$line_num" ]; then
        sed -i.bak "${line_num}s/version = \".*\"/version = \"$new_version\"/" vtcode-core/Cargo.toml
        rm vtcode-core/Cargo.toml.bak
        print_success "Updated vtcode-core version to $new_version"
    else
        print_error "Could not find version line in vtcode-core/Cargo.toml"
        return 1
    fi
}

# Function to validate package metadata
validate_metadata() {
    print_info "Validating package metadata..."

    # Check main Cargo.toml
    if ! grep -q '^description = ' Cargo.toml; then
        print_error "Missing description in Cargo.toml"
        return 1
    fi

    if ! grep -q '^license = ' Cargo.toml; then
        print_error "Missing license in Cargo.toml"
        return 1
    fi

    if ! grep -q '^repository = ' Cargo.toml; then
        print_error "Missing repository in Cargo.toml"
        return 1
    fi

    # Check vtcode-core Cargo.toml
    if ! grep -q '^description = ' vtcode-core/Cargo.toml; then
        print_error "Missing description in vtcode-core/Cargo.toml"
        return 1
    fi

    print_success "Package metadata validation passed"
}

# Function to publish to crates.io
publish_to_crates() {
    local dry_run=$1

    print_distribution "Publishing to crates.io..."

    if [[ "$dry_run" == "true" ]]; then
        print_info "Dry run - checking crates.io publishing"
        if ! cargo publish --dry-run; then
            print_error "Dry run failed for main crate"
            return 1
        fi

        if ! cargo publish --dry-run --manifest-path vtcode-core/Cargo.toml; then
            print_error "Dry run failed for vtcode-core"
            return 1
        fi

        print_success "Crates.io dry run successful"
        return 0
    fi

    # Publish vtcode-core first
    print_info "Publishing vtcode-core to crates.io..."
    if ! cargo publish --manifest-path vtcode-core/Cargo.toml; then
        print_error "Failed to publish vtcode-core"
        return 1
    fi

    print_success "Published vtcode-core to crates.io"

    # Wait for vtcode-core to be available
    print_info "Waiting for vtcode-core to be available on crates.io..."
    sleep 30

    # Publish main crate
    print_info "Publishing vtcode to crates.io..."
    if ! cargo publish; then
        print_error "Failed to publish vtcode"
        return 1
    fi

    print_success "Published vtcode to crates.io"
}

# Function to publish to npm
publish_to_npm() {
    local dry_run=$1

    print_distribution "Publishing to npm..."

    # Change to npm directory
    local original_dir=$(pwd)
    cd npm || {
        print_error "Failed to change to npm directory"
        return 1
    }

    if [[ "$dry_run" == "true" ]]; then
        print_info "Dry run - checking npm publishing"
        # Check if package.json exists in the current directory
        if [[ ! -f "package.json" ]]; then
            print_error "package.json not found in npm directory"
            cd "$original_dir"
            return 1
        fi

        # Validate package.json
        if ! npm pack --dry-run --silent --workspace=false &>/dev/null; then
            print_error "npm package validation failed"
            cd "$original_dir"
            return 1
        fi

        print_success "npm dry run successful"
        cd "$original_dir"
        return 0
    fi

    # Publish to npm from the npm directory
    print_info "Publishing to npm..."
    if ! npm publish --access public; then
        print_error "Failed to publish to npm"
        cd "$original_dir"
        return 1
    fi

    print_success "Published to npm"
    cd "$original_dir"
}

# Function to update Homebrew formula
update_homebrew_formula() {
    local version=$1
    local dry_run=$2

    if [[ ! -f "homebrew/vtcode.rb" ]]; then
        print_warning "Homebrew formula not found - skipping Homebrew update"
        return 0
    fi

    print_distribution "Updating Homebrew formula..."

    # Use the automated update script if available
    if [[ -f "scripts/update-homebrew-formula.sh" ]]; then
        if [[ "$dry_run" == "true" ]]; then
            print_info "Dry run - would update Homebrew formula to version $version"
            return 0
        fi
        
        print_info "Using automated Homebrew formula update script..."
        if ./scripts/update-homebrew-formula.sh "$version" "vinhnx/homebrew-tap"; then
            print_success "Homebrew formula updated automatically"
            return 0
        else
            print_warning "Automated update failed, falling back to manual instructions"
        fi
    fi

    # Fallback to manual instructions
    print_info "Homebrew formula needs manual update:"
    print_info "1. Update version in homebrew/vtcode.rb to $version"
    print_info "2. Update SHA256 hashes for new binaries"
    print_info "3. Commit and push changes to homebrew tap repository"
    print_info "4. Users can then run: brew install vinhnx/tap/vtcode"
}

# Function to create git tag
create_tag() {
    local version=$1
    local tag="v$version"

    if git tag -l | grep -q "^$tag$"; then
        print_error "Tag $tag already exists"
        exit 1
    fi

    git tag -a "$tag" -m "Release $tag"
    print_success "Created tag $tag"
}

# Function to build and upload binaries
build_and_upload_binaries() {
    local version=$1
    
    print_distribution "Building and uploading binaries..."
    
    # Call our new binary build and upload script
    if [[ -f "scripts/build-and-upload-binaries.sh" ]]; then
        if ! ./scripts/build-and-upload-binaries.sh -v "$version"; then
            print_warning "Binary build/upload failed, but continuing with release"
            print_info "You can manually run: ./scripts/build-and-upload-binaries.sh -v $version"
        else
            print_success "Binaries built and uploaded successfully"
        fi
    else
        print_warning "Binary build script not found - skipping binary build/upload"
        print_info "To build and upload binaries manually, run:"
        print_info "  ./scripts/build-and-upload-binaries.sh -v $version"
    fi
}

# Function to push tag to GitHub
push_tag() {
    local version=$1
    local tag="v$version"

    git push origin "$tag"
    print_success "Pushed tag $tag to GitHub"
}

# Function to push commit to GitHub
push_commit() {
    git push origin main
    print_success "Pushed commit to GitHub"
}

# Function to show usage
show_usage() {
    cat << EOF
VTCode Release Script with Fixed Sequence

USAGE:
    $0 [OPTIONS] [LEVEL|VERSION]

ARGUMENTS:
    LEVEL|VERSION    Either bump by LEVEL or set the VERSION for all packages
                     Levels: major, minor, patch
                     Version: e.g., 1.0.0, 1.2.3

OPTIONS:
    -h, --help          Show this help message
    -p, --patch         Create a patch release (increment patch version)
    -m, --minor         Create a minor release (increment minor version)
    -M, --major         Create a major release (increment major version)
    --dry-run           Show what would be done without making changes
    --skip-crates       Skip publishing to crates.io
    --skip-npm          Skip publishing to npm
    --enable-homebrew   Enable Homebrew formula update (currently disabled by default)

EXAMPLES:
    $0 1.0.0                           # Release specific version
    $0 --patch                         # Create patch release
    $0 --minor --enable-homebrew       # Create minor release with Homebrew
    $0 --patch --dry-run               # Show what patch release would do
    $0 --patch --skip-npm              # Create patch release without npm publishing

DISTRIBUTION CHANNELS:
    - crates.io: Rust package registry
    - npm: Node.js package registry
    - docs.rs: Automatic API documentation
    - Homebrew: macOS package manager (disabled by default, use --enable-homebrew)
    - GitHub Releases: Pre-built binaries

SETUP REQUIREMENTS:
    1. Cargo: Run 'cargo login' with your crates.io API token
    2. npm: Run 'npm login' with your npm account
    3. Git: Ensure you're on main branch with clean working tree
    4. Homebrew: Set up tap repository for macOS distribution (optional, use --enable-homebrew)

EOF
}

# Function to increment version
increment_version() {
    local current_version=$1
    local increment_type=$2

    # Split version into parts
    IFS='.' read -ra VERSION_PARTS <<< "$current_version"

    case $increment_type in
        patch)
            VERSION_PARTS[2]=$((VERSION_PARTS[2] + 1))
            ;;
        minor)
            VERSION_PARTS[1]=$((VERSION_PARTS[1] + 1))
            VERSION_PARTS[2]=0
            ;;
        major)
            VERSION_PARTS[0]=$((VERSION_PARTS[0] + 1))
            VERSION_PARTS[1]=0
            VERSION_PARTS[2]=0
            ;;
        *)
            print_error "Invalid increment type: $increment_type"
            exit 1
            ;;
    esac

    echo "${VERSION_PARTS[0]}.${VERSION_PARTS[1]}.${VERSION_PARTS[2]}"
}

# Main function
main() {
    local version=""
    local increment_type=""
    local dry_run=false
    local skip_crates=false
    local skip_npm=false  # Add npm skip flag
    local skip_homebrew=true  # Skip Homebrew by default for now

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            -h|--help)
                show_usage
                exit 0
                ;;
            -p|--patch)
                increment_type="patch"
                shift
                ;;
            -m|--minor)
                increment_type="minor"
                shift
                ;;
            -M|--major)
                increment_type="major"
                shift
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
                # Check if this looks like a version increment type
                if [[ "$1" == "patch" || "$1" == "minor" || "$1" == "major" ]]; then
                    increment_type="$1"
                elif [ -n "$version" ]; then
                    print_error "Multiple versions specified"
                    exit 1
                else
                    version=$1
                fi
                shift
                ;;
        esac
    done

    # Validate arguments
    if [ -n "$increment_type" ] && [ -n "$version" ]; then
        print_error "Cannot specify both increment type and version"
        exit 1
    fi

    if [ -z "$increment_type" ] && [ -z "$version" ]; then
        print_error "Must specify either version or increment type"
        show_usage
        exit 1
    fi

    # Get current versions
    local current_version=$(get_current_version)
    print_info "Current version: $current_version"
    local current_core_version=$(get_core_version)
    print_info "Current vtcode-core version: $current_core_version"

    # Determine new version
    if [ -n "$increment_type" ]; then
        version=$(increment_version "$current_version" "$increment_type")
        print_info "New version will be: $version"
    else
        print_info "Releasing version: $version"
    fi

    # Pre-flight checks
    print_info "Running pre-flight checks..."
    check_branch
    check_clean_tree
    validate_metadata

    # Check authentication for enabled providers
    if [[ "$skip_crates" != "true" ]]; then
        check_cargo_auth
    fi

    if [[ "$skip_npm" != "true" ]]; then
        check_npm_auth
    fi

    if [[ "$skip_homebrew" != "true" ]]; then
        check_homebrew_setup
    fi

    if [ "$dry_run" = true ]; then
            print_warning "DRY RUN - No changes will be made"
            echo
            echo "Would perform the following actions:"
            echo "1. Update version to $version in all package files"
            echo "2. Commit version changes to git"
            echo "3. Push commit to GitHub"
            if [[ "$skip_crates" != "true" ]]; then
                echo "4. Publish to crates.io"
                echo "5. Trigger docs.rs rebuild"
            fi
            if [[ "$skip_npm" != "true" ]]; then
                echo "6. Publish to npm"
            fi
            if [[ "$skip_homebrew" != "true" ]]; then
                echo "7. Update Homebrew formula"
            fi
            echo "8. Create and push git tag v$version"
            echo "9. Build and upload binaries for macOS"
            echo "10. GitHub Actions will create release with binaries"
            exit 0
        fi

    # Handle core version update
    local core_version=""
    if [ "$dry_run" = true ]; then
        # In dry-run mode, use the same version as main package
        core_version="$version"
        print_info "vtcode-core will be bumped to $core_version (dry-run)"
    else
        # Interactive mode - prompt for core version with default
        echo
        read -p "Enter new vtcode-core version (default: $version, leave blank to skip): " core_version_input
        local normalized_core_input
        normalized_core_input=$(printf '%s' "$core_version_input" | tr '[:upper:]' '[:lower:]')
        if [ -z "$core_version_input" ]; then
            print_warning "Skipping vtcode-core version bump"
            core_version=""
        elif [ "$normalized_core_input" = "default" ] || [ "$normalized_core_input" = "d" ] || [ "$normalized_core_input" = "y" ] || [ "$normalized_core_input" = "yes" ]; then
            core_version="$version"
            print_info "vtcode-core will be bumped to $core_version (using default)"
        elif [[ "$core_version_input" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.-]+)?(\+[a-zA-Z0-9.-]+)?$ ]]; then
            core_version="$core_version_input"
            print_info "vtcode-core will be bumped to $core_version"
        else
            print_error "Invalid version format. Please use semantic versioning (e.g., 1.2.3, 1.2.3-alpha.1) or 'default'/'y'/'yes' to use $version"
            print_info "Skipping vtcode-core version bump due to invalid input"
            core_version=""
        fi
    fi

    # Confirm release
    echo
    print_warning "This will create a release for version $version"
    echo "Distribution channels:"
    if [[ "$skip_crates" != "true" ]]; then echo "  - crates.io"; fi
    if [[ "$skip_npm" != "true" ]]; then echo "  - npm"; fi
    if [[ "$skip_homebrew" != "true" ]]; then echo "  - Homebrew"; fi
    echo "  - GitHub Releases (binaries)"
    echo
    read -p "Are you sure you want to continue? (y/N): " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        print_info "Release cancelled"
        exit 0
    fi

    # Perform release steps in FIXED SEQUENCE
    print_info "Starting release process..."

    # 1. Update version in all package files
    update_version "$version"
    local files_to_commit="Cargo.toml vtcode-core/Cargo.toml"

    if [ -n "$core_version" ]; then
        update_core_version "$core_version"
        # vtcode-core/Cargo.toml is already in files_to_commit
    fi

    # 2. Commit version changes FIRST (before publishing)
    print_info "Committing version changes..."
    git add $files_to_commit
    if ! git commit -m "chore: bump version to $version"; then
        print_error "Failed to commit version changes"
        exit 1
    fi
    print_success "Committed version bump"

    # 3. Push commit to GitHub
    if ! push_commit; then
        print_error "Failed to push commit - you may need to push manually"
        print_info "Run: git push origin main"
        exit 1
    fi

    # 4. Publish to different providers (AFTER commit/push)
    if [[ "$skip_crates" != "true" ]]; then
        if ! publish_to_crates false; then
            print_error "Failed to publish to crates.io"
            print_warning "Repository is in a consistent state (changes committed and pushed)"
            print_info "You can retry publishing later or investigate the issue"
            exit 1
        fi
        # Trigger docs.rs rebuild after successful crates.io publishing
        print_info "Waiting a moment for crates.io to propagate..."
        sleep 10
        trigger_docs_rs_rebuild "$version" false
    fi

    if [[ "$skip_npm" != "true" ]]; then
        if ! publish_to_npm false; then
            print_error "Failed to publish to npm"
            print_warning "Repository is in a consistent state (changes committed and pushed)"
            print_info "You can retry publishing later or investigate the issue"
            exit 1
        fi
    fi

    if [[ "$skip_homebrew" != "true" ]]; then
        update_homebrew_formula "$version" false
    fi

    # 5. Create and push tag (AFTER successful publishing)
    create_tag "$version"
    if ! push_tag "$version"; then
        print_error "Failed to push tag - release may be incomplete"
        print_info "You may need to push tag manually: git push origin v$version"
        exit 1
    fi

    # 6. Build and upload binaries
    build_and_upload_binaries "$version"

    print_success "Release $version created successfully!"
    print_info "Distribution Summary:"
    if [[ "$skip_crates" != "true" ]]; then
        print_info "  - Published to crates.io: https://crates.io/crates/vtcode"
        print_info "  - docs.rs updated: https://docs.rs/vtcode/$version"
        print_info "  - docs.rs core updated: https://docs.rs/vtcode-core/$version"
        print_info "  - Note: docs.rs may take 10-30 minutes to show updated documentation"
    fi
    if [[ "$skip_npm" != "true" ]]; then
        print_info "  - Published to npm: https://www.npmjs.com/package/vtcode-ai"
    fi
    if [[ "$skip_homebrew" != "true" ]]; then
        print_info "  - Homebrew formula updated (manual step required)"
    fi
    print_info "  - GitHub Release: https://github.com/vinhnx/vtcode/releases/tag/v$version"
    print_info "  - Binaries built and uploaded for macOS (x86_64 and aarch64)"
    print_info "  - Check https://github.com/vinhnx/vtcode/actions for CI status"
}

# Run main function
main "$@"
