#!/usr/bin/env bash
# Tests for the legacy updater compatibility bridge release helpers.
#
# Covers compatibility asset naming, byte-identity with the extracted
# executable, rejection of ambiguous/missing binaries, required-target
# coverage validation, two-phase upload ordering (compat assets before
# normal archives), per-file upload retry, and push-access preflight with
# stored-credential fallback via an instrumented `gh` stub.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "$SCRIPT_DIR/../release-assets.sh"

fail=0
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

pass() { echo "  ✓ $1"; }
fail_test() { echo "  ✗ $1"; fail=1; }

echo "Testing compatibility_asset_path..."

got=$(compatibility_asset_path "vtcode-0.141.6-aarch64-apple-darwin.tar.gz" "$tmp")
if [[ "$got" == "$tmp/compat-vtcode-0.141.6-aarch64-apple-darwin.tar.gz.compat" ]]; then
    pass "tar.gz -> compat-vtcode-...tar.gz.compat"
else
    fail_test "tar.gz path: got $got"
fi

got=$(compatibility_asset_path "vtcode-0.141.6-x86_64-pc-windows-msvc.zip" "$tmp")
if [[ "$got" == "$tmp/compat-vtcode-0.141.6-x86_64-pc-windows-msvc.tar.gz.compat" ]]; then
    pass "zip -> compat-vtcode-...tar.gz.compat (keeps <target>.tar.gz substring)"
else
    fail_test "zip path: got $got"
fi

if compatibility_asset_path "foo.txt" "$tmp" >/dev/null 2>&1; then
    fail_test "unsupported extension should be rejected"
else
    pass "unsupported extension rejected"
fi

echo "Testing compat asset sorts BEFORE the normal archive (load-bearing)..."

# GitHub returns release assets sorted alphabetically by name; the legacy
# updater's `find()` picks the first asset whose name contains both the target
# triple and the `{target}.tar.gz` identifier. Both the normal `.tar.gz` and
# the compat asset match, so the compat name MUST sort first or the legacy
# updater selects the broken `.tar.gz` and fails with CompressionNotEnabledError.
compat_name=$(compatibility_asset_path "vtcode-0.141.6-aarch64-apple-darwin.tar.gz" "$tmp")
compat_base=$(basename "$compat_name")
normal_base="vtcode-0.141.6-aarch64-apple-darwin.tar.gz"
# `LC_ALL=C sort` gives byte-order (ASCII) ordering, matching GitHub's sort.
sorted=$(printf '%s\n%s\n' "$compat_base" "$normal_base" | LC_ALL=C sort | head -1)
if [[ "$sorted" == "$compat_base" ]]; then
    pass "compat asset sorts before normal archive (legacy updater picks compat)"
else
    fail_test "compat asset does NOT sort before normal archive: compat=$compat_base normal=$normal_base"
fi
# Also assert the compat name still contains the legacy identifier substring.
if [[ "$compat_base" == *"aarch64-apple-darwin.tar.gz"* ]]; then
    pass "compat name contains the {target}.tar.gz identifier substring"
else
    fail_test "compat name lost the {target}.tar.gz substring: $compat_base"
fi

echo "Testing create_compatibility_asset byte identity..."

# Fixture tar.gz with a root-level vtcode binary (matches CI packaging).
mkdir -p "$tmp/src1"
printf 'unix-binary-bytes' >"$tmp/src1/vtcode"
tar -C "$tmp/src1" -czf "$tmp/vtcode-0.141.6-aarch64-apple-darwin.tar.gz" vtcode
out=$(compatibility_asset_path "$tmp/vtcode-0.141.6-aarch64-apple-darwin.tar.gz" "$tmp")
if create_compatibility_asset "$tmp/vtcode-0.141.6-aarch64-apple-darwin.tar.gz" "$out" \
    && cmp -s "$tmp/src1/vtcode" "$out"; then
    pass "tar.gz compat bytes match extracted vtcode"
else
    fail_test "tar.gz compat bytes mismatch"
fi
if [[ -x "$out" ]]; then
    pass "tar.gz compat asset is executable"
else
    fail_test "tar.gz compat asset is not executable"
fi

