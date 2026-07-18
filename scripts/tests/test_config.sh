#!/usr/bin/env bash

# Test script for VT Code config loading from home directory

echo "Testing VT Code configuration loading..."

# Check if ~/.vtcode directory exists
if [ -d "$HOME/.vtcode" ]; then
    echo "✓ Found ~/.vtcode directory"

    # Check if vtcode.toml exists in ~/.vtcode
    if [ -f "$HOME/.vtcode/vtcode.toml" ]; then
        echo "✓ Found ~/.vtcode/vtcode.toml"

        # Check if the file has content
        if [ -s "$HOME/.vtcode/vtcode.toml" ]; then
            echo "✓ Configuration file has content"
        else
            echo "⚠ Configuration file is empty"
        fi
    else
        echo "✗ ~/.vtcode/vtcode.toml not found"
    fi
else
    echo "✗ ~/.vtcode directory not found"
fi

echo "Test completed."