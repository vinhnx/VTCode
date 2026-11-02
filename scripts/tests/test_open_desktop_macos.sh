#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/vtcode-open-desktop-test.XXXXXX")"
trap 'rm -rf -- "${TEST_ROOT}"' EXIT

STUB_BIN="${TEST_ROOT}/bin"
APP_PATH="${TEST_ROOT}/VT Code Current Build.app"
OPEN_LOG="${TEST_ROOT}/open.log"
mkdir -p "${STUB_BIN}" "${APP_PATH}/Contents"
: >"${APP_PATH}/Contents/Info.plist"

cat >"${STUB_BIN}/open" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$@" >"${VTCODE_OPEN_DESKTOP_LOG}"
STUB
chmod +x "${STUB_BIN}/open"
export VTCODE_OPEN_DESKTOP_LOG="${OPEN_LOG}"
export PATH="${STUB_BIN}:${PATH}"

(
    cd "${TEST_ROOT}"
    "${ROOT_DIR}/scripts/open-desktop-macos.sh" "VT Code Current Build.app"
)
[[ "$(cat "${OPEN_LOG}")" == $'-n\n'"${APP_PATH}" ]]

if "${ROOT_DIR}/scripts/open-desktop-macos.sh" "${TEST_ROOT}/missing.app" >/dev/null 2>&1; then
    printf 'FAIL: launcher accepted a missing app bundle.\n' >&2
    exit 1
fi

printf 'PASS: exact app paths launch as a new instance and invalid bundles are rejected.\n'
