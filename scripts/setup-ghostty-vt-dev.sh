#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MANIFEST="$REPO_ROOT/vtcode-ghostty-vt-sys/ghostty-vt-manifest.toml"

if [[ ! -f "$MANIFEST" ]]; then
    echo "Ghostty VT manifest not found: $MANIFEST" >&2
    exit 1
fi

TARGET="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
if [[ -z "$TARGET" ]]; then
    echo "Unable to determine Rust host target" >&2
    exit 1
fi

helper_name() {
    if [[ "$TARGET" == *windows* ]]; then
        printf 'ghostty_vt_host.exe'
    else
        printf 'ghostty_vt_host'
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
            echo "unsupported Ghostty VT setup target: $TARGET" >&2
            exit 1
            ;;
    esac
}

if ! command -v zig >/dev/null 2>&1; then
    echo "zig is required to build Ghostty VT dev assets" >&2
    exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required to download Ghostty VT source" >&2
    exit 1
fi

COMMIT="$(awk -F'"' '/^commit = / { print $2; exit }' "$MANIFEST")"
if [[ -z "$COMMIT" ]]; then
    echo "Unable to read pinned Ghostty commit from $MANIFEST" >&2
    exit 1
fi

TARGET_DIR="$REPO_ROOT/dist/ghostty-vt/$TARGET"

build_helper() {
    local asset_dir="$1"

    if [[ -f "$asset_dir/$(helper_name)" ]]; then
        echo "Ghostty VT helper already available at $asset_dir/$(helper_name)" >&2
        return 0
    fi

    echo "Building Ghostty VT helper for $TARGET..." >&2
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
        -o "$asset_dir/$(helper_name)"
    chmod +x "$asset_dir/$(helper_name)" 2>/dev/null || true
}

if [[ -f "$TARGET_DIR/include/ghostty/vt.h" && -d "$TARGET_DIR/lib" ]]; then
    build_helper "$TARGET_DIR"
    echo "Ghostty VT assets already available at $TARGET_DIR" >&2
    exit 0
fi

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vtcode-ghostty-vt.XXXXXX")"
ARCHIVE="$WORK_DIR/ghostty.tar.gz"
SRC_ROOT="$WORK_DIR/src"

cleanup() {
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

echo "Downloading Ghostty source at commit $COMMIT..." >&2
curl -fsSL -o "$ARCHIVE" "https://github.com/ghostty-org/ghostty/archive/${COMMIT}.tar.gz"

mkdir -p "$SRC_ROOT"
tar -xzf "$ARCHIVE" -C "$SRC_ROOT"

SRC_DIR="$(find "$SRC_ROOT" -maxdepth 1 -mindepth 1 -type d | head -n 1)"
if [[ -z "$SRC_DIR" ]]; then
    echo "Failed to extract Ghostty source archive" >&2
    exit 1
fi

echo "Building Ghostty lib-vt for $TARGET..." >&2
(
    cd "$SRC_DIR"
    zig build lib-vt -Dtarget="$(zig_target)"
)

mkdir -p "$TARGET_DIR"
cp -R "$SRC_DIR/zig-out/include" "$TARGET_DIR/"
cp -R "$SRC_DIR/zig-out/lib" "$TARGET_DIR/"
build_helper "$TARGET_DIR"

echo "Ghostty VT assets staged at $TARGET_DIR" >&2
