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
printf 'VTCODE_DESKTOP_BUILD_VERSION=%s\n' "${VTCODE_DESKTOP_BUILD_VERSION:-}" >>"${VTCODE_PACKAGE_TEST_CARGO_LOG}"
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
cmp -s "${ROOT_DIR}/LICENSE-MIT" "${SUCCESS_APP}/Contents/Resources/LICENSE-MIT"
cmp -s "${ROOT_DIR}/resources/icons/vtcode-app-dynamic.icns" "${SUCCESS_APP}/Contents/Resources/VTCode.icns"
# The Liquid Glass catalog needs the macOS 26+ SDK (mirrors the gate in
# build-app-icon.sh); older build hosts skip it and the icns fallback above
# covers them.
TEST_SDK_MAJOR="$(xcrun --sdk macosx --show-sdk-version 2>/dev/null || true)"
TEST_SDK_MAJOR="${TEST_SDK_MAJOR%%.*}"
if [[ "${TEST_SDK_MAJOR}" =~ ^[0-9]+$ ]] && ((TEST_SDK_MAJOR >= 26)); then
	[[ -f "${SUCCESS_APP}/Contents/Resources/Assets.car" ]]
	xcrun --sdk macosx assetutil --info "${SUCCESS_APP}/Contents/Resources/Assets.car" >"${TEST_ROOT}/asset-info.json"
	grep -Fq -- "NSAppearanceNameDarkAqua" "${TEST_ROOT}/asset-info.json"
	grep -Fq -- "ISAppearanceTintable" "${TEST_ROOT}/asset-info.json"
	printf 'PASS: macOS 26+ SDK builds the layered Liquid Glass catalog.\n'
else
	[[ ! -f "${SUCCESS_APP}/Contents/Resources/Assets.car" ]]
	printf 'PASS: older SDK skips Assets.car; the icns fallback covers the bundle.\n'
fi
grep -Fq -- "<key>CFBundleIconName</key>" "${SUCCESS_APP}/Contents/Info.plist"
grep -Fq -- "<key>CFBundleURLTypes</key>" "${SUCCESS_APP}/Contents/Info.plist"
grep -Fq -- "<string>vtcode</string>" "${SUCCESS_APP}/Contents/Info.plist"
printf 'PASS: bundle claims the vtcode:// handoff scheme.\n'
if grep -Fq -- "<key>NSIsTouchNative</key>" "${SUCCESS_APP}/Contents/Info.plist"; then
	printf 'FAIL: bundle declares NSIsTouchNative; that disables the touch-to-mouse emulation the GUI relies on.\n' >&2
	exit 1
fi
printf 'PASS: bundle omits NSIsTouchNative, so system touch emulation stays active.\n'
# About panel: the plist's build version must match the commit stamped into the
# binary, and the bundled Credits.html supplies the repository and
# acknowledgments links.
EXPECTED_BUILD_VERSION="$(git -C "${ROOT_DIR}" rev-parse --short=12 HEAD 2>/dev/null || true)"
if [[ -z "${EXPECTED_BUILD_VERSION}" ]]; then
	EXPECTED_BUILD_VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "${ROOT_DIR}/Cargo.toml" | head -n 1)"
fi
[[ "$(plutil -extract CFBundleVersion raw "${SUCCESS_APP}/Contents/Info.plist")" == "${EXPECTED_BUILD_VERSION}" ]]
grep -Fq -- "VTCODE_DESKTOP_BUILD_VERSION=${EXPECTED_BUILD_VERSION}" "${CARGO_LOG}"
cmp -s "${ROOT_DIR}/crates/codegen/vtcode-desktop/Credits.html" "${SUCCESS_APP}/Contents/Resources/Credits.html"
grep -Fq -- "github.com/vinhnx/vtcode" "${SUCCESS_APP}/Contents/Resources/Credits.html"
printf 'PASS: About panel reports the build commit and ships GitHub/acknowledgments credits.\n'
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

# Re-running against the existing bundle without --force must refuse and hint
# at the overwrite flag.
if "${ROOT_DIR}/scripts/package-desktop-macos.sh" "${SUCCESS_APP}" >/dev/null 2>"${TEST_ROOT}/exists.err"; then
	printf 'FAIL: packaging over an existing bundle without --force must fail.\n' >&2
	exit 1
