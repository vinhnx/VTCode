#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: scripts/migrate_skills.sh [--copy|--move]"
  echo "  --copy   Copy .vtcode/skills to .agents/skills (default)"
  echo "  --move   Move .vtcode/skills to .agents/skills"
}

mode="copy"
if [[ $# -gt 1 ]]; then
  usage
  exit 2
fi
if [[ $# -eq 1 ]]; then
  case "$1" in
    --copy) mode="copy" ;;
    --move) mode="move" ;;
    -h|--help) usage; exit 0 ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 2
      ;;
  esac
fi

src=".vtcode/skills"
dst=".agents/skills"

if [[ ! -d "$src" ]]; then
  echo "No legacy skills directory found at $src. Nothing to migrate."
  exit 0
fi

mkdir -p "$dst"

shopt -s dotglob nullglob
for entry in "$src"/*; do
  name="$(basename "$entry")"
  target="$dst/$name"
  if [[ -e "$target" ]]; then
    echo "Skipping $name (already exists at $dst)"
    continue
  fi

  if [[ "$mode" == "move" ]]; then
    mv "$entry" "$target"
    echo "Moved $name -> $target"
  else
    cp -R "$entry" "$target"
    echo "Copied $name -> $target"
  fi
done

if [[ "$mode" == "move" ]]; then
  rmdir "$src" 2>/dev/null || true
fi

echo "Migration complete."
