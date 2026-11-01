#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

# ANSI styling for the launch summary; disabled for pipes, dumb terminals, or NO_COLOR.
if [[ -t 1 && -z "${NO_COLOR:-}" && "${TERM:-}" != "dumb" ]]; then
    BOLD='\033[1m'
    DIM='\033[2m'
    GREEN='\033[0;32m'
    YELLOW='\033[1;33m'
    CYAN='\033[0;36m'
    NC='\033[0m'
else
    BOLD=''
    DIM=''
    GREEN=''
    YELLOW=''
    CYAN=''
    NC=''
fi
OUTPUT_APP="${1:-${ROOT_DIR}/dist/VT Code.app}"
TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT_DIR}/target}"
if [[ "${TARGET_DIR}" != /* ]]; then
    TARGET_DIR="${PWD}/${TARGET_DIR}"
fi
if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
    TARGET_DIR="${TARGET_DIR}/${CARGO_BUILD_TARGET}"
fi
RELEASE_DIR="${TARGET_DIR}/release"

if [[ "$(uname -s)" != "Darwin" ]]; then
    printf 'Error: the VT Code desktop bundle must be built on macOS.\n' >&2
    exit 1
fi

MACOS_VERSION="$(/usr/bin/sw_vers -productVersion)"
MACOS_MAJOR="${MACOS_VERSION%%.*}"
if [[ ! "${MACOS_MAJOR}" =~ ^[0-9]+$ ]] || (( MACOS_MAJOR < 15 )); then
    printf 'Error: VT Code Desktop requires a macOS 15+ build host; found %s.\n' "${MACOS_VERSION}" >&2
    exit 1
fi

for tool in cargo codesign ditto plutil xcrun; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
        printf 'Error: required packaging tool is missing: %s\n' "${tool}" >&2
        exit 1
    fi
done

if [[ -e "${OUTPUT_APP}" ]]; then
    printf 'Error: output already exists: %s\n' "${OUTPUT_APP}" >&2
    exit 1
fi

OUTPUT_PARENT="$(dirname "${OUTPUT_APP}")"
mkdir -p "${OUTPUT_PARENT}"
TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/vtcode-desktop.XXXXXX")"
trap 'rm -rf "${TEMP_ROOT}"' EXIT
STAGED_APP="${TEMP_ROOT}/VT Code.app"
CONTENTS="${STAGED_APP}/Contents"

cargo build --locked --release --manifest-path "${ROOT_DIR}/Cargo.toml" -p vtcode -p vtcode-desktop

mkdir -p "${CONTENTS}/MacOS" "${CONTENTS}/Helpers" "${CONTENTS}/Resources"
install -m 755 "${RELEASE_DIR}/vtcode-desktop" "${CONTENTS}/MacOS/vtcode-desktop"
install -m 755 "${RELEASE_DIR}/vtcode" "${CONTENTS}/Helpers/vtcode"
install -m 644 "${ROOT_DIR}/LICENSE" "${CONTENTS}/Resources/LICENSE"
install -m 644 "${ROOT_DIR}/THIRD-PARTY-NOTICES" "${CONTENTS}/Resources/THIRD-PARTY-NOTICES"

# The bundle icon is the prebuilt Dynamic (split-tile) artwork: it reads in
# both light and dark Finder treatments, matching the desktop picker's
# default. The running app applies the saved Appearance selection
# (Dynamic/Light/Dark) to the Dock at startup. Without the artwork the
# build fails: never ship iconless. VTCODE_PACKAGE_TEST_ICON_SOURCE
# overrides the artwork path for failure-path tests.
ICON_SOURCE="${VTCODE_PACKAGE_TEST_ICON_SOURCE:-${ROOT_DIR}/resources/icons/vtcode-app-dynamic.icns}"
if [[ ! -f "${ICON_SOURCE}" ]]; then
    printf 'Error: bundle icon artwork is missing: %s\n' "${ICON_SOURCE}" >&2
    exit 1
fi
install -m 644 "${ICON_SOURCE}" "${CONTENTS}/Resources/VTCode.icns"

# Liquid Glass catalog for macOS 26+: layered Default/Dark/Tinted appearances
# composed by the OS at display time. Older systems ignore Assets.car and use
# the icns fallback above.
"${ROOT_DIR}/scripts/build-app-icon.sh" \
    "${ROOT_DIR}/resources/icons/AppIcon.icon" \
    "${CONTENTS}/Resources/Assets.car"

VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "${ROOT_DIR}/Cargo.toml" | head -n 1)"
cat >"${CONTENTS}/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>vtcode-desktop</string>
    <key>CFBundleIdentifier</key>
    <string>com.vinhnx.vtcode.desktop</string>
    <key>CFBundleName</key>
    <string>VT Code</string>
    <key>CFBundleDisplayName</key>
    <string>VT Code</string>
    <key>CFBundleIconFile</key>
    <string>VTCode.icns</string>
    <key>CFBundleIconName</key>
    <string>AppIcon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>NSHumanReadableCopyright</key>
    <string>Copyright 2025 Vinh Nguyen</string>
    <key>LSMinimumSystemVersion</key>
    <string>15.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST
plutil -lint "${CONTENTS}/Info.plist" >/dev/null

# Touch on Mac: the GUI has no native touch handling below GPUI and relies on
# AppKit's automatic touch-to-mouse emulation. Declaring NSIsTouchNative tells
# macOS the app handles touches directly, which switches that emulation off and
# makes every touch a no-op — the bundle must never ship the key.
if plutil -extract NSIsTouchNative raw "${CONTENTS}/Info.plist" >/dev/null 2>&1; then
    printf 'FAIL: Info.plist must not declare NSIsTouchNative; it disables the touch-to-mouse emulation the GUI relies on.\n' >&2
    exit 1
fi

# Ad hoc signatures are for local development; they do not grant Developer ID trust.
codesign --force --sign - "${CONTENTS}/Helpers/vtcode"
codesign --force --sign - "${CONTENTS}/MacOS/vtcode-desktop"
codesign --force --sign - "${STAGED_APP}"
codesign --verify --deep --strict "${STAGED_APP}"
ditto "${STAGED_APP}" "${OUTPUT_APP}"
# Verify the copied artifact that users will actually open.
codesign --verify --deep --strict "${OUTPUT_APP}"

printf '%b%b%s%b %b%s%b\n' "$GREEN" "$BOLD" 'Packaged locally signed app:' "$NC" "$BOLD" "${OUTPUT_APP}" "$NC"
printf '%b%s%b\n' "$YELLOW" 'Ad hoc signing does not provide Developer ID trust or notarization.' "$NC"

# Ad hoc builds trip Gatekeeper on first launch. Leave the unlock steps next
# to the bundle (it may be handed off without this terminal output) and print
# the short version here.
cat >"${OUTPUT_PARENT}/FIRST-LAUNCH.txt" <<NOTICE
VT Code Desktop - first launch
==============================
This build is signed ad hoc for local development (no Developer ID, no
notarization), so macOS Gatekeeper may block the first launch with "cannot
be opened because it is from an unidentified developer".

To open it:
  1. In Finder, right-click VT Code.app, choose Open, then Open again in
     the confirmation dialog, or
  2. After a blocked launch, open Settings > Privacy & Security and click
     Open Anyway next to the VT Code message.

You only need to do this once; later launches open normally. To
distribute without this prompt, sign with a Developer ID certificate
and notarize (see scripts/macos-release-signing.sh).
NOTICE
printf '%b%b%s%b %s\n' "$CYAN" "$BOLD" 'First launch:' "$NC" 'right-click the app and choose Open (or allow it in Settings > Privacy & Security).'
printf '%b%s%s%b\n' "$DIM" 'Details written to: ' "${OUTPUT_PARENT}/FIRST-LAUNCH.txt" "$NC"
