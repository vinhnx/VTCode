#!/usr/bin/env bash
# Shared changelog classification and insertion for canonical and legacy adapters.
# Sourcing this file only defines functions.

parse_commit_type() {
	local message="$1"
	# Extract type from conventional commit format: type(scope): message or type: message
	# Use sed to extract the type prefix
	local type
	type=$(echo "$message" | sed -E 's/^([a-z]+)(\([^)]+\))?:.*/\1/')
	if [[ "$type" == "$message" ]]; then
		echo "other"
	else
		echo "$type"
	fi
}

# Return success for release bookkeeping and owner-only tracker commit subjects.
release_commit_is_excluded() {
	local message="$1"
	local lower_msg
	lower_msg=$(echo "$message" | tr '[:upper:]' '[:lower:]')
	[[ "$lower_msg" =~ (chore\(release\):|bump version|update version|version bump|release v[0-9]+\.[0-9]+\.[0-9]+|chore.*version|chore.*release|build.*version|update.*version.*number|bump.*version.*to|update homebrew|update changelog|update.*todo|docs\(todo\)|docs\(project\).*todo|^update project$) ]]
}

# Insert a generated changelog entry above the newest version so the file stays
# newest-first. A fixed anchor (e.g. `head -n 4`) is wrong: once the first entry is
# present that line is itself a `## version` heading, so the next insert lands between
# the previous heading and its body, orphaning the body under the new version.
insert_changelog_entry() {
	local entry=$1
	local tmp
	tmp=$(mktemp)

	local first_version_line
	first_version_line=$(grep -n '^## ' CHANGELOG.md | head -n1 | cut -d: -f1 || true)

	if [[ -z "$first_version_line" ]]; then
		cat CHANGELOG.md >"$tmp"
	elif [[ "$first_version_line" -gt 1 ]]; then
		head -n "$((first_version_line - 1))" CHANGELOG.md >"$tmp"
	fi
	# A version on line 1 has no prefix; macOS head rejects a zero line count.

	# Blank separator before the new entry (avoid doubling an existing trailing blank).
	if [[ -n "$(tail -n1 "$tmp")" ]]; then
		printf '\n' >>"$tmp"
	fi
	printf '%s\n' "$entry" >>"$tmp"

	if [[ -n "$first_version_line" ]]; then
		tail -n "+$first_version_line" CHANGELOG.md >>"$tmp"
	fi

	mv "$tmp" CHANGELOG.md
}

# Remove an existing `## <version> ...` section (up to the next `## ` heading or
# EOF) so a re-run replaces stale content instead of skipping. This handles
# aborted releases that already inserted a changelog entry for the same version
# (e.g. stale 0.174.0 left behind by an earlier `--minor` run that was later
# released as 0.173.1): without removal the next run sees
# `grep -q "^## $version "` and skips, leaving the file out of order.
remove_changelog_version_section() {
	local version=$1
	[[ -f CHANGELOG.md ]] || return 0
	# Escape dots: the version is interpolated into a regex, where `.`
	# would otherwise match any character.
	grep -q "^## ${version//./\\.} " CHANGELOG.md || return 0
	local tmp
	tmp=$(mktemp)
	awk -v ver="$version" '
		/^## / {
			if ($2 == ver) { skip=1; next }
			else { skip=0 }
		}
		!skip { print }
	' CHANGELOG.md >"$tmp"
	mv "$tmp" CHANGELOG.md
}
