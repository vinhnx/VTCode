#!/bin/bash
# diagnose_resource_issues.sh - Quick diagnostic for CPU/memory issues in VT Code

set -e

echo "🔍 VT Code Resource Diagnostic Tool"
echo "═══════════════════════════════════════════════════════════════"
echo ""

# System Info
echo "📋 System Information"
echo "─────────────────────────────────────────────────────────────"

OS=$(uname -s)
echo "OS:          $OS"

if [[ "$OS" == "Darwin" ]]; then
    MEMORY=$(sysctl -n hw.memsize | awk '{printf "%.1f GB", $1 / 1024 / 1024 / 1024}')
    CPU_CORES=$(sysctl -n hw.ncpu)
else
    MEMORY=$(free -h | awk 'NR==2 {print $2}')
    CPU_CORES=$(nproc)
fi

echo "Total Memory: $MEMORY"
echo "CPU Cores:   $CPU_CORES"
echo ""

# Check for running VT Code instances
echo "🔍 Running VT Code Processes"
echo "─────────────────────────────────────────────────────────────"

VTCODE_PIDS=$(pgrep -f "vtcode" 2>/dev/null || true)

if [[ -z "$VTCODE_PIDS" ]]; then
    echo "ℹ️  No running VT Code processes found"
else
    echo "Found $(echo "$VTCODE_PIDS" | wc -l) process(es):"
    echo ""
    
    for PID in $VTCODE_PIDS; do
        echo "Process ID: $PID"
        
        if [[ "$OS" == "Darwin" ]]; then
            # macOS
            ps -p $PID -o comm= -o rss= -o vsz= -o %cpu= | \
            awk '{printf "  Command:     %s\n  Memory RSS:  %.1f MB\n  Memory VSZ:  %.1f MB\n  CPU:         %s%%\n", $1, $2/1024, $3/1024, $4}'
            
            # Get thread count
            THREADS=$(ps -p $PID -o nlwp=)
            echo "  Threads:     $THREADS"
        else
            # Linux
            ps -p $PID -o comm= -o rss= -o vsz= -o %cpu= | \
            awk '{printf "  Command:     %s\n  Memory RSS:  %.1f MB\n  Memory VSZ:  %.1f MB\n  CPU:         %s%%\n", $1, $2/1024, $3/1024, $4}'
            
            # Get thread count
            THREADS=$(ps -p $PID -o nlwp= | xargs)
            echo "  Threads:     $THREADS"
        fi
        
        echo ""
    done
fi

# Check Cargo build artifacts
echo "📦 Build Artifacts"
echo "─────────────────────────────────────────────────────────────"

if [[ -d "target" ]]; then
    TARGET_SIZE=$(du -sh target 2>/dev/null | cut -f1)
    echo "Target directory: $TARGET_SIZE"
    
    # Check for debug vs release
    if [[ -d "target/debug" ]]; then
        DEBUG_SIZE=$(du -sh target/debug 2>/dev/null | cut -f1)
        echo "  Debug:   $DEBUG_SIZE"
    fi
    
    if [[ -d "target/release" ]]; then
        RELEASE_SIZE=$(du -sh target/release 2>/dev/null | cut -f1)
        echo "  Release: $RELEASE_SIZE"
    fi
else
    echo "No target directory found"
fi

echo ""

# Check for cache/temporary files
echo "🗂️  Cache & Temporary Files"
echo "─────────────────────────────────────────────────────────────"

if [[ -d ".vtcode" ]]; then
    VTCODE_CACHE=$(du -sh .vtcode 2>/dev/null | cut -f1)
    echo ".vtcode cache:  $VTCODE_CACHE"
fi

if [[ -d ".cargo" ]]; then
    CARGO_CACHE=$(du -sh .cargo 2>/dev/null | cut -f1)
    echo ".cargo cache:   $CARGO_CACHE"
fi

echo ""

# Code statistics
echo "📊 Codebase Statistics"
echo "─────────────────────────────────────────────────────────────"

RUST_FILES=$(find . -name "*.rs" -not -path "./target/*" 2>/dev/null | wc -l)
RUST_LINES=$(find . -name "*.rs" -not -path "./target/*" -exec wc -l {} \; 2>/dev/null | awk '{sum+=$1} END {print sum}')

