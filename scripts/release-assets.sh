#!/usr/bin/env bash
# Release asset helpers for the legacy updater compatibility bridge.
#
# Broken `self_update` 1.0.0-rc.6 updaters (VT Code v0.141.0-v0.141.4) cannot
# extract gzip tar archives: they were built with the `archive-tar` feature but
# NOT `compression-tar-gz`, so a real `.tar.gz` fails with
# `CompressionNotEnabledError: 'gz' compression not supported`.
#
# How the legacy updater selects an asset (verified against the crate source):
#   1. `asset_for(target, Some("{target}.tar.gz"))` returns the FIRST asset
#      whose name `contains(target)` AND `contains("{target}.tar.gz")`.
#   2. GitHub's releases API returns the `assets` array sorted ALPHABETICALLY
#      BY NAME (ascending) -- upload order is irrelevant. So "first match" is
#      the alphabetically-first matching asset.
#   3. The downloaded file is saved under the asset's own name, and
#      `detect_archive` reads the FINAL path extension. Anything other than
#      zip/tar/tgz/gz -> `ArchiveKind::Plain(None)`, and `extract_file` then
#      copies the raw bytes verbatim to `<dir>/vtcode` -- no gzip feature used.
#
# The compatibility asset is a raw executable named
# `compat-vtcode-<v>-<target>.tar.gz.compat`. It contains the `{target}.tar.gz`
# substring (so the legacy identifier matches it) and its final extension is
# `.compat` (so it is treated as a plain binary). Crucially, the `compat-`
# prefix sorts BEFORE `vtcode-` (`c` < `v`), so it is the alphabetically-first
# match and the legacy updater picks it instead of the broken `.tar.gz`.
#
# The v0.141.5+ updater ignores these assets: its matcher requires the name to
# `starts_with("vtcode-")` AND `ends_with("{target}.tar.gz" | "{target}.zip")`,
# and `compat-vtcode-...tar.gz.compat` matches neither, so it selects the real
# archive. Both generations therefore install byte-identical binaries.
#
# These helpers derive the raw compatibility executables from the same normal
# release archives used by installers.
#
# This file is sourced by `scripts/release.sh`; it intentionally does not set
# shell options so the caller controls `set -euo pipefail`.

