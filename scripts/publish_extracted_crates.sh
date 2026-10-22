#!/usr/bin/env bash
set -euo pipefail

# Source common utilities
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# publish_extracted_crates.sh orchestrates the sequential publishes for the
# extracted VT Code crates. It follows the dependency order required by
# crates.io tarball verification and provides optional dry-run coverage so the
# same script can be used for validation ahead of the real release window.

usage() {
	cat <<'USAGE'
Usage: $0 [--dry-run] [--start-from <crate>] [--skip-tests] [--skip-docs] [--skip-tags] [--skip-follow-up]

Options:
  --dry-run          Use `cargo publish --dry-run` for each crate instead of
                     performing the real publish. This is the default when the
                     VT_RELEASE_DRY_RUN environment variable is set to 1.
  --start-from CRATE Resume publishing from the provided crate name. Valid
                     crates: vtcode-diff, vtcode-exec-events, vtcode-commons,
                     vtcode-auth, vtcode-memory, vtcode-macros, vtcode-config,
                     vtcode-indexer, vtcode-bash-runner, vtcode-utility-tool-specs,
                     vtcode-eval, vtcode-safety, vtcode-webmcp, vtcode-a2a, vtcode-llm,
                     vtcode-skills, vtcode-agent-plugins, vtcode-ui, vtcode-mcp, vtcode-core,
                     vtcode-acp, vtcode.
  --skip-tests       Skip running the workspace fmt/clippy/test checks. Use with
                     caution; the release plan expects the validation suite to
                     pass before publishing.
  --skip-docs        Skip regenerating API docs for each crate prior to
                     publishing.
  --skip-tags        Skip creating per-crate git tags after publish.
  --skip-follow-up   Skip cargo update/check after each publish.
  -h, --help         Show this help message and exit.

Environment variables:
  VT_RELEASE_DRY_RUN When set to 1, the script defaults to performing a dry
                     run. Passing `--dry-run` or providing `--start-from` still
                     works while the variable is set.
  VT_RELEASE_SKIP_DOCS
                     When set to 1, skip regenerating API docs even if
                     `--skip-docs` is not passed.
USAGE
}

DRY_RUN=${VT_RELEASE_DRY_RUN:-0}
START_FROM=""
RUN_TESTS=1
RUN_DOCS=1
RUN_TAGS=1
RUN_FOLLOW_UP=1
CURRENT_VERSION=$(get_current_version)

if [[ ${VT_RELEASE_SKIP_DOCS:-0} -eq 1 ]]; then
	RUN_DOCS=0
fi

while [[ $# -gt 0 ]]; do
	case "$1" in
	--dry-run)
		DRY_RUN=1
		shift
		;;
	--start-from)
		START_FROM="$2"
		shift 2
		;;
	--skip-tests)
		RUN_TESTS=0
		shift
		;;
	--skip-docs)
		RUN_DOCS=0
		shift
		;;
	--skip-tags)
		RUN_TAGS=0
		shift
		;;
	--skip-follow-up)
		RUN_FOLLOW_UP=0
		shift
		;;
	-h | --help)
		usage
		exit 0
		;;
	*)
		echo "Unknown option: $1" >&2
		usage
		exit 1
		;;
	esac
done

# Crates published to crates.io. The order they are published in is derived
# from the workspace dependency graph (see compute_publish_order) instead of
# being hand-maintained here: a hand-written sequence silently rots whenever a
# dependency edge is added, and crates.io then rejects the dependent crate.
PUBLISH_CRATES=(
	vtcode-diff
	vtcode-exec-events
	vtcode-commons
	vtcode-auth
	vtcode-memory
	vtcode-macros
	vtcode-config
	vtcode-indexer
	vtcode-bash-runner
	vtcode-utility-tool-specs
	vtcode-eval
	vtcode-safety
	vtcode-webmcp
	vtcode-a2a
	vtcode-llm
	vtcode-skills
	vtcode-agent-plugins
	vtcode-ui
	vtcode-mcp
	vtcode-core
	vtcode-acp
	vtcode
)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PUBLISH_ORDER_TOOL="${SCRIPT_DIR}/publish_order.py"

