#!/usr/bin/env bash
#
# Update the grouped contributors avatar grid in README.md
#
# Fetches the contributor list from the GitHub API, filters out CI accounts
# and coding agents, and regenerates the avatar HTML between the
# <!-- CONTRIBUTORS:START --> / <!-- CONTRIBUTORS:END --> marker comments.
# The grid is grouped, in order: Security Advisors, Main Contributor,
# Core Contributors (curated list), then Contributors (everyone else). Within
# the Core Contributors and Contributors groups, entries are ordered by latest
# contribution date, newest first. The whole grid is center-aligned.
# Curated security-advisor attributions (people the contributor API cannot
# list, e.g. security researchers who reported advisories) are emitted via
# SECURITY_ADVISORS.
#
# Usage: ./scripts/update-contributors-readme.sh [--dry-run]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "$SCRIPT_DIR/common.sh"

DRY_RUN=false
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=true
fi

# Users to exclude (owner, CI accounts, coding agents, bots)
EXCLUDED_USERS="vinhnguyenxuan-ct,vinhnx"

# Glob exclusions on top of EXCLUDED_USERS: the owner's automation accounts
# (vinhnguyenxuan*) plus bots and coding agents (*[bot], *-bot).

# The single main contributor (non-owner) shown after security advisors.
MAIN_USER="kernitus"

# Curated Core Contributors membership; anyone not listed here falls back to
# commit-count classification (>= CORE_MIN_COMMITS -> Core, else Contributor).
# NOTE: mouse-value-add was removed 2026-10-12 - the GitHub account is deleted
# (404), so the contributors API no longer lists them.
CORE_USERS="7jrxt42BxFZo4iAnN4CX oiwn Sachin-Bhat chenrui333 gzsombor leonj1 netbrah xcrong"
CORE_MIN_COMMITS=7

# Curated security-advisor attributions, tab-separated: login<TAB>title
# These reporters are not commit contributors, so the API fetch cannot list them.
SECURITY_ADVISORS=$'glmgbj233\tGHSA-wqgw-crr5-cr2p (security advisory)\nnnfrog\tGHSA-r249-hpfx-x2w7 (security advisory)'

# Optional curated notes per login, appended to the title attribute.
note_for() {
    case "$1" in
        7jrxt42BxFZo4iAnN4CX) echo "subagents, hooks, config & TUI fixes (#737, #738, #740-#742+)" ;;
        raphamorim) echo "PR #708, rio-vt migration" ;;
        ct-jaryn) echo "lint/model preset test gates (#769), swarm diff fix (#768), CLI test harness (#767), MCP docs (#765)" ;;
        vivekgupta-memcode) echo "Memcode OAuth setup docs (#763)" ;;
        S2thend) echo "checkpoint rewind/redo (#771)" ;;
        EvoLinkAI) echo "Evolink provider (#664)" ;;
        rs545837) echo "optional Sem code discovery docs (#783)" ;;
        georgeatparallel) echo "opt-in Parallel Search MCP config (#779)" ;;
        apewithllm) echo "OpenLLM provider guide (#804)" ;;
        *) echo "" ;;
    esac
}

# NOTE: no inline `style` here on purpose. GitHub's README sanitizer strips
# `style` attributes and re-renders avatars with its own look, so ring
# borders only ever appeared in local markdown previews. Tier meaning is
# carried by the group headings and the title text instead.
avatar_html() {
    local login=$1 url=$2 avatar=$3 title=$4
    printf '<a href="%s"><img src="%s&s=60" width="40" height="40" alt="@%s" title="%s" /></a>&nbsp;' \
        "$url" "$avatar" "$login" "$title"
}

README="$SCRIPT_DIR/../README.md"
MARKER_START="<!-- CONTRIBUTORS:START -->"
MARKER_END="<!-- CONTRIBUTORS:END -->"

if ! command -v gh &>/dev/null; then
    print_error "GitHub CLI (gh) is required. Install it first."
    exit 1
