#!/usr/bin/env bash
# Canonical release-note formatting. Sourcing this file performs no release work.
# shellcheck disable=SC1091
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/release-changelog-common.sh"

get_github_username() {
	local email=$1
	# Common email-to-username mappings
	case "$email" in
	vinhnguyen*) echo "vinhnx" ;;
	noreply@vtcode.com) echo "vtcode-release-bot" ;;
	*@users.noreply.github.com)
		local username="${email%%@*}"
		# Handle GitHub ID format: 123456+username
		if [[ "$username" == *+* ]]; then
			username="${username##*+}"
		fi
		echo "$username"
		;;
	*)
		# Extract username from email (before @)
		local username="${email%%@*}"
		echo "$username"
		;;
	esac
}

add_username_tags() {
	local changelog=$1
	local commits_range=$2

	# Create a temporary file to store the mapping of commit hashes to usernames
	local temp_mapping_file
	temp_mapping_file=$(mktemp)

	# Populate the mapping - use a subshell to avoid variable scoping issues
	# git's format mode omits the final newline; keep its last partial record.
	(
		git log "$commits_range" --no-merges --pretty=format:"%h|%ae"
	) | while IFS= read -r line || [[ -n "$line" ]]; do
		if [[ -n "$line" ]]; then
			local hash author_email
			hash=$(echo "$line" | cut -d'|' -f1)
			author_email=$(echo "$line" | cut -d'|' -f2)
			local username
			username=$(get_github_username "$author_email")
			echo "$hash|$username"
		fi
	done >"$temp_mapping_file"

	# Process changelog and add @username tags
	local result=""
	while IFS= read -r entry; do
		# Extract commit hash from entry (format: "... (commit_hash)")
		if [[ $entry =~ \(([a-f0-9]+)\)$ ]]; then
			local full_hash="${BASH_REMATCH[1]}"
			# Find username from the temporary file
			local username=""
			local found=0

			while IFS= read -r mapping_line; do
				if [[ -n "$mapping_line" && $found -eq 0 ]]; then
					local map_hash map_username
					map_hash=$(echo "$mapping_line" | cut -d'|' -f1)
					map_username=$(echo "$mapping_line" | cut -d'|' -f2)
					# Check if the full hash starts with the map hash (to match short vs full hashes)
					if [[ ${full_hash} == ${map_hash}* || ${map_hash} == ${full_hash}* ]]; then
						username="$map_username"
						found=1
					fi
				fi
			done <"$temp_mapping_file"

			if [[ -n "$username" ]]; then
				# Append @username to the entry if not already present
				if [[ $entry != *"@$username"* ]]; then
					entry="$entry (@$username)"
				fi
			fi
		fi
		result+="$entry"$'\n'
	done <<<"$changelog"

	# Clean up
	rm -f "$temp_mapping_file"

	echo "${result%$'\n'}"
}

get_type_prefix() {
	local type="$1"
	case "$type" in
	feat) echo "[FEAT]" ;;
	fix) echo "[FIX]" ;;
	perf) echo "[PERF]" ;;
	refactor) echo "[REFACTOR]" ;;
	docs) echo "[DOCS]" ;;
	test) echo "[TEST]" ;;
	build) echo "[BUILD]" ;;
	ci) echo "[CI]" ;;
	chore) echo "[CHORE]" ;;
	security) echo "[SECURITY]" ;;
	deps) echo "[DEPS]" ;;
	*) echo "" ;;
	esac
}

get_type_title() {
	local type="$1"
	case "$type" in
	feat) echo "Features" ;;
	fix) echo "Bug Fixes" ;;
	perf) echo "Performance" ;;
	refactor) echo "Refactors" ;;
	docs) echo "Documentation" ;;
	test) echo "Tests" ;;
	build) echo "Build" ;;
	ci) echo "CI" ;;
	chore) echo "Chores" ;;
	security) echo "Security" ;;
	deps) echo "Dependencies" ;;
	*) echo "Other" ;;
	esac
}

clean_commit_message() {
	local message="$1"
	# Remove conventional commit prefix (type(scope): or type:)
	echo "$message" | sed -E 's/^[a-z]+(\([^)]+\))?:[[:space:]]*//'
}

generate_contributors_section() {
	local commits_range="$1"
	local contributors=""
	local seen_usernames=""

	# Keep the oldest contributor when git omits the final newline.
	while IFS= read -r author_email || [[ -n "$author_email" ]]; do
		[[ -z "$author_email" ]] && continue
		local username
		username=$(get_github_username "$author_email")
		[[ -z "$username" || "$username" == "vtcode-release-bot" ]] && continue

		# Deduplicate (bash 3.2 compatible)
		if [[ "$seen_usernames" != *"|${username}|"* ]]; then
			seen_usernames="${seen_usernames}|${username}|"
			if [[ -n "$contributors" ]]; then
				contributors="${contributors}, @${username}"
			else
				contributors="@${username}"
			fi
		fi
	done < <(git log "$commits_range" --no-merges --pretty=format:"%ae")

	if [[ -n "$contributors" ]]; then
		echo "### Contributors"$'\n'
		echo "$contributors"
	fi
}