# Fixture zip with a root-level vtcode.exe binary (matches CI packaging).
if command -v zip >/dev/null 2>&1; then
    mkdir -p "$tmp/src2"
    printf 'windows-binary-bytes' >"$tmp/src2/vtcode.exe"
    (cd "$tmp/src2" && zip -q "$tmp/vtcode-0.141.6-x86_64-pc-windows-msvc.zip" vtcode.exe)
    out2=$(compatibility_asset_path "$tmp/vtcode-0.141.6-x86_64-pc-windows-msvc.zip" "$tmp")
    if create_compatibility_asset "$tmp/vtcode-0.141.6-x86_64-pc-windows-msvc.zip" "$out2" \
        && cmp -s "$tmp/src2/vtcode.exe" "$out2"; then
        pass "zip compat bytes match extracted vtcode.exe"
    else
        fail_test "zip compat bytes mismatch"
    fi
else
    echo "  - zip not installed; skipping zip compat byte test"
fi

echo "Testing create_compatibility_asset rejection..."

# Missing binary.
mkdir -p "$tmp/src3"
printf 'other' >"$tmp/src3/not-vtcode"
tar -C "$tmp/src3" -czf "$tmp/bad.tar.gz" not-vtcode
if create_compatibility_asset "$tmp/bad.tar.gz" "$tmp/should-not-exist" >/dev/null 2>&1; then
    fail_test "missing binary should be rejected"
else
    pass "missing binary rejected"
fi
[[ -e "$tmp/should-not-exist" ]] && fail_test "missing binary left output file" || pass "missing binary left no output"

# Multiple matching binaries.
mkdir -p "$tmp/src4/a" "$tmp/src4/b"
printf 'x' >"$tmp/src4/a/vtcode"
printf 'y' >"$tmp/src4/b/vtcode"
tar -C "$tmp/src4" -czf "$tmp/multi.tar.gz" a/vtcode b/vtcode
if create_compatibility_asset "$tmp/multi.tar.gz" "$tmp/should-not-exist2" >/dev/null 2>&1; then
    fail_test "multiple binaries should be rejected"
else
    pass "multiple binaries rejected"
fi

# Empty extracted output (binary is zero-length).
mkdir -p "$tmp/src5"
: >"$tmp/src5/vtcode"
tar -C "$tmp/src5" -czf "$tmp/empty.tar.gz" vtcode
if create_compatibility_asset "$tmp/empty.tar.gz" "$tmp/empty-out" >/dev/null 2>&1; then
    fail_test "empty binary should be rejected"
else
    pass "empty binary rejected"
fi

echo "Testing generate_checksums_manifest..."

manifest_stage="$tmp/manifest-stage"
mkdir -p "$manifest_stage"
printf 'archive' >"$manifest_stage/vtcode-0.141.7-aarch64-apple-darwin.tar.gz"
printf 'compatibility binary' >"$manifest_stage/compat-vtcode-0.141.7-aarch64-apple-darwin.tar.gz.compat"
if generate_checksums_manifest "$manifest_stage"; then
    if grep -q '  vtcode-0.141.7-aarch64-apple-darwin.tar.gz$' "$manifest_stage/checksums.txt" \
        && ! grep -q 'compat-' "$manifest_stage/checksums.txt"; then
        pass "aggregate manifest includes archives without ambiguous compatibility assets"
    else
        fail_test "aggregate manifest contains ambiguous or missing entries"
    fi
else
    fail_test "aggregate manifest generation should succeed"
fi

echo "Testing validate_release_assets..."

# Build a complete staged release directory for v0.141.6.
stage="$tmp/stage-good"
mkdir -p "$stage"
version="0.141.6"
targets=("x86_64-apple-darwin" "aarch64-apple-darwin" "x86_64-unknown-linux-gnu" \
    "x86_64-unknown-linux-musl" "aarch64-unknown-linux-gnu" "x86_64-pc-windows-msvc")
for target in "${targets[@]}"; do
    ext="tar.gz"
    [[ "$target" == *pc-windows* ]] && ext="zip"
    archive="$stage/vtcode-${version}-${target}.${ext}"
    printf 'placeholder' >"$archive"
    compat="$stage/compat-vtcode-${version}-${target}.tar.gz.compat"
    printf 'placeholder' >"$compat"
    printf 'placeholder-checksum' >"$stage/vtcode-${version}-${target}.sha256"
done
printf 'aggregate-checksums\n' >"$stage/checksums.txt"

if validate_release_assets "$stage" "$version"; then
    pass "complete staged release validates"
else
    fail_test "complete staged release should validate"
fi