fi

if ! gh auth status &>/dev/null 2>&1; then
    print_error "GitHub CLI is not authenticated. Run 'gh auth login' first."
    exit 1
fi

if ! grep -q "$MARKER_START" "$README" || ! grep -q "$MARKER_END" "$README"; then
    print_error "Markers not found in README.md. Wrap the contributors grid with"
    print_error "$MARKER_START and $MARKER_END first."
    exit 1
fi

print_info "Fetching contributors from GitHub API..."

contributors=$(gh api repos/vinhnx/vtcode/contributors --paginate --jq '
    [.[] | select(.type == "User") | {login: .login, contributions: .contributions, avatar_url: .avatar_url, html_url: .html_url}]
')

if [[ -z "$contributors" || "$contributors" == "[]" ]]; then
    print_warning "No contributors found or API call failed."
    exit 0
fi

print_info "Fetching latest contribution dates from GitHub API..."

# Author date of a login's most recent commit (ISO 8601 UTC; squash merges
# preserve the PR author). Empty when a login has no reachable commit.
latest_commit_date() {
    gh api "repos/vinhnx/vtcode/commits?author=${1}&per_page=1" \
        --jq '.[0].commit.author.date // empty' 2>/dev/null
}

latest_dates=$(echo "$contributors" | python3 -c "
import json, sys
for c in json.load(sys.stdin):
    print(c['login'])
" | while IFS= read -r login; do
    printf '%s\t%s\n' "$login" "$(latest_commit_date "$login")"
done)

print_info "Generating grouped avatar HTML..."

# Group buffers accumulated through append_line's indirect expansion (${!group_var}).
# shellcheck disable=SC2034
advisors_html=""
# shellcheck disable=SC2034
main_html=""
# shellcheck disable=SC2034
core_html=""
# shellcheck disable=SC2034
contributor_html=""

append_line() {
    local group_var=$1 line=$2
    if [[ -n "${!group_var}" ]]; then
        printf -v "$group_var" '%s\n%s' "${!group_var}" "$line"
    else
        printf -v "$group_var" '%s' "$line"
    fi
}

emit_entry() {
    local group=$1 login=$2 contributions=$3 avatar_url=$4 html_url=$5
    local label note title
    case "$group" in
        main) label="Main Contributor" ;;
        core) label="Core contributor" ;;
        *) label="Contributor" ;;
    esac
    if [[ "$contributions" -eq 1 ]]; then
        title="@${login} ${label} (${contributions} commit)"
    else
        title="@${login} ${label} (${contributions} commits)"
    fi
    note=$(note_for "$login")
    if [[ -n "$note" ]]; then
        title="${title} - ${note}"
    fi
    local line
    line=$(avatar_html "$login" "$html_url" "$avatar_url" "$title")
    append_line "${group}_html" "$line"
}

is_excluded() {
    case ",${EXCLUDED_USERS}," in
        *",${1},"*) return 0 ;;
    esac
    case "$1" in
        vinhnguyenxuan*|*\[Bb][Oo][Tt]|*-bot) return 0 ;;
    esac
    return 1
}

is_core() {
    case " ${CORE_USERS} " in
        *" ${1} "*) return 0 ;;
    esac
    [[ "$2" -ge "$CORE_MIN_COMMITS" ]]
}

while IFS=$'\t' read -r login contributions avatar_url html_url; do
    if is_excluded "$login"; then
        print_info "  Excluding: $login"
        continue
    fi
    if [[ "$login" == "$MAIN_USER" ]]; then
        emit_entry main "$login" "$contributions" "$avatar_url" "$html_url"
    elif is_core "$login" "$contributions"; then
        emit_entry core "$login" "$contributions" "$avatar_url" "$html_url"
    else
        emit_entry contributor "$login" "$contributions" "$avatar_url" "$html_url"
    fi
