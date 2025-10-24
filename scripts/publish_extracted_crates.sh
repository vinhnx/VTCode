#!/usr/bin/env bash
set -euo pipefail

# publish_extracted_crates.sh orchestrates the sequential publishes for the
# extracted VTCode crates. It mirrors the workflow defined in
# docs/component_release_plan.md and provides optional dry-run coverage so the
# same script can be used for validation ahead of the real release window.

usage() {
    cat <<USAGE
Usage: $0 [--dry-run] [--start-from <crate>] [--skip-tests] [--skip-docs]

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
  --skip-docs        Skip regenerating API docs for each crate prior to
                     publishing.
  -h, --help         Show this help message and exit.

Environment variables:
  VT_RELEASE_DRY_RUN When set to `1`, the script defaults to performing a dry
                     run. Passing `--dry-run` or providing `--start-from` still
                     works while the variable is set.
  VT_RELEASE_SKIP_DOCS
                     When set to `1`, skip regenerating API docs even if
                     `--skip-docs` is not passed.
USAGE
}

DRY_RUN=${VT_RELEASE_DRY_RUN:-0}
START_FROM=""
RUN_TESTS=1
RUN_DOCS=1

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

generate_docs() {
    local crate="$1"
    if [[ $RUN_DOCS -eq 0 ]]; then
        echo "Skipping doc generation for ${crate}."
        return
    fi
    run_cmd "cargo doc --no-deps --all-features -p ${crate}"
}

maybe_tag() {
    local tag="$1"
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
    run_cmd "cargo check -p ${crate}"
}

if [[ $RUN_TESTS -eq 1 ]]; then
    run_cmd "cargo fmt"
    run_cmd "cargo clippy --all-targets --all-features"
    if cargo nextest --version >/dev/null 2>&1; then
        run_cmd "cargo nextest run --workspace"
        # Run doctests separately since nextest doesn't execute them
        run_cmd "cargo test --doc"
    else
        echo "cargo nextest not found; falling back to cargo test"
        run_cmd "cargo test"
    fi
fi

for crate in "${CRATES[@]}"; do
    generate_docs "$crate"
    if [[ "$crate" == "vtcode-bash-runner" && $DRY_RUN -eq 0 ]]; then
        echo "Re-running vtcode-bash-runner dry run now that vtcode-commons is published..."
        run_cmd "cargo publish --dry-run -p vtcode-bash-runner"
    fi
    publish_cmd "$crate"
    tag="${crate}-v0.1.0"
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
