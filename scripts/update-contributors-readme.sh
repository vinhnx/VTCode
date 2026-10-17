#!/usr/bin/env bash
#
# Update the grouped contributors avatar grid in README.md
#
# Fetches the contributor list from the GitHub API, filters out CI accounts
# and coding agents, and regenerates the avatar HTML between the
# <!-- CONTRIBUTORS:START --> / <!-- CONTRIBUTORS:END --> marker comments.
# The grid is grouped, in order: Security Advisors, Main Contributor,
# Core Contributors (curated list), then Contributors (everyone else).
# Curated security-advisor attributions (people the contributor API cannot
# list, e.g. security researchers who reported advisories) are emitted via
# SECURITY_ADVISORS.
#
# Usage: ./scripts/update-contributors-readme.sh [--dry-run]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

DRY_RUN=false
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=true
fi

# Users to exclude (CI accounts, coding agents, bots)
EXCLUDED_USERS="vinhnguyenxuan-ct,vinhnx"

# The single main contributor (non-owner) shown after security advisors.
MAIN_USER="kernitus"

# Curated Core Contributors membership; anyone not listed here falls back to
# commit-count classification (>= CORE_MIN_COMMITS -> Core, else Contributor).
# NOTE: mouse-value-add was removed 2026-10-12 - the GitHub account is deleted
# (404), so the contributors API no longer lists them.
CORE_USERS="7jrxt42BxFZo4iAnN4CX oiwn Sachin-Bhat chenrui333 gzsombor leonj1 netbrah xcrong"
CORE_MIN_COMMITS=7

# Curated security-advisor attributions, tab-separated: login<TAB>title<TAB>border
# These reporters are not commit contributors, so the API fetch cannot list them.
SECURITY_ADVISORS=$'glmgbj233\tGHSA-wqgw-crr5-cr2p (security advisory)\t#FF6B6B\nnnfrog\tGHSA-r249-hpfx-x2w7 (security advisory)\t#FF6B6B'

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
        *) echo "" ;;
    esac
}

# Border color per group tier.
border_for_group() {
    case "$1" in
        main) echo "#FFD700" ;;
        core) echo "#50C878" ;;
        contributor) echo "#B19CD9" ;;
        advisor) echo "#FF6B6B" ;;
        *) echo "#B19CD9" ;;
    esac
}

avatar_html() {
    local login=$1 url=$2 avatar=$3 title=$4 border=$5
    printf '<a href="%s"><img src="%s&s=60" width="40" height="40" alt="@%s" title="%s" style="border-radius: 50%%; border: 2px solid %s;" /></a>&nbsp;' \
        "$url" "$avatar" "$login" "$title" "$border"
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

print_info "Generating grouped avatar HTML..."

advisors_html=""
main_html=""
core_html=""
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
    line=$(avatar_html "$login" "$html_url" "$avatar_url" "$title" "$(border_for_group "$group")")
    append_line "${group}_html" "$line"
}

while IFS=$'\t' read -r login contributions avatar_url html_url; do
    if echo "$EXCLUDED_USERS" | tr ',' '\n' | grep -qx "$login"; then
        print_info "  Excluding: $login"
        continue
    fi
    if [[ "$login" == "$MAIN_USER" ]]; then
        emit_entry main "$login" "$contributions" "$avatar_url" "$html_url"
    elif echo "$CORE_USERS" | tr ' ' '\n' | grep -qx "$login" || [[ "$contributions" -ge "$CORE_MIN_COMMITS" ]]; then
        emit_entry core "$login" "$contributions" "$avatar_url" "$html_url"
    else
        emit_entry contributor "$login" "$contributions" "$avatar_url" "$html_url"
    fi
done < <(
    echo "$contributors" | python3 -c "
import json, sys
data = json.load(sys.stdin)
for c in data:
    print(f\"{c['login']}\t{c['contributions']}\t{c['avatar_url']}\t{c['html_url']}\")
" | sort -t$'\t' -k2 -rn
)

print_info "Adding curated security-advisor attributions..."

while IFS=$'\t' read -r login title border; do
    if [[ -z "$login" ]]; then
        continue
    fi
    user_json=$(gh api "users/$login" --jq '{avatar_url: .avatar_url, html_url: .html_url}' 2>/dev/null) || {
        print_warning "Could not fetch profile for security advisor @$login; skipping."
        continue
    }
    avatar_url=$(echo "$user_json" | python3 -c "import json,sys; print(json.load(sys.stdin)['avatar_url'])")
    html_url=$(echo "$user_json" | python3 -c "import json,sys; print(json.load(sys.stdin)['html_url'])")
    local_title="@${login} ${title}"
    line=$(avatar_html "$login" "$html_url" "$avatar_url" "$local_title" "$border")
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

if [[ -z "$html" ]]; then
    print_error "No contributors to display after filtering."
    exit 1
fi

if $DRY_RUN; then
    print_info "Dry run: generated block between markers:"
    printf '%s\n' "$html"
    exit 0
fi

MARKER_START="$MARKER_START" MARKER_END="$MARKER_END" README="$README" HTML="$html" python3 - <<'PY'
import os

readme_path = os.environ["README"]
marker_start = os.environ["MARKER_START"]
marker_end = os.environ["MARKER_END"]
html = "<!-- markdownlint-disable MD013 -->\n\n" + os.environ["HTML"] + "\n\n<!-- markdownlint-enable MD013 -->"

with open(readme_path) as f:
    content = f.read()

start_idx = content.index(marker_start)
end_idx = content.index(marker_end) + len(marker_end)
new_block = marker_start + "\n\n" + html + "\n\n" + marker_end
content = content[:start_idx] + new_block + content[end_idx:]

with open(readme_path, "w") as f:
    f.write(content)

print("SUCCESS: Contributors section updated in README.md")
PY

print_success "Grouped contributors avatar grid updated in README.md"
