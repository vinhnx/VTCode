#!/usr/bin/env bash

# VT Code Release Script
#
# This script handles local releases for VT Code:
# 1. Builds binaries locally (Sanity Check)
# 2. Runs cargo-release to version, tag, and push
# 3. Hands off crates.io publishing to the staged release script
# 4. Uploads pre-built binaries to GitHub Releases
# 5. Updates and publishes Homebrew formula
#
# Usage: ./scripts/release.sh [version|level] [options]

set -euo pipefail

# Source common utilities
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "$SCRIPT_DIR/common.sh"
# Legacy updater compatibility bridge helpers (.tar.gz.compat raw exec assets).
# shellcheck disable=SC1091
source "$SCRIPT_DIR/release-assets.sh"
# Shared macOS Developer ID signing and notarization helpers.
# shellcheck disable=SC1091
source "$SCRIPT_DIR/macos-release-signing.sh"
# Changelog formatting and shared commit classification.
# shellcheck disable=SC1091
source "$SCRIPT_DIR/release-changelog.sh"

# Temporary file to store release notes
RELEASE_NOTES_FILE=$(mktemp)
trap 'rm -f "$RELEASE_NOTES_FILE"' EXIT

print_distribution() {
	printf '%b\n' "${PURPLE}DISTRIBUTION:${NC} $1"
}

package_release_archive() {
	local target=$1
	local binary_name=$2
	local archive_path=$3
	local release_dir="target/$target/release"
	if [[ "$target" == *-apple-darwin ]]; then
		sign_and_notarize_macos_binary "$release_dir/$binary_name"
	fi

	# Stage the binary plus the shared man page set into a temp dir so the
	# archive layout matches what install.sh expects (vtcode at root,
	# man/man1/*.1 alongside).
	local stage_dir
	stage_dir=$(mktemp -d)
	cp "$release_dir/$binary_name" "$stage_dir/"
	if [[ -d "$binaries_dir/man" ]]; then
		cp -R "$binaries_dir/man" "$stage_dir/"
	fi
	tar -C "$stage_dir" -czf "$archive_path" .
	rm -rf "$stage_dir"
	if [[ "$target" == *-apple-darwin ]]; then
		verify_macos_release_archive "$archive_path"
	fi
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
  --draft             Create the GitHub Release as a draft (hold publication
                      until `gh release edit <tag> --draft=false` is run).
                      Used to stage assets for smoke checks before publishing.
  --skip-crates       Skip the crates.io publish handoff
  --skip-binaries     Skip building and uploading binaries (and Homebrew update)
  --skip-docs         Skip docs.rs rebuild trigger
  --skip-release      Resume finalization for an already-tagged release: skip
                      the local sanity build, cargo-release (version/tag/push),
                      and the CI trigger, but still run Steps 4-6 (collect
                      binaries, upload assets, Homebrew). Use with an explicit
                      version, e.g. `./scripts/release.sh 0.171.5 --skip-release`.
  --full-ci           Use GitHub Actions for ALL platforms (including macOS)
                      Default: builds macOS locally, CI for Linux/Windows
  --ci-only           Trigger CI for Linux/Windows only (skip local macOS build)
                      Useful when macOS binaries already built locally
  -h, --help          Show this help message

Environment:
  UPLOAD_PARALLEL_JOBS
                      Parallel GitHub Release asset uploads in Step 4
                      (default 4).

Cost Optimization:
  Default mode (recommended):
    • macOS binaries: built locally (no CI cost, faster)
    • Linux/Windows: built on GitHub Actions (free for public repos)

  --full-ci mode (all CI, higher cost):
    • All platforms built on GitHub Actions
    • Uses 4 runners: 2x macOS, 1x Ubuntu, 1x Windows
    • Estimated cost: ~20-30 minutes of runner time

  --ci-only mode (hybrid):
    • Skip local macOS build
    • Only trigger CI for Linux/Windows
    • Use when you already have macOS binaries

USAGE
}


