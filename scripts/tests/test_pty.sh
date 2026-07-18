#!/bin/bash

# Test script for PTY functionality

echo "Testing PTY functionality..."

# Create a test directory
mkdir -p test_pty
cd test_pty

# Create a simple test file
echo "Hello, PTY!" > test.txt

# Test basic PTY command
echo "Running basic PTY command..."
vtcode chat <<< "Run 'ls -la' in a PTY"

# Test PTY command with arguments
echo "Running PTY command with arguments..."
vtcode chat <<< "Run 'cat test.txt' in a PTY"

# Test PTY command with custom terminal size
echo "Running PTY command with custom terminal size..."
vtcode chat <<< "Run 'echo \"Testing custom terminal size\"' in a PTY with 40 columns and 10 rows"

# Clean up
cd ..
rm -rf test_pty

echo "PTY testing complete."