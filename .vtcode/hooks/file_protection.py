#!/usr/bin/env python3
"""
File protection hook for VTCode.
Blocks edits to sensitive files based on file path.
Exits with code 2 to block the tool if sensitive file is detected.
"""
import json
import sys
import os

def is_sensitive_file(file_path):
    """Check if the file path is considered sensitive."""
    sensitive_patterns = [
        '.env',
        'package-lock.json',
        'yarn.lock',
        'Gemfile.lock',
        'Cargo.lock',
        '.git/',
        'config/',
        'secrets/',
        'credentials',
        'password',
        'private',
        'secret',
        'token',
        'key'
    ]
    
    # Convert to lowercase for case-insensitive matching
    lower_path = file_path.lower()
    
    # Check for exact matches or paths containing sensitive patterns
    for pattern in sensitive_patterns:
        if pattern in lower_path:
            # Additional check to make sure we're not blocking common files
            if pattern == 'config/' and 'config/' in lower_path and 'config/' in os.path.dirname(file_path) + '/':
                return True
            elif pattern in ['password', 'secret', 'token', 'key']:
                # Only block if the pattern is part of a filename, not just anywhere in the path
                filename = os.path.basename(file_path)
                if pattern in filename.lower():
                    return True
            elif pattern in ['.env', 'package-lock.json', 'yarn.lock', 'Gemfile.lock', 'Cargo.lock']:
                if os.path.basename(file_path).lower() == pattern:
                    return True
            elif pattern == '.git/':
                if '.git/' in lower_path:
                    return True
            else:
                return True
    
    return False

# Main execution
try:
    input_data = json.load(sys.stdin)
    file_path = input_data.get('tool_input', {}).get('file_path', '')
    
    if is_sensitive_file(file_path):
        print(f"Blocked modification of sensitive file: {file_path}", file=sys.stderr)
        sys.exit(2)  # Exit with code 2 to block the tool
    else:
        print(f"File {file_path} is not sensitive, allowing operation")
        
except Exception as e:
    print(f"Error in file protection hook: {e}", file=sys.stderr)
    # Don't block on error, just continue
    sys.exit(0)