#!/bin/bash
# Install the repo's git hooks via core.hooksPath (tracked, no copying).
#
# The hooks are fast and local by design — a hook, not a line in AGENTS.md,
# because agents and humans skip steps that are not enforced:
#   scripts/hooks/pre-commit — rustfmt + governance gates on the staged diff
#   scripts/hooks/pre-push   — check-dev.sh --workspace (the CI fast gate)
#
# Idempotent: re-running is safe. Refuses to replace an existing non-repo hook
# so unrelated tooling is never clobbered.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

for hook in pre-commit pre-push; do
	target=".git/hooks/$hook"
	if [ -e "$target" ] && [ ! -e "$target.repobackup" ]; then
		echo "Backing up existing $target to $target.repobackup"
		mv "$target" "$target.repobackup"
	elif [ -e "$target" ] && [ -e "$target.repobackup" ]; then
		echo "Existing $target already backed up; skipping"
	fi
done

chmod +x scripts/hooks/pre-commit scripts/hooks/pre-push
git config core.hooksPath scripts/hooks

echo "Hooks installed (core.hooksPath=scripts/hooks):"
echo "  pre-commit: rustfmt --check + governance gates on staged files"
echo "  pre-push:   ./scripts/check-dev.sh --quiet --workspace"
echo "Remove with: git config --unset core.hooksPath"