# Cached `cargo metadata --no-deps` output; the workspace graph is read many
# times per run and metadata resolution is the expensive part.
WORKSPACE_METADATA_FILE=""

load_workspace_metadata() {
	if [[ -n "$WORKSPACE_METADATA_FILE" && -s "$WORKSPACE_METADATA_FILE" ]]; then
		return 0
	fi
	local metadata_file
	metadata_file=$(mktemp)
	if ! cargo metadata --format-version 1 --no-deps >"$metadata_file" 2>/dev/null; then
		rm -f "$metadata_file"
		print_error "Unable to read the workspace graph via 'cargo metadata'."
		return 1
	fi
	WORKSPACE_METADATA_FILE="$metadata_file"
	trap 'rm -f "$WORKSPACE_METADATA_FILE"' EXIT
	return 0
}

# Resolve the publish order from the workspace dependency graph. The declared
# publish list is the set of crates; the order is a property of the graph.
compute_publish_order() {
	if ! load_workspace_metadata; then
		return 1
	fi
	local ordered
	if ! ordered=$(python3 "$PUBLISH_ORDER_TOOL" plan --metadata "$WORKSPACE_METADATA_FILE" --crates "${PUBLISH_CRATES[@]}"); then
		return 1
	fi
	local -a crates=()
	while IFS= read -r line; do
		if [[ -n "$line" ]]; then
			crates+=("$line")
		fi
	done <<<"$ordered"
	if [[ ${#crates[@]} -ne ${#PUBLISH_CRATES[@]} ]]; then
		print_error "Derived publish order covers ${#crates[@]} of ${#PUBLISH_CRATES[@]} crates — aborting."
		return 1
	fi
	CRATES=("${crates[@]}")
	return 0
}

is_version_published() {
	local crate="$1"
	local version="$2"
	local endpoint="https://crates.io/api/v1/crates/${crate}/${version}"
	if curl --silent --show-error --fail --location --user-agent "vtcode-publish-script" "$endpoint" >/dev/null 2>&1; then
		return 0
	fi
	if cargo info --registry crates-io --quiet "${crate}@${version}" >/dev/null 2>&1; then
		return 0
	fi
	return 1
}

CRATES=()
if ! compute_publish_order; then
	print_error "Aborting: no valid crates.io publish order could be derived from the workspace graph."
	exit 1
fi

if [[ -n "$START_FROM" ]]; then
	found=0
	filtered=()
	for crate in "${CRATES[@]}"; do
		if [[ $crate == "$START_FROM" ]]; then
			found=1
		fi
		if [[ $found -eq 1 ]]; then
			filtered+=("$crate")
		fi
	done
	if [[ $found -eq 0 ]]; then
		echo "Unknown crate passed to --start-from: $START_FROM" >&2
		exit 1
	fi
	CRATES=("${filtered[@]}")

	# Validate that all workspace dependencies of the starting crate that are
	# not part of the remaining sequence have already been published on
	# crates.io. This catches the common mistake of resuming from a dependent
	# crate before its prerequisites are live.
	start_crate="${CRATES[0]}"
	if ! load_workspace_metadata; then
		exit 1
	fi
	start_deps=$(python3 "$PUBLISH_ORDER_TOOL" deps --metadata "$WORKSPACE_METADATA_FILE" --crates "$start_crate")
	while IFS=$'\t' read -r _ dep; do
		if [[ -z "$dep" ]]; then
			continue
		fi
		in_remaining=0
		for c in "${CRATES[@]}"; do
			if [[ "$c" == "$dep" ]]; then
				in_remaining=1
				break
			fi
		done
		if [[ $in_remaining -eq 0 ]] && ! is_version_published "$dep" "$CURRENT_VERSION"; then
			print_error "Cannot start from '${start_crate}' because dependency '${dep}' ${CURRENT_VERSION} has not been published yet."
			print_info "Publish '${dep}' first (without --start-from), or start from an earlier crate in the sequence."
			exit 1
		fi
	done <<<"$start_deps"
fi

run_cmd() {
	echo "+ $*"
	eval "$@"
}

publish_cmd() {
	local crate="$1"
	local version="${CURRENT_VERSION}"
	if [[ $DRY_RUN -eq 1 ]]; then
		run_cmd "cargo publish --dry-run -p $crate"
		return 0
	fi

	# Idempotency: skip if already published (handles retry after transient 503)
	if is_version_published "$crate" "$version"; then
		print_info "${crate} ${version} already published on crates.io — skipping cargo publish."
		return 0
	fi

	local max_retries=5
	local attempt=1
	local backoff=10
	local last_status=0

	while [[ $attempt -le $max_retries ]]; do
		print_info "Publishing ${crate} ${version} (attempt ${attempt}/${max_retries})..."
		# Capture output to detect transient 5xx
		local tmp_out
		tmp_out=$(mktemp)
		set +e
		cargo publish -p "$crate" 2>&1 | tee "$tmp_out"
		last_status=${PIPESTATUS[0]}
		set -e

		if [[ $last_status -eq 0 ]]; then
			rm -f "$tmp_out"
			return 0
		fi

		# If crate is now published despite non-zero exit (e.g. 503 after ingest), treat as success
		if is_version_published "$crate" "$version"; then
			print_warning "cargo publish exited with ${last_status} but ${crate} ${version} is now available — treating as success."
			rm -f "$tmp_out"
			return 0
		fi

		# Handle dirty working directory for vtcode binary (cargo publish requires clean tree)
		if grep -qi "uncommitted changes" "$tmp_out" || grep -qi "allow-dirty" "$tmp_out"; then
			print_warning "cargo publish blocked by dirty working directory for ${crate}. Retrying with --allow-dirty..."
			rm -f "$tmp_out"
			tmp_out=$(mktemp)
			set +e
			cargo publish -p "$crate" --allow-dirty 2>&1 | tee "$tmp_out"
			last_status=${PIPESTATUS[0]}
			set -e
			if [[ $last_status -eq 0 ]]; then
				rm -f "$tmp_out"
				return 0
			fi
			# Check if now published despite exit code
			if is_version_published "$crate" "$version"; then
				print_warning "cargo publish --allow-dirty exited with ${last_status} but ${crate} ${version} is now available — treating as success."
				rm -f "$tmp_out"
				return 0
			fi
			# Fall through to transient/non-transient handling with new tmp_out
		fi

		# Detect transient errors: 503/502/500/429, timeout, "failed to get a 200 OK", "503", "429 Too Many Requests"
		local is_transient=0
		if grep -qiE "503|502|500|429|timeout|failed to get a 200 OK|Service Unavailable|Too Many Requests|connection.*timed out|http.*503" "$tmp_out"; then
			is_transient=1
		fi
		rm -f "$tmp_out"

		if [[ $is_transient -eq 0 ]]; then
			print_error "cargo publish for ${crate} failed with non-transient error (exit ${last_status}) — not retrying."
			return $last_status
		fi

		if [[ $attempt -lt $max_retries ]]; then
			print_warning "Transient crates.io error for ${crate} (attempt ${attempt} failed). Retrying in ${backoff}s..."
			sleep "$backoff"
			backoff=$((backoff * 2))
			# cap backoff at 60s
			if [[ $backoff -gt 60 ]]; then
				backoff=60
			fi
		else
			print_error "cargo publish for ${crate} failed after ${max_retries} attempts (last exit ${last_status})."
			return $last_status
		fi

		attempt=$((attempt + 1))
	done

	return $last_status
}

wait_for_crates_io_version() {
	local crate="$1"
	local version="$2"

	if [[ $DRY_RUN -eq 1 ]]; then
		echo "[dry-run] Skipping crates.io availability check for ${crate} ${version}."
		return 0
	fi

	local endpoint="https://crates.io/api/v1/crates/${crate}/${version}"
	local attempt=1
	local max_attempts=36
	local api_available=0

	print_info "Waiting for ${crate} ${version} to be indexed on crates.io..."
	while [[ $attempt -le $max_attempts ]]; do
		if curl --silent --show-error --fail --location --user-agent "vtcode-publish-script" "$endpoint" >/dev/null; then
			print_success "${crate} ${version} is available on crates.io"
			api_available=1
			break
		fi

		if [[ $attempt -lt $max_attempts ]]; then
			sleep 5
		fi

		attempt=$((attempt + 1))
	done

	if [[ $api_available -eq 0 ]]; then
		print_warning "Timed out waiting for ${crate} ${version} to appear on the crates.io API; checking Cargo's registry index directly."
	fi

	# The crates.io API and Cargo's local index are updated independently. The
	# API can report a release before Cargo sees it, which makes the next
	# `cargo publish` fail while resolving a just-published dependency. Querying
	# the registry explicitly refreshes Cargo's index and validates the exact
	# version that downstream packaging will require.
	attempt=1
	print_info "Waiting for Cargo to resolve ${crate} ${version} from crates.io..."
	while [[ $attempt -le $max_attempts ]]; do
		if cargo info --registry crates-io --quiet "${crate}@${version}" >/dev/null 2>&1; then
			print_success "Cargo can resolve ${crate} ${version} from crates.io"
			return 0
		fi

		if [[ $attempt -lt $max_attempts ]]; then
			sleep 5
		fi

		attempt=$((attempt + 1))
	done

	print_error "Timed out waiting for Cargo to resolve ${crate} ${version} from crates.io."
	return 1
}

generate_docs() {
	local crate="$1"
	if [[ $RUN_DOCS -eq 0 ]]; then
		echo "Skipping doc generation for ${crate}."
		return
	fi
	run_cmd "cargo doc --no-deps -p ${crate}"
}

maybe_tag() {
	local tag="$1"
	if [[ $RUN_TAGS -eq 0 ]]; then
		echo "Skipping creation of git tag ${tag}."
		return
	fi
	if [[ $DRY_RUN -eq 1 ]]; then
		echo "[dry-run] Skipping creation of git tag ${tag}."
		return
	fi
	if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
		echo "Tag ${tag} already exists; skipping creation."
		return
	fi
	run_cmd "git tag ${tag}"
}

post_publish_follow_up() {
	local crate="$1"
	if [[ $DRY_RUN -eq 1 ]]; then
		echo "[dry-run] Would run 'cargo update -p ${crate}' and 'cargo check -p ${crate}'."
		return
	fi
	run_cmd "cargo update -p ${crate}"
	if [[ $RUN_FOLLOW_UP -eq 1 ]]; then
		run_cmd "cargo check -p ${crate}"
	fi
}

if [[ $RUN_TESTS -eq 1 ]]; then
	run_cmd "cargo fmt"
	run_cmd "cargo clippy --all-targets --all-features"
	run_cmd "cargo test --workspace"
	run_cmd "cargo test --doc"
fi

for crate in "${CRATES[@]}"; do
	generate_docs "$crate"
	if [[ "$crate" == "vtcode-bash-runner" && $DRY_RUN -eq 0 ]]; then
		echo "Re-running vtcode-bash-runner dry run now that vtcode-exec-events is published..."
		run_cmd "cargo publish --dry-run -p vtcode-bash-runner"
	fi
	if ! publish_cmd "$crate"; then
		print_error "Failed to publish ${crate} ${CURRENT_VERSION}. Aborting remaining publishes."
		print_info "Resume with: bash ./scripts/publish_extracted_crates.sh --start-from ${crate} --skip-tests --skip-tags --skip-follow-up"
		print_info "Or retry this crate alone: cargo publish -p ${crate}"
		exit 1
	fi
	wait_for_crates_io_version "$crate" "$CURRENT_VERSION"
	tag="${crate}-${CURRENT_VERSION}"
	maybe_tag "${tag}"
	post_publish_follow_up "${crate}"
	echo "Completed processing for ${crate}."
	echo "---"
	if [[ $DRY_RUN -eq 1 ]]; then
		echo "[dry-run] Validate docs/changelogs and rehearse dependency bumps after each publish."
		echo "[dry-run] Use a real run without --dry-run to create tags and refresh dependencies."
	else
		echo "Review the updated Cargo.lock and bump the dependency in dependent crates before pushing ${tag}."
		echo "When ready, commit the changes, push the tag, and proceed to the next crate."
	fi
	echo "=========================="
	echo
done

echo "Release sequence complete."
if [[ $DRY_RUN -eq 1 ]]; then
	echo "All commands were executed in dry-run mode."
else
	echo "Remember to push the created tags and follow up with dependency bump PRs."
fi
