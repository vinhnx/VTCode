#!/bin/bash

# vtcode Structured Logging Lint
# Ensures no println! or eprintln! is used in library crates.
# See ARCHITECTURAL_INVARIANTS.md #4

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Library crates to check (exclude TUI bin/ and tests)
# We check all .rs files except those in tests/ or src/bin/ or src/main.rs (if in a binary crate)
# Actually, the invariant says "No println! or eprintln! in library code"
# "TUI binary src/ may use eprintln! for fatal startup errors only"

echo "Running Structured Logging Lint..."

# We use grep to find violations
# Exclude:
# - tests/ directories
# - src/main.rs
# - src/bin/ directories
# - build.rs (usually okay for build scripts to print)
# - target/ directory

VIOLATIONS=$(grep -rE "println\!|eprintln\!" . \
    --exclude-dir={.git,target,tests,node_modules,vscode-extension} \
	--exclude={"*test.rs","build.rs","Cargo.toml","*.md"} \
	--exclude="src/main.rs" \
	--exclude="src/bin/*" \
	--include="*.rs" | grep -v "//" || true)

if [ -n "$VIOLATIONS" ]; then
    echo -e "${RED}[FAIL]${NC} Found println! or eprintln! in library code:"
	echo "$VIOLATIONS"
	echo ""
	echo -e "${YELLOW}Remediation:${NC} Replace with tracing macros (info!, warn!, error!, debug!)"
	echo "See ARCHITECTURAL_INVARIANTS.md #4 for details."
	exit 1
else
    echo -e "${GREEN}[PASS]${NC} No structured logging violations found."
	exit 0
fi
