#!/bin/bash
# Memory Optimization Verification Script
# Verifies that all memory optimizations are properly implemented

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "🔍 VT Code Memory Optimization Verification"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Step 1: Check that optimized constants are in place
echo "1️⃣  Checking cache configuration constants..."
if grep -q "Duration::from_secs(120)" "$PROJECT_ROOT/vtcode-core/src/cache/mod.rs"; then
    echo "   ✅ Cache TTL optimized to 120s (2 minutes)"
else
    echo "   ❌ Cache TTL not optimized"
    exit 1
fi

if grep -q "DEFAULT_MAX_CACHE_CAPACITY.*1_000" "$PROJECT_ROOT/vtcode-core/src/cache/mod.rs"; then
    echo "   ✅ Cache capacity limited to 1,000 entries"
else
    echo "   ❌ Cache capacity not properly limited"
    exit 1
fi

# Step 2: Check parse cache optimization
echo ""
echo "2️⃣  Checking parse cache optimization..."
if grep -q "Self::new(50, 120" "$PROJECT_ROOT/vtcode-core/src/tools/tree_sitter/parse_cache.rs"; then
    echo "   ✅ Parse cache reduced to 50 entries with 120s TTL"
else
    echo "   ❌ Parse cache not optimized"
    exit 1
fi

# Step 3: Check PTY scrollback optimization
echo ""
echo "3️⃣  Checking PTY scrollback optimization..."
if grep -q "25_000_000" "$PROJECT_ROOT/vtcode-config/src/root.rs"; then
    echo "   ✅ PTY max scrollback reduced to 25MB"
else
    echo "   ❌ PTY scrollback not optimized"
    exit 1
fi

# Step 4: Check transcript cache width limiting
echo ""
echo "4️⃣  Checking transcript cache optimization..."
if grep -q "max_cached_widths: usize" "$PROJECT_ROOT/vtcode-core/src/ui/tui/session/transcript.rs"; then
    echo "   ✅ Transcript cache width limit implemented"
else
    echo "   ❌ Transcript cache not optimized"
    exit 1
fi

if grep -q "max_cached_widths: 3" "$PROJECT_ROOT/vtcode-core/src/ui/tui/session/transcript.rs"; then
    echo "   ✅ Transcript width limit set to 3"
else
    echo "   ❌ Transcript width limit not set correctly"
    exit 1
fi

# Step 5: Check memory tests exist
echo ""
echo "5️⃣  Checking memory test infrastructure..."
if [ -f "$PROJECT_ROOT/vtcode-core/src/memory_tests.rs" ]; then
    echo "   ✅ Memory tests module exists"
    
    TEST_COUNT=$(grep -c "fn test_" "$PROJECT_ROOT/vtcode-core/src/memory_tests.rs")
    echo "   ✅ Found $TEST_COUNT memory tests"
else
    echo "   ❌ Memory tests not found"
    exit 1
fi

# Step 6: Build and run tests
echo ""
echo "6️⃣  Running memory optimization tests..."
if cd "$PROJECT_ROOT" && cargo test --package vtcode-core --lib memory_tests:: --release 2>&1 | grep -q "test result: ok"; then
    echo "   ✅ All memory tests passing"
else
    echo "   ❌ Memory tests failed"
    exit 1
fi

# Step 7: Check documentation
echo ""
echo "7️⃣  Checking documentation..."
DOCS=(
    "docs/debugging/MEMORY_OPTIMIZATION.md"
    "docs/debugging/MEMORY_OPTIMIZATION_IMPLEMENTATION.md"
    "docs/debugging/MEMORY_QUICK_START.md"
)

for doc in "${DOCS[@]}"; do
    if [ -f "$PROJECT_ROOT/$doc" ]; then
        echo "   ✅ $doc exists"
    else
        echo "   ⚠️  $doc missing (optional)"
    fi
done

# Step 8: Verify build
echo ""
echo "8️⃣  Building release binary..."
if cd "$PROJECT_ROOT" && cargo build --release 2>&1 | tail -5 | grep -q "Finished"; then
    echo "   ✅ Release build successful"
else
    echo "   ❌ Release build failed"
    exit 1
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ All memory optimizations verified successfully!"
echo ""
echo "Summary of Improvements:"
echo "  • Cache TTL: 5 min → 2 min (2x faster cleanup)"
echo "  • Cache capacity: ~10k → 1k entries (tighter bounds)"
echo "  • Parse cache: 100 → 50 entries (50% reduction)"
echo "  • PTY scrollback: 50MB → 25MB/session (50% reduction)"
echo "  • Transcript cache: unbounded → max 3 width caches"
echo ""
echo "Estimated memory savings: 30-40% for typical dev sessions"
echo ""
echo "See docs/debugging/MEMORY_QUICK_START.md for next steps."
