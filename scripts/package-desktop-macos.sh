#!/usr/bin/env bash
# Package the VT Code desktop GUI as a locally (ad hoc) signed macOS .app.
#
# Usage: package-desktop-macos.sh [options] [output-path]
#
# Options:
#   -f, --force, --overwrite   Remove an existing output bundle before packaging.
#   --skip-build, --no-build   Reuse the binaries already in the release dir.
#   --no-assets, --skip-assets  Skip the Liquid Glass Assets.car catalog
#                              (the icns fallback still ships).
#   -h, --help                 Show this help and exit.
#
# Environment (each accepts "1"; equivalent to the matching flag):
#   VTCODE_PACKAGE_FORCE, VTCODE_PACKAGE_SKIP_BUILD, VTCODE_PACKAGE_SKIP_ASSETS

set -euo pipefail

START_SECONDS=${SECONDS:-0}

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

usage() {
    cat <<USAGE
Usage: $(basename "$0") [options] [output-path]

Package the VT Code desktop GUI as a locally (ad hoc) signed macOS .app.

Arguments:
  output-path                 Destination bundle (default: <repo>/dist/VT Code.app).

Options:
  -f, --force, --overwrite   Remove an existing output bundle before packaging.
                             Without this flag the script refuses to overwrite.
  --skip-build, --no-build   Skip 'cargo build' and reuse the binaries already
                             in the release directory.
  --no-assets, --skip-assets  Skip compiling the Liquid Glass Assets.car catalog;
                             the icns fallback still ships.
  -h, --help                 Show this help and exit.
USAGE
}

FORCE=0
SKIP_BUILD=0
SKIP_ASSETS=0
OUTPUT_APP=""

while (($# > 0)); do
    case "$1" in
        -f|--force|--overwrite)
            FORCE=1
            shift
            ;;
        --skip-build|--no-build)
            SKIP_BUILD=1
            shift
            ;;
        --no-assets|--skip-assets)
            SKIP_ASSETS=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        --)
            shift
            break
            ;;
        -*)
            printf 'Error: unknown option: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
        *)
            if [[ -n "${OUTPUT_APP}" ]]; then
                printf 'Error: unexpected argument: %s\n' "$1" >&2
                usage >&2
                exit 2
            fi
            OUTPUT_APP="$1"
            shift
            ;;
    esac
done

# Allow one positional argument after a `--` separator.
while (($# > 0)); do
    if [[ -n "${OUTPUT_APP}" ]]; then
        printf 'Error: unexpected argument: %s\n' "$1" >&2
        usage >&2
        exit 2
    fi
    OUTPUT_APP="$1"
    shift
done

if [[ "${VTCODE_PACKAGE_FORCE:-}" == "1" ]]; then
    FORCE=1
fi
if [[ "${VTCODE_PACKAGE_SKIP_BUILD:-}" == "1" ]]; then
    SKIP_BUILD=1
fi
if [[ "${VTCODE_PACKAGE_SKIP_ASSETS:-}" == "1" ]]; then
    SKIP_ASSETS=1
fi

if [[ -z "${OUTPUT_APP}" ]]; then
    OUTPUT_APP="${ROOT_DIR}/dist/VT Code.app"
fi

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

# Only require the tools this run actually uses, so --skip-build and
# --no-assets work on hosts without cargo or an Xcode toolchain.
REQUIRED_TOOLS="codesign ditto plutil"
if (( ! SKIP_BUILD )); then
    REQUIRED_TOOLS="${REQUIRED_TOOLS} cargo"
fi
if (( ! SKIP_ASSETS )); then
    REQUIRED_TOOLS="${REQUIRED_TOOLS} xcrun"
fi
# shellcheck disable=SC2086
for tool in ${REQUIRED_TOOLS}; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
        printf 'Error: required packaging tool is missing: %s\n' "${tool}" >&2
        exit 1
    fi
done

# Fail fast before the multi-minute cargo build: validate every static input
# up front so a missing file aborts in milliseconds, not after compiling.
for required_file in "${ROOT_DIR}/LICENSE" "${ROOT_DIR}/THIRD-PARTY-NOTICES"; do
    if [[ ! -f "${required_file}" ]]; then
        printf 'Error: required bundle input is missing: %s\n' "${required_file}" >&2
        exit 1
    fi
done

# The bundle icon is the prebuilt Dynamic (split-tile) artwork: it reads in
# both light and dark Finder treatments, matching the desktop picker's
# default. The running app applies the saved Appearance selection
# (Dynamic/Light/Dark/Clear/Tinted) to the Dock at startup. Without the artwork the
# build fails: never ship iconless. VTCODE_PACKAGE_TEST_ICON_SOURCE
# overrides the artwork path for failure-path tests.
ICON_SOURCE="${VTCODE_PACKAGE_TEST_ICON_SOURCE:-${ROOT_DIR}/resources/icons/vtcode-app-dynamic.icns}"
if [[ ! -f "${ICON_SOURCE}" ]]; then
    printf 'Error: bundle icon artwork is missing: %s\n' "${ICON_SOURCE}" >&2
    exit 1
fi

if (( ! SKIP_ASSETS )) && [[ ! -f "${ROOT_DIR}/resources/icons/AppIcon.icon/icon.json" ]]; then
    printf 'Error: icon catalog source is missing: %s\n' "${ROOT_DIR}/resources/icons/AppIcon.icon/icon.json" >&2
    exit 1
fi

VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "${ROOT_DIR}/Cargo.toml" | head -n 1)"
if [[ -z "${VERSION}" ]]; then
    printf 'Error: could not parse the workspace version from %s\n' "${ROOT_DIR}/Cargo.toml" >&2
    exit 1
fi

DESKTOP_BIN="${RELEASE_DIR}/vtcode-desktop"
HELPER_BIN="${RELEASE_DIR}/vtcode"
if (( SKIP_BUILD )); then
    for bin in "${DESKTOP_BIN}" "${HELPER_BIN}"; do
        if [[ ! -x "${bin}" ]]; then
            printf 'Error: --skip-build was given but %s is missing; run without --skip-build to build it.\n' "${bin}" >&2
            exit 1
        fi
    done
fi

# Refuse to overwrite by default; with --force the old bundle is kept until
# the new one is signed and verified, then atomically replaced just before
# the final copy, so a failed build never deletes a good bundle.
OVERWRITE=0
if [[ -e "${OUTPUT_APP}" ]]; then
    if (( FORCE )); then
        case "${OUTPUT_APP}" in
            "/"|"//")
                printf 'Error: refusing to overwrite %s\n' "${OUTPUT_APP}" >&2
                exit 1
                ;;
        esac
        OVERWRITE=1
    else
        printf 'Error: output already exists: %s (use --force to overwrite)\n' "${OUTPUT_APP}" >&2
        exit 1
    fi
