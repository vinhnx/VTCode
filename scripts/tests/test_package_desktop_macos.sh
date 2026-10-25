#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
    printf 'SKIP: macOS bundle packaging test requires Darwin.\n'
    exit 0
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/vtcode-package-test.XXXXXX")"
trap 'rm -rf -- "${TEST_ROOT}"' EXIT

STUB_BIN="${TEST_ROOT}/bin"
TARGET_DIR="${TEST_ROOT}/target"
TARGET_TRIPLE="aarch64-apple-darwin"
RELEASE_DIR="${TARGET_DIR}/${TARGET_TRIPLE}/release"
CODESIGN_LOG="${TEST_ROOT}/codesign.log"
CARGO_LOG="${TEST_ROOT}/cargo.log"
mkdir -p "${STUB_BIN}" "${RELEASE_DIR}"

cat >"${STUB_BIN}/cargo" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"${VTCODE_PACKAGE_TEST_CARGO_LOG}"
STUB

cat >"${STUB_BIN}/codesign" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"${VTCODE_PACKAGE_TEST_CODESIGN_LOG}"
last_arg="${!#}"
if [[ "${1:-}" == "--verify" && "${last_arg}" == "${VTCODE_PACKAGE_TEST_FAIL_VERIFY_PATH:-}" ]]; then
    exit 27
fi
STUB

cat >"${STUB_BIN}/sips" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
output=""
while (($#)); do
    if [[ "$1" == "--out" ]]; then
        output="$2"
        shift 2
    else
        shift
    fi
done
[[ -n "${output}" ]]
: >"${output}"
STUB

cat >"${STUB_BIN}/iconutil" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
output=""
while (($#)); do
    if [[ "$1" == "-o" ]]; then
        output="$2"
        shift 2
    else
        shift
    fi
done
[[ -n "${output}" ]]
: >"${output}"
STUB

chmod +x "${STUB_BIN}/cargo" "${STUB_BIN}/codesign" "${STUB_BIN}/sips" "${STUB_BIN}/iconutil"
printf 'custom target desktop binary\n' >"${RELEASE_DIR}/vtcode-desktop"
printf 'custom target CLI helper\n' >"${RELEASE_DIR}/vtcode"
chmod +x "${RELEASE_DIR}/vtcode-desktop" "${RELEASE_DIR}/vtcode"

export CARGO_TARGET_DIR="${TARGET_DIR}"
export CARGO_BUILD_TARGET="${TARGET_TRIPLE}"
export VTCODE_PACKAGE_TEST_CODESIGN_LOG="${CODESIGN_LOG}"
export VTCODE_PACKAGE_TEST_CARGO_LOG="${CARGO_LOG}"
export PATH="${STUB_BIN}:${PATH}"

SUCCESS_APP="${TEST_ROOT}/success/VT Code.app"
"${ROOT_DIR}/scripts/package-desktop-macos.sh" "${SUCCESS_APP}"
[[ "$(cat "${SUCCESS_APP}/Contents/MacOS/vtcode-desktop")" == "custom target desktop binary" ]]
[[ "$(cat "${SUCCESS_APP}/Contents/Helpers/vtcode")" == "custom target CLI helper" ]]
grep -Fq -- "build --locked --release --manifest-path ${ROOT_DIR}/Cargo.toml -p vtcode -p vtcode-desktop" "${CARGO_LOG}"
grep -Fq -- "--verify --deep --strict ${SUCCESS_APP}" "${CODESIGN_LOG}"
printf 'PASS: custom Cargo target artifacts are packaged and the copied app is verified.\n'

FAILED_APP="${TEST_ROOT}/failed/VT Code.app"
if VTCODE_PACKAGE_TEST_FAIL_VERIFY_PATH="${FAILED_APP}" \
    "${ROOT_DIR}/scripts/package-desktop-macos.sh" "${FAILED_APP}" >/dev/null 2>&1; then
    printf 'FAIL: package script accepted a failed final bundle verification.\n' >&2
    exit 1
fi
grep -Fq -- "--verify --deep --strict ${FAILED_APP}" "${CODESIGN_LOG}"
printf 'PASS: failed verification of the copied app fails packaging.\n'
