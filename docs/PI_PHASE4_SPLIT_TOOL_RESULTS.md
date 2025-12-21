# Phase 4: Split Tool Results (LLM vs UI Content)

**Status**: Design Phase
**Expected Impact**: 20-30% additional savings on tool-heavy sessions
**Complexity**: High (architectural change)
**Priority**: High

---

## Problem Statement

### Current Situation

Tool results are sent **in full** to both the LLM and the UI:

```rust
// Current flow
tool_result = execute_tool(tool_call);
// Full result sent to BOTH:
context.push(tool_result);  // → LLM sees everything
ui.display(tool_result);    // → User sees same thing
```

**Issues**:
1. **Token waste**: LLM doesn't need ANSI codes, formatting, verbose output
2. **Context bloat**: Large tool outputs consume precious context
3. **Cost inefficiency**: Paying for tokens the model doesn't use
4. **Missed opportunity**: UI could show richer output

### Example: grep_file Result

**Current (sent to both LLM and UI)**:
```
Found 127 matches in 15 files:

src/tools/grep.rs:45:    pub fn execute_grep(pattern: &str) -> Result<String> {
src/tools/grep.rs:67:        let matches = grep_impl(pattern)?;
src/tools/grep.rs:89:    fn grep_impl(pattern: &str) -> Result<Vec<Match>> {
src/tools/list.rs:23:    // Uses grep internally for filtering
...
[112 more lines]
```

**Total tokens**: ~2,500

**What LLM actually needs**:
```
grep_file: Found 127 matches in 15 files
Key locations: src/tools/grep.rs (3 matches), src/tools/list.rs (1 match)
Pattern appears in: execute_grep(), grep_impl() functions
Summary: Pattern widely used across tool implementations
```

**Tokens**: ~50 (95% reduction!)

**What UI should show**:
```
[Rich formatted output with syntax highlighting]
[Clickable file paths]
[Expandable context]
[Full 127 matches with preview]
```

---

## Solution: Dual-Channel Tool Results

### Core Concept

Tools return **two outputs**:
1. **LLM Content** - Concise summary for model context
2. **UI Content** - Rich output for user display

```rust
pub struct ToolResult {
    /// Concise summary for LLM (optimized tokens)
    pub llm_content: String,

    /// Rich output for UI (full details)
    pub ui_content: String,

    /// Whether the tool succeeded
    pub success: bool,

    /// Optional metadata
    pub metadata: HashMap<String, Value>,
}
```

### Flow Diagram

```
Tool Execution
      ↓
Generate Dual Output
      ↓
   ┌──────┴──────┐
   ↓             ↓
LLM Content   UI Content
   ↓             ↓
Context      Display
```

---

## Design Details

### 1. ToolResult Structure

```rust
// vtcode-core/src/tools/result.rs

#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Tool name
    pub tool_name: String,

    /// Concise summary for LLM context (token-optimized)
    pub llm_content: String,

    /// Rich output for UI display (full details)
    pub ui_content: String,

    /// Success status
    pub success: bool,

    /// Error message if failed
    pub error: Option<String>,

    /// Structured metadata for both channels
    pub metadata: ToolMetadata,
}

#[derive(Debug, Clone, Default)]
pub struct ToolMetadata {
    /// File paths referenced (for UI linking)
    pub files: Vec<PathBuf>,

    /// Key-value pairs for structured data
    pub data: HashMap<String, Value>,

    /// Token counts
    pub llm_tokens: usize,
    pub ui_tokens: usize,
    pub savings_tokens: usize,  // ui_tokens - llm_tokens
}
```

### 2. Tool Trait Extension

```rust
// vtcode-core/src/tools/mod.rs

#[async_trait]
pub trait Tool {
    // Current method (deprecated)
    async fn execute(&self, args: Value) -> Result<String> {
        let result = self.execute_dual(args).await?;
        Ok(result.llm_content)  // Backward compatibility
    }

    // New dual-output method
    async fn execute_dual(&self, args: Value) -> Result<ToolResult>;
}
```

