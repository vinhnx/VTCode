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

ALLOWLIST_FILE="scripts/workflow_security_allowlist.txt"

# Exact `path:trigger` match against the allowlist (comments/blank lines ignored).
is_trigger_allowlisted() {
    local file="$1"
    local trigger="$2"
    [ -f "$ALLOWLIST_FILE" ] || return 1
    grep -v '^[[:space:]]*#' "$ALLOWLIST_FILE" | grep -v '^[[:space:]]*$' | tr -d ' \t\r' | grep -qxF "${file}:${trigger}"
}

for file in .github/workflows/*.yml; do
    while IFS=: read -r _path line _text; do
        if ! is_trigger_allowlisted "$file" "pull_request_target"; then
            report_error "$file" "$line" "forbidden trigger 'pull_request_target' is not allowed"
        fi
    done < <(rg -nH '^[[:space:]]*pull_request_target[[:space:]]*:' "$file" || true)

    while IFS=: read -r _path line _text; do
        if ! is_trigger_allowlisted "$file" "workflow_run"; then
            report_error "$file" "$line" "forbidden trigger 'workflow_run' is not allowed"
        fi
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

# Stale allowlist entries fail: every exception must reference an existing
# workflow that still contains the trigger.
if [ -f "$ALLOWLIST_FILE" ]; then
    # SC2094 is a false positive: report_error writes to stderr, never to the file being read.
    # shellcheck disable=SC2094
    while IFS= read -r entry; do
        entry="$(printf '%s' "$entry" | tr -d '[:space:]')"
        if [ -z "$entry" ]; then
            continue
        fi
        case "$entry" in \#*) continue;; esac
        entry_file="${entry%%:*}"
        entry_trigger="${entry##*:}"
        if [ ! -f "$entry_file" ]; then
            report_error "$ALLOWLIST_FILE" "0" "stale exception '$entry': workflow does not exist"
        elif ! rg -q "^[[:space:]]*${entry_trigger}[[:space:]]*:" "$entry_file"; then
            report_error "$ALLOWLIST_FILE" "0" "stale exception '$entry': trigger no longer present"
        fi
    done < "$ALLOWLIST_FILE"
fi

if [ "$failed" -ne 0 ]; then
    echo "[workflow-security] FAILED" >&2
    exit 1
fi

echo "[workflow-security] OK: all workflows pass security policy checks"
