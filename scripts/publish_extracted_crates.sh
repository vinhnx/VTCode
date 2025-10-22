#!/usr/bin/env bash
set -euo pipefail

# publish_extracted_crates.sh orchestrates the sequential publishes for the
# extracted VTCode crates. It mirrors the workflow defined in
# docs/component_release_plan.md and provides optional dry-run coverage so the
# same script can be used for validation ahead of the real release window.

usage() {
    cat <<USAGE
Usage: $0 [--dry-run] [--start-from <crate>] [--skip-tests]

Options:
  --dry-run          Use `cargo publish --dry-run` for each crate instead of
                     performing the real publish. This is the default when the
                     VT_RELEASE_DRY_RUN environment variable is set to `1`.
  --start-from CRATE Resume publishing from the provided crate name. Valid
                     crates: vtcode-commons, vtcode-markdown-store,
                     vtcode-indexer, vtcode-bash-runner, vtcode-exec-events.
  --skip-tests       Skip running the workspace fmt/clippy/test checks. Use with
                     caution; the release plan expects the validation suite to
                     pass before publishing.
  -h, --help         Show this help message and exit.

Environment variables:
  VT_RELEASE_DRY_RUN When set to `1`, the script defaults to performing a dry
                     run. Passing `--dry-run` or providing `--start-from` still
                     works while the variable is set.
USAGE
}

DRY_RUN=${VT_RELEASE_DRY_RUN:-0}
START_FROM=""
RUN_TESTS=1

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
        -h|--help)
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

CRATES=(
    vtcode-commons
    vtcode-markdown-store
    vtcode-indexer
    vtcode-bash-runner
    vtcode-exec-events
)

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
fi

run_cmd() {
    echo "+ $*"
    eval "$@"
}

publish_cmd() {
    local crate="$1"
    if [[ $DRY_RUN -eq 1 ]]; then
        run_cmd "cargo publish --dry-run -p $crate"
    else
        run_cmd "cargo publish -p $crate"
    fi
}

if [[ $RUN_TESTS -eq 1 ]]; then
    run_cmd "cargo fmt"
    run_cmd "cargo clippy --all-targets --all-features"
    run_cmd "cargo test"
fi

for crate in "${CRATES[@]}"; do
    if [[ "$crate" == "vtcode-bash-runner" && $DRY_RUN -eq 0 ]]; then
        echo "Re-running vtcode-bash-runner dry run now that vtcode-commons is published..."
        run_cmd "cargo publish --dry-run -p vtcode-bash-runner"
    fi
    publish_cmd "$crate"
    tag="${crate}-v0.1.0"
    run_cmd "git tag ${tag}"
    run_cmd "cargo update -p ${crate}"
    run_cmd "cargo check -p ${crate}"
    echo "Completed processing for ${crate}."
    echo "---"
    echo "Review the updated Cargo.lock and bump the dependency in dependent crates before pushing ${tag}."
    echo "When ready, commit the changes, push the tag, and proceed to the next crate."
    echo "=========================="
    echo
done

echo "Release sequence complete." 
if [[ $DRY_RUN -eq 1 ]]; then
    echo "All commands were executed in dry-run mode."
else
    echo "Remember to push the created tags and follow up with dependency bump PRs."
fi