# Changelog generation using git-cliff
update_changelog_from_commits() {
	local version=$1
	local dry_run_flag=$2

	print_info "Generating changelog for version $version using git-cliff..."

	# Find the previous semver tag (handles both v0.82.0 and 0.82.0 formats)
	local previous_version
	previous_version=$(git tag | grep -E '^[vV]?[0-9]+\.[0-9]+\.[0-9]+$' | sed 's/^[vV]//' | sort -t. -k1,1rn -k2,2rn -k3,3rn | awk -v ver="$version" '$0 != ver {print; exit}')

	if [[ -n "$previous_version" ]]; then
		print_info "Previous version tag: $previous_version"
	else
		print_info "No previous semver tag found"
	fi

	# Check if git-cliff is available
	if command -v git-cliff >/dev/null 2>&1; then
		print_info "Using git-cliff for changelog generation"

		# Set GitHub token for git-cliff if available
		local github_token=""
		if command -v gh >/dev/null 2>&1; then
			github_token=$(gh auth token 2>/dev/null || true)
		fi

		# Set up git-cliff arguments
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

		# Generate changelog entry for the new version
		local temp_changelog
		temp_changelog=$(mktemp)

		# Generate changelog for the specific version
		# Use range from previous version to current if available
		if [[ -n "$previous_version" ]]; then
			print_info "Generating changelog from $previous_version to $version"
			git-cliff "${cliff_args[@]}" --output "$temp_changelog" "${previous_version}..HEAD" 2>/dev/null ||
				git-cliff "${cliff_args[@]}" --output "$temp_changelog" 2>/dev/null || true
		else
			git-cliff "${cliff_args[@]}" --output "$temp_changelog" 2>/dev/null || true
		fi

		if [[ -s "$temp_changelog" ]]; then
			# Extract the new version section from git-cliff output
			local changelog_content
			changelog_content=$(cat "$temp_changelog")

			# Extract content between first and second version headers (portable across macOS/Linux)
			local version_section
			version_section=$(echo "$changelog_content" | awk '/^## /{if(++n==2)exit} n==1' 2>/dev/null || true)

			# Build Full Changelog URL
			local full_changelog_url
			if [[ -n "$previous_version" ]]; then
				full_changelog_url="https://github.com/vinhnx/vtcode/compare/${previous_version}...${version}"
			else
				full_changelog_url="https://github.com/vinhnx/vtcode/releases/tag/${version}"
			fi

			# Save to global variable for release notes use (GitHub Release body)
			{
				echo "## What's Changed"
				echo ""
				if [[ -n "$version_section" ]]; then
					echo "$version_section"
				else
					# Fallback: use full changelog if extraction failed
					echo "$changelog_content"
				fi
				echo ""
				echo "**Full Changelog**: ${full_changelog_url}"
			} >"$RELEASE_NOTES_FILE"

			if [[ -f CHANGELOG.md ]]; then
				# A re-run must replace a stale section (e.g. an aborted
				# `--minor` run that pre-inserted this version) instead of
				# skipping: skipping leaves the file out of order and drops
				# the real commits for this release.
				if grep -q "^## $version " CHANGELOG.md; then
					print_warning "Version $version already exists in CHANGELOG.md, regenerating entry"
					remove_changelog_version_section "$version"
				fi
				# Insert git-cliff's generated content above the newest version
				if [[ -n "$version_section" ]]; then
					insert_changelog_entry "$version_section"
				else
					insert_changelog_entry "$changelog_content"
				fi
			else
				# Create new changelog with git-cliff output
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

# Built-in changelog generation (fallback when git-cliff is not available)
update_changelog_builtin() {
	local version=$1
	local dry_run_flag=$2

	print_info "Generating changelog for version $version from commits (builtin)..."

	# Find the most recent tag that follows SemVer (vX.Y.Z or X.Y.Z) in commit history
	# We exclude the version we're about to release if it already exists as a tag
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
		if [[ -n "$previous_tag" ]]; then
			echo "**Full Changelog**: https://github.com/vinhnx/vtcode/compare/${previous_tag}...${version}"
		else
			echo "**Full Changelog**: https://github.com/vinhnx/vtcode/releases/tag/${version}"
		fi
	} >"$RELEASE_NOTES_FILE"

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
		if grep -q "^## $version " CHANGELOG.md; then
			print_warning "Version $version already exists in CHANGELOG.md, regenerating entry"
			remove_changelog_version_section "$version"
		fi
		# Insert new entry above the newest version
		insert_changelog_entry "$changelog_entry"
	else
		{
			printf '%s\n' "# Changelog - vtcode"
			printf '%s\n' ""
			printf '%s\n' "All notable changes to vtcode will be documented in this file."
			printf '%s\n' ""
			printf '%b\n' "$changelog_entry"
		} >CHANGELOG.md
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
		# shellcheck disable=SC2016
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
	local crates=(
		vtcode-diff vtcode-commons vtcode-auth vtcode-exec-events vtcode-webmcp vtcode-memory vtcode-macros
		vtcode-config vtcode-indexer vtcode-bash-runner vtcode-utility-tool-specs vtcode-eval
		vtcode-safety vtcode-a2a vtcode-llm vtcode-skills vtcode-agent-plugins vtcode-ui vtcode-mcp
		vtcode-core vtcode-acp vtcode
	)
	for crate in "${crates[@]}"; do
		curl -s -o /dev/null "https://docs.rs/${crate}/${version}" || true
	done
}

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

	# If checksums were not passed as arguments, try dist/ then GitHub release
	if [[ -z "$x86_64_macos_sha" ]]; then
		x86_64_macos_sha=$(cat "dist/vtcode-$version-x86_64-apple-darwin.sha256" 2>/dev/null | awk '{print $1}' || echo "")
	fi
	if [[ -z "$aarch64_macos_sha" ]]; then
		aarch64_macos_sha=$(cat "dist/vtcode-$version-aarch64-apple-darwin.sha256" 2>/dev/null | awk '{print $1}' || echo "")
	fi
	if [[ -z "$aarch64_linux_sha" ]]; then
		aarch64_linux_sha=$(cat "dist/vtcode-$version-aarch64-unknown-linux-gnu.sha256" 2>/dev/null | awk '{print $1}' || echo "")
	fi

	# Last resort: download .sha256 files from the GitHub release
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

		local tap_token="${HOMEBREW_TAP_TOKEN:-${GITHUB_TOKEN:-}}"
		if [[ -z "$tap_token" ]]; then
			print_error "No token available for homebrew-tap. Set HOMEBREW_TAP_TOKEN or GITHUB_TOKEN."
			exit 1
		fi

		local tap_repo_url="https://x-access-token:${tap_token}@github.com/vinhnx/homebrew-tap.git"

		if ! git clone "$tap_repo_url" "$temp_dir" >/dev/null 2>&1; then
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

		if ! git -C "$temp_dir" push "$tap_repo_url" HEAD:main; then
			print_error "Failed to push Homebrew tap update"
			exit 1
		fi

		print_success "Published vtcode formula to vinhnx/homebrew-tap"
	); then
		return 1
	fi
}

