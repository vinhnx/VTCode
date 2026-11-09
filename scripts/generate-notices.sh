#!/usr/bin/env bash
# Regenerate THIRD-PARTY-NOTICES from Cargo.lock using cargo-about.
#
# The file has three parts:
#   1. Manual header  — in-tree source ports and inspired-by components
#                       (scripts/templates/third-party-header.txt)
#   2. Auto-generated — dependency listing from cargo-about
#                       (scripts/templates/third-party-notices.hbs)
#   3. License links  — SPDX license text references
#
# Prerequisites:
#   cargo install --locked --features cli cargo-about
#
# Usage:
#   scripts/generate-notices.sh          # write to THIRD-PARTY-NOTICES
#   scripts/generate-notices.sh --check  # exit 1 if regeneration needed
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root_dir="$(cd "$script_dir/.." && pwd)"
header="$script_dir/templates/third-party-header.txt"
template="$script_dir/templates/third-party-notices.hbs"
output="$root_dir/THIRD-PARTY-NOTICES"

if ! command -v cargo-about &>/dev/null; then
    echo "ERROR: cargo-about is not installed." >&2
    echo "  Install with: cargo install --locked --features cli cargo-about" >&2
    exit 1
fi

# Generate the auto-generated dependency section. Workspace scope covers every
# shipped binary (root CLI plus the desktop app); single-package scope would
# silently drop desktop-only dependencies such as rquickjs.
body=$(cd "$root_dir" && cargo about generate --workspace --locked "$template" 2>/dev/null)

# Assemble the full file.
{
    cat "$header"
    echo "$body"
    echo ""
    echo "================================================================================"
    echo "PART II — LICENSE TEXTS"
    echo "================================================================================"
    echo ""
    echo "Full license texts for the licenses referenced above are available at:"
    echo "  https://spdx.org/licenses/"
    echo ""
    echo "Key licenses used in this project:"
    echo "  - Apache-2.0: http://www.apache.org/licenses/LICENSE-2.0"
    echo "  - MIT: https://opensource.org/licenses/MIT"
    echo "  - BSD-3-Clause: https://opensource.org/licenses/BSD-3-Clause"
    echo "  - BSD-2-Clause: https://opensource.org/licenses/BSD-2-Clause"
    echo "  - ISC: https://opensource.org/licenses/ISC"
} > /tmp/tpn-new.txt

if [[ "${1:-}" == "--check" ]]; then
    # CI mode: compare without modifying the file.
    if ! diff -q "$output" /tmp/tpn-new.txt &>/dev/null; then
        echo "ERROR: THIRD-PARTY-NOTICES is out of date." >&2
        echo "  Run: scripts/generate-notices.sh" >&2
        diff "$output" /tmp/tpn-new.txt | head -30 >&2
        rm -f /tmp/tpn-new.txt
        exit 1
    fi
    rm -f /tmp/tpn-new.txt
    echo "THIRD-PARTY-NOTICES is up to date."
    exit 0
fi

mv /tmp/tpn-new.txt "$output"
echo "Generated $output"
echo "  Review the changes and commit if the dependency listing looks correct."
