#!/usr/bin/env bash

# Run the exact zizmor audit enforced by .github/workflows/verify-lockfile.yml.
# The version comes from the CI workflow so local and CI audits cannot drift.
#
# Exit codes: 0 = clean, 1 = findings/audit failure, 3 = tool unavailable
# (the caller may skip, since CI always enforces the gate).

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

ci_workflow=".github/workflows/verify-lockfile.yml"
version=$(sed -n 's/^[[:space:]]*ZIZMOR_VERSION:[[:space:]]*//p' "$ci_workflow" | head -1 | tr -d '"[:space:]')
if [ -z "$version" ]; then
	echo "[zizmor] cannot read ZIZMOR_VERSION from $ci_workflow" >&2
	exit 1
fi

args=(--offline --no-progress --persona=regular .)

# uvx fetches a prebuilt wheel, matching how the audit runs in CI.
if command -v uvx >/dev/null 2>&1; then
	if uvx "zizmor@$version" --version >/dev/null 2>&1; then
		exec uvx "zizmor@$version" "${args[@]}"
	fi
fi

# Otherwise accept a locally installed binary only when its version matches.
if command -v zizmor >/dev/null 2>&1; then
	installed=$(zizmor --version 2>/dev/null || true)
	case "$installed" in
	*"$version"*) exec zizmor "${args[@]}" ;;
	*)
		echo "[zizmor] installed version '$installed' differs from CI's '$version'" >&2
		;;
	esac
fi

echo "[zizmor] unavailable: install uv, or install zizmor $version on PATH" >&2
exit 3
