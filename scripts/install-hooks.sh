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

current_hooks_path=$(git config core.hooksPath || true)
if [ -n "$current_hooks_path" ] && [ "$current_hooks_path" != "scripts/hooks" ]; then
	echo "Refusing to install: core.hooksPath is already set to '$current_hooks_path'." >&2
	echo "Unset it first (git config --unset core.hooksPath) if you want the repo hooks." >&2
	exit 1
fi

# .git/hooks/ is left untouched: core.hooksPath bypasses that directory
# entirely, so any hooks there keep working once core.hooksPath is unset.
chmod +x scripts/hooks/pre-commit scripts/hooks/pre-push
git config core.hooksPath scripts/hooks

echo "Hooks installed (core.hooksPath=scripts/hooks):"
echo "  pre-commit: rustfmt --check + governance gates on staged files"
echo "  pre-push:   ./scripts/check-dev.sh --quiet --workspace"
echo "Remove with: git config --unset core.hooksPath"
