#!/usr/bin/env bash
# Contract tests for scripts/install.sh::verify_checksum.
#
# The shell installer shares the native updater's fail-closed checksum policy
# (docs/SECURITY.md): exact archive-name matching, no bare digests from an
# aggregate manifest, no conflicting records, a bounded metadata download, and
# an abort when no hashing tool is available. These stay hermetic: curl is
# stubbed from a fixtures directory and no network is touched.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
INSTALL_SH="${ROOT_DIR}/scripts/install.sh"

fail() {
	printf 'FAIL: %s\n' "$1" >&2
	exit 1
}
pass() {
	printf 'PASS: %s\n' "$1"
}

TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/vtcode-install-checksum-test.XXXXXX")"
export HOME="${TEST_ROOT}/home"
export INSTALL_DIR="${TEST_ROOT}/bin"
mkdir -p "${HOME}"

# Source helpers without running main (guarded by the BASH_SOURCE check).
# shellcheck disable=SC1090,SC1091
source "${INSTALL_SH}"
trap 'rm -rf -- "${TEST_ROOT}"; show_cursor' EXIT

# Override the release host after sourcing so every request stays local.
GITHUB_RELEASES="https://example.test/releases/download"
export GITHUB_RELEASES

STUB_BIN="${TEST_ROOT}/stub-bin"
mkdir -p "${STUB_BIN}"
cat >"${STUB_BIN}/curl" <<'STUB'
#!/bin/bash
set -u
fixtures="${VTCODE_INSTALL_TEST_FIXTURES:-}"
url="${!#}"
outfile=""
prev=""
for arg in "$@"; do
    if [[ "$prev" == "-o" ]]; then outfile="$arg"; fi
    prev="$arg"
done
base="$(basename "$url")"
if [[ -n "$outfile" && -n "$fixtures" && -f "$fixtures/$base" ]]; then
    cp "$fixtures/$base" "$outfile"
    exit 0
fi
exit 1
STUB
chmod +x "${STUB_BIN}/curl"
export PATH="${STUB_BIN}:${PATH}"

if command -v shasum >/dev/null 2>&1; then
	HASH=(shasum -a 256)
elif command -v sha256sum >/dev/null 2>&1; then
	HASH=(sha256sum)
else
	printf 'SKIP: no local SHA-256 tool available.\n'
	exit 0
fi

ARCHIVE_NAME="vtcode-1.2.3-aarch64-apple-darwin.tar.gz"
ARCHIVE="${TEST_ROOT}/${ARCHIVE_NAME}"
printf 'verified payload\n' >"${ARCHIVE}"
GOOD_DIGEST="$("${HASH[@]}" "${ARCHIVE}" | awk '{print $1}')"
BAD_DIGEST="0000000000000000000000000000000000000000000000000000000000000000"
OTHER_NAME="vtcode-1.2.3-x86_64-apple-darwin.tar.gz"

FIXTURES="${TEST_ROOT}/fixtures"
mkdir -p "${FIXTURES}"
export VTCODE_INSTALL_TEST_FIXTURES="${FIXTURES}"

write_aggregate() {
	printf '%s\n' "$1" >"${FIXTURES}/checksums.txt"
}

# Named aggregate record for the exact archive verifies.
write_aggregate "${GOOD_DIGEST}  ${ARCHIVE_NAME}"
verify_checksum "${ARCHIVE}" v1.2.3 "${ARCHIVE_NAME}" >/dev/null ||
	fail "a correct aggregate digest must verify"
pass "aggregate checksum for the exact archive verifies"

# Windows-style binary marker is tolerated (two tokens, leading '*').
write_aggregate "${GOOD_DIGEST} *${ARCHIVE_NAME}"
verify_checksum "${ARCHIVE}" v1.2.3 "${ARCHIVE_NAME}" >/dev/null ||
	fail "a bare-mode (*) aggregate record must verify"
pass "aggregate checksum with a binary-mode marker verifies"

# A digest for a different archive must never be used via substring matching.
write_aggregate "${GOOD_DIGEST}  ${OTHER_NAME}"
if verify_checksum "${ARCHIVE}" v1.2.3 "${ARCHIVE_NAME}" >/dev/null 2>&1; then
	fail "another archive's digest must not verify this archive"
fi
pass "aggregate checksum for a different archive is rejected"

