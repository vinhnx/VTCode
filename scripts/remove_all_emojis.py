#!/usr/bin/env python3
"""
Remove ALL emojis from ALL Python and MD files
"""

import os
import re
from pathlib import Path

def remove_emojis_from_file(file_path):
    """Remove non-ASCII characters from a single file, assuming they are emojis/symbols."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        # This replaces any non-ASCII character (ord > 127) with an empty string.
        # This effectively removes almost all emojis, which are outside ASCII range.
        cleaned_content = re.sub(r'[^\x00-\x7F]+', '', content)
        
        if content != cleaned_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(cleaned_content)
            return True
        return False
    except UnicodeDecodeError:
        # Ignore files that cannot be decoded as UTF-8 (likely binaries)
        return False
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return False

def find_python_and_md_files():
    """Find all Python and MD files."""
    base_path = Path("/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode")
    
    # Find all .py and .md files recursively
    py_files = list(base_path.rglob("*.py"))
    md_files = list(base_path.rglob("*.md"))
    
    # Filter out unwanted directories
    filtered_files = []
    for file in py_files + md_files:
        path_str = str(file)
        if "__pycache__" in path_str or ".git" in path_str or "node_modules" in path_str or ".venv" in path_str:
            continue
        filtered_files.append(file)
    
    return filtered_files

def main():
    """Remove emojis from all Python and MD files."""
    
    print("Removing emojis from ALL Python and MD files...")
    print("=" * 60)
    
    files = find_python_and_md_files()
    processed = 0
    modified = 0
    
    for file_path in sorted(files):
        processed += 1
        if remove_emojis_from_file(file_path):
            modified += 1
            print(f" Cleaned: {file_path.relative_to(Path.cwd())}")
    
    print("=" * 60)
    print(f"Processed: {processed} files")
    print(f"Modified: {modified} files (had emojis)")

if __name__ == "__main__":
    main()
