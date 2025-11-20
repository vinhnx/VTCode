# Tool Policy Async Conversion - In Progress

## Date: October 24, 2025

## Summary

Converting `ToolPolicyManager` in `vtcode-core/src/tool_policy.rs` from blocking filesystem operations to fully async using `tokio::fs`.

## Status: Core Conversion Complete, Callers Need Updating

### Core File: `vtcode-core/src/tool_policy.rs`

#### Methods Converted to Async

**Constructor Methods:**
1. `new()` - Now async
2. `new_with_workspace()` - Now async
3. `new_with_config_path()` - Now async

**Internal Helper Methods:**
4. `get_config_path()` - Now async
5. `get_workspace_config_path()` - Now async
6. `load_or_create_config()` - Now async
7. `write_config()` - Now async
8. `reset_to_default()` - Now async
9. `save_config()` - Now async

**Public API Methods:**
10. `apply_tools_config()` - Now async
11. `update_available_tools()` - Now async
12. `update_mcp_tools()` - Now async
13. `set_mcp_tool_policy()` - Now async
14. `set_mcp_allowlist()` - Now async
15. `set_policy()` - Now async
16. `reset_all_to_prompt()` - Now async
17. `allow_all_tools()` - Now async
18. `deny_all_tools()` - Now async
19. `should_execute_tool()` - Now async
20. `prompt_user_for_tool()` - Now async (private)

#### Filesystem Operations Converted

- `fs::create_dir_all()` → `tokio::fs::create_dir_all().await`
- `fs::read_to_string()` → `tokio::fs::read_to_string().await`
- `fs::write()` → `tokio::fs::write().await`
- `fs::rename()` → `tokio::fs::rename().await`
- `path.exists()` → `tokio::fs::try_exists().await.unwrap_or(false)`

#### Test Updates

1. `test_policy_updates` - `#[test]` → `#[tokio::test]`

### Callers That Need Updating

The following files have compilation errors and need their calls updated to use `.await`:

1. **`vtcode-core/src/cli/tool_policy_commands.rs`**
   - `ToolPolicyManager::new()` needs `.await`

2. **`vtcode-core/src/tools/registry/policy.rs`**
   - `ToolPolicyManager::new_with_workspace()` needs `.await`
   - `policy.update_available_tools()` needs `.await`
   - Multiple other method calls need `.await`

## Benefits (Once Complete)

1. **Non-blocking I/O**: Policy file operations won't block the async runtime
2. **Better Responsiveness**: UI remains responsive during policy updates
3. **Consistency**: All configuration operations follow async patterns
4. **Scalability**: Ready for concurrent policy operations

## Next Steps

1. Update `cli/tool_policy_commands.rs` to await async calls
2. Update `tools/registry/policy.rs` to await async calls
3. Search for other callers in the codebase
4. Run tests to verify functionality
5. Update documentation

## Completion Checklist

- [x] Core `tool_policy.rs` converted to async (20 methods)
- [x] Internal methods made async
- [x] Public API methods made async
- [x] Tests updated
- [x] CLI callers updated (`cli/tool_policy_commands.rs`)
- [x] Registry policy gateway updated (`tools/registry/policy.rs`)
- [x] Registry mod.rs updated (all methods made async)
- [x] Command file callers updated (`commands/analyze.rs`, `commands/create_project.rs`, `commands/validate.rs`)
- [x] Core agent files updated (`core/agent/bootstrap.rs`, `core/agent/runner.rs`, `core/agent/core.rs`)
- [x] ACP integration updated (`src/acp/zed.rs`)
- [x] All tests updated
- [x] Compilation successful ✓ 
- [ ] Full test suite passing (needs verification)
- [x] Documentation updated

## Status: ✓  COMPLETE - Library Compiles Successfully!

## Completed Work

### ✓  `vtcode-core/src/tool_policy.rs` - Fully Converted
- All 20 methods converted to async
- All filesystem operations using `tokio::fs`
- Tests updated

### ✓  `vtcode-core/src/cli/tool_policy_commands.rs` - Fully Updated
- All 6 command handlers updated to await async calls

### ✓  `vtcode-core/src/tools/registry/policy.rs` - Fully Updated
- All 10 methods converted to async
- `ToolPolicyGateway` fully async

### ✓  `vtcode-core/src/tools/registry/mod.rs` - Fully Updated
- All 6 constructor methods made async
- All 12 policy-related methods made async
- Test updated

## Remaining Work

### Callers of `ToolRegistry::new()` and related methods

The following files have compilation errors because they call `ToolRegistry::new()` without awaiting:

1. **`vtcode-core/src/commands/analyze.rs`** - 5 call sites
   - Lines 44, 73, 100, 128, 198
   - Need to await `ToolRegistry::new()`

2. **`vtcode-core/src/commands/create_project.rs`** - 5+ call sites
   - Lines 40, 78, 129, 175, 201
   - Need to await `ToolRegistry::new()`

3. **Other command files** - Unknown number
   - Need to search and update all callers

### Required Changes

All files that call these methods need to:
1. Make their functions `async` if not already
2. Add `.await` to `ToolRegistry::new()` calls
3. Add `.await` to policy-related method calls

Example:
```rust
// Before
let registry = ToolRegistry::new(workspace);
registry.allow_all_tools()?;

// After
let mut registry = ToolRegistry::new(workspace).await;
registry.allow_all_tools().await?;
```

## Impact

**Phase 2 Progress**: 1 of 7 files in progress (tool_policy.rs)
**Overall Progress**: 8 of 15 files (53% → targeting 60%)

---

**Started**: October 24, 2025  
**Status**: ⏳ In Progress - Core Complete, Callers Pending  
**Next**: Update caller sites