fi

OUTPUT_PARENT="$(dirname "${OUTPUT_APP}")"
mkdir -p "${OUTPUT_PARENT}"
TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/vtcode-desktop.XXXXXX")"
trap 'rm -rf -- "${TEMP_ROOT:-}"' EXIT
STAGED_APP="${TEMP_ROOT}/VT Code.app"
CONTENTS="${STAGED_APP}/Contents"

if (( SKIP_BUILD )); then
    printf '%b%s%b\n' "${DIM}" "Skipping cargo build; reusing ${RELEASE_DIR}." "${NC}"
else
    cargo build --locked --release --manifest-path "${ROOT_DIR}/Cargo.toml" -p vtcode -p vtcode-desktop
fi

# A misconfigured CARGO_TARGET_DIR/CARGO_BUILD_TARGET otherwise surfaces as a
# cryptic install(1) failure below; name the missing binary instead.
for bin in "${DESKTOP_BIN}" "${HELPER_BIN}"; do
    if [[ ! -f "${bin}" ]]; then
        printf 'Error: expected binary is missing after the build: %s\n' "${bin}" >&2
        printf 'Check CARGO_TARGET_DIR and CARGO_BUILD_TARGET.\n' >&2
        exit 1
    fi
done

mkdir -p "${CONTENTS}/MacOS" "${CONTENTS}/Helpers" "${CONTENTS}/Resources"
install -m 755 "${DESKTOP_BIN}" "${CONTENTS}/MacOS/vtcode-desktop"
install -m 755 "${HELPER_BIN}" "${CONTENTS}/Helpers/vtcode"
install -m 644 "${ROOT_DIR}/LICENSE" "${CONTENTS}/Resources/LICENSE"
install -m 644 "${ROOT_DIR}/THIRD-PARTY-NOTICES" "${CONTENTS}/Resources/THIRD-PARTY-NOTICES"
install -m 644 "${ICON_SOURCE}" "${CONTENTS}/Resources/VTCode.icns"

if (( SKIP_ASSETS )); then
    printf '%b%s%b\n' "${DIM}" 'Skipping Assets.car; the icns fallback covers the bundle.' "${NC}"
else
    # Liquid Glass catalog for macOS 26+: layered Default/Dark/Tinted appearances
    # composed by the OS at display time. Older systems ignore Assets.car and use
    # the icns fallback above. Build hosts with an older SDK skip the catalog
    # with a warning instead of failing the bundle.
    "${ROOT_DIR}/scripts/build-app-icon.sh" \
        "${ROOT_DIR}/resources/icons/AppIcon.icon" \
        "${CONTENTS}/Resources/Assets.car"
    if [[ ! -f "${CONTENTS}/Resources/Assets.car" ]]; then
        printf 'Warning: continuing without Assets.car; the icns fallback covers systems without the Liquid Glass catalog.\n' >&2
    fi
fi

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
if (( OVERWRITE )); then
    rm -rf -- "${OUTPUT_APP}"
fi
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
  2. After a blocked launch, open System Settings > Privacy & Security and
     click Open Anyway next to the VT Code message.

You only need to do this once; later launches open normally. To
distribute without this prompt, sign with a Developer ID certificate
and notarize (see scripts/macos-release-signing.sh).
NOTICE
printf '%b%b%s%b %s\n' "$CYAN" "$BOLD" 'First launch:' "$NC" 'right-click the app and choose Open (or allow it in System Settings > Privacy & Security).'
printf '%b%s%s%b\n' "$DIM" 'Details written to: ' "${OUTPUT_PARENT}/FIRST-LAUNCH.txt" "$NC"

ELAPSED=$((SECONDS - START_SECONDS))
if (( ELAPSED >= 60 )); then
    DURATION="$((ELAPSED / 60))m$((ELAPSED % 60))s"
else
    DURATION="${ELAPSED}s"
fi
printf '%b%s%s%b\n' "$DIM" 'Done in ' "${DURATION}." "$NC"
