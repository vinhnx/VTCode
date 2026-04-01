#!/usr/bin/env bash

set -euo pipefail

TARGET="${1:-}"
STAGE_DIR="${2:-}"

if [[ -z "$STAGE_DIR" ]]; then
    if [[ -n "$TARGET" && $# -eq 1 ]]; then
        STAGE_DIR="$TARGET"
        TARGET=""
    else
        echo "usage: $0 [<target-triple>] <stage-dir>" >&2
        exit 1
    fi
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOTSTRAP_ENABLED="${VTCODE_GHOSTTY_VT_AUTO_SETUP:-1}"

if [[ -z "$TARGET" ]]; then
    TARGET="$(rustc -vV | sed -n 's/^host: //p')"
fi

if [[ -z "$TARGET" ]]; then
    echo "Unable to determine Rust host target" >&2
    exit 1
fi

supports_ghostty_runtime() {
    case "$TARGET" in
        *darwin* | *linux*)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

if ! supports_ghostty_runtime; then
    echo "Ghostty VT runtime is not supported on $TARGET. PTY snapshots will use legacy_vt100."
    exit 0
fi

has_staged_runtime_library() {
    compgen -G "$STAGE_DIR/libghostty-vt*.dylib" >/dev/null || \
        compgen -G "$STAGE_DIR/libghostty-vt*.so*" >/dev/null || \
        compgen -G "$STAGE_DIR/*.dll" >/dev/null
}

stage_runtime_libraries() {
    bash "$SCRIPT_DIR/stage-ghostty-vt-assets.sh" "$TARGET" "$(dirname "$STAGE_DIR")" >/dev/null 2>&1 || true
}

if [[ ! -d "$STAGE_DIR" ]]; then
    mkdir -p "$STAGE_DIR"
fi

stage_runtime_libraries

if has_staged_runtime_library; then
    echo "Ghostty VT runtime libraries staged in $STAGE_DIR"
    exit 0
fi

if [[ "$BOOTSTRAP_ENABLED" != "0" ]]; then
    echo "Bootstrapping Ghostty VT dev assets for $TARGET..."
    if ! bash "$SCRIPT_DIR/setup-ghostty-vt-dev.sh" "$TARGET"; then
        echo "Ghostty VT bootstrap failed. Falling back to legacy_vt100 snapshots." >&2
    else
        stage_runtime_libraries
    fi
fi

if has_staged_runtime_library; then
    echo "Ghostty VT runtime libraries staged in $STAGE_DIR"
else
    echo "Ghostty VT runtime libraries unavailable. PTY snapshots will use legacy_vt100."
fi
