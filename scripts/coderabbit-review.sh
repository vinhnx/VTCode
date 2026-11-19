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
FORCE=false
# Hourly throttle: CodeRabbit bills pushes, not commits, and a gated review always
# covers the full selected diff. One review per hour per diff is enough for a
# self-verification loop and keeps a 3-8/hour allowance from being burned by
# repeated local runs on the same tree.
THROTTLE_SECONDS=${CODERABBIT_THROTTLE_SECONDS:-3600}
# Grace period: findings are reported but never fail the run until the deadline
# passes. ISO-8601 UTC (e.g. 2026-12-01T00:00:00Z). Empty means enforce now; set a
# future date to adopt the review without blocking work.
GRACE_UNTIL=${CODERABBIT_GRACE_UNTIL:-}
# Where the throttle and last outcome are recorded. Local and untracked, so it
# never becomes repository state.
STATE_DIR=${CODERABBIT_STATE_DIR:-.git/coderabbit-review}

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
	echo "  --force             Ignore the hourly throttle and review this diff again"
	echo "  --help, -h          Show this help message"
	echo ""
	echo "Environment:"
	echo "  CODERABBIT_GATE_SEVERITY   Default gate severity when --gate is absent"
	echo "  CODERABBIT_REVIEW_TIMEOUT  Seconds to wait before aborting (default: 1800)"
	echo "  CODERABBIT_THROTTLE_SECONDS  Reuse a verdict for the same diff within this"
	echo "                               window (default: 3600; 0 disables the throttle)"
	echo "  CODERABBIT_GRACE_UNTIL     ISO-8601 UTC deadline (e.g. 2026-12-01T00:00:00Z)."
	echo "                             Findings warn instead of failing until then;"
	echo "                             unset enforces immediately."
	echo ""
	echo "Exit codes:"
	echo "  0  gate passed (or no changes to review, or grace period downgraded a block)"
	echo "  1  review incomplete or unavailable"
	echo "  2  gate blocked by a finding at or above the threshold"
	echo ""
	echo "Note: Free plan allows 3 CLI reviews/hour. Gated runs review the full diff,"
	echo "so the throttle keys on the diff and reuses a verdict for the same tree."
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
		--force)
			FORCE=true
			shift
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

	# The throttle key must include the diff itself. Keying on time alone would
	# serve a stale "passed" for code that changed since the last review.
	local key
	key=$(diff_key) || {
		print_warning "Could not fingerprint the diff; running without the throttle."
		FORCE=true
		key="nokey"
	}
	local last_status=0

	if [ "$FORCE" != true ] && within_throttle_window "$key"; then
		last_status=$(cached_status "$key")
		print_status "CodeRabbit already reviewed this exact diff under an hour ago (cached: exit ${last_status})."
		echo "Re-run with --force to review again, or wait ${THROTTLE_SECONDS}s."
		apply_grace "$last_status" "cached"
		exit $?
	fi

	print_status "Running CodeRabbit review (gate: ${GATE_SEVERITY}, timeout: ${timeout_seconds}s)..."
	# Findings stream to stdout as NDJSON; keep them visible for the agent loop.
	local status=0
	run_with_timeout "$timeout_seconds" coderabbit review --agent --gate "$GATE_SEVERITY" "${args[@]}" || status=$?

	record_review "$key" "$status"
	apply_grace "$status" "fresh"
}

# Fingerprint of everything the review will look at, so the throttle can never
# serve a verdict for a diff that has since changed. Covers the selected scope
# plus the gate severity, because a stricter gate is a different question.
diff_key() {
	local payload=""
	case "$REVIEW_SCOPE" in
	uncommitted) payload=$(
		git status --porcelain 2>/dev/null
		git diff 2>/dev/null
	) ;;
	committed)
		local base
		base=$(resolve_base_ref) || return 1
		payload=$(git diff "${base}...HEAD" 2>/dev/null)
		;;
	*) payload=$(
		git status --porcelain 2>/dev/null
		git diff 2>/dev/null
	) ;;
	esac
	printf '%s\n%s\n%s\n' "$GATE_SEVERITY" "$REVIEW_SCOPE" "$payload" |
		git hash-object --stdin 2>/dev/null
}

