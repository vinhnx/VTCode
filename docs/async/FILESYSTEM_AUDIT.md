# Filesystem Operations Audit

## Executive Summary

**Status**: ⚠️ **Additional async conversions recommended**

Found **15 files** with `std::fs` usage that should be reviewed for async conversion.

## Categories

### 1. ✅ Already Async (No Action Needed)
- `vtcode-core/src/tools/file_ops.rs` - Uses `tokio::fs::File::create` ✅
- `vtcode-core/src/tools/file_search.rs` - Only uses `std::fs::Metadata` type (not I/O) ✅

### 2. ⚠️ Test Files (Low Priority)
These use blocking I/O but only in tests:
- `vtcode-core/src/project_doc.rs` - Test functions only
- `vtcode-core/src/utils/vtcodegitignore.rs` - Test functions only
- `vtcode-core/src/instructions.rs` - Test functions only
- `vtcode-core/src/core/trajectory.rs` - Test functions only

**Recommendation**: Leave as-is (tests can use blocking I/O)

### 3. 🔴 High Priority (Should Convert)

#### A. Core Agent Intelligence
**File**: `vtcode-core/src/core/agent/intelligence.rs`

**Issues**:
```rust
// Line 196
let source_code = std::fs::read_to_string(file_path)?;

// Line 215
let source_code = std::fs::read_to_string(file_path)?;

// Line 299
if let Ok(source_code) = std::fs::read_to_string(path) {
```

**Impact**: High - Used in code analysis features
**Recommendation**: Convert to `tokio::fs::read_to_string().await`

#### B. Snapshot Management
**File**: `vtcode-core/src/core/agent/snapshots.rs`

**Issues**:
```rust
// Line 322
let mut file = fs::File::create(&path)?;
file.write_all(&data)?;
```

**Impact**: Medium - Used for checkpoint creation
**Recommendation**: Convert to `tokio::fs::File::create().await` and `AsyncWriteExt`

#### C. PTY Manager
**File**: `vtcode-core/src/tools/pty.rs`

**Issues**:
```rust
use std::fs;
// Used for session management
```

**Impact**: Medium - PTY session file operations
**Recommendation**: Review usage and convert to async

### 4. 🟡 Medium Priority (Configuration/Setup)

#### A. Tool Policy
**File**: `vtcode-core/src/tool_policy.rs`

**Issues**: Uses `std::fs` for policy file operations
**Impact**: Medium - Policy loading/saving
**Recommendation**: Convert to async

#### B. Prompt System
**Files**:
- `vtcode-core/src/prompts/system.rs`
- `vtcode-core/src/prompts/custom.rs`

**Issues**: Uses `std::fs` for prompt file loading
**Impact**: Medium - Prompt template loading
**Recommendation**: Convert to async

#### C. Configuration Files
**Files**:
- `vtcode-core/src/utils/dot_config.rs`
- `vtcode-core/src/cli/args.rs`

**Issues**: Uses `std::fs` for config file operations
**Impact**: Medium - Configuration loading
**Recommendation**: Convert to async

### 5. 🟢 Low Priority (Utilities/One-time Operations)

#### A. Project Documentation
**File**: `vtcode-core/src/project_doc.rs`

**Issues**: Uses `std::fs::metadata` for git root detection
**Impact**: Low - One-time setup operation
**Recommendation**: Can convert but not urgent

#### B. Utilities
**Files**:
- `vtcode-core/src/utils/utils.rs`
- `vtcode-core/src/utils/session_archive.rs`
- `vtcode-core/src/execpolicy/mod.rs`

**Issues**: Various `std::fs` usage
**Impact**: Low - Utility functions
**Recommendation**: Review and convert if used in hot paths

#### C. CLI Commands
**Files**:
- `vtcode-core/src/cli/man_pages.rs`
- `vtcode-core/src/cli/mcp_commands.rs`

**Issues**: Uses `std::fs` for file generation
**Impact**: Low - CLI operations (not in hot path)
**Recommendation**: Can remain blocking

#### D. Code Quality Metrics
**Files**:
- `vtcode-core/src/code/code_quality/metrics/coverage.rs`
- `vtcode-core/src/code/code_quality/metrics/complexity.rs`

**Issues**: Uses `std::fs` for file reading
**Impact**: Low - Analysis features
**Recommendation**: Convert if used frequently

#### E. Instructions System
**File**: `vtcode-core/src/instructions.rs`

**Issues**: Uses `std::fs` and `File::open`
**Impact**: Medium - Instruction file loading
**Recommendation**: Convert to async

