#!/usr/bin/env bash
# Homebrew tap publication for the CLI formula and the desktop cask.
#
# This file is sourced by `scripts/release.sh`; it intentionally does not set
# shell options so the caller controls `set -euo pipefail`. Formula/cask byte
# transformations live in `scripts/homebrew-formula.sh`; this module owns only
# checksum lookups, local commits, and tap publication.

publish_homebrew_tap() {
	local version=$1
	local x86_64_macos_sha=${2:-}
	local aarch64_macos_sha=${3:-}
	local aarch64_linux_sha=${4:-}
	local x86_64_linux_sha=${5:-}
	local aarch64_desktop_sha=${6:-}
	local formula_path="homebrew/vtcode.rb"
	local cask_path="homebrew/vtcode-desktop.rb"

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
	if [[ -z "$aarch64_desktop_sha" ]]; then
		aarch64_desktop_sha=$(cat "dist/vtcode-desktop-$version-aarch64-apple-darwin.zip.sha256" 2>/dev/null | awk '{print $1}' || echo "")
	fi

	# Last resort: download .sha256 files from the GitHub release. The desktop
	# digest rides along with the same download, so a resumed release whose
	# `dist/` was already cleaned still refreshes the cask. A release that ships
	# no desktop archive leaves it empty and skips the cask below.
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
			if [[ -z "$aarch64_desktop_sha" ]]; then
				aarch64_desktop_sha=$(cat "$sha_tmp/vtcode-desktop-$version-aarch64-apple-darwin.zip.sha256" 2>/dev/null | awk '{print $1}' || echo "")
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

	local formula_changed=0
	if git diff --quiet -- "$formula_path"; then
		print_info "Local Homebrew formula is already up to date"
	else
		formula_changed=1
	fi

	# The desktop cask is best-effort: releases can ship CLI-only, and Intel
	# hosts build no aarch64 desktop archive.
	local cask_ready=0
	local cask_changed=0
	if [[ -n "$aarch64_desktop_sha" && -f "$cask_path" ]]; then
		print_info "Updating local Homebrew cask at $cask_path..."
		if ! update_homebrew_cask_file "$cask_path" "$version" "$aarch64_desktop_sha"; then
			print_error "Failed to update local Homebrew cask"
			return 1
		fi
		cask_ready=1
		if git diff --quiet -- "$cask_path"; then
			print_info "Local Homebrew cask is already up to date"
		else
			cask_changed=1
		fi
	elif [[ -n "$aarch64_desktop_sha" ]]; then
		print_warning "Homebrew cask not found at $cask_path; skipping cask update"
	else
		print_warning "No Apple Silicon desktop checksum available; skipping Homebrew cask update"
	fi

	local -a homebrew_paths=()
	if ((formula_changed)); then
		homebrew_paths+=("$formula_path")
	fi
	if ((cask_changed)); then
		homebrew_paths+=("$cask_path")
	fi

	if ((${#homebrew_paths[@]} == 0)); then
		print_info "Local Homebrew formula and cask are already up to date"
	else
		git add "${homebrew_paths[@]}"
		if GIT_AUTHOR_NAME="vtcode-release-bot" \
			GIT_AUTHOR_EMAIL="noreply@vtcode.com" \
			GIT_COMMITTER_NAME="vtcode-release-bot" \
			GIT_COMMITTER_EMAIL="noreply@vtcode.com" \
			git commit -m "chore: update homebrew formula and cask to $version [skip ci]"; then
			print_success "Local Homebrew formula and cask updated and committed"
			if git push origin main --no-verify; then
				print_success "Homebrew formula and cask commit pushed to origin"
			else
				print_warning "Failed to push Homebrew formula and cask commit to origin"
			fi
		else
			print_warning "Failed to commit local Homebrew formula and cask update"
		fi
	fi

	print_info "Publishing Homebrew formula and cask to vinhnx/homebrew-tap..."

	local temp_dir
	temp_dir=$(mktemp -d 2>/dev/null || mktemp -d -t 'vtcode-homebrew')

	if ! (
		# Bash cannot read a function-local from an EXIT trap when the
		# subshell exits early, so hand the path to a plain shell variable.
		_homebrew_cleanup_dir="$temp_dir"
		trap 'rm -rf "$_homebrew_cleanup_dir"' EXIT

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

		local -a tap_paths=(vtcode.rb)
		cp "$formula_path" "$temp_dir/vtcode.rb"
		if ((cask_ready)); then
			mkdir -p "$temp_dir/Casks"
			cp "$cask_path" "$temp_dir/Casks/vtcode-desktop.rb"
			tap_paths+=(Casks/vtcode-desktop.rb)
		fi

		git -C "$temp_dir" add -- "${tap_paths[@]}"

		if git -C "$temp_dir" diff --cached --quiet; then
			print_info "Homebrew tap formula and cask are already up to date"
			exit 0
		fi

		if ! GIT_AUTHOR_NAME="vtcode-release-bot" \
			GIT_AUTHOR_EMAIL="noreply@vtcode.com" \
			GIT_COMMITTER_NAME="vtcode-release-bot" \
			GIT_COMMITTER_EMAIL="noreply@vtcode.com" \
			git -C "$temp_dir" commit -m "Update vtcode Homebrew packages to v$version"; then
			print_error "Failed to commit Homebrew tap update"
			exit 1
		fi

		if ! git -C "$temp_dir" push "$tap_repo_url" HEAD:main; then
			print_error "Failed to push Homebrew tap update"
			exit 1
		fi

		print_success "Published vtcode formula and desktop cask to vinhnx/homebrew-tap"
	); then
		return 1
	fi
}
