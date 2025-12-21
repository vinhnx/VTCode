# Phase 3: Progressive Tool Documentation Loading

**Status**: Design Complete, Implementation Pending
**Estimated Savings**: 2,000-3,000 tokens per request
**Complexity**: Medium-High (architectural change)

## Problem Statement

### Current Situation
VT Code loads **all 22+ tool descriptions upfront**, sending them in every LLM request:

```rust
// Example from grep_file tool (lines 191-221):
FunctionDeclaration {
    name: "grep_file",
    description: "Fast regex-based code search using ripgrep... [300+ characters]",
    parameters: {
        "pattern": "Regex pattern or literal string... [detailed desc]",
        "path": "Directory path... [detailed desc]",
        "max_results": "Maximum number of results... [detailed desc]",
        // ... 15+ more parameters, each with detailed descriptions
    }
}
```

### Token Overhead Analysis

| Tool | Description Tokens | Parameters Tokens | Total |
|------|-------------------|-------------------|-------|
| grep_file | ~75 | ~150 | ~225 |
| list_files | ~30 | ~120 | ~150 |
| run_pty_cmd | ~50 | ~100 | ~150 |
| read_file | ~20 | ~80 | ~100 |
| create_file | ~20 | ~60 | ~80 |
| edit_file | ~30 | ~100 | ~130 |
| ... (16 more tools) | ... | ... | ... |

**Total current overhead**: ~2,500-3,500 tokens for tool definitions alone

### Pi's Approach

Pi-coding-agent uses **4 simple tools** with minimal descriptions:

```
read    - Read file contents
write   - Write content to a file
edit    - Edit file by replacing exact text
bash    - Execute a bash command
```

**Total**: ~150 tokens

### The Gap

VT Code: **2,500-3,500 tokens**
Pi: **~150 tokens**
**Gap**: **16-23x more overhead**

## Solution: Progressive Disclosure

### Core Concept

**Load tool documentation progressively**:

1. **Initial request**: Send minimal tool signatures only
2. **On tool use**: LLM sees brief signature, uses tool
3. **On error**: System provides detailed docs for that specific tool
4. **On request**: Agent can explicitly request full docs via `search_tools`

### Three-Tier Documentation Model

#### Tier 1: Minimal Signature (Always Loaded)
```rust
FunctionDeclaration {
    name: "grep_file",
    description: "Search code with regex (ripgrep)",
    parameters: {
        "pattern": {"type": "string", "description": "Search pattern"},
        "path": {"type": "string", "description": "Directory", "default": "."},
        // Only required params shown
    }
}
```
**Tokens**: ~40 (reduction from ~225)

#### Tier 2: Standard Documentation (On Demand)
```rust
// Loaded when tool is first used or on error
description: "Fast regex-based code search. Supports glob patterns,
file-type filtering, context lines. Respects .gitignore by default."
```
**Tokens**: ~80 (medium detail)

#### Tier 3: Full Documentation (Explicit Request)
```rust
// Loaded via search_tools or help command
description: "Fast regex-based code search using ripgrep (replaces ast-grep).
Find patterns, functions, definitions, TODOs, errors, imports, and API calls
across files. Respects .gitignore/.ignore by default. Supports glob patterns,
file-type filtering, context lines, and regex/literal matching. Essential for
code navigation and analysis. Note: pattern is required; use literal: true for
exact string matching. Invalid regex patterns will be rejected with helpful
error messages."
// + All 18 parameters with detailed descriptions
```
**Tokens**: ~225 (current)

## Implementation Design

### 1. Tool Documentation Struct

