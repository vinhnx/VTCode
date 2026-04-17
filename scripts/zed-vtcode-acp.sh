#!/bin/sh

set -eu

REPO="/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode"
LOG_FILE="/tmp/zed-vtcode-acp-launch.log"
STDERR_FILE="/tmp/zed-vtcode-acp-stderr.log"
export RUST_LOG="${RUST_LOG:-info}"
export VT_ACP_ENABLED=1
export VT_ACP_ZED_ENABLED=1
export VTCODE_PROVIDER=ollama

{
  echo "==== $(date '+%Y-%m-%d %H:%M:%S %z') ===="
  echo "pwd(before)=$(pwd)"
  echo "argv=$0 $*"
  echo "PATH=$PATH"
} >> "$LOG_FILE"

cd "$REPO"
exec "$REPO/target/debug/vtcode" \
  --config "$REPO/vtcode.toml" \
  --provider ollama \
  --model gpt-oss:120b-cloud \
  --api-key-env OLLAMA_API_KEY \
  --enable-skills \
  acp 2>>"$STDERR_FILE"
