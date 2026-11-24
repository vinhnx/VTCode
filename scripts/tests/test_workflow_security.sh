#!/bin/bash
# Fixture test for scripts/check_workflow_security.sh tool-pin policy:
# install-action `tool:` entries must carry an explicit @version.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT

mkdir -p "$fixture/.github/workflows" "$fixture/scripts"
cp "$repo_root/scripts/check_workflow_security.sh" "$fixture/scripts/"

# Minimal lockfile covering the fixture workflow's single `uses:` ref, so the
# tool-pin cases below exercise only the tool rule, not the pin rule.
cat >"$fixture/.github/workflows/actions.lock" <<'EOF'
version: 'v0.0.3'
workflows:
    '.github/workflows/fixture.yml':
        - 'taiki-e/install-action@v2.90.5'
dependencies:
    'taiki-e/install-action@v2.90.5':
        ref: 'v2.90.5'
        commit: 'sha1-ab49d3c64680143e184118bed66a46419fc15519'
EOF

write_workflow() {
    {
        printf '%s\n' "name: Fixture" "on: push" "jobs:" "  build:" \
            "    runs-on: ubuntu-latest" "    steps:" \
            "      - uses: taiki-e/install-action@v2.90.5" "        with:"
        printf '%s\n' "$1"
    } >"$fixture/.github/workflows/fixture.yml"
}

run_gate() {
    (cd "$fixture" && bash scripts/check_workflow_security.sh) >"$fixture/out.log" 2>&1
}

expect_pass() {
    local name="$1"
    local tool_lines="$2"
    write_workflow "$tool_lines"
    if run_gate; then
        echo "ok: $name"
    else
        echo "FAIL: $name should pass" >&2
        cat "$fixture/out.log" >&2
        exit 1
    fi
}

expect_fail() {
    local name="$1"
    local tool_lines="$2"
    local want="$3"
    write_workflow "$tool_lines"
    if run_gate; then
        echo "FAIL: $name should fail" >&2
        exit 1
    elif grep -qF "$want" "$fixture/out.log"; then
        echo "ok: $name"
    else
        echo "FAIL: $name failed without '$want'" >&2
        cat "$fixture/out.log" >&2
        exit 1
    fi
}

expect_pass "pinned tool" "          tool: ripgrep@15.2.0"
expect_pass "expression-resolved version" '          tool: nextest@${{ steps.nextest.outputs.version }}'
expect_pass "pinned multi-tool list" "          tool: foo@1.0, bar@2.0"
expect_pass "quoted pin with trailing comment" '          tool: "ripgrep@15.2.0" # bump deliberately'
expect_fail "unpinned tool" "          tool: ripgrep" "tool 'ripgrep' must pin an explicit version"
expect_fail "mixed pinned/unpinned list" "          tool: foo@1.0, bar" "tool 'bar' must pin an explicit version"
expect_fail "empty tool value" "          tool:" "must name a tool with an explicit version"
expect_fail "bare at-sign" "          tool: ripgrep@" "tool 'ripgrep@' must pin an explicit version"
expect_fail "empty tool name" "          tool: @1.0" "tool '@1.0' must pin an explicit version"

echo "workflow-security tool-pin tests passed"