#### F. Prompt Caching
**File**: `vtcode-core/src/core/prompt_caching.rs`

**Issues**: Uses `std::fs` for cache operations
**Impact**: Medium - Cache I/O
**Recommendation**: Convert to async

## Detailed Recommendations

### Phase 1: High Priority (Immediate) - 3 files

1. **`core/agent/intelligence.rs`** ⚠️ CRITICAL
   - Convert 3 `std::fs::read_to_string` calls
   - Make methods async
   - Impact: Code analysis features

2. **`core/agent/snapshots.rs`** ⚠️ HIGH
   - Convert `fs::File::create` to async
   - Use `tokio::io::AsyncWriteExt`
   - Impact: Checkpoint creation

3. **`tools/pty.rs`** ⚠️ HIGH
   - Review `std::fs` usage
   - Convert session file operations
   - Impact: PTY session management

### Phase 2: Medium Priority (Next Week) - 7 files

4. **`tool_policy.rs`** - Policy file I/O
5. **`prompts/system.rs`** - System prompt loading
6. **`prompts/custom.rs`** - Custom prompt loading
7. **`utils/dot_config.rs`** - Config file operations
8. **`instructions.rs`** - Instruction file loading
9. **`core/prompt_caching.rs`** - Cache I/O
10. **`cli/args.rs`** - Config loading

### Phase 3: Low Priority (Optional) - 5 files

11. **`project_doc.rs`** - Git root detection
12. **`utils/utils.rs`** - Utility functions
13. **`utils/session_archive.rs`** - Archive operations
14. **`code/code_quality/metrics/*.rs`** - Metrics calculation
15. **CLI tools** - Man pages, MCP commands

## Implementation Plan

### Week 1: Critical Files
```rust
// Example: intelligence.rs
// Before
let source_code = std::fs::read_to_string(file_path)?;

// After
let source_code = tokio::fs::read_to_string(file_path).await?;

// Update method signatures
pub async fn analyze_file(&mut self, file_path: &Path) -> Result<Analysis>
```

### Week 2: Configuration & Prompts
```rust
// Example: tool_policy.rs
// Before
let content = std::fs::read_to_string(path)?;

// After
let content = tokio::fs::read_to_string(path).await?;
```

### Week 3: Utilities & Optional
- Review usage patterns
- Convert if in hot paths
- Leave CLI tools as blocking (acceptable)

## Testing Strategy

### For Each Converted File
1. Run unit tests: `cargo test --lib`
2. Check compilation: `cargo check`
3. Run clippy: `cargo clippy`
4. Integration testing

### Validation
- Ensure no blocking operations in async context
- Verify Send trait requirements
- Check for performance improvements

## Risk Assessment

### High Priority Files
- **Risk**: Medium - Core functionality
- **Mitigation**: Thorough testing, gradual rollout
- **Benefit**: Significant - Better responsiveness

### Medium Priority Files
- **Risk**: Low - Configuration loading
- **Mitigation**: Standard testing
- **Benefit**: Consistency, better architecture

### Low Priority Files
- **Risk**: Very Low - Utility functions
- **Mitigation**: Optional conversion
- **Benefit**: Marginal - Completeness

## Estimated Effort

| Phase | Files | Effort | Priority |
|-------|-------|--------|----------|
| Phase 1 | 3 files | 4-6 hours | High |
| Phase 2 | 7 files | 8-10 hours | Medium |
| Phase 3 | 5 files | 4-6 hours | Low |
| **Total** | **15 files** | **16-22 hours** | - |

## Current Status

### Completed ✅
- Tool implementations (5 files)
- File operations core
- Search and grep tools
- HTTP operations

### In Progress 🔄
- None currently

### Pending ⏳
- 15 files identified above

## Recommendations

### Immediate Action
1. Convert `core/agent/intelligence.rs` (CRITICAL)
2. Convert `core/agent/snapshots.rs` (HIGH)
3. Review `tools/pty.rs` (HIGH)

### Next Steps
1. Create issues for Phase 1 files
2. Assign priorities
3. Begin conversion with testing
4. Document changes

### Long Term
- Complete Phase 2 within 2 weeks
- Evaluate Phase 3 based on usage patterns
- Consider leaving CLI tools as blocking

## Conclusion

While the core tool execution is now 100% async, there are additional files that would benefit from async conversion, particularly in the agent intelligence and snapshot management areas. The high-priority files should be converted soon, while medium and low priority files can be addressed gradually.

**Next Action**: Start with `core/agent/intelligence.rs` as it's used in code analysis features and would benefit most from async I/O.
