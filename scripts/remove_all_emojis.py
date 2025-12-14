#!/usr/bin/env python3
"""
Remove ALL emojis from ALL Python and MD files
"""

import os
import re
from pathlib import Path

def remove_emojis_from_file(file_path):
    """Remove emojis from a single file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        # Pattern to match most emojis
        emoji_pattern = re.compile(
            "["
            "\U0001F600-\U0001F64F"  # emoticons
            "\U0001F300-\U0001F5FF"  # symbols & pictographs
            "\U0001F680-\U0001F6FF"  # transport & map symbols
            "\U0001F700-\U0001F77F"  # alchemical symbols
            "\U0001F780-\U0001F7FF"  # Geometric Shapes Extended
            "\U0001F800-\U0001F8FF"  # Supplemental Arrows-C
            "\U0001F900-\U0001F9FF"  # Supplemental Symbols and Pictographs
            "\U0001FA00-\U0001FA6F"  # Chess Symbols
            "\U0001FA70-\U0001FAFF"  # Symbols and Pictographs Extended-A
            "\U00002702-\U000027B0"  # Dingbats
            "\U000024C2-\U0001F251"
            "]+", 
            flags=re.UNICODE
        )
        
        cleaned_content = emoji_pattern.sub(r"", content)
        
        # Also remove common emoji-like patterns
        cleaned_content = re.sub(r'[]', '', cleaned_content)
        
        if content != cleaned_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(cleaned_content)
            return True
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
        # Skip __pycache__ and .git directories
        if "__pycache__" in str(file) or ".git" in str(file):
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
