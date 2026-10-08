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
            cd "$stage_dir"
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

# Background helper for parallel uploads. Args are expanded in the parent
# before fork, avoiding the `$idx` race of inline subshells.
_upload_single_asset() {
    if [ $# -ne 3 ]; then
        echo "usage: _upload_single_asset <version> <file> <status-file>" >&2
        return 2
    fi
    local version=$1
    local file=$2
    local status_file=$3
    if upload_release_asset_with_retry "$version" "$file"; then
        echo "0" >"$status_file"
    else
        echo "1:$file" >"$status_file"
    fi
}

# Upload multiple release assets in parallel with per-file retry.
#
#   upload_release_assets_parallel <version> <file>...
#
# Sequential `gh release upload` of ~20 assets (6 raw compat binaries at
# 40-80MB each + archives + checksums + installers) dominates Step 4 wall
# time. GitHub re-sorts assets alphabetically by name, so upload order is
# irrelevant and parallel uploads are safe. Honors UPLOAD_PARALLEL_JOBS
# (default 4) to avoid throttling uploads.github.com. Bash 3.2 compatible
# (no `wait -n`): launches background jobs and polls `jobs -p`.
# Returns nonzero if any asset failed after its own retries.
upload_release_assets_parallel() {
    if [ $# -lt 2 ]; then
        echo "usage: upload_release_assets_parallel <version> <file>..." >&2
        return 2
    fi
    local version=$1
    shift
    local max_jobs="${UPLOAD_PARALLEL_JOBS:-4}"
    case "$max_jobs" in
    '' | *[!0-9]*) max_jobs=4 ;;
    esac
    if [ "$max_jobs" -lt 1 ]; then
        max_jobs=1
    fi

    local status_dir
    status_dir=$(mktemp -d 2>/dev/null || mktemp -d -t 'vtcode-upload-status')
    local idx=0
    local launched=0
    local file
    # Batched fan-out: launch up to max_jobs, then `wait` for the whole
    # batch before launching more. `wait` reaps deterministically, unlike
    # polling `jobs -p`, whose count of finished-but-unreaped jobs is
    # shell-dependent and can wedge the loop forever.
    for file in "$@"; do
        idx=$((idx + 1))
        launched=$((launched + 1))
        # Args to the background helper are expanded in the parent before
        # fork, so each job gets its own file/status pair (a bare
        # `( ... $idx ... ) &` would race: $idx expands in the subshell
        # after the parent may have incremented it).
        _upload_single_asset "$version" "$file" "$status_dir/$idx.status" &
        if [ "$launched" -ge "$max_jobs" ]; then
            wait || true
            launched=0
        fi
    done
    # `wait` under `set -e` must not abort the release on a single failure;
    # per-file statuses below are authoritative.
    wait || true
    local failed=0
    local i st
    # Iterate the expected indices rather than the glob: a background job
    # killed before writing its status file must fail the release, not pass
    # silently (an unmatched glob would just `continue` to success).
    for ((i = 1; i <= idx; i++)); do
        st="$status_dir/$i.status"
        local content
        content=$(cat "$st" 2>/dev/null || echo "1:unknown")
        if [ "$content" != "0" ]; then
            echo "failed to upload ${content#1:} to GitHub Release" >&2
            failed=1
        fi
    done
    rm -rf "$status_dir"
    return "$failed"
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