```rust
// vtcode-core/src/tools/registry/documentation.rs

pub struct ToolDocumentation {
    /// Tool name
    pub name: &'static str,

    /// Minimal signature (Tier 1) - always sent to LLM
    pub signature: ToolSignature,

    /// Standard docs (Tier 2) - cached, sent on first use
    pub standard: OnceCell<ToolDocs>,

    /// Full docs (Tier 3) - loaded on explicit request
    pub full: OnceCell<ToolDocs>,
}

pub struct ToolSignature {
    /// Brief one-line description (15-30 chars)
    pub brief: &'static str,

    /// Required parameters only
    pub required_params: Vec<ParamSignature>,

    /// Estimated token count (~30-50)
    pub token_estimate: u32,
}

pub struct ParamSignature {
    pub name: &'static str,
    pub type_hint: &'static str,
    pub brief: &'static str, // 5-10 chars max
}

pub struct ToolDocs {
    pub description: String,
    pub parameters: HashMap<String, ParamDocs>,
    pub examples: Vec<String>,
    pub token_count: usize,
}
```

### 2. Loading Strategy

```rust
// vtcode-core/src/tools/registry/progressive_loader.rs

pub enum DocumentationMode {
    /// Minimal - only signatures (pi-style)
    Minimal,

    /// Progressive - signatures upfront, details on demand
    Progressive,

    /// Full - current behavior (all docs upfront)
    Full,
}

pub struct ProgressiveToolLoader {
    mode: DocumentationMode,
    signature_cache: HashMap<String, ToolSignature>,
    docs_cache: HashMap<String, ToolDocs>,
}

impl ProgressiveToolLoader {
    pub fn build_declarations(&self, mode: DocumentationMode) -> Vec<FunctionDeclaration> {
        match mode {
            DocumentationMode::Minimal => self.build_minimal_declarations(),
            DocumentationMode::Progressive => self.build_progressive_declarations(),
            DocumentationMode::Full => self.build_full_declarations(), // current
        }
    }

    fn build_minimal_declarations(&self) -> Vec<FunctionDeclaration> {
        // Only signatures, ~40 tokens per tool
        // Total: ~800-1,000 tokens for 22 tools
    }

    fn build_progressive_declarations(&self) -> Vec<FunctionDeclaration> {
        // Signatures + usage hints
        // Total: ~1,200-1,500 tokens
    }

    pub fn get_tool_docs(&mut self, tool_name: &str, tier: DocumentationTier) -> Option<&ToolDocs> {
        // Lazy load documentation tier on demand
    }
}
```

### 3. Error-Driven Documentation

```rust
// When a tool call fails due to missing/invalid parameters:

if tool_error.is_parameter_error() {
    // Inject detailed parameter docs for THIS tool only
    let detailed_params = loader.get_tool_docs(tool_name, DocumentationTier::Full)?;

    let error_with_docs = format!(
        "Tool '{}' failed: {}\n\nDetailed parameter documentation:\n{}",
        tool_name,
        error_message,
        detailed_params.format_for_llm()
    );

    // LLM sees error + detailed docs, tries again with correct params
}
```

### 4. Configuration

```toml
[agent]
system_prompt_mode = "minimal"
tool_documentation_mode = "progressive"  # or "minimal", "full"
```

### 5. Integration with System Prompt Modes

| System Prompt Mode | Tool Doc Mode | Combined Savings |
|-------------------|---------------|------------------|
| Minimal | Minimal | ~7,500 tokens (87% + 70%) |
| Minimal | Progressive | ~7,000 tokens (87% + 50%) |
| Default | Progressive | ~2,500 tokens (0% + 70%) |
| Default | Full | 0 tokens (current) |

## Token Savings Breakdown

### Minimal Tool Docs Mode
```
Current:    ~3,000 tokens (all tools, full docs)
Minimal:      ~800 tokens (signatures only)
Savings:    ~2,200 tokens (73% reduction)
```

### Progressive Tool Docs Mode
```
Current:    ~3,000 tokens (all tools, full docs)
Progressive: ~1,200 tokens (signatures + smart hints)
Savings:    ~1,800 tokens (60% reduction)
```

### Combined with Minimal System Prompt
```
Current total:  ~10,300 tokens (6,500 prompt + 3,000 tools + 800 config)
Minimal combo:   ~2,300 tokens (700 prompt + 800 tools + 800 config)
Savings:        ~8,000 tokens (78% total reduction!)
```

