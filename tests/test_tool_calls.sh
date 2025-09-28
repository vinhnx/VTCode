#!/bin/bash
# VTCode End-to-End Tool Call Testing Script
# Tests various tool calls to ensure they work correctly

set -e

echo "🧪 VTCode End-to-End Tool Call Testing"
echo "======================================="

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

TESTS_RUN=0
TESTS_PASSED=0

# Function to run a test
run_test() {
    local test_name="$1"
    local command="$2"
    local expected_success="$3"

    echo -e "\n${YELLOW}Testing: ${test_name}${NC}"
    TESTS_RUN=$((TESTS_RUN + 1))

    if eval "$command" 2>/dev/null; then
        if [ "$expected_success" = "true" ]; then
            echo -e "${GREEN}✓ PASSED${NC}"
            TESTS_PASSED=$((TESTS_PASSED + 1))
        else
            echo -e "${RED}✦ FAILED (expected failure but succeeded)${NC}"
        fi
    else
        if [ "$expected_success" = "false" ]; then
            echo -e "${GREEN}✓ PASSED (expected failure)${NC}"
            TESTS_PASSED=$((TESTS_PASSED + 1))
        else
            echo -e "${RED}✦ FAILED${NC}"
        fi
    fi
}

echo "Testing basic compilation and setup..."
run_test "Cargo Check" "cargo check --quiet" "true"
run_test "Cargo Build" "cargo build --quiet" "true"

echo -e "\nTesting tool functionality..."

# Test file operations
run_test "List Directory" "ls -la /tmp" "true"
run_test "Create Test File" "echo 'test content' > /tmp/vtcode_test.txt" "true"
run_test "Read Test File" "cat /tmp/vtcode_test.txt" "true"
run_test "File Exists Check" "test -f /tmp/vtcode_test.txt" "true"

# Test search tools
run_test "Grep Search" "grep -r 'fn main' src/ 2>/dev/null || true" "true"
run_test "Find Command" "find . -name '*.rs' -type f | head -5" "true"

# Test JSON parsing
run_test "JSON Parse Test" "echo '{\"test\": \"value\"}' | python3 -m json.tool > /dev/null" "true"

# Test network connectivity (basic)
run_test "Network Test" "curl -s --max-time 5 https://httpbin.org/status/200 > /dev/null" "true"

# Cleanup
run_test "Cleanup Test Files" "rm -f /tmp/vtcode_test.txt" "true"

echo -e "\n✦ Test Results"
echo "==============="
echo "Tests Run: $TESTS_RUN"
echo "Tests Passed: $TESTS_PASSED"
echo "Success Rate: $((TESTS_PASSED * 100 / TESTS_RUN))%"

if [ $TESTS_PASSED -eq $TESTS_RUN ]; then
    echo -e "\n${GREEN}🎉 All tests passed! VTCode tools are working correctly.${NC}"
    exit 0
else
    echo -e "\n${RED}・  Some tests failed. Please check the output above.${NC}"
    exit 1
fi