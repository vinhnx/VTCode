#!/usr/bin/env python3
"""
VT Code Status Line Script (Python Version)
Receives JSON input from vtcode via stdin and outputs a formatted status line
"""
import json
import sys
import os

def main():
    # Read JSON from stdin
    try:
        input_data = json.load(sys.stdin)
    except json.JSONDecodeError:
        print("Error: Invalid JSON input")
        return

    # Extract values
    model = input_data.get('model', {}).get('display_name') or input_data.get('model', {}).get('id') or 'unknown'
    current_dir = os.path.basename(input_data.get('workspace', {}).get('current_dir', ''))
    reasoning = input_data.get('runtime', {}).get('reasoning_effort', '')
    version = input_data.get('version', 'unknown')

    # Extract git information
    git_info = input_data.get('git', {})
    git_branch = git_info.get('branch', '')
    git_dirty = git_info.get('dirty', False)

    # Format git status
    git_status = ""
    if git_branch:
        git_status = f" on {git_branch} *" if git_dirty else f" on {git_branch}"

    # Format reasoning effort
    reasoning_display = f" | thinking: {reasoning}" if reasoning else ""

    # Extract context information
    context_info = input_data.get('context', {})
    context_util = context_info.get('utilization_percent', 0)
    context_tokens = context_info.get('total_tokens', 0)

    # Format context information
    context_display = ""
    if context_util and context_util != 0:
        context_display = f" | ctx: {context_util}% ({context_tokens} tokens)"

    # Build and print the status line
    print(f"[{model}] in {current_dir}{git_status}{reasoning_display}{context_display} | v{version}")

if __name__ == "__main__":
    main()