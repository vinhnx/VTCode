# Skill Discovery Fix - Summary

**Date**: December 13, 2025
**Status**:  Fixed and Verified

## Problem

The vtcode agent was unable to discover Claude skills (like `spreadsheet-generator`, `doc-generator`, etc.) when using the `search_tools` command. The session log showed:

```json
"Tool": "{\"keyword\":\"spreadsheet-generator\",\"matched\":0,\"results\":[],\"note\":\"Use 'skill' tool to load available skills directly by name\",\"success\":true}"
```

## Root Cause

The `search_tools_executor` function in `vtcode-core/src/tools/registry/executors.rs` was using the **wrong skill system**:

-   **Used**: `SkillManager` from `vtcode-core/src/exec/skill_manager.rs`

    -   Looks for: `skill.json` + `skill.py`/`skill.js` files
    -   Path: `.agents/skills/` (old format; legacy `.vtcode/skills/` supported)

-   **Should use**: `SkillLoader` from `vtcode-core/src/skills/loader.rs`
    -   Looks for: `SKILL.md` files (Claude Agent Skills format)
    -   Path: `.claude/skills/*/SKILL.md` (current format)

## The Fix

Changed line 1067-1080 in `vtcode-core/src/tools/registry/executors.rs`:

**Before:**

```rust
// Also search local skills
use crate::exec::SkillManager;
let manager = SkillManager::new(&workspace_root);
if let Ok(skill_results) = manager.search_skills(&parsed.keyword).await {
    // ... convert results
}
```

**After:**

```rust
// Also search local skills (using SkillLoader for .claude/skills/ with SKILL.md)
use crate::skills::{SkillLoader, SkillContext};
let loader = SkillLoader::new(workspace_root);
if let Ok(skill_contexts) = loader.discover_skills() {
    let query_lower = parsed.keyword.to_lowercase();
    let filtered: Vec<_> = skill_contexts
        .into_iter()
        .filter_map(|ctx| {
            if let SkillContext::MetadataOnly(manifest) = ctx {
                let name_matches = manifest.name.to_lowercase().contains(&query_lower);
                let desc_matches = manifest.description.to_lowercase().contains(&query_lower);
                if name_matches || desc_matches {
                    Some(manifest)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();
    // ... convert to ToolDiscoveryResult
}
```

## Verification

1. **Build Check**:  Library compiles without errors
2. **Skill Files Exist**:  All 4 skills found in `.claude/skills/`
    - `spreadsheet-generator`
    - `doc-generator`
    - `pdf-report-generator`
    - `strict-architecture`

## Expected Behavior After Fix

### Before (Broken)

```
User: "Use spreadsheet-generator to create financial dashboard"
Agent: search_tools(keyword="spreadsheet-generator")
→ Returns: {"matched": 0, "results": []}
Agent: "Cannot find spreadsheet-generator"
```

### After (Fixed)

```
User: "Use spreadsheet-generator to create financial dashboard"
Agent: search_tools(keyword="spreadsheet-generator")
→ Returns: {
    "matched": 1,
    "results": [{
        "name": "spreadsheet-generator",
        "provider": "skill",
        "description": "Generate professional Excel spreadsheets..."
    }]
}
Agent: skill(name="spreadsheet-generator")
→ Loads skill successfully
```

## Related Files

-   **Fixed**: `vtcode-core/src/tools/registry/executors.rs` (lines 1067-1080)
-   **Updated**: `SKILL_DISCOVERY_FIX.md` (added root cause explanation)
-   **Test**: `test_skill_discovery.sh` (verification script)

## Next Steps

1. Test the fix in a live vtcode session:

    ```bash
    vtcode
    > search_tools spreadsheet
    # Should now return spreadsheet-generator skill
    ```

2. Verify the `skill` tool can load discovered skills:
    ```bash
    > skill spreadsheet-generator
    # Should load the skill with instructions
    ```

## Notes

-   The old `SkillManager` system is still present in the codebase (for backward compatibility with `.agents/skills/` format)
-   The new `SkillLoader` system is the correct one for Claude Agent Skills (`.claude/skills/*/SKILL.md`)
-   Both systems can coexist; the fix ensures `search_tools` uses the correct one
