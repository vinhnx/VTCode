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
# Shared four-target Homebrew formula transformation.
# shellcheck disable=SC1091
source "$SCRIPT_DIR/homebrew-formula.sh"
# Legacy updater compatibility bridge helpers (.tar.gz.compat raw exec assets).
# shellcheck disable=SC1091
source "$SCRIPT_DIR/release-assets.sh"
# Shared macOS Developer ID signing and notarization helpers.
# shellcheck disable=SC1091
source "$SCRIPT_DIR/macos-release-signing.sh"
# Changelog formatting and shared commit classification.
# shellcheck disable=SC1091
source "$SCRIPT_DIR/release-changelog.sh"
# Preflight checks, usage, and GitHub CLI authentication helpers.
# shellcheck disable=SC1091
source "$SCRIPT_DIR/release-preflight.sh"
# Changelog generation and documentation map refresh helpers.
# shellcheck disable=SC1091
source "$SCRIPT_DIR/release-changelog-ops.sh"
# Binary packaging, CI builds, and docs.rs rebuild helpers.
# shellcheck disable=SC1091
source "$SCRIPT_DIR/release-build.sh"

# Stop `gh` from probing the terminal for its background color.
#
# With stdout on a TTY, `gh` detects the color scheme by asking the terminal for
# its background color (muesli/termenv `HasDarkBackground` -> `termStatusReport`:
# an OSC 11 query plus an `ESC[6n` cursor-position report) and reading the reply
# back from /dev/tty. The asset upload runs four `gh` processes concurrently;
# they race for the same /dev/tty reply, the losers time out, and the unread
# replies are later echoed to the screen as garbage escape sequences -- e.g.
# `ESC]11;rgb:0000/0000/0000ESC\` followed by `ESC[52;1R`. `NO_COLOR` makes `gh`
# skip the probe entirely, and `GH_SPINNER_DISABLED` replaces the motion spinner
# with textual progress so the parallel upload lines stay readable.
# Verified against gh 2.102.0: on a TTY it emits `ESC]11;?` + `ESC[6n`, and with
# NO_COLOR set it emits neither. Both are `${VAR:-default}`, so they can be
# overridden, but note that gh treats *any* set NO_COLOR as "no color" -- the
# only way to get gh's colors (and the probe, and the leak) back is to keep
# NO_COLOR unset entirely.
export NO_COLOR=${NO_COLOR:-1}
export GH_SPINNER_DISABLED=${GH_SPINNER_DISABLED:-1}

# Temporary file to store release notes
RELEASE_NOTES_FILE=$(mktemp)
trap 'rm -f "$RELEASE_NOTES_FILE"' EXIT

