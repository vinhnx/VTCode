#!/usr/bin/env bash
set -euo pipefail

# yank-stale-crates.sh — Yanks all published versions of vtcode-tui,
# vtcode-design, and vtcode-theme from crates.io. These crates have been
# consolidated into vtcode-ui and should no longer be resolvable by new
# consumers.
#
# Prerequisites:
#   - Authenticated with crates.io: run `cargo login` first
#   - curl and python3 available
#
# Usage:
#   ./scripts/yank-stale-crates.sh [--dry-run]

DRY_RUN=0
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=1
    echo "DRY RUN — will print commands without executing them"
fi

UA="vtcode-release-script"
CRATES=(vtcode-design vtcode-theme vtcode-tui)
TOTAL_YANKED=0
TOTAL_FAILED=0

for crate in "${CRATES[@]}"; do
    echo "=== Processing $crate ==="
    page=1
    seek=""
    crate_yanked=0

    while true; do
        if [[ -z "$seek" ]]; then
            url="https://crates.io/api/v1/crates/${crate}/versions?per_page=100&page=${page}"
        else
            url="https://crates.io/api/v1/crates/${crate}/versions${seek}"
        fi

        resp=$(curl -s -H "User-Agent: $UA" "$url")

        # Extract non-yanked versions
        versions=$(echo "$resp" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for v in data.get('versions', []):
    if not v.get('yanked'):
        print(v['num'])
" 2>/dev/null)

        if [[ -z "$versions" ]]; then
            break
        fi

        while IFS= read -r ver; do
            if [[ "$DRY_RUN" -eq 1 ]]; then
                echo "  [dry-run] would yank ${crate}@${ver}"
            else
                echo "  yanking ${crate}@${ver}..."
                if cargo yank --version "$ver" --crate "$crate" 2>&1; then
                    crate_yanked=$((crate_yanked + 1))
                else
                    echo "  FAILED: ${crate}@${ver}"
                    TOTAL_FAILED=$((TOTAL_FAILED + 1))
                fi
                # Rate-limit to avoid overwhelming crates.io
                sleep 0.5
            fi
        done <<< "$versions"

        # Check for next page via seek token
        seek=$(echo "$resp" | python3 -c "
import sys, json
data = json.load(sys.stdin)
next_page = data.get('meta', {}).get('next_page', '')
if next_page:
    print(next_page)
" 2>/dev/null)

        if [[ -z "$seek" ]]; then
            break
        fi
        page=$((page + 1))
    done

    echo "  $crate: yanked $crate_yanked versions"
    TOTAL_YANKED=$((TOTAL_YANKED + crate_yanked))
done

echo ""
echo "=== Summary ==="
echo "Total yanked: $TOTAL_YANKED"
echo "Total failed: $TOTAL_FAILED"

if [[ "$DRY_RUN" -eq 0 && "$TOTAL_FAILED" -gt 0 ]]; then
    echo "WARNING: Some yanks failed. Check output above and retry if needed."
    exit 1
fi