# Conflicting digests for the same archive make the manifest unusable.
write_aggregate "${GOOD_DIGEST}  ${ARCHIVE_NAME}
${BAD_DIGEST}  ${ARCHIVE_NAME}"
if verify_checksum "${ARCHIVE}" v1.2.3 "${ARCHIVE_NAME}" >/dev/null 2>&1; then
	fail "conflicting aggregate digests must be rejected"
fi
pass "conflicting aggregate digests fail closed"

# A bare digest is only valid in a per-archive sidecar, never in the manifest.
write_aggregate "${GOOD_DIGEST}"
if verify_checksum "${ARCHIVE}" v1.2.3 "${ARCHIVE_NAME}" >/dev/null 2>&1; then
	fail "a bare digest in the aggregate manifest must be rejected"
fi
pass "bare digest in an aggregate manifest fails closed"

rm -f "${FIXTURES}/checksums.txt"
printf '%s\n' "${GOOD_DIGEST}" >"${FIXTURES}/${ARCHIVE_NAME}.sha256"
verify_checksum "${ARCHIVE}" v1.2.3 "${ARCHIVE_NAME}" >/dev/null ||
	fail "a single bare digest in the archive sidecar must verify"
pass "bare digest in a per-archive sidecar verifies"

# Sidecar named records must also match the exact archive name.
printf '%s  %s\n' "${BAD_DIGEST}" "${OTHER_NAME}" >"${FIXTURES}/${ARCHIVE_NAME}.sha256"
if verify_checksum "${ARCHIVE}" v1.2.3 "${ARCHIVE_NAME}" >/dev/null 2>&1; then
	fail "a sidecar naming a different archive must be rejected"
fi
pass "sidecar with a mismatched filename fails closed"

# The legacy extension-stripped sidecar remains supported.
LEGACY_SIDECAR="vtcode-1.2.3-aarch64-apple-darwin.sha256"
printf '%s\n' "${GOOD_DIGEST}" >"${FIXTURES}/${LEGACY_SIDECAR}"
rm -f "${FIXTURES}/${ARCHIVE_NAME}.sha256"
verify_checksum "${ARCHIVE}" v1.2.3 "${ARCHIVE_NAME}" >/dev/null ||
	fail "the legacy extension-stripped sidecar must verify"
pass "legacy extension-stripped sidecar verifies"

# An oversized metadata file is never parsed as checksum data.
head -c $((MAX_CHECKSUM_BYTES + 1)) /dev/zero | tr '\0' 'a' >"${FIXTURES}/${LEGACY_SIDECAR}"
if verify_checksum "${ARCHIVE}" v1.2.3 "${ARCHIVE_NAME}" >/dev/null 2>&1; then
	fail "oversized checksum metadata must be rejected"
fi
pass "oversized checksum metadata fails closed"

# Missing metadata is an abort, not a warning (matches src/AGENTS.md).
rm -f "${FIXTURES}/${LEGACY_SIDECAR}"
missing_output="$(verify_checksum "${ARCHIVE}" v1.2.3 "${ARCHIVE_NAME}" 2>&1)" &&
	fail "missing checksum metadata must fail the install"
printf '%s' "${missing_output}" | grep -F 'refusing to install it unverified' >/dev/null ||
	fail "missing metadata must name the refusal"
pass "missing checksum metadata fails closed with a clear reason"

# No hashing tool on PATH must fail rather than skipping verification.
MIN_BIN="${TEST_ROOT}/min-bin"
mkdir -p "${MIN_BIN}"
cp "${STUB_BIN}/curl" "${MIN_BIN}/curl"
for tool in awk wc tr mktemp rm basename cp; do
	tool_path="$(command -v "${tool}")" || fail "missing host tool: ${tool}"
	ln -s "${tool_path}" "${MIN_BIN}/${tool}"
done
printf '%s\n' "${GOOD_DIGEST}" >"${FIXTURES}/${ARCHIVE_NAME}.sha256"
tool_output="$(PATH="${MIN_BIN}" verify_checksum "${ARCHIVE}" v1.2.3 "${ARCHIVE_NAME}" 2>&1)" &&
	fail "a missing hashing tool must fail the install"
printf '%s' "${tool_output}" | grep -F 'refusing to install unverified' >/dev/null ||
	fail "missing hashing tool must name the refusal"
pass "missing hashing tool fails closed"
