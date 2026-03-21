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

helper_name() {
    if [[ "$TARGET" == *windows* ]]; then
        printf 'ghostty_vt_host.exe'
    else
        printf 'ghostty_vt_host'
    fi
}

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

zig_target() {
    case "$TARGET" in
        x86_64-unknown-linux-gnu) printf 'x86_64-linux-gnu' ;;
        x86_64-unknown-linux-musl) printf 'x86_64-linux-musl' ;;
        aarch64-unknown-linux-gnu) printf 'aarch64-linux-gnu' ;;
        aarch64-unknown-linux-musl) printf 'aarch64-linux-musl' ;;
        x86_64-pc-windows-msvc) printf 'x86_64-windows-msvc' ;;
        aarch64-pc-windows-msvc) printf 'aarch64-windows-msvc' ;;
        x86_64-apple-darwin) printf 'x86_64-macos' ;;
        aarch64-apple-darwin) printf 'aarch64-macos' ;;
        *)
            echo "unsupported Ghostty VT packaging target: $TARGET" >&2
            exit 1
            ;;
    esac
}

stage_dir="$RELEASE_DIR/ghostty-vt"
mkdir -p "$RELEASE_DIR"

if ! asset_dir="$(find_asset_dir)"; then
    echo "Ghostty VT assets not found for $TARGET; packaging VT Code without sidecar" >&2
    exit 0
fi

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

helper_path="$stage_dir/$(helper_name)"
if [[ -f "$asset_dir/$(helper_name)" ]]; then
    cp "$asset_dir/$(helper_name)" "$helper_path"
elif [[ -f "$asset_dir/bin/$(helper_name)" ]]; then
    cp "$asset_dir/bin/$(helper_name)" "$helper_path"
else
    if ! command -v zig >/dev/null 2>&1; then
        echo "zig is required to build Ghostty VT helper for packaging" >&2
        exit 1
    fi

    zig cc \
        -target "$(zig_target)" \
        "$REPO_ROOT/vtcode-ghostty-vt-sys/csrc/ghostty_vt_host.c" \
        -std=c11 \
        -O2 \
        "-I$asset_dir/include" \
        "-L$asset_dir/lib" \
        -lghostty-vt \
        $( [[ "$TARGET" == *darwin* ]] && printf '%s' '-Wl,-rpath,@loader_path' ) \
        $( [[ "$TARGET" == *linux* ]] && printf '%s' '-Wl,-rpath,$ORIGIN' ) \
        -o "$helper_path"
fi

chmod +x "$helper_path" 2>/dev/null || true
echo "Staged Ghostty VT sidecar assets in $stage_dir" >&2
