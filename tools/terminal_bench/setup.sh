#!/bin/bash
set -euo pipefail

if ! command -v apt-get >/dev/null 2>&1; then
    echo 'apt-get is required to install vtcode dependencies' >&2
    exit 1
fi

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y build-essential curl git pkg-config libssl-dev

if ! command -v rustup >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | bash -s -- -y --default-toolchain stable
fi

# shellcheck disable=SC1091
source "$HOME/.cargo/env"

cargo install --locked vtcode