### 3. Summarization Strategies

Different tools need different summarization approaches:

#### **Strategy 1: Count-Based Summary** (grep_file, list_files)
```rust
fn summarize_search_results(matches: Vec<Match>) -> String {
    format!(
        "Found {} matches in {} files. Key files: {}. Pattern in: {}",
        matches.len(),
        count_unique_files(&matches),
        top_files(&matches, 3),
        top_functions(&matches, 3)
    )
}
```

**Before**: 2,500 tokens
**After**: ~50 tokens
**Savings**: 98%

#### **Strategy 2: Diff Summary** (edit_file, apply_patch)
```rust
fn summarize_file_change(old: &str, new: &str, path: &Path) -> String {
    let stats = diff_stats(old, new);
    format!(
        "Modified {}: +{} lines, -{} lines. Changes: {}",
        path.display(),
        stats.additions,
        stats.deletions,
        summarize_hunks(&stats.hunks, 2)
    )
}
```

**Before**: 1,200 tokens (full diff)
**After**: ~40 tokens
**Savings**: 97%

#### **Strategy 3: Execution Summary** (run_pty_cmd)
```rust
fn summarize_command_output(output: &CommandOutput) -> String {
    if output.success {
        format!(
            "Command succeeded. Output: {} lines. Exit code: 0. Key: {}",
            output.stdout.lines().count(),
            extract_key_info(&output.stdout)
        )
    } else {
        format!(
            "Command failed. Exit code: {}. Error: {}",
            output.exit_code,
            truncate(&output.stderr, 100)
        )
    }
}
```

**Before**: 800 tokens (full output)
**After**: ~30 tokens
**Savings**: 96%

#### **Strategy 4: Content Summary** (read_file)
```rust
fn summarize_file_content(content: &str, path: &Path) -> String {
    let lines = content.lines().count();
    let language = detect_language(path);
    let structure = analyze_structure(content, language);

    format!(
        "Read {}: {} lines, {} language. Contains: {}",
        path.display(),
        lines,
        language,
        structure  // e.g., "3 functions, 2 structs, 15 imports"
    )
}
```

**Before**: 3,000 tokens (full file)
**After**: ~40 tokens
**Savings**: 99%

---

## Implementation Plan

### Phase 4A: Infrastructure

1. **Create ToolResult struct** (`vtcode-core/src/tools/result.rs`)
   - Dual content fields
   - Metadata tracking
   - Token counting

2. **Update Tool trait** (`vtcode-core/src/tools/mod.rs`)
   - Add `execute_dual()` method
   - Keep `execute()` for compatibility

3. **Create summarizers** (`vtcode-core/src/tools/summarizers/`)
   - `mod.rs` - Strategy trait
   - `search.rs` - grep_file, list_files
   - `edit.rs` - edit_file, apply_patch
   - `command.rs` - run_pty_cmd
   - `read.rs` - read_file, create_file

4. **Update context management**
   - Separate LLM context from UI display
   - Track token savings per tool call

### Phase 4B: Tool Migration

Migrate tools in priority order (highest token usage first):

**Priority 1: High-volume tools** (Week 1)
- [ ] `grep_file` - Search results
- [ ] `read_file` - File content
- [ ] `list_files` - Directory listings

**Priority 2: Edit tools** (Week 2)
- [ ] `edit_file` - Diffs
- [ ] `apply_patch` - Patch application
- [ ] `create_file` - File creation

**Priority 3: Execution tools** (Week 3)
- [ ] `run_pty_cmd` - Command output
- [ ] `execute_code` - Code execution results

**Priority 4: Remaining tools** (Week 4)
- [ ] All other built-in tools

### Phase 4C: UI Integration

1. **Update TUI renderer** to use ui_content
2. **Add token savings display** in status bar
3. **Implement rich formatting** for UI content
   - Syntax highlighting
   - Clickable file paths
   - Expandable sections