state_file() {
	printf '%s\n' "${STATE_DIR}/${1}"
}

# True when this exact diff was reviewed inside the throttle window.
within_throttle_window() {
	local key=$1
	local stamp_file
	stamp_file=$(state_file "last-run")
	[ -f "$stamp_file" ] || return 1
	local recorded_key recorded_at now
	recorded_key=$(sed -n '1p' "$stamp_file" 2>/dev/null)
	recorded_at=$(sed -n '2p' "$stamp_file" 2>/dev/null)
	[ "$recorded_key" = "$key" ] || return 1
	case "$recorded_at" in
	'' | *[!0-9]*) return 1 ;;
	esac
	now=$(date +%s)
	[ $((now - recorded_at)) -lt "$THROTTLE_SECONDS" ]
}

cached_status() {
	local key=$1
	local stamp_file
	stamp_file=$(state_file "last-run")
	local recorded_key recorded_status
	recorded_key=$(sed -n '1p' "$stamp_file" 2>/dev/null)
	recorded_status=$(sed -n '3p' "$stamp_file" 2>/dev/null)
	if [ "$recorded_key" = "$key" ]; then
		case "$recorded_status" in
		'' | *[!0-9]*) printf '1\n' ;;
		*) printf '%s\n' "$recorded_status" ;;
		esac
	else
		printf '1\n'
	fi
}

record_review() {
	local key=$1 status=$2
	mkdir -p "$STATE_DIR" 2>/dev/null || return 0
	printf '%s\n%s\n%s\n' "$key" "$(date +%s)" "$status" >"$(state_file "last-run")" 2>/dev/null || true
}

# True while findings must not fail the run. The deadline is the single source of
# truth: an empty value means already enforcing, not "warn forever". A grace
# period that cannot expire would silently disable the gate, which is the failure
# mode most likely to go unnoticed.
within_grace_period() {
	[ -n "$GRACE_UNTIL" ] || return 1
	local now
	now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
	[[ "$now" < "$GRACE_UNTIL" ]]
}

# Translate the review exit status into this script's exit status, downgrading a
# blocking result to a warning while the grace period is active. "Incomplete"
# (1) is never downgraded: an unevaluated review is not evidence of cleanliness.
apply_grace() {
	local status=$1 origin=$2

	case "$status" in
	0)
		if [ "$origin" = "cached" ]; then
			print_success "Cached CodeRabbit gate passed."
		else
			print_success "CodeRabbit gate passed."
			echo ""
			echo "Findings are advisory. Run 'coderabbit review findings' to revisit them."
		fi
		return 0
		;;
	1)
		print_error "CodeRabbit review was incomplete or could not be evaluated."
		echo "Retry, or narrow the scope with --dir / --committed / --base."
		return 1
		;;
	2)
		if within_grace_period; then
			print_warning "CodeRabbit found issues at or above '${GATE_SEVERITY}', but the grace period is active."
			echo "Treating this as a warning. Enforcement begins ${GRACE_UNTIL}."
			echo "Clear CODERABBIT_GRACE_UNTIL to enforce now."
			return 0
		fi
		print_error "CodeRabbit gate blocked: finding at or above '${GATE_SEVERITY}'."
		echo "Address the findings above, then run this script again."
		return 2
		;;
	124)
		print_error "CodeRabbit review timed out after ${CODERABBIT_REVIEW_TIMEOUT:-1800}s."
		echo "Raise CODERABBIT_REVIEW_TIMEOUT or narrow the scope."
		return 1
		;;
	*)
		print_error "CodeRabbit review failed with exit code ${status}."
		return 1
		;;
	esac
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
	main "$@"
fi