## Implementation Phases

### Phase 3A: Signature Extraction ✅ (Current)
- [x] Audit current tool documentation overhead
- [x] Design three-tier model
- [ ] Extract minimal signatures for all 22 tools
- [ ] Create ToolSignature struct
- [ ] Implement signature builder

### Phase 3B: Progressive Loader
- [ ] Create ProgressiveToolLoader
- [ ] Implement DocumentationMode enum
- [ ] Add configuration field to AgentConfig
- [ ] Wire up mode selection

### Phase 3C: Error-Driven Loading
- [ ] Detect parameter errors
- [ ] Inject tier-2 docs on first error
- [ ] Cache loaded docs per session
- [ ] Measure effectiveness

### Phase 3D: Testing & Validation
- [ ] Unit tests for each tier
- [ ] Integration tests with real LLM calls
- [ ] Measure token savings
- [ ] Benchmark task completion rates

### Phase 3E: Documentation
- [ ] User guide for tool doc modes
- [ ] Migration guide
- [ ] Performance comparisons

## Expected Impact

### Token Savings
- **Minimal mode**: 73% reduction (2,200 tokens)
- **Progressive mode**: 60% reduction (1,800 tokens)
- **Combined with minimal prompt**: 78% total reduction (8,000 tokens)

### Performance
- **Faster requests**: Less input to process
- **Lower costs**: Fewer prompt tokens
- **Same capability**: Error-driven loading ensures context when needed

### User Experience
- **Transparent**: Works automatically
- **Configurable**: Choose your mode
- **Observable**: Debug logging shows what's loaded

## Risks & Mitigations

### Risk: Model Confusion
**Problem**: Model tries to use tool without seeing full docs
**Mitigation**: Error-driven loading provides docs on first failure

### Risk: Extra Round-Trip
**Problem**: Tool fails, docs loaded, retry adds latency
**Mitigation**: Acceptable tradeoff (1 extra turn for 70% token savings)

### Risk: Breaking Existing Sessions
**Problem**: Users expect current behavior
**Mitigation**: Default mode = full (no change), progressive is opt-in

### Risk: Increased Complexity
**Problem**: More code to maintain
**Mitigation**: Clean abstraction, comprehensive tests

## Benchmarking Plan

### Test Cases
1. **Simple task** (bug fix): Count tool calls, failures, tokens
2. **Complex task** (refactor): Measure completion time, quality
3. **Error recovery**: Verify error-driven docs work
4. **Long session**: Test documentation caching

### Success Criteria
- ✅ ≥60% token reduction in progressive mode
- ✅ No increase in task failure rate
- ✅ ≤10% increase in average turn count
- ✅ All tests passing

## Next Steps

1. **Create ToolSignature definitions** for all 22 tools (manual curation)
2. **Implement ProgressiveToolLoader** struct
3. **Add configuration field** to AgentConfig
4. **Wire up mode selection** in tool registry
5. **Test with real LLM** calls
6. **Measure and document** results

## Alternative: Just Use Bash (Pi's Approach)

**Ultra-minimal**: Reduce to 4 tools (read, write, edit, bash) like pi.

**Pros**:
- Maximum simplicity
- Minimal token overhead (<200 tokens)
- Proven to work (Terminal-Bench results)

**Cons**:
- Requires models to know bash commands
- Less structured (no grep_file specific params)
- Harder to validate/sandbox

**Decision**: Implement progressive loading first (preserves VT Code's structured tools), consider pi-minimal as future option.

## References

- **Pi approach**: https://mariozechner.at/posts/2025-11-30-pi-coding-agent/
- **Current tool definitions**: `vtcode-core/src/tools/registry/declarations.rs`
- **Analysis**: `docs/PI_CODING_AGENT_ANALYSIS.md`

---

**Status**: Design complete, ready for implementation
**Priority**: High (Phase 3 of pi integration)
**Estimated effort**: 2-3 days
**Expected ROI**: 60-73% tool documentation token reduction
