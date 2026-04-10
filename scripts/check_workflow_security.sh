#!/bin/bash

set -euo pipefail

failed=0

report_error() {
    local file="$1"
    local line="$2"
    local message="$3"
    echo "[workflow-security] ${file}:${line}: ${message}" >&2
    failed=1
}

for file in .github/workflows/*.yml; do
    while IFS=: read -r _path line _text; do
        report_error "$file" "$line" "forbidden trigger 'pull_request_target' is not allowed"
    done < <(rg -nH '^[[:space:]]*pull_request_target[[:space:]]*:' "$file" || true)

    while IFS=: read -r _path line _text; do
        report_error "$file" "$line" "forbidden trigger 'workflow_run' is not allowed"
    done < <(rg -nH '^[[:space:]]*workflow_run[[:space:]]*:' "$file" || true)

    while IFS=: read -r _path line text; do
        uses_ref=$(printf '%s\n' "$text" | sed -E 's/^[[:space:]-]*uses:[[:space:]]*"?([^"[:space:]#]+)"?.*$/\1/')

        case "$uses_ref" in
            ./*|docker://*)
                continue
                ;;
        esac

        if [[ "$uses_ref" != *"@"* ]]; then
            report_error "$file" "$line" "external action '$uses_ref' must include an immutable @<40-char-sha> ref"
            continue
        fi

        ref_suffix="${uses_ref##*@}"
        if [[ "$ref_suffix" =~ ^(master|main|stable)$ ]]; then
            report_error "$file" "$line" "branch ref '@$ref_suffix' is not allowed for GitHub Actions"
            continue
        fi

        if [[ ! "$ref_suffix" =~ ^[0-9a-fA-F]{40}$ ]]; then
            report_error "$file" "$line" "action '$uses_ref' is not pinned to a full 40-char commit SHA"
        fi
    done < <(rg -nH '^[[:space:]]*(-[[:space:]]*)?uses:[[:space:]]*' "$file" || true)
done

if [ "$failed" -ne 0 ]; then
    echo "[workflow-security] FAILED" >&2
    exit 1
fi

echo "[workflow-security] OK: all workflows pass security policy checks"
