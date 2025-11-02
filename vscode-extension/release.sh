#!/bin/bash
set -e

# VTCode VSCode Extension Release Script
# This script automates the release process for the VSCode extension
# Usage: ./release.sh [patch|minor|major]

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Helper functions
print_step() {
	echo -e "${BLUE}==>${NC} ${1}"
}

print_success() {
	echo -e "${GREEN}✓${NC} ${1}"
}

print_error() {
	echo -e "${RED}✗${NC} ${1}"
}

print_warning() {
	echo -e "${YELLOW}⚠${NC} ${1}"
}

# Check if required tools are installed
check_dependencies() {
	print_step "Checking dependencies..."

	local missing_deps=()

	if ! command -v node &> /dev/null; then
		missing_deps+=("node")
	fi

	if ! command -v npm &> /dev/null; then
		missing_deps+=("npm")
	fi

	if ! command -v git &> /dev/null; then
		missing_deps+=("git")
	fi

	if ! command -v jq &> /dev/null; then
		missing_deps+=("jq")
	fi

	if ! npm list -g @vscode/vsce &> /dev/null; then
		print_warning "vsce not installed globally, installing..."
		npm install -g @vscode/vsce
	fi

	if ! npm list -g ovsx &> /dev/null; then
		print_warning "ovsx not installed globally, installing..."
		npm install -g ovsx
	fi

	if [ ${#missing_deps[@]} -ne 0 ]; then
		print_error "Missing dependencies: ${missing_deps[*]}"
		exit 1
	fi

	print_success "All dependencies are installed"
}

# Get current version from package.json
get_current_version() {
	jq -r '.version' package.json
}

# Bump version in package.json
bump_version() {
	local bump_type=$1
	local current_version=$(get_current_version)

	print_step "Current version: $current_version"

	# Parse version
	IFS='.' read -r -a version_parts <<< "$current_version"
	local major="${version_parts[0]}"
	local minor="${version_parts[1]}"
	local patch="${version_parts[2]}"

	# Bump based on type
	case $bump_type in
		major)
			major=$((major + 1))
			minor=0
			patch=0
			;;
		minor)
			minor=$((minor + 1))
			patch=0
			;;
		patch)
			patch=$((patch + 1))
			;;
		*)
			print_error "Invalid bump type: $bump_type (use: patch, minor, or major)"
			exit 1
			;;
	esac

	local new_version="${major}.${minor}.${patch}"

	# Update package.json
	jq ".version = \"$new_version\"" package.json > package.json.tmp
	mv package.json.tmp package.json

	print_success "Version bumped to: $new_version"
	echo "$new_version"
}

# Update CHANGELOG.md
update_changelog() {
	local version=$1
	local date=$(date +%Y-%m-%d)

	print_step "Updating CHANGELOG.md..."

	# Replace [Unreleased] with version and date
	if [[ "$OSTYPE" == "darwin"* ]]; then
		# macOS
		sed -i '' "s/## \[Unreleased\]/## [Unreleased]\n\n## [$version] - $date/" CHANGELOG.md
	else
		# Linux
		sed -i "s/## \[Unreleased\]/## [Unreleased]\n\n## [$version] - $date/" CHANGELOG.md
	fi

	print_success "CHANGELOG.md updated"
}

# Build the extension
build_extension() {
	print_step "Building extension..."

	npm run bundle

	print_success "Extension built successfully"
}

# Package the extension
package_extension() {
	local version=$1

	print_step "Packaging extension..."

	npm run package

	if [ -f "vtcode-companion-${version}.vsix" ]; then
		print_success "Extension packaged: vtcode-companion-${version}.vsix"
	else
		print_error "Failed to create package"
		exit 1
	fi
}

# Commit changes
commit_changes() {
	local version=$1

	print_step "Committing changes..."

	git add package.json CHANGELOG.md
	git commit -m "chore: release vscode extension v${version}"

	print_success "Changes committed"
}

# Create and push git tag
create_git_tag() {
	local version=$1
	local tag_name="vscode-v${version}"

	print_step "Creating git tag: $tag_name"

	# Check if tag already exists locally
	if git tag -l "$tag_name" | grep -q "$tag_name"; then
		print_warning "Tag $tag_name already exists locally, deleting..."
		git tag -d "$tag_name"
	fi

	# Create annotated tag
	git tag -a "$tag_name" -m "VSCode Extension Release v${version}"

	print_success "Tag created: $tag_name"
}