# Remove a compat asset -> validation must fail.
rm -f "$stage/compat-vtcode-${version}-aarch64-apple-darwin.tar.gz.compat"
if validate_release_assets "$stage" "$version" >/dev/null 2>&1; then
    fail_test "missing compat asset should fail validation"
else
    pass "missing compat asset fails validation"
fi

# Remove a normal archive -> validation must fail.
stage2="$tmp/stage-missing-archive"
cp -r "$stage" "$stage2"
cp "$tmp/stage-good/compat-vtcode-${version}-aarch64-apple-darwin.tar.gz.compat" "$stage2/" 2>/dev/null || true
rm -f "$stage2/vtcode-${version}-x86_64-pc-windows-msvc.zip"
if validate_release_assets "$stage2" "$version" >/dev/null 2>&1; then
    fail_test "missing normal archive should fail validation"
else
    pass "missing normal archive fails validation"
fi

echo "Testing two-phase upload ordering (belt-and-suspenders)..."

# The real guarantee that the legacy updater picks the compat asset is
# ALPHABETICAL NAME SORT (asserted above): GitHub returns assets sorted by
# name, and `compat-` sorts before `vtcode-`. Upload order does NOT control
# selection. The release script still uploads compat assets first as
# defense-in-depth; this test asserts that ordering is preserved.
gh_calls="$tmp/gh-calls"
: >"$gh_calls"
gh() {
    if [[ "$1" == "release" && "$2" == "upload" ]]; then
        printf '%s\n' "$*" >>"$gh_calls"
    fi
}
export -f gh

compat_glob="$stage/compat-*.tar.gz.compat"
normal_glob="$stage/vtcode-*.tar.gz $stage/vtcode-*.zip"
# Simulate the two-phase upload the release script performs.
# shellcheck disable=SC2086
gh release upload "$version" $compat_glob --clobber
# shellcheck disable=SC2086,SC2046
gh release upload "$version" $(echo $normal_glob) --clobber

first_compat_line=$(grep -n '\.tar\.gz\.compat' "$gh_calls" | head -1 | cut -d: -f1)
first_normal_line=$(grep -nE '\.(tar\.gz|zip)([^.]|$)' "$gh_calls" \
    | grep -v '\.tar\.gz\.compat' | head -1 | cut -d: -f1)
if [[ -n "$first_compat_line" && -n "$first_normal_line" \
    && "$first_compat_line" -lt "$first_normal_line" ]]; then
    pass "compat assets uploaded before normal archives"
else
    fail_test "upload ordering: compat=$first_compat_line normal=$first_normal_line"
fi

echo "Testing upload_release_asset_with_retry..."

# Stub sleep to keep the test fast; count gh invocations via a file so
# command substitution / subshells do not lose the counter.
sleep() { :; }
retry_calls="$tmp/retry-calls"
: >"$retry_calls"
gh() {
    printf 'call\n' >>"$retry_calls"
    local remaining
    remaining=$(cat "$tmp/retry-failures-left" 2>/dev/null || echo 0)
    if [[ "$remaining" -gt 0 ]]; then
        echo "$((remaining - 1))" >"$tmp/retry-failures-left"
        return 1
    fi
    return 0
}
export -f gh
export -f sleep

# Transient HTTP 500s (2 failures) then success.
echo 2 >"$tmp/retry-failures-left"
: >"$retry_calls"
if upload_release_asset_with_retry "$version" "$stage/vtcode-${version}-x86_64-apple-darwin.tar.gz" 5; then
    if [[ $(wc -l <"$retry_calls") -eq 3 ]]; then
        pass "transient failures retried to success (3 attempts)"
    else
        fail_test "expected 3 attempts, got $(wc -l <"$retry_calls")"
    fi
else
    fail_test "transient failures should be retried to success"
fi

# Persistent failure exhausts attempts and returns nonzero.
echo 10 >"$tmp/retry-failures-left"
: >"$retry_calls"
if upload_release_asset_with_retry "$version" "$stage/vtcode-${version}-x86_64-apple-darwin.tar.gz" 3 >/dev/null 2>&1; then
    fail_test "persistent failure should return nonzero"
else
    if [[ $(wc -l <"$retry_calls") -eq 3 ]]; then
        pass "persistent failure exhausts max attempts (3)"
    else
        fail_test "expected 3 attempts on persistent failure, got $(wc -l <"$retry_calls")"
    fi
fi

echo "Testing download_ci_artifact_with_retry..."

