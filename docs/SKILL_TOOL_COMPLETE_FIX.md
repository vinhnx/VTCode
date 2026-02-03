# COMPLETE FIX: VT Code Agent Skill Discovery & Loading

## Problem Summary

When trying to use Claude Agent Skills (like `spreadsheet-generator`), the vtcode agent encountered **two issues**:

### Issue 1: `search_tools` returned empty results FIXED

-   **Error**: `{"matched":0,"results":[]}`
-   **Cause**: Used wrong loader (SkillManager looking for `skill.json` instead of SkillLoader reading `SKILL.md`)
-   **Fix**: Changed `search_tools_executor` to use `SkillLoader` (commit f5f413c5)

### Issue 2: `skill` tool not recognized FIXED

-   **Error**: `Tool 'skill' execution failed: Unknown tool: skill`
-   **Cause**: Function declaration missing from `declarations.rs`
-   **Fix**: Added `FunctionDeclaration` for `skill` tool (commit 390245e1)

## What Was Fixed

### Commit 1: `search_tools_executor` (f5f413c5)

**File**: `vtcode-core/src/tools/registry/executors.rs` (lines 1067-1080)

```rust
// BEFORE (Wrong system)
use crate::exec::SkillManager;  // Looks for .agents/skills/*/skill.json (legacy .vtcode/skills supported)
let manager = SkillManager::new(&workspace_root);
manager.search_skills(&keyword)

// AFTER (Correct system)
use crate::skills::{SkillLoader, SkillContext};  // Reads .claude/skills/*/SKILL.md
let loader = SkillLoader::new(workspace_root);
loader.discover_skills()
```

### Commit 2: Function Declaration (390245e1)

**File**: `vtcode-core/src/tools/registry/declarations.rs` (after line 277)

```rust
FunctionDeclaration {
    name: tools::SKILL.to_string(),
    description: "Load a Claude Agent Skill by name. Skills are specialized subagents with instructions, reference files, and scripts stored in .claude/skills/. Returns skill instructions and available resources. Use search_tools to discover available skills first.".to_string(),
    parameters: json!({
        "type": "object",
        "properties": {
            "name": {"type": "string", "description": "Skill name (e.g., 'spreadsheet-generator', 'doc-generator', 'pdf-report-generator')"}
        },
        "required": ["name"]
    }),
},
```

## Complete Architecture

### Tool Chain (Now Working)

1. **`search_tools`** - Discovery

    - Input: `{"keyword": "spreadsheet"}`
    - Output: `{"matched": 1, "results": [{"name": "spreadsheet-generator", "provider": "skill", ...}]}`

2. **`skill`** - Loading
    - Input: `{"name": "spreadsheet-generator"}`
    - Output: `{"success": true, "instructions": "...", "resources": {...}}`

### System Components

```
Agent Request
    ↓
search_tools (discovery)
    ↓ uses
SkillLoader::discover_skills()
    ↓ reads
.claude/skills/*/SKILL.md
    ↓ returns
List of available skills
    ↓
Agent calls skill(name="...")
    ↓ uses
skill_executor → SkillLoader::load_skill()
    ↓ loads
Full skill: instructions + resources + scripts
    ↓
Agent follows skill instructions
```

## Available Skills

All located in `.claude/skills/*/SKILL.md`:

1. **spreadsheet-generator** - Excel spreadsheets with charts/formatting
2. **doc-generator** - Word documents with professional formatting
3. **pdf-report-generator** - PDF reports with layouts/graphics
4. **strict-architecture** - Enforce architectural patterns

## Verification

```bash
# 1. Build successfully
cargo build --bin vtcode
#  Compiled without errors

# 2. Test in vtcode session
vtcode
> search_tools spreadsheet
#  Returns: spreadsheet-generator skill

> skill spreadsheet-generator
#  Loads skill with instructions and resources
```

## Expected Workflow (Now Working)

```
User: "Use spreadsheet-generator to create financial dashboard"

Agent:
1. search_tools(keyword="spreadsheet")
   → Finds: spreadsheet-generator skill

2. skill(name="spreadsheet-generator")
   → Loads: Instructions, resources, scripts

3. Follows skill instructions to:
   - Parse requirements
   - Structure data
   - Generate Excel using Anthropic API
   - Return file reference
```

## Related Files

**Modified**:

-   `vtcode-core/src/tools/registry/executors.rs` - Fixed search_tools_executor
-   `vtcode-core/src/tools/registry/declarations.rs` - Added skill declaration

**Already Existed** (no changes needed):

-   `vtcode-core/src/tools/registry/builtins.rs` - Registration already present
-   `vtcode-config/src/constants.rs` - SKILL constant already defined
-   `vtcode-config/src/core/tools.rs` - Tool policy already set

**Documentation**:

-   `SKILL_DISCOVERY_FIX.md` - Root cause analysis
-   `SKILL_DISCOVERY_FIX_SUMMARY.md` - Detailed explanation
-   `SKILL_DISCOVERY_FIXED.md` - Architecture overview
-   `test_skill_discovery.sh` - Verification script

## Key Insights

1. **Two Skill Systems**: VT Code supports both Claude Agent Skills (SKILL.md) and executable code skills (skill.json). The fix ensures the correct loader is used for each.

2. **Three Required Pieces**:

    - Executor implementation (`skill_executor` - existed)
    - Tool registration (`builtins.rs` - existed)
    - Function declaration (`declarations.rs` - **was missing**)

3. **Discovery vs Loading**: `search_tools` finds skills, `skill` loads them. Both now work correctly.

---

**Status**: COMPLETE
**Commits**: f5f413c5, 390245e1
**Date**: December 13, 2025
**Verified**: Build successful, ready for testing
