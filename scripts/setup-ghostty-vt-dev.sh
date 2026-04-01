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
    echo "Ghostty VT dev assets are only supported for macOS and Linux targets: $TARGET" >&2
    exit 1
fi

zig_target() {
    case "$TARGET" in
        x86_64-unknown-linux-gnu) printf 'x86_64-linux-gnu' ;;
        x86_64-unknown-linux-musl) printf 'x86_64-linux-musl' ;;
        aarch64-unknown-linux-gnu) printf 'aarch64-linux-gnu' ;;
        aarch64-unknown-linux-musl) printf 'aarch64-linux-musl' ;;
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

if [[ -f "$TARGET_DIR/include/ghostty/vt.h" && -d "$TARGET_DIR/lib" ]]; then
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

echo "Ghostty VT runtime libraries staged at $TARGET_DIR" >&2
