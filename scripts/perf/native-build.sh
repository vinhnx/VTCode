#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EXTRA_RUSTFLAGS="-C target-cpu=native"

if [[ -n "${RUSTFLAGS:-}" ]]; then
  export RUSTFLAGS="${RUSTFLAGS} ${EXTRA_RUSTFLAGS}"
else
  export RUSTFLAGS="${EXTRA_RUSTFLAGS}"
fi

echo "[perf] local native build (portable release artifacts remain unchanged)"
(cd "${ROOT_DIR}" && cargo build --profile release-fast "$@")