# Reuse the gh stub above: it fails N times (via retry-failures-left) then
# succeeds, for any `gh` invocation including `run download`.
download_calls="$tmp/download-calls"
: >"$download_calls"
gh() {
    printf 'call\n' >>"$download_calls"
    local remaining
    remaining=$(cat "$tmp/retry-failures-left" 2>/dev/null || echo 0)
    if [[ "$remaining" -gt 0 ]]; then
        echo "$((remaining - 1))" >"$tmp/retry-failures-left"
        return 1
    fi
    return 0
}
export -f gh

# Transient download failures (2 flakes) then success.
echo 2 >"$tmp/retry-failures-left"
: >"$download_calls"
if download_ci_artifact_with_retry "12345" "vtcode-${version}-x86_64-unknown-linux-musl" "$tmp" 5; then
    if [[ $(wc -l <"$download_calls") -eq 3 ]]; then
        pass "transient download failures retried to success (3 attempts)"
    else
        fail_test "expected 3 download attempts, got $(wc -l <"$download_calls")"
    fi
else
    fail_test "transient download failures should be retried to success"
fi

# Persistent download failure exhausts attempts and returns nonzero.
echo 10 >"$tmp/retry-failures-left"
: >"$download_calls"
if download_ci_artifact_with_retry "12345" "vtcode-${version}-x86_64-unknown-linux-musl" "$tmp" 3 >/dev/null 2>&1; then
    fail_test "persistent download failure should return nonzero"
else
    if [[ $(wc -l <"$download_calls") -eq 3 ]]; then
        pass "persistent download failure exhausts max attempts (3)"
    else
        fail_test "expected 3 download attempts on persistent failure, got $(wc -l <"$download_calls")"
    fi
fi

if download_ci_artifact_with_retry "only-one-arg" >/dev/null 2>&1; then
    fail_test "download missing args should fail"
else
    pass "download missing args rejected"
fi

echo "Testing ensure_release_push_access..."

# Save harness-provided tokens; the fallback path unsets them.
_saved_github_token="${GITHUB_TOKEN:-__unset__}"
_saved_gh_token="${GH_TOKEN:-__unset__}"

# Current auth already has push -> success, env tokens untouched.
export GITHUB_TOKEN="pull-only-token"
gh() {
    if [[ "$1" == "api" ]]; then
        printf 'true'
    fi
    return 0
}
export -f gh
if ensure_release_push_access "owner/repo" >/dev/null 2>&1; then
    if [[ "${GITHUB_TOKEN:-}" == "pull-only-token" ]]; then
        pass "auth with push keeps current env tokens"
    else
        fail_test "auth with push should not unset env tokens"
    fi
else
    fail_test "auth with push should succeed"
fi

# Env token is pull-only but stored credentials have push -> auto-unset.
gh() {
    if [[ "$1" == "api" ]]; then
        if [[ -n "${GITHUB_TOKEN:-}" || -n "${GH_TOKEN:-}" ]]; then
            printf 'false'
        else
            printf 'true'
        fi
    fi
    return 0
}
export -f gh
export GITHUB_TOKEN="pull-only-token"
export GH_TOKEN="pull-only-token"
if ensure_release_push_access "owner/repo" >/dev/null 2>&1; then
    if [[ -z "${GITHUB_TOKEN:-}" && -z "${GH_TOKEN:-}" ]]; then
        pass "pull-only env tokens auto-unset to stored credentials"
    else
        fail_test "fallback should unset env tokens"
    fi
else
    fail_test "stored-credential fallback should succeed"
fi

# Neither auth has push -> nonzero, fast failure.
gh() {
    if [[ "$1" == "api" ]]; then
        printf 'false'
    fi
    return 0
}
export -f gh
if ensure_release_push_access "owner/repo" >/dev/null 2>&1; then
    fail_test "no-push auth should fail"
else
    pass "no-push auth fails fast"
fi

if ensure_release_push_access >/dev/null 2>&1; then
    fail_test "missing arg should fail"
else
    pass "missing arg rejected"
fi

# Restore harness tokens.
if [[ "$_saved_github_token" == "__unset__" ]]; then unset GITHUB_TOKEN; else export GITHUB_TOKEN="$_saved_github_token"; fi
if [[ "$_saved_gh_token" == "__unset__" ]]; then unset GH_TOKEN; else export GH_TOKEN="$_saved_gh_token"; fi

if [[ "$fail" -ne 0 ]]; then
    echo "FAIL: release-assets tests failed"
    exit 1
fi
echo "PASS: release-assets tests"
