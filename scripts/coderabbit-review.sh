#!/usr/bin/env bash
#
# Opt-in AI review of the current diff via the CodeRabbit CLI.
#
# This is NOT part of the default `check-dev.sh` gate. That gate is a 10-30s
# deterministic, hermetic check; a CodeRabbit review is a 15-minute,
# network-bound, LLM-nondeterministic call with a small hourly allowance
# (Free plan: 3 CLI reviews/hour). Keeping it opt-in keeps CI and offline work
# unaffected.
#
# Exit codes follow `check-dev.sh` conventions (0 pass / 1 fail) except that the
# gate outcome is also distinguished:
#   0  gate passed, or the review was skipped because the diff is empty
#   1  review incomplete (failed, missed files, or reused an earlier review)
#   2  gate blocked: a finding at or above the threshold
#
# Gated reviews always run fresh over the whole selected diff, so they consume
# an allowance unit and take longer than an incremental rerun.

set -eu

# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

GATE_SEVERITY=${CODERABBIT_GATE_SEVERITY:-major}
REVIEW_SCOPE=""
BASE_REF=""

# Bound the call so a stalled network review cannot hang the agent loop. macOS
# bash 3.2 has no builtin timeout, so fall back to GNU timeout/gtimeout when
# present and run unbounded (documented) when neither exists.
run_with_timeout() {
	local seconds=$1
	shift
	if command -v timeout >/dev/null 2>&1; then
		timeout "$seconds" "$@"
	elif command -v gtimeout >/dev/null 2>&1; then
		gtimeout "$seconds" "$@"
	else
		"$@"
	fi
}

print_usage() {
	echo "CodeRabbit review (opt-in) for VT Code"
	echo ""
	echo "Usage: $0 [OPTIONS]"
	echo ""
	echo "Runs \`coderabbit review --agent --gate <severity>\` over the current diff"
	echo "and translates the outcome into check-dev-style exit codes."
	echo ""
	echo "Options:"
	echo "  --committed         Review only committed changes"
	echo "  --uncommitted       Review only uncommitted changes"
	echo "  --base <branch>     Compare against a base branch (default: repo default)"
	echo "  --gate <severity>   Minimum blocking severity: critical|major|minor|trivial|info"
	echo "                      (default: ${GATE_SEVERITY}, or CODERABBIT_GATE_SEVERITY)"
	echo "  --dir <path>        Review only changes inside a directory"
	echo "  --help, -h          Show this help message"
	echo ""
	echo "Environment:"
	echo "  CODERABBIT_GATE_SEVERITY   Default gate severity when --gate is absent"
	echo "  CODERABBIT_REVIEW_TIMEOUT  Seconds to wait before aborting (default: 1800)"
	echo ""
	echo "Exit codes:"
	echo "  0  gate passed (or no changes to review)"
	echo "  1  review incomplete or unavailable"
	echo "  2  gate blocked by a finding at or above the threshold"
	echo ""
	echo "Note: Free plan allows 3 CLI reviews/hour. Gated runs review the full diff."
}

require_coderabbit() {
	if command -v coderabbit >/dev/null 2>&1; then
		return 0
	fi
	print_error "coderabbit CLI not found on PATH."
	echo "Install: https://docs.coderabbit.ai/cli  (then 'coderabbit auth login')"
	return 1
}

# A gated review of an unchanged tree still costs an allowance unit, so skip
# early when there is nothing to look at. Each scope needs its own test: a
# `--committed` comparison against HEAD would detect *uncommitted* work, which
# is the opposite of that flag's meaning.
#
# Returns 0 ("there is something to review") when the scope cannot be resolved,
# so an unusual checkout defers to the CLI instead of silently skipping.
has_changes() {
	case "$REVIEW_SCOPE" in
	uncommitted) has_uncommitted_changes ;;
	committed) has_committed_changes ;;
	*) has_uncommitted_changes || has_committed_changes ;;
	esac
}

has_uncommitted_changes() {
	if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
		return 0
	fi
	return 1
}

has_committed_changes() {
	local base
	if ! base=$(resolve_base_ref); then
		return 0
	fi
	if ! git rev-parse --verify --quiet "$base" >/dev/null 2>&1; then
		return 0
	fi
	if git diff --quiet "${base}...HEAD" 2>/dev/null; then
		return 1
	fi
	return 0
}

# The branch to compare committed work against. Unresolvable is not an error:
# the caller then defers to the CLI instead of guessing.
resolve_base_ref() {
	if [ -n "$BASE_REF" ]; then
		printf '%s\n' "$BASE_REF"
		return 0
	fi
	if git symbolic-ref -q refs/remotes/origin/HEAD >/dev/null 2>&1; then
		git symbolic-ref -q --short refs/remotes/origin/HEAD
		return 0
	fi
	if git rev-parse --verify --quiet main >/dev/null 2>&1; then
		printf 'main\n'
		return 0
	fi
	return 1
}

main() {
	local timeout_seconds=${CODERABBIT_REVIEW_TIMEOUT:-1800}
	local args=()

	while [[ $# -gt 0 ]]; do
		case "$1" in
		--committed)
			REVIEW_SCOPE="committed"
			args+=("--committed")
			shift
			;;
		--uncommitted)
			REVIEW_SCOPE="uncommitted"
			args+=("--uncommitted")
			shift
			;;
		--base)
			BASE_REF=${2:?--base requires a branch}
			args+=("--base" "$BASE_REF")
			shift 2
			;;
		--gate)
			GATE_SEVERITY=${2:?--gate requires a severity}
			shift 2
			;;
		--dir)
			args+=("--dir" "${2:?--dir requires a path}")
			shift 2
			;;
		--help | -h)
			print_usage
			exit 0
			;;
		*)
			print_error "Unknown argument: $1"
			echo "Run '$0 --help' for usage information."
			exit 1
			;;
		esac
	done

	case "$GATE_SEVERITY" in
	critical | major | minor | trivial | info) ;;
	*)
		print_error "Invalid --gate severity: $GATE_SEVERITY"
		echo "Expected: critical, major, minor, trivial, or info."
		exit 1
		;;
	esac

	require_coderabbit || exit 1

	if ! has_changes; then
		print_success "No changes to review. Skipping CodeRabbit."
		exit 0
	fi

	print_status "Running CodeRabbit review (gate: ${GATE_SEVERITY}, timeout: ${timeout_seconds}s)..."
	# Findings stream to stdout as NDJSON; keep them visible for the agent loop.
	local status=0
	run_with_timeout "$timeout_seconds" coderabbit review --agent --gate "$GATE_SEVERITY" "${args[@]}" || status=$?

	case "$status" in
	0)
		print_success "CodeRabbit gate passed."
		echo ""
		echo "Findings are advisory. Run 'coderabbit review findings' to revisit them."
		exit 0
		;;
	1)
		print_error "CodeRabbit review was incomplete or could not be evaluated."
		echo "Retry, or narrow the scope with --dir / --committed / --base."
		exit 1
		;;
	2)
		print_error "CodeRabbit gate blocked: finding at or above '${GATE_SEVERITY}'."
		echo "Address the findings above, then run this script again."
		exit 2
		;;
	124)
		print_error "CodeRabbit review timed out after ${timeout_seconds}s."
		echo "Raise CODERABBIT_REVIEW_TIMEOUT or narrow the scope."
		exit 1
		;;
	*)
		print_error "CodeRabbit review failed with exit code ${status}."
		exit 1
		;;
	esac
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
	main "$@"
fi
