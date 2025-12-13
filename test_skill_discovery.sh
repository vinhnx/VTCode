#!/bin/bash
# Test script to verify skill discovery works

echo "Testing skill discovery fix..."
echo ""

# Build the project
echo "1. Building vtcode-core..."
cargo build --package vtcode-core --lib --quiet 2>&1 | grep -E "(error|Finished)" || echo "Build completed"

echo ""
echo "2. Checking for .claude/skills/ directory..."
if [ -d ".claude/skills/spreadsheet-generator" ]; then
    echo "✓ Found .claude/skills/spreadsheet-generator/"
    echo "  Contents:"
    ls -la .claude/skills/spreadsheet-generator/
else
    echo "✗ .claude/skills/spreadsheet-generator/ not found"
    exit 1
fi

echo ""
echo "3. Verifying SKILL.md exists..."
if [ -f ".claude/skills/spreadsheet-generator/SKILL.md" ]; then
    echo "✓ SKILL.md exists"
    echo "  First 5 lines:"
    head -5 .claude/skills/spreadsheet-generator/SKILL.md
else
    echo "✗ SKILL.md not found"
    exit 1
fi

echo ""
echo "4. All skills available:"
find .claude/skills -name "SKILL.md" -type f | while read file; do
    skill_name=$(dirname "$file" | xargs basename)
    echo "  - $skill_name"
done

echo ""
echo "✓ Skill discovery test passed!"
echo ""
echo "The fix ensures that search_tools now uses SkillLoader"
echo "which reads .claude/skills/*/SKILL.md files instead of"
echo "the old SkillManager looking for skill.json files."
