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

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vtcode-actool.XXXXXX")"
trap 'rm -rf "${WORK_DIR}"' EXIT

xcrun actool "${ICON_BUNDLE}" \
    --compile "${WORK_DIR}" \
    --app-icon AppIcon \
    --include-all-app-icons \
    --enable-on-demand-resources NO \
    --development-region en \
    --target-device mac \
    --minimum-deployment-target 26.0 \
    --platform macosx \
    --optimization space \
    --output-partial-info-plist "${WORK_DIR}/partial.plist" >/dev/null

if [[ ! -f "${WORK_DIR}/Assets.car" ]]; then
    printf 'Error: actool did not emit Assets.car.\n' >&2
    exit 1
fi
mkdir -p "$(dirname "${OUTPUT_CAR}")"
install -m 644 "${WORK_DIR}/Assets.car" "${OUTPUT_CAR}"
printf 'Built %s\n' "${OUTPUT_CAR}"
