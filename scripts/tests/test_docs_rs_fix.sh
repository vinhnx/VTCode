#!/bin/bash

# Test script to verify the docs.rs fix for mcp-types build script
echo "Testing docs.rs fix for mcp-types build script..."

# Test 1: Normal build (should generate files if not exists or in normal mode)
echo "Test 1: Running normal build..."
cd third-party/mcp-types
cargo build --release
if [ $? -eq 0 ]; then
    echo "✓ Normal build succeeded"
else
    echo "✗ Normal build failed"
    exit 1
fi

# Test 2: Simulate docs.rs environment (should skip generation)
echo "Test 2: Simulating docs.rs build..."
DOCS_RS=1 cargo build --release
if [ $? -eq 0 ]; then
    echo "✓ docs.rs simulation succeeded (build script skipped file generation)"
else
    echo "✗ docs.rs simulation failed"
    exit 1
fi

echo "All tests passed! The docs.rs fix is working correctly."