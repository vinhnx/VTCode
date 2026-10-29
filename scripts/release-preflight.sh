#!/usr/bin/env bash
# Release preflight helpers: usage, branch/tree guards, and GitHub CLI setup.
#
# This file is sourced by `scripts/release.sh`; it intentionally does not set
# shell options so the caller controls `set -euo pipefail`.

print_distribution() {
	printf '%b\n' "${PURPLE}DISTRIBUTION:${NC} $1"
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
  --ci-only           Trigger CI for Linux/Windows only (skip local macOS build)
                      Useful when macOS binaries already built locally
  -h, --help          Show this help message

Cost Optimization:
  Default mode (recommended):
    • macOS binaries: built locally (no CI cost, faster)
    • Linux/Windows: built on GitHub Actions (free for public repos)

  --ci-only mode (hybrid):
    • Skip local macOS build
    • Only trigger CI for Linux/Windows
    • Use when you already have macOS binaries

USAGE
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

# GitHub CLI authentication setup.
#
#   ensure_github_cli <dry-run>
ensure_github_cli() {
	local dry_run=$1
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
}
