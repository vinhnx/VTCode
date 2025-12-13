# Skill Discovery & Agent Integration Fix

## Problem

The vtcode agent was unable to discover and use skills (like `spreadsheet-generator`) because:

1. **`search_tools` only searched MCP tools**, not local skills
2. **Agent loops prevented retries** - when search failed, it would retry the same search repeatedly
3. **No clear guidance** in AGENTS.md about how to use the `skill` tool
4. **Skills not registered as tools** - the skill executors existed but weren't integrated

## Root Cause

-   `search_tools_executor` in `vtcode-core/src/tools/registry/executors.rs` only called `ToolDiscovery::search_tools()`, which searched MCP client tools exclusively
-   When MCP client was unavailable or skills didn't match, the search returned empty results
-   Agent kept retrying `search_tools` instead of using the `skill` tool directly
-   **CRITICAL BUG**: Used `SkillManager` (looking for `skill.json` + `skill.py`/`skill.js`) instead of `SkillLoader` (which reads `.claude/skills/*/SKILL.md`)

## Solution

### 1. Enhanced `search_tools` Executor (executors.rs)

Modified `search_tools_executor` to:

-   **Gracefully handle missing MCP client** (no longer requires MCP client to be configured)
-   **Search both MCP tools AND local skills** in `.claude/skills/` directory (FIXED: now uses `SkillLoader` instead of `SkillManager`)
-   **Return unified results** with provider metadata ("skill" for local skills, etc.)
-   **Add helpful note** to guide agents toward the `skill` tool

```rust
// Now searches:
// 1. MCP tools (if client available)
// 2. Local skills via SkillLoader (reads .claude/skills/*/SKILL.md)
// 3. Returns combined results with provider metadata
```

**Key changes:**

-   MCP client is now optional (wrapped in `if let Some(mcp_client)`)
-   **FIXED**: Changed from `SkillManager::search_skills()` to `SkillLoader::discover_skills()` to properly read `.claude/skills/*/SKILL.md` files
-   Skills are filtered by keyword (name or description match)
-   Skills are converted to `ToolDiscoveryResult` format for consistent response
-   Response includes helpful hint about using `skill` tool directly

### 2. Updated AGENTS.md Guidelines

Added clear documentation:

```markdown
**Skills Discovery & Loading**:

-   To search for available skills: `search_tools(keyword="spreadsheet")` returns both MCP tools and local skills
-   To load a skill directly: `skill(name="spreadsheet-generator")` - does NOT require search first
-   Skills provide pre-built solutions (doc-generator, spreadsheet-generator, pdf-report-generator, etc.)
-   Once loaded, skills inject instructions and bundled resources for agent context
-   **IMPORTANT**: When user requests a skill by name, use `skill(name="...")` directly instead of repeated search_tools calls
```

This prevents infinite loops by guiding agents to use the correct tool.

## Files Modified

1. **vtcode-core/src/tools/registry/executors.rs**

    - Lines 1030-1099: Enhanced `search_tools_executor` function
    - Now searches both MCP tools and local skills
    - Made MCP client optional

2. **vtcode-core/src/prompts/system.rs**

    - Lines 70-72: Updated system prompt with skill guidance
    - Added `skill` to tool picker list
    - Added dedicated "Skills" section explaining discovery and usage
    - Emphasizes using `skill(name="...")` directly instead of repeated search_tools

3. **AGENTS.md** (Updated)
    - Lines 388-401: Added skills discovery & loading section
    - Clarified when to use `search_tools` vs `skill`
    - Emphasized importance of using `skill` tool directly for known skill names

## Behavior Changes

### Before

```
User: "Use spreadsheet-generator to create financial dashboard"
Agent: search_tools(keyword="spreadsheet-generator")
→ Returns empty (no MCP tools match, skills not searched)
Agent: [retries same search repeatedly]
→ LOOP DETECTION: Tool blocked due to identical repeated calls
Agent: "Cannot find spreadsheet-generator"
```

### After (with enhanced system prompt guidance)

```
User: "Use spreadsheet-generator to create financial dashboard"
Agent reads system prompt: "Use skill(name="...") directly for known names"
→ skill(name="spreadsheet-generator")
→ Loads skill successfully with instructions + resources
→ Executes skill with parameters

Alternative flow with search:
User: "What financial tools are available?"
Agent: search_tools(keyword="financial")
→ Returns both MCP tools AND local skills with provider="skill"
→ Identifies spreadsheet-generator skill
→ skill(name="spreadsheet-generator") to load
```

## Testing

```bash
# Build check passes
cargo check
# No compilation errors

# Verify the change
grep -n "search_skills\|SkillManager" vtcode-core/src/tools/registry/executors.rs
```

## Impact

1. **Loop Prevention**: Eliminates infinite retry loops when skills are searched
2. **Skill Discovery**: `search_tools` now properly discovers local skills
3. **System Prompt Guidance**: Enhanced default system prompt educates agents on proper skill usage
4. **Better UX**: Guidance prevents agents from misusing tools and getting stuck
5. **Graceful Degradation**: Works even if MCP client is unavailable
6. **Unified Results**: Skills and tools returned in same format with provider metadata

## Related

-   `.claude/skills/` - Where Anthropic skills are stored
-   `.vtcode/skills/` - Where vtcode local skills are stored
-   `vtcode-core/src/exec/skill_manager.rs` - Skill persistence & management
-   `vtcode-core/src/tools/registry/executors.rs` - Tool executor implementations