fi
grep -Fq -- "--force" "${TEST_ROOT}/exists.err"
printf 'PASS: existing output without --force fails with an overwrite hint.\n'

# A failed --force run must leave the previous bundle untouched: the old app
# is only removed once the new one is signed and verified.
printf 'previous bundle marker\n' >"${SUCCESS_APP}/Contents/previous-marker.txt"
if VTCODE_PACKAGE_TEST_ICON_SOURCE="${TEST_ROOT}/no-such-icon.icns" \
	"${ROOT_DIR}/scripts/package-desktop-macos.sh" --force "${SUCCESS_APP}" >/dev/null 2>&1; then
	printf 'FAIL: --force with missing icon artwork must fail packaging.\n' >&2
	exit 1
fi
[[ "$(cat "${SUCCESS_APP}/Contents/previous-marker.txt")" == "previous bundle marker" ]]
printf 'PASS: failed --force run preserves the previous bundle.\n'

# --force after the positional path (option order must not matter) replaces
# the bundle.
"${ROOT_DIR}/scripts/package-desktop-macos.sh" "${SUCCESS_APP}" --force >/dev/null
[[ "$(cat "${SUCCESS_APP}/Contents/MacOS/vtcode-desktop")" == "custom target desktop binary" ]]
[[ ! -e "${SUCCESS_APP}/Contents/previous-marker.txt" ]]
printf 'PASS: --force overwrites the existing bundle.\n'

# --skip-build reuses the release binaries without invoking cargo.
SKIP_BUILD_APP="${TEST_ROOT}/skip-build/VT Code.app"
CARGO_LINES_BEFORE="$(wc -l <"${CARGO_LOG}" | tr -d ' ')"
"${ROOT_DIR}/scripts/package-desktop-macos.sh" --skip-build "${SKIP_BUILD_APP}" >/dev/null
[[ "$(cat "${SKIP_BUILD_APP}/Contents/Helpers/vtcode")" == "custom target CLI helper" ]]
CARGO_LINES_AFTER="$(wc -l <"${CARGO_LOG}" | tr -d ' ')"
[[ "${CARGO_LINES_BEFORE}" == "${CARGO_LINES_AFTER}" ]]
printf 'PASS: --skip-build reuses release binaries without invoking cargo.\n'

# --skip-build with no binaries to reuse must fail instead of shipping nothing.
if CARGO_TARGET_DIR="${TEST_ROOT}/empty-target" \
	"${ROOT_DIR}/scripts/package-desktop-macos.sh" --skip-build "${TEST_ROOT}/empty/VT Code.app" >/dev/null 2>&1; then
	printf 'FAIL: --skip-build with missing binaries must fail.\n' >&2
	exit 1
fi
printf 'PASS: --skip-build with missing binaries fails.\n'

# --no-assets skips the Liquid Glass catalog on every host (older SDKs skip
# it anyway; macOS 26+ SDKs would otherwise build it).
NO_ASSETS_APP="${TEST_ROOT}/no-assets/VT Code.app"
"${ROOT_DIR}/scripts/package-desktop-macos.sh" --no-assets "${NO_ASSETS_APP}" >/dev/null
[[ ! -f "${NO_ASSETS_APP}/Contents/Resources/Assets.car" ]]
cmp -s "${ROOT_DIR}/resources/icons/vtcode-app-dynamic.icns" "${NO_ASSETS_APP}/Contents/Resources/VTCode.icns"
printf 'PASS: --no-assets ships the icns fallback without Assets.car.\n'

"${ROOT_DIR}/scripts/package-desktop-macos.sh" --help >"${TEST_ROOT}/help.txt"
grep -Fq -- "Usage:" "${TEST_ROOT}/help.txt"
grep -Fq -- "--force" "${TEST_ROOT}/help.txt"
printf 'PASS: --help describes usage and the overwrite flag.\n'

if "${ROOT_DIR}/scripts/package-desktop-macos.sh" --no-such-flag >/dev/null 2>&1; then
	printf 'FAIL: unknown option must fail.\n' >&2
	exit 1
fi
printf 'PASS: unknown option fails.\n'
