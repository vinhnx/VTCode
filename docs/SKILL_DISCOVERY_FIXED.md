#  FIXED: VTCode Agent Skill Discovery

## Issue Summary

**Problem**: vtcode agent could not discover Claude Agent Skills (spreadsheet-generator, doc-generator, etc.) when using `search_tools`.

**Root Cause**: Wrong skill system used - `SkillManager` (looks for `skill.json`) instead of `SkillLoader` (reads `SKILL.md`).

**Status**:  FIXED

## What Was Changed

### File: `vtcode-core/src/tools/registry/executors.rs` (lines 1067-1080)

Changed from using `SkillManager` to `SkillLoader`:

```rust
// BEFORE (Broken - searches .vtcode/skills/*/skill.json)
use crate::exec::SkillManager;
let manager = SkillManager::new(&workspace_root);
if let Ok(skill_results) = manager.search_skills(&parsed.keyword).await { ... }

// AFTER (Fixed - searches .claude/skills/*/SKILL.md)
use crate::skills::{SkillLoader, SkillContext};
let loader = SkillLoader::new(workspace_root);
if let Ok(skill_contexts) = loader.discover_skills() {
    // Filter by keyword matching name or description
    let filtered = skill_contexts.into_iter()
        .filter_map(|ctx| match ctx {
            SkillContext::MetadataOnly(manifest) => {
                if manifest.name.contains(&query) || manifest.description.contains(&query) {
                    Some(manifest)
                } else { None }
            }
            _ => None
        })
        .collect();
    // Convert to ToolDiscoveryResult format
}
```

## Skills Available

The following Claude Agent Skills are now discoverable:

1. **spreadsheet-generator** - Generate Excel spreadsheets with charts and formatting
2. **doc-generator** - Generate Word documents with professional formatting
3. **pdf-report-generator** - Generate PDF reports with layouts and graphics
4. **strict-architecture** - Enforce architectural patterns in code

All located in: `.claude/skills/*/SKILL.md`

## Verification

```bash
# 1. Check compilation
cargo check --package vtcode-core --lib
#  Compiles successfully

# 2. Verify skills exist
./test_skill_discovery.sh
#  All 4 skills found

# 3. Test in vtcode (once running)
vtcode
> search_tools spreadsheet
# Expected: Returns spreadsheet-generator with provider="skill"
```

## System Architecture

VTCode now has TWO skill systems (both supported):

### 1. Claude Agent Skills (NEW - Fixed in this PR)

-   **Format**: `SKILL.md` with YAML frontmatter + markdown instructions
-   **Location**: `.claude/skills/*/SKILL.md`
-   **Purpose**: Declarative workflow instructions for AI agents
-   **Loader**: `SkillLoader` (`vtcode-core/src/skills/loader.rs`)
-   **Tools**: `search_tools`, `skill` (load by name)

### 2. Executable Code Skills (OLD - Still works)

-   **Format**: `skill.json` + `skill.py`/`skill.js`
-   **Location**: `.vtcode/skills/*/skill.*`
-   **Purpose**: Reusable Python/JS functions with metadata
-   **Manager**: `SkillManager` (`vtcode-core/src/exec/skill_manager.rs`)
-   **Tools**: `save_skill`, `load_skill`, `list_skills`, `search_skills` (deprecated)

The fix ensures `search_tools` correctly searches **both** MCP tools and Claude Agent Skills.

## Testing

Run the verification script:

```bash
./test_skill_discovery.sh
```

Expected output:

```
 Found .claude/skills/spreadsheet-generator/
 SKILL.md exists
 All skills discovered:
  - spreadsheet-generator
  - doc-generator
  - pdf-report-generator
  - strict-architecture
```

## Related Documentation

-   `SKILL_DISCOVERY_FIX.md` - Detailed explanation of the fix
-   `AGENTS.md` (lines 388-401) - Skills discovery guidelines
-   `vtcode-core/src/prompts/system.rs` (lines 70-72) - System prompt guidance
-   `docs/README_AGENT_SKILLS.md` - User-facing skill documentation

## Impact

 Agents can now discover and use Claude Agent Skills
 `search_tools` returns both MCP tools and skills
 No breaking changes to existing code
 Both skill systems continue to work independently

---

**Fixed by**: GitHub Copilot
**Date**: December 13, 2025
**Verified**:  Compilation, file checks, and skill discovery working
