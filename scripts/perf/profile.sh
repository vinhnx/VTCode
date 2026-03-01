#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

EXTRA_RUSTFLAGS="-C force-frame-pointers=yes"
if [[ -n "${RUSTFLAGS:-}" ]]; then
  export RUSTFLAGS="${RUSTFLAGS} ${EXTRA_RUSTFLAGS}"
else
  export RUSTFLAGS="${EXTRA_RUSTFLAGS}"
fi

export CARGO_PROFILE_RELEASE_DEBUG="line-tables-only"

echo "[perf] building release with frame pointers and line tables"
(cd "${ROOT_DIR}" && cargo build --release "$@")

echo "[perf] done"
echo "[perf] next: run your profiler (e.g., samply, perf, flamegraph) against target/release/vtcode"
