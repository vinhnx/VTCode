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

for tool in cargo codesign ditto sips iconutil plutil; do
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

# Build the native icon from the same artwork embedded in the desktop logo.
ICONSET="${TEMP_ROOT}/VTCode.iconset"
mkdir -p "${ICONSET}"
for size in 16 32 128 256 512; do
    sips -z "${size}" "${size}" "${ROOT_DIR}/resources/icons/vtcode-profile-512.png" \
        --out "${ICONSET}/icon_${size}x${size}.png" >/dev/null
    retina_size=$((size * 2))
    sips -z "${retina_size}" "${retina_size}" "${ROOT_DIR}/resources/icons/vtcode-profile-512.png" \
        --out "${ICONSET}/icon_${size}x${size}@2x.png" >/dev/null
done
# iconutil's icns-write path is broken on some hosts (macOS 27 rejects even
# previously working iconsets while the read path works). When the artwork is
# intact but iconutil fails, reuse the icon from the newest prior bundle in
# the output directory instead of aborting, and say so loudly. Without intact
# artwork or a fallback icon the build still fails: never ship iconless.
if ! iconutil -c icns "${ICONSET}" -o "${CONTENTS}/Resources/VTCode.icns" 2>/dev/null; then
    if [[ "$(find "${ICONSET}" -name '*.png' | wc -l)" -ne 10 ]]; then
        printf 'Error: icon artwork is incomplete; refusing icon fallback.\n' >&2
        exit 1
    fi
    FALLBACK_ICON=""
    FALLBACK_MTIME=0
    for candidate in "${OUTPUT_PARENT}"/*.app/Contents/Resources/VTCode.icns; do
        [[ -e "${candidate}" ]] || continue
        mtime="$(stat -f '%m' "${candidate}")"
        if (( mtime > FALLBACK_MTIME )); then
            FALLBACK_MTIME="${mtime}"
            FALLBACK_ICON="${candidate}"
        fi
    done
    if [[ -z "${FALLBACK_ICON}" ]]; then
        printf 'Error: iconutil failed and no prior-bundle VTCode.icns fallback exists in %s.\n' "${OUTPUT_PARENT}" >&2
        exit 1
    fi
    printf 'Warning: iconutil failed on this host; reusing icon from %s.\n' "${FALLBACK_ICON}" >&2
    cp "${FALLBACK_ICON}" "${CONTENTS}/Resources/VTCode.icns"
fi

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
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>LSMinimumSystemVersion</key>
    <string>15.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST
plutil -lint "${CONTENTS}/Info.plist" >/dev/null

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