done < <(
    echo "$contributors" | LATEST_DATES="$latest_dates" python3 -c "
import json, os, sys
dates = dict(
    line.split('\t')
    for line in os.environ.get('LATEST_DATES', '').splitlines()
    if line
)
rows = [
    (dates.get(c['login']) or '1970-01-01T00:00:00Z', c)
    for c in json.load(sys.stdin)
]
for _, c in sorted(rows, key=lambda row: row[0], reverse=True):
    print(f\"{c['login']}\t{c['contributions']}\t{c['avatar_url']}\t{c['html_url']}\")
"
)

print_info "Adding curated security-advisor attributions..."

while IFS=$'\t' read -r login title _; do
    if [[ -z "$login" ]]; then
        continue
    fi
    profile=$(gh api "users/$login" --jq '[.avatar_url, .html_url] | @tsv' 2>/dev/null) || {
        print_warning "Could not fetch profile for security advisor @$login; skipping."
        continue
    }
    IFS=$'\t' read -r avatar_url html_url <<< "$profile"
    local_title="@${login} ${title}"
    line=$(avatar_html "$login" "$html_url" "$avatar_url" "$local_title")
    append_line advisors_html "$line"
done <<< "$SECURITY_ADVISORS"

html=""
emit_group() {
    local header=$1 var=$2
    local body=${!var}
    if [[ -z "$body" ]]; then
        return
    fi
    if [[ -n "$html" ]]; then
        html+=$'\n\n'
    fi
    html+="### ${header}"$'\n\n'"${body}"
}

emit_group "Security Advisors" advisors_html
emit_group "Main Contributor" main_html
emit_group "Core Contributors" core_html
emit_group "Contributors" contributor_html

# Center-align the whole grid, mirroring the Support VT Code section.
html='<div align="center">'$'\n\n'"${html}"$'\n\n'"</div>"

COUNT_START="<!-- CONTRIBUTORS-COUNT:START -->"
COUNT_END="<!-- CONTRIBUTORS-COUNT:END -->"

if [[ -z "$html" ]]; then
    print_error "No contributors to display after filtering."
    exit 1
fi

if $DRY_RUN; then
    print_info "Dry run: generated block between markers:"
    printf '%s\n' "$html"
    exit 0
fi

update_count=true
if ! grep -q "$COUNT_START" "$README" || ! grep -q "$COUNT_END" "$README"; then
    update_count=false
    print_warning "Count markers not found in README.md ($COUNT_START / $COUNT_END); skipping count update."
fi

MARKER_START="$MARKER_START" MARKER_END="$MARKER_END" README="$README" HTML="$html" \
    COUNT_START="$COUNT_START" COUNT_END="$COUNT_END" UPDATE_COUNT="$update_count" python3 - <<'PY'
import os

readme_path = os.environ["README"]
with open(readme_path) as f:
    content = f.read()

if os.environ["UPDATE_COUNT"] == "true":
    count = os.environ["HTML"].count("<a href=")
    summary = f"<summary>View all contributors ({count})</summary>"
    start_idx = content.index(os.environ["COUNT_START"]) + len(os.environ["COUNT_START"])
    end_idx = content.index(os.environ["COUNT_END"])
    content = content[:start_idx] + summary + content[end_idx:]
    print(f"SUCCESS: Contributor summary updated to {summary}")

html = "<!-- markdownlint-disable MD013 -->\n\n" + os.environ["HTML"] + "\n\n<!-- markdownlint-enable MD013 -->"
start_idx = content.index(os.environ["MARKER_START"])
end_idx = content.index(os.environ["MARKER_END"]) + len(os.environ["MARKER_END"])
content = content[:start_idx] + os.environ["MARKER_START"] + "\n\n" + html + "\n\n" + os.environ["MARKER_END"] + content[end_idx:]

with open(readme_path, "w") as f:
    f.write(content)

print("SUCCESS: Contributors section updated in README.md")
PY

print_success "Grouped contributors avatar grid updated in README.md"
