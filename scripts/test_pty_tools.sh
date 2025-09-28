#!/bin/bash
# VTCode Tools Focused Test
# Tests availability and basic functionality of external tools

set -e

echo "🧪 VTCode Tools Test"
echo "===================="

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

TESTS_RUN=0
TESTS_PASSED=0

run_test() {
    local test_name="$1"
    local command="$2"
    local expected_contains="$3"

    echo -e "\n${YELLOW}Testing: ${test_name}${NC}"
    TESTS_RUN=$((TESTS_RUN + 1))

    if eval "$command" 2>/dev/null | grep -q "$expected_contains"; then
        echo -e "${GREEN}PASSED${NC}"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}✦ FAILED${NC}"
        echo "Command: $command"
        echo "Expected: $expected_contains"
    fi
}

echo -e "\nTesting External Tools Availability"
echo "====================================="

# Test 1: CK search
run_test "CK Search Available" "which ck" "ck"

# Test 2: Ripgrep
run_test "Ripgrep Available" "which rg" "rg"

# Test 3: AST-grep
run_test "AST Grep Available" "which ast-grep" "ast-grep"


echo -e "\n🧪 Testing Tool Functionality"
echo "============================"

#! Following tests renumbered after removal of PTY checks
# Test 4: Ripgrep functionality
run_test "Ripgrep Functionality" "echo 'test content' > /tmp/vtcode_test.txt && rg 'test' /tmp/vtcode_test.txt && rm /tmp/vtcode_test.txt" "test content"

# Test 5: AST-grep functionality
run_test "AST Grep Functionality" "echo 'fn test() {}' > /tmp/vtcode_test.rs && ast-grep --lang rust --pattern 'fn \$A() {}' /tmp/vtcode_test.rs && rm /tmp/vtcode_test.rs" "fn test()"

echo -e "\nTesting Code Structure"
echo "========================="


echo -e "\n✦ Test Results"
echo "==============="
echo "Tests Run: $TESTS_RUN"
echo "Tests Passed: $TESTS_PASSED"
echo "Success Rate: $((TESTS_PASSED * 100 / TESTS_RUN))%"

if [ $TESTS_PASSED -eq $TESTS_RUN ]; then
    echo -e "\n${GREEN}🎉 All tool tests passed!${NC}"
    echo "External tools are available and functional"
    exit 0
else
    echo -e "\n${RED} Some tests failed. Check the output above.${NC}"
    exit 1
fi
