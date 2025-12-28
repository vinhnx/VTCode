#!/bin/bash
# monitor_performance.sh - Real-time VT Code performance monitoring
# Usage: ./scripts/monitor_performance.sh [num_turns] [interval_seconds]

set -e

NUM_TURNS=${1:-100}
INTERVAL=${2:-5}
OUTPUT_FILE="vtcode_performance_metrics.csv"

echo "🔍 VT Code Performance Monitor"
echo "════════════════════════════════════════════════"
echo "Turns to run: $NUM_TURNS"
echo "Monitoring interval: ${INTERVAL}s"
echo "Output file: $OUTPUT_FILE"
echo ""

# Create CSV header
echo "timestamp_iso,turn_number,rss_mb,vsz_mb,cpu_percent,num_threads,cache_hits,cache_misses,tool_calls" > "$OUTPUT_FILE"

# Function to get process metrics
get_metrics() {
    local pid=$1
    local turn=$2
    
    if ! kill -0 $pid 2>/dev/null; then
        return 1
    fi
    
    # Get process stats (macOS and Linux compatible)
    if [[ "$OSTYPE" == "darwin"* ]]; then
        # macOS
        local stats=$(ps -p $pid -o rss= -o vsz= | awk '{print $1, $2}')
        local rss=$(echo $stats | awk '{printf "%.1f", $1 / 1024}')
        local vsz=$(echo $stats | awk '{printf "%.1f", $2 / 1024}')
        local cpu=$(ps -p $pid -o %cpu= | xargs)
        local threads=$(ps -p $pid -o nlwp=)
    else
        # Linux
        local stats=$(ps -p $pid -o rss= -o vsz= | awk '{print $1, $2}')
        local rss=$(echo $stats | awk '{printf "%.1f", $1 / 1024}')
        local vsz=$(echo $stats | awk '{printf "%.1f", $2 / 1024}')
        local cpu=$(ps -p $pid -o %cpu= | xargs)
        local threads=$(ps -p $pid -o nlwp= | xargs)
    fi
    
    # Get timestamp
    local timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
    
    # Write to CSV (placeholder values for cache stats - would need instrumentation)
    echo "$timestamp,$turn,$rss,$vsz,$cpu,$threads,0,0,0" >> "$OUTPUT_FILE"
    
    echo "[$timestamp] Turn $turn | RSS: ${rss}MB | VSZ: ${vsz}MB | CPU: ${cpu}% | Threads: $threads"
}

# Kill background jobs on exit
cleanup() {
    if [[ ! -z "$VTCODE_PID" ]] && kill -0 $VTCODE_PID 2>/dev/null; then
        echo "Stopping VT Code..."
        kill $VTCODE_PID 2>/dev/null || true
        wait $VTCODE_PID 2>/dev/null || true
    fi
    echo ""
    echo "✅ Monitoring complete. Results saved to: $OUTPUT_FILE"
}

trap cleanup EXIT

echo "Starting VT Code..."
echo ""

# Create a test script that sends multiple queries
TEST_SCRIPT=$(mktemp)
cat > "$TEST_SCRIPT" << 'EOF'
#!/bin/bash
for i in $(seq 1 %NUM_TURNS%); do
  case $((i % 4)) in
    0) echo "Query $i: List files in src/" ;;
    1) echo "Query $i: Analyze this function" ;;
    2) echo "Query $i: Find memory leaks" ;;
    3) echo "Query $i: Check cache efficiency" ;;
  esac
  sleep 1
done
echo "exit"
EOF

sed -i '' "s/%NUM_TURNS%/$NUM_TURNS/g" "$TEST_SCRIPT"
chmod +x "$TEST_SCRIPT"

# Start VT Code in the background with test input
cargo run -- ask "Starting performance test..." &
VTCODE_PID=$!

sleep 2  # Let it start up

echo "Monitoring PID $VTCODE_PID..."
echo ""

# Monitor process
TURN=0
while kill -0 $VTCODE_PID 2>/dev/null; do
    TURN=$((TURN + 1))
    get_metrics $VTCODE_PID $TURN
    
    if [[ $TURN -ge $NUM_TURNS ]]; then
        echo ""
        echo "Completed $NUM_TURNS turns. Stopping VT Code..."
        kill $VTCODE_PID 2>/dev/null || true
        break
    fi
    
    sleep $INTERVAL
done

# Wait for process to finish
wait $VTCODE_PID 2>/dev/null || true

# Cleanup
rm -f "$TEST_SCRIPT"

# Print summary
echo ""
echo "═══════════════════════════════════════════════"
echo "📊 Performance Summary"
echo "═══════════════════════════════════════════════"
echo ""

if [[ -f "$OUTPUT_FILE" ]]; then
    echo "Memory Usage Statistics:"
    tail -n +2 "$OUTPUT_FILE" | awk -F',' '{
        rss+=$3; vsz+=$4; cpu+=$5; threads+=$6; n++
    }
    END {
        if (n > 0) {
            printf "  Average RSS:     %.1f MB\n", rss/n
            printf "  Average VSZ:     %.1f MB\n", vsz/n
            printf "  Average CPU:     %.1f%%\n", cpu/n
            printf "  Average Threads: %.0f\n", threads/n
        }
    }'
    
    echo ""
    echo "Peak Memory Usage:"
    tail -n +2 "$OUTPUT_FILE" | awk -F',' '{
        if ($3 > max_rss) max_rss = $3
        if ($4 > max_vsz) max_vsz = $4
    }
    END {
        printf "  Peak RSS:        %.1f MB\n", max_rss
        printf "  Peak VSZ:        %.1f MB\n", max_vsz
    }'
    
    echo ""
    echo "Data saved to: $OUTPUT_FILE"
    echo ""
    echo "To analyze results:"
    echo "  $ python3 scripts/analyze_metrics.py $OUTPUT_FILE"
fi