generate_structured_changelog() {
	local commits_range="$1"

	# Highlight types shown at the top; everything else goes under "Other Changes"
	local highlight_types="feat fix docs"
	local other_types="perf refactor security test build ci deps chore other"

	# Initialize storage for each type (using prefix variables instead of associative arrays)
	local feat_commits=""
	local fix_commits=""
	local perf_commits=""
	local refactor_commits=""
	local security_commits=""
	local docs_commits=""
	local test_commits=""
	local build_commits=""
	local ci_commits=""
	local deps_commits=""
	local chore_commits=""
	local other_commits=""

	# Get commits with their hashes, subjects, and author emails in a single git log call
	# A single-commit range must still be processed without a trailing newline.
	while IFS='|' read -r hash message author_email || [[ -n "$hash" ]]; do
		[[ -z "$hash" ]] && continue

		local type
		type=$(parse_commit_type "$message")
		local clean_msg
		clean_msg=$(clean_commit_message "$message")

		if release_commit_is_excluded "$message"; then
			continue
		fi

		local username=""
		if [[ -n "$author_email" ]]; then
			username=$(get_github_username "$author_email")
		fi

		# Build entry
		local entry="- $clean_msg ($hash)"
		if [[ -n "$username" && "$username" != "vtcode-release-bot" ]]; then
			entry="$entry (@$username)"
		fi

		# Add to appropriate group using prefix variables
		case "$type" in
		feat) feat_commits="${feat_commits}${entry}"$'\n' ;;
		fix) fix_commits="${fix_commits}${entry}"$'\n' ;;
		perf) perf_commits="${perf_commits}${entry}"$'\n' ;;
		refactor) refactor_commits="${refactor_commits}${entry}"$'\n' ;;
		security) security_commits="${security_commits}${entry}"$'\n' ;;
		docs) docs_commits="${docs_commits}${entry}"$'\n' ;;
		test) test_commits="${test_commits}${entry}"$'\n' ;;
		build) build_commits="${build_commits}${entry}"$'\n' ;;
		ci) ci_commits="${ci_commits}${entry}"$'\n' ;;
		deps) deps_commits="${deps_commits}${entry}"$'\n' ;;
		chore) chore_commits="${chore_commits}${entry}"$'\n' ;;
		*) other_commits="${other_commits}${entry}"$'\n' ;;
		esac
	done < <(git log "$commits_range" --no-merges --pretty=format:"%h|%s|%ae")

	# --- Build output with Highlights / Other Changes split ---
	local output=""
	local has_highlights=false
	local has_other=false

	# Highlights section (Features, Bug Fixes, Documentation)
	output+="### Highlights"$'\n\n'

	for type in $highlight_types; do
		local commits=""
		case "$type" in
		feat) commits="$feat_commits" ;;
		fix) commits="$fix_commits" ;;
		docs) commits="$docs_commits" ;;
		esac

		if [[ -n "$commits" ]]; then
			local title
			title=$(get_type_title "$type")
			output+="#### ${title}"$'\n\n'
			output+="${commits}"$'\n'
			has_highlights=true
		fi
	done

	if [[ "$has_highlights" == false ]]; then
		output+="*No highlighted changes*"$'\n\n'
	fi

	# Other Changes section (Performance, Refactors, Security, Tests, Build, CI, Deps, Chores, Other)
	local other_output=""

	for type in $other_types; do
		local commits=""
		case "$type" in
		perf) commits="$perf_commits" ;;
		refactor) commits="$refactor_commits" ;;
		security) commits="$security_commits" ;;
		test) commits="$test_commits" ;;
		build) commits="$build_commits" ;;
		ci) commits="$ci_commits" ;;
		deps) commits="$deps_commits" ;;
		chore) commits="$chore_commits" ;;
		other) commits="$other_commits" ;;
		esac

		if [[ -n "$commits" ]]; then
			local title
			title=$(get_type_title "$type")
			other_output+="#### ${title}"$'\n\n'
			other_output+="${commits}"$'\n'
			has_other=true
		fi
	done

	if [[ "$has_other" == true ]]; then
		output+="### Other Changes"$'\n\n'
		output+="${other_output}"
	fi

	# Contributors section
	local contributors_section
	contributors_section=$(generate_contributors_section "$commits_range")
	if [[ -n "$contributors_section" ]]; then
		output+="${contributors_section}"$'\n'
	fi

	if [[ "$has_highlights" == false && "$has_other" == false ]]; then
		output="*No significant changes*"$'\n'
	fi

	echo "$output"
}
