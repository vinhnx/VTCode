#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 <target-triple> <release-dir>" >&2
    exit 2
fi

TARGET="$1"
RELEASE_DIR="$2"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

requires_bundled_runtime() {
    case "$TARGET" in
        *darwin* | *linux*)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

if ! requires_bundled_runtime; then
    echo "Ghostty VT release assets are not required for $TARGET" >&2
    exit 0
fi

if VTCODE_GHOSTTY_VT_REQUIRE_ASSETS=1 bash "$SCRIPT_DIR/stage-ghostty-vt-assets.sh" "$TARGET" "$RELEASE_DIR"; then
    exit 0
fi

bash "$SCRIPT_DIR/setup-ghostty-vt-dev.sh" "$TARGET"
VTCODE_GHOSTTY_VT_REQUIRE_ASSETS=1 bash "$SCRIPT_DIR/stage-ghostty-vt-assets.sh" "$TARGET" "$RELEASE_DIR"