4. **Add metrics dashboard**
   - Token savings per session
   - Cost reduction tracking
   - Tool usage analytics

### Phase 4D: Testing & Validation

1. **Unit tests** for each summarizer
2. **Integration tests** for dual-output flow
3. **Token counting validation**
4. **Quality assessment** - LLM still effective?

---

## Expected Savings

### Projected Token Reduction

Based on typical VT Code sessions:

| Tool | Current Avg | After Summary | Savings | Usage % |
|------|-------------|---------------|---------|---------|
| grep_file | 2,500 | 50 | **98%** | 30% |
| read_file | 3,000 | 40 | **99%** | 25% |
| list_files | 800 | 30 | **96%** | 15% |
| run_pty_cmd | 1,200 | 35 | **97%** | 12% |
| edit_file | 1,500 | 45 | **97%** | 10% |
| Others | 500 | 30 | **94%** | 8% |

**Weighted average**: **97% reduction on tool output tokens**

### Session-Level Impact

**Scenario**: 10-turn session with heavy tool usage
- 20 tool calls total
- Average 1,500 tokens per result (current)
- **Current total**: 30,000 tokens tool output
- **After split**: ~900 tokens LLM content
- **Savings**: 29,100 tokens (97%)

### Combined with Phases 1-3

| Phase | Component | Tokens Before | Tokens After | Reduction |
|-------|-----------|---------------|--------------|-----------|
| 1-2 | System prompt | 6,500 | 700 | 87% |
| 3 | Tool definitions | 3,000 | 800 | 73% |
| **4** | **Tool results** (per session) | **30,000** | **900** | **97%** |
| **Total** | **39,500** | **2,400** | **94%** |

**Cost impact**: ~$60/1M → ~$4/1M prompts (**93% cheaper**)

---

## Risks & Mitigations

### Risk 1: Information Loss

**Problem**: Summarization loses critical details for LLM decision-making

**Mitigation**:
1. **Conservative summarization** initially
2. **Metadata preservation** - key facts in structured form
3. **Quality testing** - run Terminal-Bench to detect regressions
4. **Fallback mode** - Config option to disable splitting

### Risk 2: Complexity

**Problem**: Maintaining two output formats per tool

**Mitigation**:
1. **Summarizer abstraction** - Reusable strategies
2. **Gradual migration** - One tool at a time
3. **Backward compatibility** - Old `execute()` still works
4. **Comprehensive tests** - Validate both outputs

### Risk 3: UI Rendering Burden

**Problem**: Rich UI content harder to render

**Mitigation**:
1. **Lazy rendering** - Expand sections on demand
2. **Streaming support** - Progressive display
3. **Caching** - Rendered output reuse

### Risk 4: Breaking Existing Sessions

**Problem**: Checkpoint/resume compatibility

**Mitigation**:
1. **Config flag** - `split_tool_results_enabled = false` (default)
2. **Opt-in rollout** - Users enable explicitly
3. **Version detection** - Handle old checkpoints gracefully

---

## Configuration

```toml
[agent]
# Enable split tool results (Phase 4)
split_tool_results_enabled = false  # Default: off (opt-in)

# Summarization aggressiveness
tool_summarization_level = "balanced"  # minimal, balanced, conservative

# UI rendering options
tool_ui_rich_formatting = true
tool_ui_syntax_highlighting = true
tool_ui_clickable_paths = true
```

---

## Metrics & Observability

### Track and Display

1. **Token savings per tool call**
   ```
   grep_file: 2,450 → 48 tokens (98% saved)
   ```

2. **Session total savings**
   ```
   Session: 29,100 tokens saved across 20 tool calls
   Cost savings: $0.43 this session
   ```

3. **Cumulative statistics**
   ```
   Today: 145K tokens saved
   This week: 892K tokens saved
   Cost reduction: $13.38/week
   ```

### Debug Logging

```bash
RUST_LOG=vtcode_core::tools::result=debug cargo run
```

