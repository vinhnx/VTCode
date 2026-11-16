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
LOCKFILE=".github/workflows/actions.lock"

# Exact `path:trigger` match against the allowlist (comments/blank lines ignored).
is_trigger_allowlisted() {
    local file="$1"
    local trigger="$2"
    [ -f "$ALLOWLIST_FILE" ] || return 1
    grep -v '^[[:space:]]*#' "$ALLOWLIST_FILE" | grep -v '^[[:space:]]*$' | tr -d ' \t\r' | grep -qxF "${file}:${trigger}"
}

# Every symbolic ref used by a workflow must resolve to a 40-char commit in the
# lockfile. This is the immutability guarantee: tags/branches may be mutable,
# but the lockfile pins the exact commit the runner executes. A bare SHA used
# directly in a workflow (e.g. a reusable workflow at `jobs.<id>.uses`, which
# the lockfile format does not cover) is already immutable and needs no entry.
# Output is lowercased because the lockfile normalises owner/repo casing.
pinned_refs() {
    [ -f "$LOCKFILE" ] || return 1
    awk '
        /^dependencies:[[:space:]]*$/ { in_deps = 1; next }
        in_deps && /^[^[:space:]#]/ { in_deps = 0 }
        in_deps && /^    [^[:space:]].*:$/ {
            current = $0
            sub(/^    /, "", current)
            sub(/:$/, "", current)
            gsub(/["\047]/, "", current)
            next
        }
        in_deps && /^        commit: / {
            commit = $0
            sub(/^        commit: /, "", commit)
            gsub(/["\047]/, "", commit)
            if (commit ~ /^sha1-[0-9a-fA-F]+$/ && length(commit) == 45) {
                print tolower(current)
            }
        }
    ' "$LOCKFILE"
}

if [ ! -f "$LOCKFILE" ]; then
    report_error "$LOCKFILE" "0" "missing workflow lockfile; run \`gh actions-lock\`"
elif ! grep -q '^version:' "$LOCKFILE"; then
    report_error "$LOCKFILE" "1" "malformed lockfile: missing version field"
fi

pinned_list=$(pinned_refs || true)

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
        ./* | docker://* | \$/*)
            continue
            ;;
        esac

        if [[ "$uses_ref" != *"@"* ]]; then
            report_error "$file" "$line" "external action '$uses_ref' must include an @<ref>"
            continue
        fi

        ref_suffix="${uses_ref##*@}"
        # A full SHA written inline is inherently immutable (reusable workflows
        # are not covered by the lockfile format).
        if [[ "$ref_suffix" =~ ^[0-9a-fA-F]{40}$ ]]; then
            continue
        fi

        if ! printf '%s\n' "$pinned_list" | grep -qxF "$(printf '%s' "$uses_ref" | tr '[:upper:]' '[:lower:]')"; then
            report_error "$file" "$line" "action '$uses_ref' has no lockfile entry pinning it to a commit SHA; run \`gh actions-lock\`"
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
        case "$entry" in \#*) continue ;; esac
        entry_file="${entry%%:*}"
        entry_trigger="${entry##*:}"
        if [ ! -f "$entry_file" ]; then
            report_error "$ALLOWLIST_FILE" "0" "stale exception '$entry': workflow does not exist"
        elif ! rg -q "^[[:space:]]*${entry_trigger}[[:space:]]*:" "$entry_file"; then
            report_error "$ALLOWLIST_FILE" "0" "stale exception '$entry': trigger no longer present"
        fi
    done <"$ALLOWLIST_FILE"
fi

if [ "$failed" -ne 0 ]; then
    echo "[workflow-security] FAILED" >&2
    exit 1
fi

echo "[workflow-security] OK: all workflows pass security policy checks"
