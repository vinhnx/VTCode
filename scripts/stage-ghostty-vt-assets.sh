#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 <target-triple> <release-dir>" >&2
    exit 2
fi

TARGET="$1"
RELEASE_DIR="$2"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ASSET_ROOT="${VTCODE_GHOSTTY_VT_ASSET_DIR:-$REPO_ROOT/dist/ghostty-vt}"
REQUIRE_ASSETS="${VTCODE_GHOSTTY_VT_REQUIRE_ASSETS:-0}"

has_asset_layout() {
    local path="$1"
    [[ -f "$path/include/ghostty/vt.h" && -d "$path/lib" ]]
}

find_asset_dir() {
    if has_asset_layout "$ASSET_ROOT"; then
        printf '%s\n' "$ASSET_ROOT"
        return 0
    fi

    local target_dir="$ASSET_ROOT/$TARGET"
    if has_asset_layout "$target_dir"; then
        printf '%s\n' "$target_dir"
        return 0
    fi

    return 1
}

copy_matching_files() {
    local pattern="$1"
    local destination="$2"
    local files=()
    while IFS= read -r file; do
        files+=("$file")
    done < <(compgen -G "$pattern")
    if [[ ${#files[@]} -gt 0 ]]; then
        cp "${files[@]}" "$destination/"
    fi
}

stage_dir="$RELEASE_DIR/ghostty-vt"
mkdir -p "$RELEASE_DIR"

if ! asset_dir="$(find_asset_dir)"; then
    if [[ "$REQUIRE_ASSETS" == "1" ]]; then
        echo "Ghostty VT assets not found for $TARGET" >&2
        exit 1
    fi
    echo "Ghostty VT assets not found for $TARGET; packaging VT Code without runtime libraries" >&2
    exit 0
fi

rm -rf "$stage_dir"
mkdir -p "$stage_dir"

case "$TARGET" in
    *windows*)
        copy_matching_files "$asset_dir/lib/*.dll" "$stage_dir"
        ;;
    *darwin*)
        copy_matching_files "$asset_dir/lib/libghostty-vt*.dylib" "$stage_dir"
        ;;
    *)
        copy_matching_files "$asset_dir/lib/libghostty-vt*.so*" "$stage_dir"
        ;;
esac

case "$TARGET" in
    *windows*)
        compgen -G "$stage_dir/*.dll" >/dev/null || {
            echo "Ghostty VT asset dir '$asset_dir' has no runtime DLLs" >&2
            exit 1
        }
        ;;
    *darwin*)
        compgen -G "$stage_dir/libghostty-vt*.dylib" >/dev/null || {
            echo "Ghostty VT asset dir '$asset_dir' has no runtime dylibs" >&2
            exit 1
        }
        ;;
    *)
        compgen -G "$stage_dir/libghostty-vt*.so*" >/dev/null || {
            echo "Ghostty VT asset dir '$asset_dir' has no runtime shared libraries" >&2
            exit 1
        }
        ;;
esac

echo "Staged Ghostty VT runtime libraries in $stage_dir" >&2
