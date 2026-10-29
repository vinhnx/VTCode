#!/usr/bin/env bash
# Release changelog helpers: git-cliff generation with a built-in fallback,
# plus the documentation map refresh committed before the version bump.
#
# This file is sourced by `scripts/release.sh`; it intentionally does not set
# shell options so the caller controls `set -euo pipefail`.

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

# Regenerate the documentation map and sync embedded assets, committing the
# result so the release tag includes an up-to-date map.
#
#   regenerate_docs_map <dry-run>
regenerate_docs_map() {
	local dry_run=$1
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
}
