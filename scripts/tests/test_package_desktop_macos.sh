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

chmod +x "${STUB_BIN}/cargo" "${STUB_BIN}/codesign"
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
cmp -s "${ROOT_DIR}/THIRD-PARTY-NOTICES" "${SUCCESS_APP}/Contents/Resources/THIRD-PARTY-NOTICES"
cmp -s "${ROOT_DIR}/LICENSE" "${SUCCESS_APP}/Contents/Resources/LICENSE"
cmp -s "${ROOT_DIR}/resources/icons/vtcode-app-dynamic.icns" "${SUCCESS_APP}/Contents/Resources/VTCode.icns"
[[ -f "${SUCCESS_APP}/Contents/Resources/Assets.car" ]]
grep -Fq -- "<key>CFBundleIconName</key>" "${SUCCESS_APP}/Contents/Info.plist"
xcrun --sdk macosx assetutil --info "${SUCCESS_APP}/Contents/Resources/Assets.car" > "${TEST_ROOT}/asset-info.json"
grep -Fq -- "NSAppearanceNameDarkAqua" "${TEST_ROOT}/asset-info.json"
grep -Fq -- "ISAppearanceTintable" "${TEST_ROOT}/asset-info.json"
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

MISSING_ICON_APP="${TEST_ROOT}/missing-icon/VT Code.app"
if VTCODE_PACKAGE_TEST_ICON_SOURCE="${TEST_ROOT}/no-such-icon.icns" \
    "${ROOT_DIR}/scripts/package-desktop-macos.sh" "${MISSING_ICON_APP}" >/dev/null 2>&1; then
    printf 'FAIL: missing bundle icon artwork must fail packaging.\n' >&2
    exit 1
fi
printf 'PASS: missing bundle icon artwork fails packaging.\n'