publish_homebrew_tap() {
	local version=$1
	local x86_64_macos_sha=${2:-}
	local aarch64_macos_sha=${3:-}
	local aarch64_linux_sha=${4:-}
	local x86_64_linux_sha=${5:-}
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
	if [[ -z "$x86_64_linux_sha" ]]; then
		x86_64_linux_sha=$(cat "dist/vtcode-$version-x86_64-unknown-linux-gnu.sha256" 2>/dev/null | awk '{print $1}' || echo "")
	fi

	# Last resort: download .sha256 files from the GitHub release
	if [[ -z "$x86_64_macos_sha" || -z "$aarch64_macos_sha" || -z "$aarch64_linux_sha" || -z "$x86_64_linux_sha" ]]; then
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
			if [[ -z "$x86_64_linux_sha" ]]; then
				x86_64_linux_sha=$(cat "$sha_tmp/vtcode-$version-x86_64-unknown-linux-gnu.sha256" 2>/dev/null | awk '{print $1}' || echo "")
			fi
		fi
		rm -rf "$sha_tmp"
	fi

	if [[ -z "$x86_64_macos_sha" || -z "$aarch64_macos_sha" || -z "$aarch64_linux_sha" || -z "$x86_64_linux_sha" ]]; then
		print_error "Missing Homebrew target checksums, cannot publish Homebrew tap"
		return 1
	fi

	print_info "Updating local Homebrew formula at $formula_path..."
	if ! update_homebrew_formula_file "$formula_path" "$version" "$x86_64_macos_sha" "$aarch64_macos_sha" "$aarch64_linux_sha" "$x86_64_linux_sha"; then
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
	ensure_github_cli "$dry_run"

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
	regenerate_docs_map "$dry_run"

	# 1. Local sanity check (fast `cargo check`, no codegen/LTO).
	# Skipped under --skip-release: the irreversible version bump already happened,
	# so the pre-bump sanity check has no purpose and Step 4 builds the real
	# artifacts anyway.
	# NOTE: this deliberately does NOT build binaries. Step 1 used to run the
	# full `build-and-upload-binaries.sh --only-build-local` (two fat-LTO
	# macOS release builds), then Step 4 rebuilt both architectures again after
	# the version bump invalidated the cache — the Step 1 output was always
	# thrown away. A release-profile `cargo check` validates the same
	# workspace (features, targets, lockfile) in ~1-2 min without codegen.
	if [[ "$skip_binaries" == 'false' && "$skip_release" == 'false' ]]; then
		if [[ "$dry_run" == 'true' ]]; then
			print_info "Step 1 (dry-run): Would run cargo check for x86_64-apple-darwin and aarch64-apple-darwin"
		else
			print_info "Step 1: Local sanity check (macOS: both architectures)..."
			env CARGO_BUILD_RUSTC_WRAPPER= RUSTC_WRAPPER= cargo check --locked --profile release \
				--target x86_64-apple-darwin --target aarch64-apple-darwin --jobs 4
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
		trigger_ci_builds "$released_version" "$dry_run"
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
		# include it in every platform archive. Runs in the background
		# alongside the macOS builds: `cargo run -p xtask` compiles the dev
		# profile (minutes) while the release builds run fat LTO — overlapping
		# hides the man-page compile inside the longer release compile.
		# `--jobs 2` leaves cores for the two `--jobs 4` release builds.
		local man_stage="$binaries_dir/man/man1"
		cargo run --locked --jobs 2 -p xtask -- gen-man --out-dir "$man_stage" &>/dev/null &
		local pid_man=$!

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
		# cargo-auditable embeds the dependency list into the binary (.dep-v0
		# section) so shipped archives are scannable with `cargo audit bin`.
		# Optional: without it, builds proceed unchanged and the artifacts just
		# lack self-description.
		local -a cargo_cmd=(build)
		if command -v cargo-auditable > /dev/null 2>&1; then
			cargo_cmd=(auditable build)
			print_info "cargo-auditable detected: binaries will embed dependency audit data"
		else
			print_warning "cargo-auditable not installed; release binaries will not embed dependency audit data (cargo install cargo-auditable --locked)"
		fi
		env CARGO_TARGET_X86_64_APPLE_DARWIN_RUSTFLAGS="-C link-arg=-Wl,-dead_strip" \
			cargo "${cargo_cmd[@]}" --locked --profile release --target x86_64-apple-darwin --jobs 4 &>/dev/null &
		local pid_x86=$!
		env CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS="-C link-arg=-Wl,-dead_strip" \
			cargo "${cargo_cmd[@]}" --locked --profile release --target aarch64-apple-darwin --jobs 4 &>/dev/null &
		local pid_arm=$!

		# Man pages must be ready before packaging (every archive embeds
		# man/). This only blocks if gen-man outlasts the builds; the
		# release compiles keep running concurrently.
		if wait "$pid_man" 2>/dev/null; then
			print_info "Generated $(find "$man_stage" -maxdepth 1 -type f 2>/dev/null | wc -l | tr -d ' ') man pages"
		else
			print_warning "Man page generation failed; archives will ship without man pages"
		fi

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

		# Build the macOS desktop app bundle and stage its distribution zip.
		# Sequential after the arch CLI builds: packaging reuses their cargo
		# cache via CARGO_BUILD_TARGET but compiles the GUI crate fresh, and a
		# third concurrent release build would thrash the shared target dir.
		# The zip + sha256 land in $binaries_dir where checksums.txt and the
		# upload queue pick them up automatically.
		print_info "Building macOS desktop app..."
		local require_desktop="${RELEASE_REQUIRE_DESKTOP:-true}"
		if build_macos_desktop_archive "$released_version" "$binaries_dir"; then
			print_success "Built macOS desktop app"
		elif [[ "$require_desktop" == "true" ]]; then
			print_error "Desktop app build failed; aborting release (set RELEASE_REQUIRE_DESKTOP=false to ship CLI-only)"
			exit 1
		else
			print_warning "Desktop app build failed; continuing without it (RELEASE_REQUIRE_DESKTOP=false)"
		fi

		# Download Linux and Windows binaries from CI artifacts
		print_info "Downloading Linux and Windows binaries from CI..."

		# Use the run_id from step 3.5 if CI was successful, otherwise try to find one
		local run_id="${CI_RUN_ID:-}"

		if [[ -z "$run_id" ]]; then
			# Newest successful run that actually built THIS version. Falling
			# back to the plain newest successful run would select the previous
			# release's run while this version's CI is still queued/building;
			# its artifacts are named for the old version, so every download
			# misses and the release aborts. Match on the version embedded in
			# the artifact names instead.
			print_info "Looking for a successful CI run that built $released_version..."
			run_id=$(find_ci_run_for_version "$released_version" 20 || true)
		fi

		if [[ -z "$run_id" ]]; then
			# No success yet — the typical `--skip-release` resume while CI
			# still builds. Wait for the newest active run instead of
			# aborting into a macOS-only release.
			# gh 2.102.0 quirk: server-side `--branch`/`--event` filters
			# return `[]` with `--json` (see find_ci_run_for_version), so
			# filter locally in jq instead.
			local active_run=""
			active_run=$(gh run list --workflow build-linux-windows.yml --limit 5 --json databaseId,status,headBranch,event --jq '.[] | select((.status == "in_progress" or .status == "queued") and .headBranch == "main" and .event == "workflow_dispatch") | .databaseId' 2>/dev/null | head -1 || true)
			if [[ -n "$active_run" ]]; then
				print_info "CI run $active_run still in progress; waiting for completion..."
				# `gh run watch` blocks until the run completes and exits
				# nonzero unless the conclusion is success. CI jobs carry
				# 45m timeouts, so a stalled run is bounded server-side. The
				# active run is not guaranteed to be this version's (a retry of
				# a previous version could still be building), so confirm it
				# published this version's artifacts before adopting it.
				if gh run watch "$active_run" --exit-status >/dev/null 2>&1; then
					if ci_run_has_version_artifact "$active_run" "$released_version"; then
						print_success "CI builds completed successfully"
						run_id="$active_run"
					else
						print_warning "CI run $active_run did not build $released_version - will use macOS binaries only"
					fi
				else
					print_warning "CI run $active_run did not succeed - will use macOS binaries only"
				fi
			fi
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
			if download_ci_artifact_with_retry "$run_id" "vtcode-${released_version}-x86_64-unknown-linux-gnu" "$ci_artifacts_dir"; then
				mv "$ci_artifacts_dir"/*.tar.gz "$binaries_dir/" 2>/dev/null || true
				mv "$ci_artifacts_dir"/*.sha256 "$binaries_dir/" 2>/dev/null || true
				print_success "Downloaded: Linux x86_64 gnu"
			else
				print_warning "Could not download: Linux x86_64 gnu (run $run_id)"
			fi

			# Download Linux x86_64 musl artifact
			print_info "Downloading Linux x86_64 musl artifact..."
			if download_ci_artifact_with_retry "$run_id" "vtcode-${released_version}-x86_64-unknown-linux-musl" "$ci_artifacts_dir"; then
				mv "$ci_artifacts_dir"/*.tar.gz "$binaries_dir/" 2>/dev/null || true
				mv "$ci_artifacts_dir"/*.sha256 "$binaries_dir/" 2>/dev/null || true
				print_success "Downloaded: Linux x86_64 musl"
			else
				print_warning "Could not download: Linux x86_64 musl (run $run_id)"
			fi

			# Download Linux aarch64 artifact
			print_info "Downloading Linux aarch64 artifact..."
			if download_ci_artifact_with_retry "$run_id" "vtcode-${released_version}-aarch64-unknown-linux-gnu" "$ci_artifacts_dir"; then
				mv "$ci_artifacts_dir"/*.tar.gz "$binaries_dir/" 2>/dev/null || true
				mv "$ci_artifacts_dir"/*.sha256 "$binaries_dir/" 2>/dev/null || true
				print_success "Downloaded: Linux aarch64"
			else
				print_warning "Could not download: Linux aarch64 (run $run_id)"
			fi

			# Download Windows x86_64 artifact. Required for the bridge; the
			# required-target coverage check below fails the release if missing.
			if [[ "$require_windows" == "true" ]]; then
				print_info "Downloading Windows x86_64 artifact..."
				if download_ci_artifact_with_retry "$run_id" "vtcode-${released_version}-x86_64-pc-windows-msvc" "$ci_artifacts_dir"; then
					mv "$ci_artifacts_dir"/*.zip "$binaries_dir/" 2>/dev/null || true
					mv "$ci_artifacts_dir"/*.sha256 "$binaries_dir/" 2>/dev/null || true
					print_success "Downloaded: Windows x86_64"
				else
					print_warning "Could not download: Windows x86_64 (run $run_id)"
				fi
			else
				print_info "Skipping Windows artifact download (RELEASE_REQUIRE_WINDOWS=false)"
			fi

			rm -rf "$ci_artifacts_dir"
		else
			print_warning "No CI workflow run found - will use macOS binaries only"
			# Surface what actually exists so a silent filter/API failure is
			# diagnosable: the newest runs of this workflow with their
			# conclusions (no --branch/--event filters — see the gh quirk
			# documented in release-assets.sh find_ci_run_for_version).
			gh run list --workflow build-linux-windows.yml --limit 5 \
				--json databaseId,status,conclusion,createdAt,headBranch,event \
				--jq '.[] | "  run \(.databaseId): status=\(.status) conclusion=\(.conclusion) branch=\(.headBranch) event=\(.event)"' \
				2>/dev/null || true
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
			print_info "CI run used: $run_id — verify with: gh run view $run_id --json jobs --jq '.jobs[] | \"\\(.name): \\(.conclusion)\"'"
			print_info "If the workflow succeeded, re-run with --skip-release to retry the download (transient gh failures are retried 5x)."
			print_info "Otherwise ensure the build-linux-windows workflow succeeded and macOS builds completed."
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
		print_info "Generating legacy updater compatibility assets (parallel)..."
		local -a compat_assets=()
		# Each asset derives from a distinct archive into a distinct output,
		# so all six generate concurrently. Failures are collected via status
		# files (background jobs cannot `exit 1` the parent) and abort below,
		# preserving the previous fail-fast behavior.
		local compat_status_dir
		compat_status_dir=$(mktemp -d)
		local -a compat_pids=()
		for item in "${required_targets[@]}"; do
			local ctarget="${item%%:*}"
			local cext="${item##*:}"
			local normal_archive="$binaries_dir/vtcode-${released_version}-${ctarget}.${cext}"
			local compat_path
			if ! compat_path=$(compatibility_asset_path "$normal_archive" "$binaries_dir"); then
				print_error "Failed to derive compatibility asset path for $normal_archive"
				exit 1
			fi
			compat_assets+=("$compat_path")
			(
				if ! create_compatibility_asset "$normal_archive" "$compat_path"; then
					touch "$compat_status_dir/$(basename "$compat_path").failed"
				elif [[ "$ctarget" == *-apple-darwin ]] && ! verify_macos_release_binary "$compat_path"; then
					touch "$compat_status_dir/$(basename "$compat_path").failed"
				fi
			) &
			compat_pids+=($!)
		done
		local compat_pid
		for compat_pid in "${compat_pids[@]}"; do
			wait "$compat_pid" 2>/dev/null || true
		done
		local compat_failed_file
		for compat_failed_file in "$compat_status_dir"/*.failed; do
			if [[ -e "$compat_failed_file" ]]; then
				print_error "Failed to generate compatibility asset $(basename "$compat_failed_file" .failed)"
				rm -rf "$compat_status_dir"
				exit 1
			fi
		done
		rm -rf "$compat_status_dir"
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
		# prefix is what makes the legacy updater pick them). Uploading compat
		# first is harmless defense-in-depth kept for clarity.
		local upload_failed=0
		# Single parallel upload queue: compatibility assets first, then normal
		# archives, checksums, and install scripts. Upload order does NOT
		# control legacy selection (GitHub re-sorts assets alphabetically by
		# name; the `compat-` prefix is what makes the legacy updater pick
		# them), so all files upload concurrently (4-way) to cut wall time
		# from ~5-10m sequential to ~2-3m. Each asset is retried
		# independently inside `upload_release_asset_with_retry` (transient
		# HTTP 500 on large raw compat binaries), and failures are collected
		# via status files since background jobs cannot set flags directly.
		local -a upload_queue=()
		if [[ ${#compat_assets[@]} -gt 0 ]]; then
			print_info "Queuing ${#compat_assets[@]} compatibility assets (legacy bridge)..."
			upload_queue+=("${compat_assets[@]}")
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
			print_info "Queuing ${#filtered_normal_files[@]} normal archives, checksums, and install scripts..."
			upload_queue+=("${filtered_normal_files[@]}")
		fi

		if [[ ${#upload_queue[@]} -gt 0 ]]; then
			print_info "Uploading ${#upload_queue[@]} assets (4 parallel)..."
			local upload_status_dir
			upload_status_dir=$(mktemp -d)
			local -a upload_pids=()
			local upload_file
			for upload_file in "${upload_queue[@]}"; do
				(
					upload_release_asset_with_retry "$released_version" "$upload_file" ||
						touch "$upload_status_dir/$(basename "$upload_file").failed"
				) &
				upload_pids+=($!)
				while [[ ${#upload_pids[@]} -ge 4 ]]; do
					wait "${upload_pids[0]}" 2>/dev/null || true
					upload_pids=("${upload_pids[@]:1}")
				done
			done
			local upload_pid
			for upload_pid in "${upload_pids[@]}"; do
				wait "$upload_pid" 2>/dev/null || true
			done
			local upload_failed_file
			for upload_failed_file in "$upload_status_dir"/*.failed; do
				if [[ -e "$upload_failed_file" ]]; then
					print_error "Failed to upload $(basename "$upload_failed_file" .failed) to GitHub Release"
					upload_failed=1
				fi
			done
			rm -rf "$upload_status_dir"
			if [[ "$upload_failed" -ne 0 ]]; then
				print_error "Failed to upload binaries to GitHub Release"
			fi
		fi

		if [[ "$upload_failed" -ne 0 ]]; then
			exit 1
		fi
		print_success "All binaries, compatibility assets, checksums.txt, and install scripts uploaded successfully"

		# Point the release notes at the desktop app (idempotent on re-runs;
		# skipped when no desktop archive was staged).
		shopt -s nullglob
		local -a desktop_zips=("$binaries_dir"/vtcode-desktop-*.zip)
		shopt -u nullglob
		if [[ ${#desktop_zips[@]} -eq 1 ]]; then
			# _vtcode_macos_release_signing_mode is set by the sourced signing lib.
			# shellcheck disable=SC2154
			if ! append_desktop_release_notes "$released_version" "$(basename "${desktop_zips[0]}")" "$_vtcode_macos_release_signing_mode"; then
				print_warning "Could not update release notes with the desktop app section"
			fi
		elif [[ ${#desktop_zips[@]} -gt 1 ]]; then
			print_warning "Multiple desktop archives staged; skipping release notes update"
		fi

		# Extract checksums before cleanup for Homebrew formula update
		local release_x86_sha=""
		local release_arm_sha=""
		local release_arm_linux_sha=""
		local release_x86_linux_sha=""
		if [[ -f "$binaries_dir/vtcode-$released_version-x86_64-apple-darwin.sha256" ]]; then
			release_x86_sha=$(awk '{print $1}' "$binaries_dir/vtcode-$released_version-x86_64-apple-darwin.sha256")
		fi
		if [[ -f "$binaries_dir/vtcode-$released_version-aarch64-apple-darwin.sha256" ]]; then
			release_arm_sha=$(awk '{print $1}' "$binaries_dir/vtcode-$released_version-aarch64-apple-darwin.sha256")
		fi
		if [[ -f "$binaries_dir/vtcode-$released_version-aarch64-unknown-linux-gnu.sha256" ]]; then
			release_arm_linux_sha=$(awk '{print $1}' "$binaries_dir/vtcode-$released_version-aarch64-unknown-linux-gnu.sha256")
		fi
		if [[ -f "$binaries_dir/vtcode-$released_version-x86_64-unknown-linux-gnu.sha256" ]]; then
			release_x86_linux_sha=$(awk '{print $1}' "$binaries_dir/vtcode-$released_version-x86_64-unknown-linux-gnu.sha256")
		fi

		# Cleanup
		rm -rf "$binaries_dir"
	fi

	# 5. Publish Homebrew tap
	if [[ "$skip_binaries" == 'false' ]]; then
		print_info "Step 5: Publishing Homebrew formula to vinhnx/homebrew-tap..."
		publish_homebrew_tap "$released_version" "${release_x86_sha:-}" "${release_arm_sha:-}" "${release_arm_linux_sha:-}" "${release_x86_linux_sha:-}"
	fi

	# 6. Handle docs.rs rebuild
	if [[ "$skip_crates" == 'false' && "$skip_docs" == 'false' ]]; then
		trigger_docs_rs_rebuild "$released_version" false
	fi

	print_success "Release process finished for $released_version"
	print_info "Distribution:"
	print_info "  ✓ Cargo (crates.io)"
	print_info "  ✓ GitHub Releases (all platforms: macOS local + Linux/Windows CI)"
	print_info "  ✓ macOS Desktop app (vtcode-desktop-<version>-<target>.zip + .sha256)"
	print_info "  ✓ Homebrew (vinhnx/homebrew-tap/vtcode)"
	print_info ""
	print_info "Cost optimization:"
	print_info "  • macOS binaries: built locally (no CI cost)"
	print_info "  • Linux/Windows binaries: built on GitHub Actions (free for public repo)"
	print_info ""
	# Fixed: previous line 1561 had truncated `nfo` (from `print_info`) -> `nfo: command not found`
	print_info "Tip: Use --ci-only to skip the local macOS build and use CI for Linux/Windows"
}

main "$@"