echo "Rust files:     $RUST_FILES"
echo "Lines of code:  $RUST_LINES"

# Workspace info
if [[ -f "Cargo.toml" ]]; then
    WORKSPACES=$(grep -c '^\[workspace\]' Cargo.toml 2>/dev/null || echo 0)
    MEMBERS=$(grep -c 'members' Cargo.toml 2>/dev/null || echo 0)
    
    if [[ $WORKSPACES -gt 0 ]]; then
        echo "Workspace:      Yes"
        echo "Workspace type: Cargo workspace"
    fi
fi

echo ""

# Potential Issues
echo "⚠️  Potential Issues"
echo "─────────────────────────────────────────────────────────────"

ISSUES=0

# Check for debug profile in dev
if grep -q 'opt-level = 0' Cargo.toml 2>/dev/null; then
    echo "⚠️  Debug profile detected (opt-level = 0)"
    echo "   → Use 'cargo build --release' for better performance"
    ISSUES=$((ISSUES + 1))
fi

# Check for LTO settings
if ! grep -q 'lto' Cargo.toml 2>/dev/null; then
    echo "⚠️  LTO not configured in Cargo.toml"
    echo "   → Add 'lto = true' to [profile.release] for optimizations"
    ISSUES=$((ISSUES + 1))
fi

# Check for unbounded caches
if grep -r 'HashMap::new()' --include="*.rs" . 2>/dev/null | grep -v test | grep -q .; then
    HASHMAP_COUNT=$(grep -r 'HashMap::new()' --include="*.rs" . 2>/dev/null | grep -v test | wc -l)
    echo "⚠️  Found $HASHMAP_COUNT unbounded HashMap allocations"
    echo "   → Consider adding size limits or TTL expiration"
    ISSUES=$((ISSUES + 1))
fi

# Check for excessive cloning
CLONE_COUNT=$(grep -r '\.clone()' --include="*.rs" . 2>/dev/null | grep -v test | wc -l)
if [[ $CLONE_COUNT -gt 100 ]]; then
    echo "⚠️  Found $CLONE_COUNT .clone() calls"
    echo "   → Review for unnecessary allocations (use references when possible)"
    ISSUES=$((ISSUES + 1))
fi

# Check for tree-sitter parsers
PARSER_COUNT=$(grep -r 'tree_sitter::Parser::new()' --include="*.rs" . 2>/dev/null | wc -l)
if [[ $PARSER_COUNT -gt 1 ]]; then
    echo "⚠️  Found $PARSER_COUNT tree-sitter parser initializations"
    echo "   → Consider implementing a parser pool for reuse"
    ISSUES=$((ISSUES + 1))
fi

if [[ $ISSUES -eq 0 ]]; then
    echo "✅ No obvious resource issues detected"
fi

echo ""

# Recommendations
echo "💡 Recommendations"
echo "─────────────────────────────────────────────────────────────"

if [[ "$OS" == "Darwin" ]]; then
    echo "For macOS:"
    echo "  1. Profile with Instruments (Xcode):"
    echo "     xcrun xctrace record --template 'System Trace' --launch — cargo run -- ask 'query'"
    echo ""
    echo "  2. Monitor with Activity Monitor:"
    echo "     open -a Activity\\ Monitor"
    echo ""
else
    echo "For Linux:"
    echo "  1. Profile with perf:"
    echo "     perf record -g cargo run -- ask 'query'"
    echo "     perf report"
    echo ""
    echo "  2. Monitor with htop:"
    echo "     htop -p \$(pgrep -f vtcode)"
    echo ""
fi

echo "  3. Run performance tests:"
echo "     cargo test --lib performance_tests -- --nocapture"
echo ""

echo "  4. Use the monitoring script:"
echo "     ./scripts/monitor_performance.sh 100 5"
echo ""

echo "  5. Review the debugging guide:"
echo "     open docs/performance/CPU_MEMORY_DEBUGGING_GUIDE.md"
echo ""

echo "═══════════════════════════════════════════════════════════════"
echo "✅ Diagnostic complete"
