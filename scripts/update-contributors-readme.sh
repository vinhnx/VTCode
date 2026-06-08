#!/usr/bin/env bash
#
# Update the contributors avatar grid in README.md
#
# Fetches the contributor list from the GitHub API, filters out CI accounts
# and coding agents, and regenerates the avatar HTML between marker comments.
#
# Usage: ./scripts/update-contributors-readme.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

# Users to exclude (CI accounts, coding agents, bots)
EXCLUDED_USERS="vinhnguyenxuan-ct,vinhnx"

README="$SCRIPT_DIR/../README.md"

if ! command -v gh &>/dev/null; then
    print_error "GitHub CLI (gh) is required. Install it first."
    exit 1
fi

if ! gh auth status &>/dev/null 2>&1; then
    print_error "GitHub CLI is not authenticated. Run 'gh auth login' first."
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

print_info "Generating avatar HTML..."

html=""

while IFS=$'\t' read -r login contributions avatar_url html_url; do
    if echo "$EXCLUDED_USERS" | tr ',' '\n' | grep -qx "$login"; then
        print_info "  Excluding: $login"
        continue
    fi
    if [[ -n "$html" ]]; then
        html+=$'\n'
    fi
    html+="<a href=\"${html_url}\"><img src=\"${avatar_url}&s=60\" width=\"40\" height=\"40\" alt=\"@${login}\" title=\"@${login} (${contributions} contributions)\" style=\"border-radius: 10px\" /></a>&nbsp;"
done < <(
    echo "$contributors" | python3 -c "
import json, sys
data = json.load(sys.stdin)
for c in data:
    print(f\"{c['login']}\t{c['contributions']}\t{c['avatar_url']}\t{c['html_url']}\")
" | sort -t$'\t' -k2 -rn
)

if [[ -z "$html" ]]; then
    print_error "No contributors to display after filtering."
    exit 1
fi

MARKER_START="<!-- CONTRIBUTORS:START -->"
MARKER_END="<!-- CONTRIBUTORS:END -->"

if ! grep -q "$MARKER_START" "$README"; then
    print_info "Markers not found in README, inserting..."
    cat <<EOF >> "$README"

$MARKER_START
$MARKER_END
EOF
fi

python3 -c "
import sys
readme_path = '$README'
marker_start = '$MARKER_START'
marker_end = '$MARKER_END'

with open(readme_path, 'r') as f:
    content = f.read()

new_block = marker_start + '\n\n' + '''$html''' + '\n\n' + marker_end

if marker_start in content and marker_end in content:
    start_idx = content.index(marker_start)
    end_idx = content.index(marker_end) + len(marker_end)
    new_content = content[:start_idx] + new_block + content[end_idx:]
    with open(readme_path, 'w') as f:
        f.write(new_content)
    print('SUCCESS: Contributors section updated in README.md')
else:
    print('ERROR: Could not find markers in README.md')
    sys.exit(1)
"

print_success "Contributors avatar grid updated in README.md"