# Push to GitHub
push_to_github() {
	local version=$1
	local tag_name="vscode-v${version}"
	local current_branch=$(git rev-parse --abbrev-ref HEAD)

	print_step "Pushing to GitHub..."

	# Push commits
	git push origin "$current_branch"

	# Push tag
	git push origin "$tag_name"

	print_success "Pushed to GitHub"
	print_warning "Create a GitHub release at: https://github.com/vinhnx/vtcode/releases/new?tag=$tag_name"
}

# Publish to VSCode Marketplace
publish_vscode_marketplace() {
	local version=$1

	print_step "Publishing to VSCode Marketplace..."

	vsce publish

	print_success "Published to VSCode Marketplace"
	echo "         URL: https://marketplace.visualstudio.com/items?itemName=nguyenxuanvinh.vtcode-companion"
}

# Publish to Open VSX Registry
publish_open_vsx() {
	local version=$1
	local vsix_file="vtcode-companion-${version}.vsix"

	print_step "Publishing to Open VSX Registry..."

	if [ ! -f "$vsix_file" ]; then
		print_error "VSIX file not found: $vsix_file"
		exit 1
	fi

	ovsx publish "$vsix_file"

	print_success "Published to Open VSX Registry"
	echo "         URL: https://open-vsx.org/extension/nguyenxuanvinh/vtcode-companion"
}

# Cleanup old VSIX files
cleanup_old_vsix() {
	local current_version=$1

	print_step "Cleaning up old VSIX files..."

	# Keep only the current version
	find . -maxdepth 1 -name "vtcode-companion-*.vsix" ! -name "vtcode-companion-${current_version}.vsix" -delete

	print_success "Cleanup completed"
}

# Main release flow
main() {
	echo -e "${BLUE}╔════════════════════════════════════════════════════════╗${NC}"
	echo -e "${BLUE}║${NC}  ${GREEN}VTCode VSCode Extension Release Script${NC}          ${BLUE}║${NC}"
	echo -e "${BLUE}╚════════════════════════════════════════════════════════╝${NC}"
	echo

	# Parse arguments
	local bump_type=${1:-patch}

	if [[ ! "$bump_type" =~ ^(patch|minor|major)$ ]]; then
		print_error "Usage: $0 [patch|minor|major]"
		exit 1
	fi

	# Check dependencies
	check_dependencies

	# Check for uncommitted changes
	if [[ -n $(git status -s) ]]; then
		print_warning "You have uncommitted changes. Continue? (y/n)"
		read -r response
		if [[ ! "$response" =~ ^[Yy]$ ]]; then
			print_error "Release aborted"
			exit 1
		fi
	fi

	# Bump version
	local new_version=$(bump_version "$bump_type")

	# Update changelog
	update_changelog "$new_version"

	# Build extension
	build_extension

	# Package extension
	package_extension "$new_version"

	# Commit changes
	commit_changes "$new_version"

	# Create git tag
	create_git_tag "$new_version"

	# Push to GitHub
	echo
	print_warning "Push to GitHub? (y/n)"
	read -r response
	if [[ "$response" =~ ^[Yy]$ ]]; then
		push_to_github "$new_version"
	else
		print_warning "Skipping GitHub push (remember to push manually later)"
	fi

	# Publish to VSCode Marketplace
	echo
	print_warning "Publish to VSCode Marketplace? (y/n)"
	read -r response
	if [[ "$response" =~ ^[Yy]$ ]]; then
		publish_vscode_marketplace "$new_version"
	else
		print_warning "Skipping VSCode Marketplace publish"
	fi

	# Publish to Open VSX
	echo
	print_warning "Publish to Open VSX Registry? (y/n)"
	read -r response
	if [[ "$response" =~ ^[Yy]$ ]]; then
		publish_open_vsx "$new_version"
	else
		print_warning "Skipping Open VSX Registry publish"
	fi

	# Cleanup
	cleanup_old_vsix "$new_version"

	# Summary
	echo
	echo -e "${BLUE}╔════════════════════════════════════════════════════════╗${NC}"
	echo -e "${BLUE}║${NC}  ${GREEN}Release Complete! 🎉${NC}                              ${BLUE}║${NC}"
	echo -e "${BLUE}╚════════════════════════════════════════════════════════╝${NC}"
	echo
	print_success "Version: v${new_version}"
	print_success "Git tag: vscode-v${new_version}"
	print_success "Package: vtcode-companion-${new_version}.vsix"
	echo
	echo -e "${YELLOW}Next steps:${NC}"
	echo "  1. Create GitHub release: https://github.com/vinhnx/vtcode/releases/new?tag=vscode-v${new_version}"
	echo "  2. Attach vtcode-companion-${new_version}.vsix to the release"
	echo "  3. Test installation: code --install-extension vtcode-companion-${new_version}.vsix"
	echo
}

# Run main function
main "$@"