main() {
	local release_argument=''
	local increment_type=''
	local dry_run=false
	local release_draft=false
	local skip_crates=false
	local skip_binaries=false
	local skip_docs=false
	local skip_release=false
	local full_ci=false
	local ci_only=false

	while [[ $# -gt 0 ]]; do
		case "$1" in
		-h | --help)
			show_usage
			exit 0
			;;
		-p | --patch)
			increment_type='patch'
			shift
			;;
		-m | --minor)
			increment_type='minor'
			shift
			;;
		-M | --major)
			increment_type='major'
			shift
			;;
		--dry-run)
			dry_run=true
			shift
			;;
		--draft)
			release_draft=true
			shift
			;;
		--skip-crates)
			skip_crates=true
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
			skip_release=true
			shift
			;;
		--full-ci)
			full_ci=true
			shift
			;;
		--ci-only)
			ci_only=true
			shift
			;;
		--path)
			print_error "'--path' is not supported by cargo-release."
			print_info "Use '-p <package-name>' to select a workspace package, or cd into the crate directory."
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

		if gh auth status >/dev/null 2>&1; then
			print_info "Switching to GitHub account vinhnx..."
			if unset GITHUB_TOKEN && gh auth switch -u vinhnx >/dev/null 2>&1; then
				print_success "Switched to GitHub account vinhnx"
			elif [[ "$dry_run" == 'true' ]]; then
				print_info "Dry run - continuing without switching GitHub account"
			else
				print_error "Could not switch to GitHub account vinhnx. Run \`gh auth switch -u vinhnx\` or re-authenticate before releasing."
				exit 1
			fi
		elif [[ "$dry_run" == 'true' ]]; then
			print_info "GitHub CLI is not authenticated; dry run will continue without GitHub publishing."
		else
			print_error "GitHub CLI is not authenticated. Run \`gh auth login -h github.com\` before releasing."
			exit 1
		fi

		# Skip the refresh step that causes hangs, assuming user has proper scopes.
		print_info "GitHub CLI scopes refresh is skipped; re-authenticate manually if GitHub operations fail."
	elif [[ "$dry_run" == 'true' ]]; then
		print_info "GitHub CLI not found. Dry run will continue without GitHub publishing."
	else
		print_error "GitHub CLI not found. Install \`gh\` before releasing."
		exit 1
	fi

	local current_version
	current_version=$(get_current_version)
	print_info "Current version: $current_version"

	# Calculate next version.
	# Prefer `cargo xtask bump-version --bump <major|minor|patch>` for reliable
	# semver arithmetic. The bash arithmetic below is kept for backward
	# compatibility with existing CI workflows that source this script directly.
	local next_version
	if [[ "$release_argument" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
		next_version="$release_argument"
	else
		IFS='.' read -ra v <<<"$current_version"
		case "$release_argument" in
		major)
			local major_num=$((v[0] + 1))
			next_version="${major_num}.0.0"
			;;
		minor)
			local minor_num=$((v[1] + 1))
			next_version="${v[0]}.${minor_num}.0"
			;;
		patch)
			local patch_num=$((v[2] + 1))
			next_version="${v[0]}.${v[1]}.${patch_num}"
			;;
		esac
	fi

	if [[ -z "$next_version" ]]; then
		print_error "Invalid release argument: '$release_argument'"
		print_info "Usage: $0 [patch|minor|major|<version>] [options]"
		exit 1
	fi

	if [[ "$dry_run" == 'true' ]]; then
		print_info "Running in dry-run mode for $next_version"
	else
		print_info "Releasing version: $next_version"
	fi

	# Check if using full CI mode
	if [[ "$full_ci" == 'true' ]]; then
		print_info "Full CI mode: Using GitHub Actions for ALL platforms (including macOS)"

		if [[ "$dry_run" == 'true' ]]; then
			print_info "Dry run - would trigger full CI workflow for $next_version"
			print_info "Command: gh workflow run release.yml --field tag=$next_version"
		else
			# Trigger full CI release workflow
			if gh workflow run release.yml --field tag="$next_version"; then
				print_success "Full CI workflow triggered for $next_version"
				print_info "Monitor progress: https://github.com/vinhnx/vtcode/actions/workflows/release.yml"
				print_info ""
				print_info "The CI will:"
				print_info "  1. Build all platforms (macOS, Linux, Windows)"
				print_info "  2. Create GitHub Release"
				print_info "  3. Upload all binaries"
				print_info ""
				print_info "Note: cargo-release and changelog updates still run locally"
			else
				print_error "Failed to trigger full CI workflow"
				exit 1
			fi
		fi

		# In full CI mode, skip local binary builds but still do cargo-release
		skip_binaries=true
	fi

	# Check if using CI-only mode (Linux/Windows only, skip macOS)
	if [[ "$ci_only" == 'true' ]]; then
		print_info "CI-only mode: Triggering CI for Linux/Windows only (skip macOS build)"

		if [[ "$dry_run" == 'true' ]]; then
			print_info "Dry run - would trigger CI workflow for $next_version"
			print_info "Command: gh workflow run build-linux-windows.yml --field tag=$next_version"
		else
			# Trigger CI for Linux/Windows only
			if gh workflow run build-linux-windows.yml --field tag="$next_version"; then
				print_success "CI workflow triggered for $next_version"
				print_info "Monitor progress: https://github.com/vinhnx/vtcode/actions/workflows/build-linux-windows.yml"
			else
				print_warning "Failed to trigger CI workflow"
			fi
		fi

		# Skip local binary builds
		skip_binaries=true
	fi

	# 0.5 Regenerate Documentation Map
	print_info "Step 0.5: Regenerating documentation map and syncing assets..."
	if [[ "$dry_run" == 'true' ]]; then
		print_info "Dry run - would run: python3 scripts/generate_docs_map.py && python3 scripts/sync_embedded_assets.py"
	else
		python3 scripts/generate_docs_map.py
		python3 scripts/sync_embedded_assets.py
		git add docs/modules/vtcode_docs_map.md
		if ! git diff --cached --quiet; then
			GIT_AUTHOR_NAME="vtcode-release-bot" \
				GIT_AUTHOR_EMAIL="noreply@vtcode.com" \
				GIT_COMMITTER_NAME="vtcode-release-bot" \
				GIT_COMMITTER_EMAIL="noreply@vtcode.com" \
				git commit -m "docs: update documentation map [skip ci]"
			print_success "Documentation map updated and committed"
		else
			print_info "Documentation map already up to date"
		fi
	fi

	# 1. Local Build (both macOS architectures for Homebrew, or current platform on Linux)
	# Skipped under --skip-release: the irreversible version bump already happened,
	# so the pre-bump sanity build has no purpose and Step 4 rebuilds the real
	# artifacts anyway.
	if [[ "$skip_binaries" == 'false' && "$skip_release" == 'false' ]]; then
		if [[ "$dry_run" == 'true' ]]; then
			print_info "Step 1 (dry-run): Would build binaries for x86_64-apple-darwin and aarch64-apple-darwin"
		else
			print_info "Step 1: Local binary build (macOS: both architectures, Linux: current platform)..."

			local build_args=(-v "$next_version" --only-build-local)
			env CARGO_BUILD_RUSTC_WRAPPER= RUSTC_WRAPPER= ./scripts/build-and-upload-binaries.sh "${build_args[@]}"
		fi
	fi

	# 2. Changelog Update & capture for Release Notes
	print_info "Step 2: Generating changelog and release notes..."
	update_changelog_from_commits "$next_version" "$dry_run"

	# 3. Cargo Release (version, tag, and push only)
	print_info "Step 3: Running cargo release (version, tag, and push only)..."

	# `--registry crates-io` is passed even though we never publish here (--no-publish).
	# Without it, cargo-release unconditionally fetches every workspace crate's entry from
	# the crates.io sparse index (to decide `ensure_owners`). For freshly published crates
	# those index paths are slow/uncached at the CDN edge and cargo-release has no retry, so a
	# single 30s timeout aborts the whole release. Naming a registry short-circuits that lookup
	# (cargo-release's `CratesIoIndex::krate` returns early when a registry is set) and is safe
	# because `--no-publish` means the registry is otherwise unused.
	local command=(cargo release "$release_argument" --workspace --config release.toml --execute --no-confirm --no-publish --registry crates-io)

	if [[ "$skip_release" == 'true' ]]; then
		print_info "Skipping cargo release (--skip-release); keeping existing version and tag $next_version."
	elif [[ "$dry_run" == 'true' ]]; then
		print_info "Dry run - would run: ${command[*]}"
	else
		env CARGO_BUILD_RUSTC_WRAPPER= RUSTC_WRAPPER= "${command[@]}"
	fi

	if [[ "$dry_run" == 'true' ]]; then
		if [[ "$skip_crates" == 'false' ]]; then
			print_info "Dry run - would run: ./scripts/publish_extracted_crates.sh --dry-run --skip-tests --skip-tags --skip-follow-up"
		fi
		print_success 'Dry run completed'
		exit 0
	fi

	if [[ "$skip_crates" == 'false' && "$skip_release" == 'false' ]]; then
		print_distribution "Publishing crates in dependency order..."
		# Ensure publish script is executable (defensive against lost +x on fresh checkout or noexec FS)
		if [[ ! -x "$SCRIPT_DIR/publish_extracted_crates.sh" ]]; then
			chmod +x "$SCRIPT_DIR/publish_extracted_crates.sh" 2>/dev/null || true
		fi
		if ! bash "$SCRIPT_DIR/publish_extracted_crates.sh" --skip-tests --skip-tags --skip-follow-up; then
			print_error "Crate publishing failed."
			print_info "You can resume crate publishing with: bash ./scripts/publish_extracted_crates.sh --start-from <crate> --skip-tests --skip-tags --skip-follow-up"
			if [[ "${RELEASE_CONTINUE_ON_PUBLISH_FAIL:-0}" == "1" ]]; then
				print_warning "RELEASE_CONTINUE_ON_PUBLISH_FAIL=1 — continuing to GitHub Release/binaries/Homebrew despite publish failure."
			else
				print_error "Aborting release. Remaining steps (GitHub Release, binaries, Homebrew) will NOT run until crates.io publish succeeds."
				print_info "Fix crates.io connectivity, then resume publishing. To force continuation despite failure, re-run with RELEASE_CONTINUE_ON_PUBLISH_FAIL=1"
				exit 1
			fi
		fi
	fi

	# Confirm version after cargo-release
	local released_version
	released_version=$(get_current_version)
	if [[ "$released_version" != "$next_version" ]]; then
		print_warning "Released version $released_version differs from expected $next_version"
	fi

	# 3.5 Trigger CI for Linux and Windows builds
	# Skipped under --skip-release: the CI run for this tag already exists and
	# Step 4's fallback discovery reuses its successful artifacts.
	if [[ "$skip_binaries" == 'false' && "$skip_release" == 'false' ]]; then
		print_info "Step 3.5: Triggering CI for Linux and Windows builds..."

		if [[ "$dry_run" == 'true' ]]; then
			print_info "Dry run - would trigger CI workflow for $released_version"
		else
			# Push tags to ensure CI can checkout the correct ref
			print_info "Pushing tags to GitHub..."
			git push origin --tags --no-verify 2>/dev/null || true

			# Trigger the build-linux-windows workflow
			if gh workflow run build-linux-windows.yml --field tag="$released_version"; then
				print_success "CI workflow triggered for $released_version"

				# Wait for CI to complete (with timeout)
				print_info "Waiting for CI builds to complete (timeout: 60 minutes)..."
				local wait_start
				wait_start=$(date +%s)
				local timeout=7800 # 130 minutes
				local run_id=""

				# Get the workflow run ID - wait for it to appear
				local find_run_attempts=0
				local max_find_attempts=24 # Wait up to 2 minutes for run to appear
				while [[ -z "$run_id" && $find_run_attempts -lt $max_find_attempts ]]; do
					sleep 5
					# Look for the most recent run of this workflow
					run_id=$(gh run list --workflow build-linux-windows.yml --limit 1 --json databaseId --jq '.[0].databaseId' 2>/dev/null || true)
					find_run_attempts=$((find_run_attempts + 1))
				done

				if [[ -z "$run_id" ]]; then
					print_warning "Could not find CI workflow run - will use macOS binaries only"
				else
					# Wait for the run to complete
					local status="in_progress"
					local conclusion=""
					while [[ "$status" == "in_progress" || "$status" == "queued" ]]; do
						sleep 30
						local run_info
						run_info=$(gh run view "$run_id" --json status,conclusion 2>/dev/null || echo '{"status":"failed","conclusion":"failure"}')
						status=$(echo "$run_info" | jq -r '.status')
						conclusion=$(echo "$run_info" | jq -r '.conclusion')

						# Check timeout
						local now
						now=$(date +%s)
						local elapsed=$((now - wait_start))
						if [[ $elapsed -gt $timeout ]]; then
							print_warning "CI build timeout after $timeout seconds - will use macOS binaries only"
							break
						fi

						print_info "CI status: $status (${elapsed}s elapsed)"
					done

					if [[ "$conclusion" == "success" ]]; then
						print_success "CI builds completed successfully"
						# Store run_id for later download
						CI_RUN_ID="$run_id"
					else
						print_warning "CI build failed with conclusion: $conclusion - will use macOS binaries only"
						gh run view "$run_id" --log || true
					fi
				fi
			else
				print_warning "Failed to trigger CI workflow - will use macOS binaries only"
			fi
		fi
	fi

	# GitHub Release Creation and Binary Upload via gh
	print_info "Step 4: Creating GitHub Release with binaries..."

	# Ensure GITHUB_TOKEN is available
	if [[ -z "${GITHUB_TOKEN:-}" ]] && command -v gh >/dev/null 2>&1; then
		export GITHUB_TOKEN
		GITHUB_TOKEN=$(gh auth token)
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

		# Create GitHub release with release notes. `--draft` (bare) holds the
		# release as a draft for smoke checks; otherwise publish immediately.
		local gh_draft_flag="--draft=false"
		if [[ "$release_draft" == 'true' ]]; then
			gh_draft_flag="--draft"
		fi
		if gh release create "$released_version" \
			--title "$released_version" \
			--notes "$release_body" \
			"$gh_draft_flag" \
			--prerelease=false; then
			if [[ "$release_draft" == 'true' ]]; then
				print_success "GitHub Release $released_version created as a draft"
				print_info "Publish with: gh release edit $released_version --draft=false --latest"
			else
				print_success "GitHub Release $released_version created successfully"
			fi
		else
			print_error "Failed to create GitHub Release"
			exit 1
		fi
	fi

	# 4. Collect and Upload All Binaries
	if [[ "$skip_binaries" == 'false' ]]; then
		print_info "Step 4: Collecting binaries for all platforms..."

		local binaries_dir="/tmp/vtcode-release-$released_version"
		mkdir -p "$binaries_dir"

		# Preflight: uploads need push access, but harness environments often
		# export a pull-only GITHUB_TOKEN (uploads then 404). Fail fast here
		# -- before the expensive macOS builds -- instead of after uploading.
		local release_repo
		release_repo=$(gh repo view --json nameWithOwner --jq .nameWithOwner 2>/dev/null || true)
		if [[ -n "$release_repo" ]]; then
			if ! ensure_release_push_access "$release_repo"; then
				print_error "Cannot upload release assets without push access to $release_repo; aborting"
				exit 1
			fi
		else
			print_warning "Could not determine repo slug; skipping push-access preflight check"
		fi

		# Generate the full man page set (main + every subcommand) once and
		# include it in every platform archive.
		local man_stage="$binaries_dir/man/man1"
		if cargo run --locked -p xtask -- gen-man --out-dir "$man_stage" >/dev/null 2>&1; then
			print_info "Generated $(ls "$man_stage" | wc -l | tr -d ' ') man pages"
		else
			print_warning "Man page generation failed; archives will ship without man pages"
		fi

		# Build macOS binaries in parallel
		print_info "Building macOS binaries in parallel..."
		macos_release_signing_preflight

		# Binary size directly impacts cold-start time (dyld page-faults on the
		# Mach-O).  [profile.release] uses opt-level="z" + LTO + codegen-units=1
		# to minimize size.  -Wl,-dead_strip removes unreachable functions that
		# survive LTO (trait objects, callback registrations), further shrinking
		# __TEXT.  The flag is set via CARGO_TARGET_*_RUSTFLAGS so it is
		# self-contained in this script and does not depend on a gitignored
		# .cargo/config.toml being present on the build machine.
		# --locked ensures the Cargo.lock matches Cargo.toml so the size-optimized
		# release profile is actually used.
		env CARGO_TARGET_X86_64_APPLE_DARWIN_RUSTFLAGS="-C link-arg=-Wl,-dead_strip" \
			cargo build --locked --profile release --target x86_64-apple-darwin --jobs 4 &>/dev/null &
		local pid_x86=$!
		env CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS="-C link-arg=-Wl,-dead_strip" \
			cargo build --locked --profile release --target aarch64-apple-darwin --jobs 4 &>/dev/null &
		local pid_arm=$!

		# Wait for x86_64
		if wait "$pid_x86"; then
			package_release_archive \
				"x86_64-apple-darwin" \
				"vtcode" \
				"$binaries_dir/vtcode-$released_version-x86_64-apple-darwin.tar.gz"
			shasum -a 256 "$binaries_dir/vtcode-$released_version-x86_64-apple-darwin.tar.gz" >"$binaries_dir/vtcode-$released_version-x86_64-apple-darwin.sha256"
			local x86_bin="target/x86_64-apple-darwin/release/vtcode"
			local x86_size_h
			x86_size_h=$(du -h "$x86_bin" 2>/dev/null | awk '{print $1}')
			print_success "Built macOS x86_64 (binary: ${x86_size_h:-unknown})"
		else
			print_warning "Failed to build macOS x86_64"
		fi

		# Wait for aarch64
		if wait "$pid_arm"; then
			package_release_archive \
				"aarch64-apple-darwin" \
				"vtcode" \
				"$binaries_dir/vtcode-$released_version-aarch64-apple-darwin.tar.gz"
			shasum -a 256 "$binaries_dir/vtcode-$released_version-aarch64-apple-darwin.tar.gz" >"$binaries_dir/vtcode-$released_version-aarch64-apple-darwin.sha256"
			local arm_bin="target/aarch64-apple-darwin/release/vtcode"
			local arm_size_h
			arm_size_h=$(du -h "$arm_bin" 2>/dev/null | awk '{print $1}')
			print_success "Built macOS aarch64 (Apple Silicon) (binary: ${arm_size_h:-unknown})"

			# Cold-start spot check: verify sub-1s launch on the primary platform.
			# A fresh /tmp copy forces a cold dyld load (new inode → page faults).
			# This catches regressions in binary size or startup-path work that
			# would otherwise ship unnoticed.
			local cold_bin="/tmp/vtcode-cold-check-$$"
			cp "$arm_bin" "$cold_bin" 2>/dev/null
			if [[ -x "$cold_bin" ]]; then
				local cold_ms
				cold_ms=$(/usr/bin/time -p "$cold_bin" --version 2>&1 | awk '/^real/{print int($2 * 1000)}')
				rm -f "$cold_bin"
				if [[ -n "$cold_ms" ]]; then
					if [[ "$cold_ms" -lt 1000 ]]; then
						print_success "Cold-start check: ${cold_ms}ms (sub-1s ✓)"
					else
						print_warning "Cold-start check: ${cold_ms}ms (above 1s — investigate binary size)"
					fi
				fi
			fi
		else
			print_warning "Failed to build macOS aarch64"
		fi

		# Download Linux and Windows binaries from CI artifacts
		print_info "Downloading Linux and Windows binaries from CI..."

		# Use the run_id from step 3.5 if CI was successful, otherwise try to find one
		local run_id="${CI_RUN_ID:-}"

		if [[ -z "$run_id" ]]; then
			# Try to find a successful run if CI_RUN_ID wasn't set
			run_id=$(gh run list --workflow build-linux-windows.yml --branch main --event workflow_dispatch --limit 1 --json databaseId,conclusion --jq '.[] | select(.conclusion == "success") | .databaseId' | head -1)
		fi

		# Windows x86_64 is REQUIRED for the legacy updater bridge by default so
		# every release rescues all users. The bridge also rescues Windows
		# v0.141.0-v0.141.4 users, whose updater used the `{target}.tar.gz`
		# identifier that never matched the published `.zip` (so they could not
		# self-update at all). Set RELEASE_REQUIRE_WINDOWS=false only for an
		# emergency macOS/Linux rescue when Windows CI is flaky and would
		# otherwise block the release.
		local require_windows="${RELEASE_REQUIRE_WINDOWS:-true}"

		if [[ -n "$run_id" ]]; then
			# Download all artifacts from the CI run
			local ci_artifacts_dir="/tmp/vtcode-ci-artifacts-$released_version"
			mkdir -p "$ci_artifacts_dir"

			# Download Linux x86_64 artifact
			print_info "Downloading Linux x86_64 artifact..."
			if gh run download "$run_id" --name "vtcode-${released_version}-x86_64-unknown-linux-gnu" --dir "$ci_artifacts_dir" 2>/dev/null; then
				mv "$ci_artifacts_dir"/*.tar.gz "$binaries_dir/" 2>/dev/null || true
				mv "$ci_artifacts_dir"/*.sha256 "$binaries_dir/" 2>/dev/null || true
				print_success "Downloaded: Linux x86_64 gnu"
			else
				print_warning "Could not download: Linux x86_64 gnu"
			fi

			# Download Linux x86_64 musl artifact
			print_info "Downloading Linux x86_64 musl artifact..."
			if gh run download "$run_id" --name "vtcode-${released_version}-x86_64-unknown-linux-musl" --dir "$ci_artifacts_dir" 2>/dev/null; then
				mv "$ci_artifacts_dir"/*.tar.gz "$binaries_dir/" 2>/dev/null || true
				mv "$ci_artifacts_dir"/*.sha256 "$binaries_dir/" 2>/dev/null || true
				print_success "Downloaded: Linux x86_64 musl"
			else
				print_warning "Could not download: Linux x86_64 musl"
			fi

			# Download Linux aarch64 artifact
			print_info "Downloading Linux aarch64 artifact..."
			if gh run download "$run_id" --name "vtcode-${released_version}-aarch64-unknown-linux-gnu" --dir "$ci_artifacts_dir" 2>/dev/null; then
				mv "$ci_artifacts_dir"/*.tar.gz "$binaries_dir/" 2>/dev/null || true
				mv "$ci_artifacts_dir"/*.sha256 "$binaries_dir/" 2>/dev/null || true
				print_success "Downloaded: Linux aarch64"
			else
				print_warning "Could not download: Linux aarch64"
			fi

			# Download Windows x86_64 artifact. Required for the bridge; the
			# required-target coverage check below fails the release if missing.
			if [[ "$require_windows" == "true" ]]; then
				print_info "Downloading Windows x86_64 artifact..."
				if gh run download "$run_id" --name "vtcode-${released_version}-x86_64-pc-windows-msvc" --dir "$ci_artifacts_dir" 2>/dev/null; then
					mv "$ci_artifacts_dir"/*.zip "$binaries_dir/" 2>/dev/null || true
					mv "$ci_artifacts_dir"/*.sha256 "$binaries_dir/" 2>/dev/null || true
					print_success "Downloaded: Windows x86_64"
				else
					print_warning "Could not download: Windows x86_64"
				fi
			else
				print_info "Skipping Windows artifact download (RELEASE_REQUIRE_WINDOWS=false)"
			fi

			rm -rf "$ci_artifacts_dir"
		else
			print_warning "No CI workflow run found - will use macOS binaries only"
		fi

		# Required target coverage for the legacy updater bridge. Every listed
		# target must publish a normal archive so a raw compatibility executable
		# (compat-vtcode-<v>-<target>.tar.gz.compat) can be derived for the broken
		# v0.141.0-v0.141.4 updaters. Windows is required by default; opt out
		# with RELEASE_REQUIRE_WINDOWS=false for an emergency rescue (see above).
		local -a required_targets=(
			"x86_64-apple-darwin:tar.gz"
			"aarch64-apple-darwin:tar.gz"
			"x86_64-unknown-linux-gnu:tar.gz"
			"x86_64-unknown-linux-musl:tar.gz"
			"aarch64-unknown-linux-gnu:tar.gz"
		)
		if [[ "$require_windows" == "true" ]]; then
			required_targets+=("x86_64-pc-windows-msvc:zip")
		fi
		local missing_required=0
		for item in "${required_targets[@]}"; do
			local rtarget="${item%%:*}"
			local rext="${item##*:}"
			if [[ ! -f "$binaries_dir/vtcode-${released_version}-${rtarget}.${rext}" ]]; then
				print_error "Missing required release archive: vtcode-${released_version}-${rtarget}.${rext}"
				missing_required=1
			fi
		done
		if [[ "$missing_required" -ne 0 ]]; then
			print_error "Release aborted: not all required target archives are present."
			print_info "Ensure the build-linux-windows workflow succeeded and macOS builds completed."
			exit 1
		fi

		# Generate one raw compatibility executable per normal archive. The
		# legacy self_update matcher returns the FIRST asset (GitHub sorts the
		# assets array alphabetically by name) whose name contains both the
		# target triple and the `{target}.tar.gz` identifier. `compat-` sorts
		# before `vtcode-`, so the `compat-vtcode-<v>-<target>.tar.gz.compat`
		# asset is selected; its `.compat` final extension is treated as a plain
		# uncompressed binary, sidestepping the missing `compression-tar-gz`
		# feature. See scripts/release-assets.sh for the full rationale.
		print_info "Generating legacy updater compatibility assets..."
		local -a compat_assets=()
		for item in "${required_targets[@]}"; do
			local ctarget="${item%%:*}"
			local cext="${item##*:}"
			local normal_archive="$binaries_dir/vtcode-${released_version}-${ctarget}.${cext}"
			local compat_path
			if ! compat_path=$(compatibility_asset_path "$normal_archive" "$binaries_dir"); then
				print_error "Failed to derive compatibility asset path for $normal_archive"
				exit 1
			fi
			if ! create_compatibility_asset "$normal_archive" "$compat_path"; then
				print_error "Failed to generate compatibility asset from $normal_archive"
				exit 1
			fi
			if [[ "$ctarget" == *-apple-darwin ]]; then
				if ! verify_macos_release_binary "$compat_path"; then
					print_error "macOS compatibility asset failed Gatekeeper verification: $compat_path"
					exit 1
				fi
			fi
			compat_assets+=("$compat_path")
		done
		print_success "Generated ${#compat_assets[@]} compatibility assets"

		# Upload all binaries to GitHub Release
		print_info "Uploading binaries to GitHub Release..."

		# Keep compatibility binaries out of the aggregate manifest. Older
		# updaters match checksum filenames by substring and would otherwise
		# accept `compat-<archive>.compat` as the checksum for `<archive>`.
		if ! generate_checksums_manifest "$binaries_dir"; then
			print_error "Failed to generate checksums.txt"
			exit 1
		fi

		# Validate the complete staged release (compat + archive + checksum per
		# target, plus checksums.txt) before uploading anything. The full
		# contract requires Windows; skip it when Windows was explicitly
		# excluded via RELEASE_REQUIRE_WINDOWS=false.
		if [[ "$require_windows" == "true" ]]; then
			if ! validate_release_assets "$binaries_dir" "$released_version"; then
				print_error "Release asset validation failed; aborting upload"
				exit 1
			fi
		fi

		# Ensure install scripts are executable
		chmod +x "$SCRIPT_DIR/install.sh" 2>/dev/null || true
		chmod +x "$SCRIPT_DIR/install.ps1" 2>/dev/null || true

		# Two-phase upload: compatibility assets first, then normal archives,
		# checksums, and install scripts. Upload order does NOT control legacy
		# selection (GitHub re-sorts assets alphabetically by name; the `compat-`
		# prefix is what makes the legacy updater pick them). Each phase
		# uploads in parallel (UPLOAD_PARALLEL_JOBS, default 4); per-file
		# retry handles transient HTTP 500s from uploads.github.com on large
		# (~40-80MB) raw compat binaries.
		local upload_failed=0
		if [[ ${#compat_assets[@]} -gt 0 ]]; then
			print_info "Uploading compatibility assets (legacy bridge)..."
			if ! upload_release_assets_parallel "$released_version" "${compat_assets[@]}"; then
				print_error "Failed to upload compatibility assets to GitHub Release"
				upload_failed=1
			fi
		fi

		shopt -s nullglob
		local -a normal_release_files=(
			"$binaries_dir"/vtcode-*.tar.gz
			"$binaries_dir"/vtcode-*.zip
			"$binaries_dir"/vtcode-*.sha256
			"$SCRIPT_DIR/install.sh"
			"$SCRIPT_DIR/install.ps1"
		)
		shopt -u nullglob
		# Exclude compatibility assets from the normal upload glob; they
		# start with `compat-` so `vtcode-*.tar.gz`/`vtcode-*.zip` already
		# skip them, but this guard defends against re-runs/renames.
		local -a filtered_normal_files=()
		local nf
		for nf in "${normal_release_files[@]}"; do
			case "$(basename "$nf")" in
			compat-*.tar.gz.compat) continue ;;
			esac
			filtered_normal_files+=("$nf")
		done
		# Only include checksums.txt if it has content
		if [[ -s "$binaries_dir/checksums.txt" ]]; then
			filtered_normal_files+=("$binaries_dir/checksums.txt")
		fi
		if [[ ${#filtered_normal_files[@]} -gt 0 ]]; then
			print_info "Uploading normal archives, checksums, and install scripts..."
			if ! upload_release_assets_parallel "$released_version" "${filtered_normal_files[@]}"; then
				print_error "Failed to upload binaries to GitHub Release"
				upload_failed=1
			fi
		fi

		if [[ "$upload_failed" -ne 0 ]]; then
			exit 1
		fi
		print_success "All binaries, compatibility assets, checksums.txt, and install scripts uploaded successfully"

		# Extract checksums before cleanup for Homebrew formula update
		local release_x86_sha=""
		local release_arm_sha=""
		local release_arm_linux_sha=""
		if [[ -f "$binaries_dir/vtcode-$released_version-x86_64-apple-darwin.sha256" ]]; then
			release_x86_sha=$(awk '{print $1}' "$binaries_dir/vtcode-$released_version-x86_64-apple-darwin.sha256")
		fi
		if [[ -f "$binaries_dir/vtcode-$released_version-aarch64-apple-darwin.sha256" ]]; then
			release_arm_sha=$(awk '{print $1}' "$binaries_dir/vtcode-$released_version-aarch64-apple-darwin.sha256")
		fi
		if [[ -f "$binaries_dir/vtcode-$released_version-aarch64-unknown-linux-gnu.sha256" ]]; then
			release_arm_linux_sha=$(awk '{print $1}' "$binaries_dir/vtcode-$released_version-aarch64-unknown-linux-gnu.sha256")
		fi

		# Cleanup
		rm -rf "$binaries_dir"
	fi

	# 5. Publish Homebrew tap
	if [[ "$skip_binaries" == 'false' ]]; then
		print_info "Step 5: Publishing Homebrew formula to vinhnx/homebrew-tap..."
		publish_homebrew_tap "$released_version" "${release_x86_sha:-}" "${release_arm_sha:-}" "${release_arm_linux_sha:-}"
	fi

	# 6. Handle docs.rs rebuild
	if [[ "$skip_crates" == 'false' && "$skip_docs" == 'false' ]]; then
		trigger_docs_rs_rebuild "$released_version" false
	fi

	print_success "Release process finished for $released_version"
	print_info "Distribution:"
	print_info "  ✓ Cargo (crates.io)"
	print_info "  ✓ GitHub Releases (all platforms: macOS local + Linux/Windows CI)"
	print_info "  ✓ Homebrew (vinhnx/homebrew-tap/vtcode)"
	print_info ""
	print_info "Cost optimization:"
	print_info "  • macOS binaries: built locally (no CI cost)"
	print_info "  • Linux/Windows binaries: built on GitHub Actions (free for public repo)"
	print_info ""
	# Fixed: previous line 1561 had truncated `nfo` (from `print_info`) -> `nfo: command not found`
	print_info "Tip: Use --full-ci to build ALL platforms on GitHub Actions"
}

main "$@"
