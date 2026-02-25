#!/usr/bin/env python3
"""
Cleanup root directory .md files and organize them
"""

import os
import shutil
from pathlib import Path

def cleanup_root_files():
    """Move .md files from root to appropriate directories."""
    
    base_dir = Path("/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode")
    
    # Mapping of files to their destinations
    file_mappings = {
        "claude_api_skills_integration.md": "docs/skills-enhanced/claude-api-integration.md",
        "claude_api_skills_tutorial_analysis.md": "docs/skills-enhanced/tutorial-analysis.md",
        "skills_best_practices.md": "docs/skills-enhanced/best-practices.md",
        "skills_usage_guide.md": "docs/skills-enhanced/usage-guide.md",
        "vtcode_agent_skills_architecture.md": "examples/skills/architecture.md",
        "vtcode_claude_skills_analysis.md": "docs/skills-enhanced/claude-analysis.md",
        "vtcode_claude_skills_implementation.md": "docs/skills-enhanced/implementation.md",
        "vtcode_skills_improvement_guide.md": "docs/skills-enhanced/improvement-guide.md",
        "vtcode_skills_improvements_proven.md": "docs/skills-enhanced/improvements-proven.md",
        "FINAL_SUMMARY.md": "docs/skills-enhanced/final-summary.md",
    }
    
    print("Cleaning up root directory .md files...")
    print("=" * 60)
    
    moved = 0
    skipped = 0
    
    for filename, dest_path in file_mappings.items():
        source_path = base_dir / filename
        dest_full_path = base_dir / dest_path
        
        if source_path.exists():
            try:
                # Create destination directory
                dest_full_path.parent.mkdir(parents=True, exist_ok=True)
                
                # Move file
                shutil.move(str(source_path), str(dest_full_path))
                
                print(f" Moved: {filename} -> {dest_path}")
                moved += 1
            except Exception as e:
                print(f" Error moving {filename}: {e}")
                skipped += 1
        else:
            print(f"- Skipped: {filename} (not found)")
            skipped += 1
    
    print("=" * 60)
    print(f" Moved: {moved} files")
    print(f"- Skipped: {skipped} files")
    
    # Check remaining files
    remaining = list(base_dir.glob("*.md"))
    remaining = [f for f in remaining if f.name not in [
        "AGENTS.md", "CHANGELOG.md", "CODE_OF_CONDUCT.md", "CONTRIBUTING.md",
    ]]
    
    if remaining:
        print(f"\nNote: {len(remaining)} .md files remain in root (likely intentional)")
        for f in remaining[:5]:  # Show first 5
            print(f"  - {f.name}")

if __name__ == "__main__":
    cleanup_root_files()
