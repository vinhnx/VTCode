#!/usr/bin/env python3
"""
Organize VT Code skills files and remove emojis
"""

import os
import re
import shutil
from pathlib import Path

def remove_emojis(text):
    """Remove emojis from text."""
    # Pattern to match most common emojis
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
    return emoji_pattern.sub(r"", text)

def organize_file(source_path, dest_path):
    """Copy file to destination and remove emojis."""
    try:
        if source_path.exists():
            # Read, remove emojis, write to destination
            content = source_path.read_text(encoding='utf-8')
            cleaned_content = remove_emojis(content)
            
            dest_path.parent.mkdir(parents=True, exist_ok=True)
            dest_path.write_text(cleaned_content, encoding='utf-8')
            
            print(f" Processed: {source_path.name} -> {dest_path}")
            return True
    except Exception as e:
        print(f" Error processing {source_path}: {e}")
        return False

def main():
    """Organize files and remove emojis."""
    
    base_dir = Path("/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode")
    
    # Define file mappings
    file_mappings = [
        # Documentation files
        ("vtcode_skills_improvements_proven.md", "docs/skills-enhanced/improvements-proven.md"),
        ("FINAL_SUMMARY.md", "docs/skills-enhanced/final-summary.md"),
        ("vtcode_skills_improvement_guide.md", "docs/skills-enhanced/improvement-guide.md"),
        ("claude_api_skills_tutorial.py", "docs/skills-enhanced/claude-api-tutorial.py"),
        ("claude_api_skills_tutorial_analysis.md", "docs/skills-enhanced/tutorial-analysis.md"),
        ("claude_api_skills_integration.md", "docs/skills-enhanced/api-integration.md"),
        
        # Core implementation
        ("vtcode_skills_production.py", "src/skills-enhanced/production.py"),
        ("vtcode_skills_enhanced.py", "src/skills-enhanced/enhanced.py"),
        
        # Examples
        ("vtcode_agent_skills_demo.py", "examples/skills/demo.py"),
        ("vtcode_agent_skills_architecture.md", "examples/skills/architecture-example.md"),
        
        # Tests
        ("vtcode_skills_enhanced.py", "tests/skills/test_enhanced.py"),
    ]
    
    print("Organizing VT Code skills files and removing emojis...")
    print("=" * 60)
    
    processed = 0
    errors = 0
    
    for source_name, dest_path in file_mappings:
        source_path = base_dir / source_name
        dest_full_path = base_dir / dest_path
        
        if organize_file(source_path, dest_full_path):
            processed += 1
        else:
            errors += 1
    
    print("=" * 60)
    print(f" Processed: {processed} files")
    if errors > 0:
        print(f" Errors: {errors} files")
    else:
        print(" All files processed successfully")
    
    print("\nOrganization complete!")
    print("Files organized into:")
    print("  - docs/skills-enhanced/")
    print("  - src/skills-enhanced/")
    print("  - examples/skills/")
    print("  - tests/skills/")

if __name__ == "__main__":
    main()