**Output**:
```
DEBUG Tool result: grep_file
  LLM content: 48 tokens
  UI content: 2,450 tokens
  Savings: 2,402 tokens (98%)

DEBUG Session totals:
  Tool calls: 20
  Tokens saved: 29,100
  Average savings: 97%
```

---

## Success Criteria

Phase 4 is successful if:

1. ✅ **Token savings**: ≥90% reduction on tool output
2. ✅ **Quality maintained**: Terminal-Bench scores within 5% of baseline
3. ✅ **UI enhanced**: Richer display than before
4. ✅ **Zero regressions**: All existing tests passing
5. ✅ **Opt-in safe**: Default behavior unchanged
6. ✅ **Measurable ROI**: Clear cost savings demonstrated

---

## Timeline Estimate

**Phase 4A: Infrastructure** - 1 week
- ToolResult struct, trait updates, summarizers

**Phase 4B: Tool Migration** - 3 weeks
- Priority 1-4 tool implementations

**Phase 4C: UI Integration** - 1 week
- TUI updates, metrics display

**Phase 4D: Testing** - 1 week
- Validation, benchmarks, quality assessment

**Total**: **6 weeks** to full implementation

---

## Alternative Approaches

### Option 1: AI-Powered Summarization

Use a small model (Haiku, GPT-4 Mini) to summarize tool results.

**Pros**:
- Adaptive summarization
- Context-aware reduction

**Cons**:
- Extra API call per tool
- Latency increase
- Adds cost (offsets savings)

**Decision**: Not recommended. Rule-based summarization is fast, free, and deterministic.

### Option 2: User-Configurable Verbosity

Let users set token budgets per tool type.

**Pros**:
- User control
- Flexible optimization

**Cons**:
- Complex configuration
- Hard to tune correctly

**Decision**: Consider for future enhancement, start with smart defaults.

### Option 3: Streaming Context Window

Only keep recent tool results in context, stream older ones out.

**Pros**:
- Works with current tool outputs
- No tool changes needed

**Cons**:
- Loses historical context
- Doesn't save tokens (just moves them)

**Decision**: Complementary to Phase 4, not a replacement.

---

## Next Steps

### Immediate (This Week)

1. ✅ **Design complete** (this document)
2. [ ] **Create ToolResult struct** in vtcode-core
3. [ ] **Implement first summarizer** (grep_file)
4. [ ] **Prototype integration** with one tool

### Short-term (Next 2 Weeks)

1. [ ] **Migrate Priority 1 tools** (grep, read, list)
2. [ ] **Update context management**
3. [ ] **Basic UI integration**
4. [ ] **Initial metrics tracking**

### Medium-term (Month 2)

1. [ ] **Complete all tool migrations**
2. [ ] **Full UI enhancement**
3. [ ] **Comprehensive testing**
4. [ ] **Terminal-Bench validation**

### Long-term (Month 3+)

1. [ ] **User feedback collection**
2. [ ] **Refinement based on usage**
3. [ ] **Cost savings case studies**
4. [ ] **Phase 5: Advanced features**

---

## References

- **Pi article**: https://mariozechner.at/posts/2025-11-30-pi-coding-agent/
- **Context efficiency**: Core pi-coding-agent philosophy
- **Phase 1-2 summary**: `IMPLEMENTATION_COMPLETE.md`
- **Phase 3 summary**: `docs/PI_PHASE3_COMPLETE.md`

---

## Summary

**What**: Split tool results into LLM content (concise) and UI content (rich)
**Why**: Tool outputs waste ~30K tokens/session that LLM doesn't need
**How**: Dual-output ToolResult struct with smart summarization strategies
**Impact**: Up to 97% reduction on tool output tokens, 20-30% session savings
**Status**: Design complete, ready for implementation

**Combined Phases 1-4 Impact**: From 39.5K → 2.4K tokens/session (**94% reduction**)

---

**Next**: Implement ToolResult infrastructure and migrate first tool (grep_file)

🚀 **Phase 4: Make every token count.** 🚀
