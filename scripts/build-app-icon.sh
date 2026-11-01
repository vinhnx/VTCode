#!/usr/bin/env bash
# Compile an Icon Composer .icon bundle into Assets.car via actool.
# Usage: build-app-icon.sh <AppIcon.icon> <output Assets.car>
# Requires Xcode CLT or full Xcode (xcrun + macosx SDK). Fails loudly when
# actool is missing: the Liquid Glass catalog cannot be produced without it.
set -euo pipefail

if (($# != 2)); then
    printf 'Usage: %s <AppIcon.icon> <output Assets.car>\n' "$0" >&2
    exit 2
fi

ICON_BUNDLE="$1"
OUTPUT_CAR="$2"

if ! command -v xcrun >/dev/null 2>&1; then
    printf 'Error: xcrun not found; install Xcode command line tools to build Assets.car.\n' >&2
    exit 1
fi
if [[ ! -f "${ICON_BUNDLE}/icon.json" ]]; then
    printf 'Error: icon bundle is missing icon.json: %s\n' "${ICON_BUNDLE}" >&2
    exit 1
fi

# The layered Liquid Glass catalog needs the macOS 26+ SDK. Older build hosts
# (including CI runners on macos-15 images) cannot produce it; skip with a
# warning and let the icns fallback installed by the packaging script cover
# them, instead of failing the whole bundle.
SDK_VERSION="$(xcrun --sdk macosx --show-sdk-version 2>/dev/null || true)"
SDK_MAJOR="${SDK_VERSION%%.*}"
if [[ ! "${SDK_MAJOR}" =~ ^[0-9]+$ ]] || (( SDK_MAJOR < 26 )); then
    printf 'Warning: macOS SDK %s cannot build the Liquid Glass catalog (needs 26+); skipping Assets.car.\n' \
        "${SDK_VERSION:-unknown}" >&2
    exit 0
fi

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vtcode-actool.XXXXXX")"
trap 'rm -rf "${WORK_DIR}"' EXIT

actool_output=""
if ! actool_output="$(xcrun actool "${ICON_BUNDLE}" \
    --compile "${WORK_DIR}" \
    --app-icon AppIcon \
    --include-all-app-icons \
    --enable-on-demand-resources NO \
    --development-region en \
    --target-device mac \
    --minimum-deployment-target 26.0 \
    --platform macosx \
    --optimization space \
    --output-partial-info-plist "${WORK_DIR}/partial.plist" 2>&1)"; then
    printf 'Error: actool failed to compile the icon catalog:\n%s\n' "${actool_output}" >&2
    exit 1
fi

if [[ ! -f "${WORK_DIR}/Assets.car" ]]; then
    printf 'Error: actool did not emit Assets.car:\n%s\n' "${actool_output}" >&2
    exit 1
fi
mkdir -p "$(dirname "${OUTPUT_CAR}")"
install -m 644 "${WORK_DIR}/Assets.car" "${OUTPUT_CAR}"
printf 'Built %s\n' "${OUTPUT_CAR}"
