#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
OUTPUT_APP="${1:-${ROOT_DIR}/dist/VT Code.app}"

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

for tool in cargo codesign ditto; do
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
install -m 755 "${ROOT_DIR}/target/release/vtcode-desktop" "${CONTENTS}/MacOS/vtcode-desktop"
install -m 755 "${ROOT_DIR}/target/release/vtcode" "${CONTENTS}/Helpers/vtcode"
install -m 644 "${ROOT_DIR}/LICENSE" "${CONTENTS}/Resources/LICENSE"
install -m 644 "${ROOT_DIR}/THIRD-PARTY-NOTICES" "${CONTENTS}/Resources/THIRD-PARTY-NOTICES"

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

# Ad hoc signatures are for local development; they do not grant Developer ID trust.
codesign --force --sign - "${CONTENTS}/Helpers/vtcode"
codesign --force --sign - "${CONTENTS}/MacOS/vtcode-desktop"
codesign --force --sign - "${STAGED_APP}"
codesign --verify --deep --strict "${STAGED_APP}"
ditto "${STAGED_APP}" "${OUTPUT_APP}"

printf 'Packaged locally signed app: %s\n' "${OUTPUT_APP}"
printf 'Ad hoc signing does not provide Developer ID trust or notarization.\n'