# Print the compatibility asset path for a normal release archive.
#
#   compatibility_asset_path <archive> <output-dir>
#
# Both `.tar.gz` (macOS/Linux) and `.zip` (Windows) archives map to a
# `compat-<stem>.tar.gz.compat` asset. The `compat-` prefix is load-bearing:
# it makes the asset sort before `vtcode-<v>-<target>.tar.gz` so the legacy
# updater's alphabetically-first `find()` picks the raw binary. Returns
# nonzero on an unsupported archive extension.
compatibility_asset_path() {
    if [[ $# -ne 2 ]]; then
        echo "usage: compatibility_asset_path <archive> <output-dir>" >&2
        return 2
    fi
    local archive=$1
    local output_dir=$2
    local name
    name=$(basename "$archive")
    local stem
    stem="${name%.tar.gz}"
    stem="${stem%.zip}"
    if [[ "$stem" == "$name" ]]; then
        echo "unsupported archive extension: $name (expected .tar.gz or .zip)" >&2
        return 1
    fi
    printf '%s/compat-%s.tar.gz.compat\n' "$output_dir" "$stem"
}

# Extract the platform executable from a normal release archive into a raw
# `.tar.gz.compat` file.
#
#   create_compatibility_asset <archive> <output>
#
# The archive must contain exactly one `vtcode` (Unix) or `vtcode.exe`
# (Windows) entry at any path. Rejects unsupported suffixes, missing binaries,
# empty output, and multiple matching binary entries.
create_compatibility_asset() {
    if [[ $# -ne 2 ]]; then
        echo "usage: create_compatibility_asset <archive> <output>" >&2
        return 2
    fi
    local archive=$1
    local output=$2
    local name
    name=$(basename "$archive")

    local binary
    if [[ "$name" == *.tar.gz ]]; then
        binary="vtcode"
    elif [[ "$name" == *.zip ]]; then
        binary="vtcode.exe"
    else
        echo "unsupported archive extension: $name (expected .tar.gz or .zip)" >&2
        return 1
    fi

    # Validate exactly one matching binary entry to avoid ambiguity.
    local matches
    if [[ "$name" == *.tar.gz ]]; then
        matches=$(tar -tf "$archive" 2>/dev/null | grep -E "(^|/)${binary}\$" || true)
    else
        matches=$(unzip -Z1 "$archive" 2>/dev/null | grep -E "(^|/)${binary}\$" || true)
    fi
    local count
    count=$(printf '%s\n' "$matches" | grep -c . || true)
    if [[ "$count" -eq 0 ]]; then
        echo "archive $name does not contain ${binary}" >&2
        return 1
    fi
    if [[ "$count" -gt 1 ]]; then
        echo "archive $name contains multiple ${binary} entries" >&2
        return 1
    fi

    local entry
    entry=$(printf '%s\n' "$matches" | head -n1)

    : >"$output"
    if [[ "$name" == *.tar.gz ]]; then
        tar -xOf "$archive" "$entry" >"$output"
    else
        unzip -p "$archive" "$entry" >"$output"
    fi

    if [[ ! -s "$output" ]]; then
        echo "extracted compatibility asset is empty: $name" >&2
        rm -f "$output"
        return 1
    fi
    if [[ "$name" == *.tar.gz ]] && ! chmod +x "$output"; then
        echo "could not make Unix compatibility asset executable: $name" >&2
        rm -f "$output"
        return 1
    fi
    return 0
}

# Print the desktop app distribution archive name for a release version.
#
#   desktop_archive_name <version> <macos-target>
#
# The name MUST keep the updater-safe shape `vtcode-desktop-<v>-<target>.zip`:
# the v0.141.5+ updater requires exactly one asset matching
# `starts_with("vtcode-") && ends_with("{target}.tar.gz"|"{target}.zip")`
# (`select_archive_asset` in src/updater/github.rs), and the legacy updater
# matches `{target}.tar.gz` substrings. A `.zip` macOS asset matches neither
# selector, so it rides the normal upload queue without breaking CLI
# self-update. Never rename this to `.tar.gz` or a Windows triple.
desktop_archive_name() {
    if [[ $# -ne 2 ]]; then
        echo "usage: desktop_archive_name <version> <macos-target>" >&2
        return 2
    fi
    printf 'vtcode-desktop-%s-%s.zip\n' "$1" "$2"
}

# Append the desktop app section to a GitHub release's notes.
#
#   append_desktop_release_notes <version> <desktop-zip-name> <signing-mode>
#
# Idempotent: skips when the notes already reference the desktop archive, so
# `--skip-release` re-runs never duplicate the section. <signing-mode> is
# "notarized" or "unsigned" (mirrors the signing lib mode) and selects the
# first-launch guidance.
append_desktop_release_notes() {
    if [[ $# -ne 3 ]]; then
        echo "usage: append_desktop_release_notes <version> <desktop-zip-name> <signing-mode>" >&2
        return 2
    fi
    local version=$1
    local zip_name=$2
    local mode=$3
    local arch_note
    case "$zip_name" in
    *aarch64-apple-darwin*)
        arch_note="Apple Silicon"
        ;;
    *x86_64-apple-darwin*)
        arch_note="Intel"
        ;;
    *)
        echo "unexpected desktop archive name: $zip_name" >&2
        return 1
        ;;
    esac
    local body
    if ! body=$(gh release view "$version" --json body --jq .body 2>/dev/null); then
        echo "could not read release notes for $version" >&2
        return 1
    fi
    if grep -Fq -- "$zip_name" <<<"$body"; then
        return 0
    fi
    local first_launch
    if [[ "$mode" == "notarized" ]]; then
        first_launch="The app is Developer ID signed and notarized, so it opens normally."
    else
        first_launch="**One-time macOS security step:** this build is ad hoc signed (no Developer ID), so Gatekeeper blocks the first launch. Allow it once in System Settings > Privacy & Security (**Open Anyway**), then open the app again. See FIRST-LAUNCH.txt inside the zip."
    fi
    local section
    # Two leading newlines: gh strips the body's trailing newline, so the
    # section needs its own blank line separator.
    section=$(cat <<EOF


### Desktop app (macOS)

- Download \`$zip_name\` ($arch_note, macOS 15+), unzip, and drag **VT Code.app** to Applications.
- $first_launch
- The bundle includes its matching \`vtcode\` CLI helper; open the app anytime with \`vtcode desktop\`.
- Verify with \`$zip_name.sha256\` or \`checksums.txt\`.
EOF
    )
    if ! gh release edit "$version" --notes "${body}${section}"; then
        echo "could not update release notes for $version" >&2
        return 1
    fi
}

# Generate the aggregate checksum manifest for installable archives.
#
# Compatibility assets are intentionally excluded: updater versions that use
# substring filename matching can otherwise mistake
# `compat-<archive>.compat` for `<archive>`.
generate_checksums_manifest() {
    if [[ $# -ne 1 ]]; then
        echo "usage: generate_checksums_manifest <stage-dir>" >&2
        return 2
    fi
    local stage_dir=$1
    local -a checksum_command
    if command -v sha256sum &>/dev/null; then
        checksum_command=(sha256sum)
    elif command -v shasum &>/dev/null; then
        checksum_command=(shasum -a 256)
    else
        echo "neither sha256sum nor shasum found" >&2
        return 1
    fi

    local nullglob_was_set=0
    shopt -q nullglob && nullglob_was_set=1
    shopt -s nullglob
    local -a archives=("$stage_dir"/*.tar.gz "$stage_dir"/*.zip)
    [[ "$nullglob_was_set" -eq 1 ]] || shopt -u nullglob

    local manifest_tmp="$stage_dir/checksums.txt.tmp"
    : >"$manifest_tmp"
    local archive
    for archive in "${archives[@]}"; do
        (
            cd "$stage_dir" || return 1
            "${checksum_command[@]}" "$(basename "$archive")"
        ) >>"$manifest_tmp"
    done
    mv "$manifest_tmp" "$stage_dir/checksums.txt"
}

# Upload a single release asset with retry on transient failures.
#
#   upload_release_asset_with_retry <version> <file> [max-attempts]
#
# `gh release upload` of large raw compat binaries (~40-80MB) intermittently
# fails with HTTP 500 from uploads.github.com. Uploading all compat assets in
# one invocation means a single flaky asset fails the whole batch. Upload
# per-file with exponential backoff so a transient 500 is retried instead of
# aborting the release. Returns nonzero after exhausting attempts.
upload_release_asset_with_retry() {
    if [[ $# -lt 2 || $# -gt 3 ]]; then
        echo "usage: upload_release_asset_with_retry <version> <file> [max-attempts]" >&2
        return 2
    fi
    local version=$1
    local file=$2
    local max_attempts=${3:-5}
    local attempt=1
    while [[ "$attempt" -le "$max_attempts" ]]; do
        if gh release upload "$version" "$file" --clobber; then
            return 0
        fi
        if [[ "$attempt" -eq "$max_attempts" ]]; then
            echo "failed to upload $file after $max_attempts attempts" >&2
            return 1
        fi
        local backoff=$((5 * (1 << (attempt - 1))))
        [[ "$backoff" -gt 60 ]] && backoff=60
        echo "upload of $file failed (attempt $attempt/$max_attempts); retrying in ${backoff}s..." >&2
        sleep "$backoff"
        attempt=$((attempt + 1))
    done
}

# Download a single CI artifact with retry on transient failures.
#
#   download_ci_artifact_with_retry <run-id> <artifact-name> <dest-dir> [max-attempts]
#
# `gh run download` intermittently fails (network flake, API 500) which
# previously aborted the whole release at the required-target coverage check
# with a misleading "ensure the workflow succeeded" hint. Retry with
# exponential backoff and keep stderr visible so the real error is diagnosable.
# Returns nonzero after exhausting attempts.
download_ci_artifact_with_retry() {
    if [[ $# -lt 3 || $# -gt 4 ]]; then
        echo "usage: download_ci_artifact_with_retry <run-id> <artifact-name> <dest-dir> [max-attempts]" >&2
        return 2
    fi
    local run_id=$1
    local artifact_name=$2
    local dest_dir=$3
    local max_attempts=${4:-5}
    local attempt=1
    while [[ "$attempt" -le "$max_attempts" ]]; do
        if gh run download "$run_id" --name "$artifact_name" --dir "$dest_dir"; then
            return 0
        fi
        if [[ "$attempt" -eq "$max_attempts" ]]; then
            echo "failed to download CI artifact $artifact_name (run $run_id) after $max_attempts attempts" >&2
            return 1
        fi
        local backoff=$((5 * (1 << (attempt - 1))))
        [[ "$backoff" -gt 60 ]] && backoff=60
        echo "download of $artifact_name failed (attempt $attempt/$max_attempts); retrying in ${backoff}s..." >&2
        sleep "$backoff"
        attempt=$((attempt + 1))
    done
}

# Return 0 when CI run <run-id> published a build artifact for <version>.
#
#   ci_run_has_version_artifact <run-id> <version>
#
# `build-linux-windows.yml` names its artifacts `vtcode-<tag>-<target>`, and a
# `workflow_dispatch` run does NOT expose its `tag` input through the API
# (`.inputs` is null), so the artifact name is the only reliable way to tell
# which release a run built. Uses the `{owner}/{repo}` placeholder so it works
# from any checkout of the repo. Returns nonzero on any API failure.
ci_run_has_version_artifact() {
    if [[ $# -ne 2 ]]; then
        echo "usage: ci_run_has_version_artifact <run-id> <version>" >&2
        return 2
    fi
    local run_id=$1
    local version=$2
    local names
    names=$(gh api --paginate "repos/{owner}/{repo}/actions/runs/${run_id}/artifacts" \
        --jq '.artifacts[].name' 2>/dev/null) || return 1
    grep -qxF "vtcode-${version}-x86_64-unknown-linux-gnu" <<<"$names"
}

# Print the newest successful `build-linux-windows.yml` run that built
# <version>, or nothing (nonzero) when no such run exists.
#
#   find_ci_run_for_version <version> [max-runs]
#
# Recency alone is not enough to identify a release's CI run. While a new
# version's run is still queued or building, the newest *successful* run
# belongs to the PREVIOUS version; its artifacts are named
# `vtcode-<old-version>-...`, so every download of the new version's artifact
# names misses and the release aborts with "Missing required release archive".
# Filtering candidates by the version embedded in their artifact names skips
# stale runs and finds the right one.
#
# gh 2.102.0 quirk (verified 2026-10-22): `gh run list --json ...` combined
# with the server-side `--branch` or `--event` filters returns `[]` even when
# matching runs exist (the same rows appear in plain-text output and in
# `--json` without those filters). Filter `--branch main --event
# workflow_dispatch` locally in `--jq` instead of passing them server-side.
find_ci_run_for_version() {
    if [[ $# -lt 1 || $# -gt 2 ]]; then
        echo "usage: find_ci_run_for_version <version> [max-runs]" >&2
        return 2
    fi
    local version=$1
    local max_runs=${2:-20}
    local candidate
    while IFS= read -r candidate; do
        [[ -n "$candidate" ]] || continue
        if ci_run_has_version_artifact "$candidate" "$version"; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done < <(gh run list --workflow build-linux-windows.yml --limit "$max_runs" \
        --json databaseId,conclusion,headBranch,event \
        --jq '.[] | select(.conclusion == "success" and .headBranch == "main" and .event == "workflow_dispatch") | .databaseId' 2>/dev/null)
    return 1
}

# Ensure `gh` authenticates with push access to the release repo.
#
#   ensure_release_push_access <owner/repo>
#
# Harness/CI environments often export a pull-only GITHUB_TOKEN, under which
# `gh release upload` fails with HTTP 404 from uploads.github.com (GitHub
# returns 404 instead of 403 when the token cannot write). If the current
# auth lacks push, fall back to stored (keyring) credentials by unsetting
# GITHUB_TOKEN/GH_TOKEN for the remainder of the release process. Fails when
# neither auth has push so the release aborts before building/uploading.
ensure_release_push_access() {
    if [[ $# -ne 1 ]]; then
        echo "usage: ensure_release_push_access <owner/repo>" >&2
        return 2
    fi
    local repo=$1
    if [[ "$(gh api "repos/$repo" --jq .permissions.push 2>/dev/null)" == "true" ]]; then
        return 0
    fi
    if [[ -n "${GITHUB_TOKEN:-}" || -n "${GH_TOKEN:-}" ]]; then
        echo "current gh auth lacks push access to $repo; falling back to stored credentials (unsetting GITHUB_TOKEN/GH_TOKEN)..." >&2
        unset GITHUB_TOKEN GH_TOKEN
        if [[ "$(gh api "repos/$repo" --jq .permissions.push 2>/dev/null)" == "true" ]]; then
            echo "using stored gh credentials with push access to $repo" >&2
            return 0
        fi
    fi
    echo "gh auth lacks push access to $repo; authenticate with an account that has write access (e.g. 'gh auth login')" >&2
    return 1
}

# Validate a staged release directory has complete target coverage.
#
#   validate_release_assets <stage-dir> <version>
#
# Fails when any required target lacks its compatibility binary, normal
# archive, or checksum sidecar.
validate_release_assets() {
    if [[ $# -ne 2 ]]; then
        echo "usage: validate_release_assets <stage-dir> <version>" >&2
        return 2
    fi
    local stage_dir=$1
    local version=$2
    local -a required_targets=(
        "x86_64-apple-darwin:tar.gz"
        "aarch64-apple-darwin:tar.gz"
        "x86_64-unknown-linux-gnu:tar.gz"
        "x86_64-unknown-linux-musl:tar.gz"
        "aarch64-unknown-linux-gnu:tar.gz"
        "x86_64-pc-windows-msvc:zip"
    )
    local missing=0
    local item target ext archive compat checksum
    for item in "${required_targets[@]}"; do
        target="${item%%:*}"
        ext="${item##*:}"
        archive="vtcode-${version}-${target}.${ext}"
        compat="compat-vtcode-${version}-${target}.tar.gz.compat"
        checksum="vtcode-${version}-${target}.sha256"
        for file in "$compat" "$archive" "$checksum"; do
            if [[ ! -f "$stage_dir/$file" ]]; then
                echo "missing required release asset: $file" >&2
                missing=1
            fi
        done
    done
    if [[ ! -f "$stage_dir/checksums.txt" ]]; then
        echo "missing required release asset: checksums.txt" >&2
        missing=1
    fi
    if [[ "$missing" -ne 0 ]]; then
        echo "release asset validation failed for v${version}" >&2
        return 1
    fi
    return 0
}

# Report embedded dependency-audit coverage for each staged release archive.
#
#   report_audit_data_coverage <stage-dir> <version>
#
# Extracts the binary from every `vtcode-<version>-*` archive present and runs
# `cargo audit bin` over it, printing one `audit-data:` line per artifact:
# EMBEDDED (with dependency count), MISSING, TOO-LARGE, or UNREADABLE.
# Presence of archives is validate_release_assets' job; this function only
# reports on what is already staged, so it owns no target list to keep in sync.
#
# Cross-built musl/aarch64-linux-gnu artifacts cannot embed cargo-auditable
# data (cargo runs inside the cross container), so MISSING is expected for
# them. MISSING anywhere else prints a WARNING: either the auditable build
# step regressed or the release machine lacks cargo-auditable.
#
# Always returns 0: this is a report, not a gate. release.sh treats
# cargo-auditable as optional today; only flip this to fatal together with
# that policy. Skips quietly (single stderr line) when cargo-audit is absent.
# Each scan refreshes the advisory DB over the network; an offline machine
# reports UNREADABLE, never stale data.
report_audit_data_coverage() {
    if [[ $# -ne 2 ]]; then
        echo "usage: report_audit_data_coverage <stage-dir> <version>" >&2
        return 2
    fi
    local stage_dir=$1
    local version=$2
    if ! command -v cargo-audit >/dev/null 2>&1; then
        echo "audit-data: skipped (cargo-audit not installed; cargo install cargo-audit --locked)" >&2
        return 0
    fi
    local archive name tmp scan deps
    for archive in "$stage_dir"/vtcode-"$version"-*.tar.gz "$stage_dir"/vtcode-"$version"-*.zip; do
        [[ -f "$archive" ]] || continue
        name=$(basename "$archive")
        tmp=$(mktemp)
        if ! create_compatibility_asset "$archive" "$tmp" >/dev/null 2>&1; then
            echo "audit-data: $name: UNREADABLE (could not extract binary)"
            rm -f "$tmp"
            continue
        fi
        # `cargo audit bin` exit status reports advisories, not data presence,
        # so classify on its output text instead (verified signals: "Found
        # 'cargo auditable' data" vs "was not built with 'cargo auditable'"
        # vs "exceeds max size limit").
        scan=$(cargo audit bin "$tmp" 2>&1 || true)
        rm -f "$tmp"
        if printf '%s\n' "$scan" | grep -q "exceeds max size limit"; then
            echo "audit-data: $name: TOO-LARGE (exceeds the cargo-audit binary size cap; cannot verify)"
        elif printf '%s\n' "$scan" | grep -q "was not built with 'cargo auditable'"; then
            case "$name" in
                *musl*|*aarch64-unknown-linux-gnu*)
                    echo "audit-data: $name: MISSING (expected: cross-built artifacts cannot embed cargo-auditable data)"
                    ;;
                *)
                    echo "audit-data: $name: MISSING (WARNING: expected embedded audit data; check the cargo-auditable build step)"
                    ;;
            esac
        elif printf '%s\n' "$scan" | grep -q "Found 'cargo auditable' data"; then
            deps=$(printf '%s\n' "$scan" | grep -oE '\([0-9]+ dependencies\)' | head -n1 || true)
            if [[ -n "$deps" ]]; then
                echo "audit-data: $name: EMBEDDED $deps"
            else
                echo "audit-data: $name: EMBEDDED"
            fi
        else
            echo "audit-data: $name: UNREADABLE (cargo audit bin produced no verdict)"
        fi
    done
    return 0
}
