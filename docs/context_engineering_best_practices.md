# Context Engineering Best Practices for VTCode

Based on Anthropic's Context Engineering guide, this document analyzes VTCode's implementation and provides best practices for optimal context management.

## Understanding Context Engineering vs Prompt Engineering

### Single-Turn Prompt Engineering
**Context window:**
- System prompt
- User message
→ [Model] → Assistant message

### Multi-Turn Context Engineering (Agents)
**Possible context to give model:**
- Documentation, Tools, Memory files
- Comprehensive instructions
- Domain knowledge
- Message history

↓ **CURATION (happens each turn)** ↓

**Context window:**
- System prompt
- Selected docs
- Memory file
- Selected tools
- User message
- Message history

→ [Model] → Assistant message → Tool call → Tool result (feedback loop)

**Key Insight:** Context engineering is ITERATIVE. The curation phase happens each time we decide what to pass to the model.

## System Prompt Calibration: The "Just Right" Approach

### ❌ Too Specific (Brittle)
Problems:
- Hardcoded if-else logic
- Overly prescriptive steps (e.g., "MUST FOLLOW THESE STEPS: 1, 2, 3...")
- Exhaustive edge case handling
- Micromanagement of model behavior
- **Result**: Fragile, difficult to maintain, fails on unexpected inputs

Example of what to avoid:
```
You MUST FOLLOW THESE STEPS:
1. Identify the intent as one of ["intent_a", "intent_b", "intent_c"]
2. If intent is "intent_a" ask 3 follow-up questions, then always call tool_x
   - If user is in country Y, do step Z
   - If user mentions keyword W, follow these 9 sub-steps...
3. Here is a list of 47 cases that should be tagged as "requires_escalation"...
```

### ✅ Just Right (Current VTCode Approach)
Characteristics:
- **Core Responsibilities**: Clear role and purpose
- **Response Framework**: General guidance, not rigid steps
- **Guidelines**: Principles that help the model decide
- **Flexibility**: Room for model to adapt to situations

VTCode's current prompts follow this approach well:
```
## Core Principles
Work within `WORKSPACE_DIR`. Use targeted exploration (search, inspect) before making changes. 
Keep context minimal—load only what's needed for the current step.

## Context Strategy
- Use search tools (rg, ast-grep) to find relevant code before reading files
- Load file metadata (paths, sizes) as references; read content only when necessary
- Summarize tool outputs; avoid echoing large results
- Preserve recent decisions and errors in your working memory
```

**Strengths:**
- ✅ Concise (~200 tokens vs previous ~600)
- ✅ Actionable guidelines
- ✅ Clear priorities (search first, read second)
- ✅ Flexibility for model reasoning

### ❌ Too Vague
Problems:
- Assumes shared context that doesn't exist
- Lacks concrete guidance
- Model has to guess at expectations
- **Result**: Inconsistent behavior, requires many back-and-forth clarifications

Example of what to avoid:
```
You are a bakery assistant. You should attempt to solve customer issues.
You should embody the values and essence of the company brand.
Escalate to a human if needed.
```

## Current VTCode Implementation Analysis

### Strengths ✅

1. **Token Efficiency**
   - System prompt: 600 → 200 tokens (67% reduction)
   - Tool descriptions: Average 400 → 80 tokens (80% reduction)
   - Total savings: ~4,000 tokens upfront

2. **Progressive Disclosure**
   - "Search first, read second" pattern
   - "Load file metadata as references; read content only when necessary"
   - Explicit context minimization guidance

3. **Response Framework** (Implicit)
   - Explore → Act → Validate pattern
   - Clear tool selection guidance
   - Safety boundaries defined

4. **Flexibility**
   - No rigid if-else rules
   - Room for model reasoning
   - Adaptable to different scenarios

5. **Component-Level Tracking**
   - Token budget per component (system prompt, messages, tool results)
   - Real-time monitoring
   - Configurable thresholds

### Areas for Enhancement 🔧

#### 1. More Explicit Response Framework

**Current:**
```
## Behavior
Explore first, act second. Use progressive disclosure—start with lightweight searches,
drill down as needed. Maintain a mental model of your recent actions for coherent multi-turn work.
```

**Enhanced (following "Just Right" pattern):**
```
## Response Framework
1. **Assess the situation** – Ask clarifying questions if the request is ambiguous
2. **Gather context efficiently** – Use search tools to locate relevant code before reading files
3. **Make precise changes** – Use targeted edits over file rewrites when possible
4. **Verify outcomes** – Check that changes work as expected
5. **Confirm completion** – Summarize what was done and verify user satisfaction

## Guidelines
- When multiple approaches exist, choose the simplest that fully addresses the issue
- If a user mentions a file, search for it first to understand its context
- Always preserve existing patterns and coding style
- For destructive operations (delete, major refactor), explain the impact before proceeding
- Acknowledge urgency or frustration in the user's tone and respond with appropriate clarity
```

#### 2. Context Curation Strategy

**Add explicit guidance on WHAT context to include:**
```
## Context Management (Curation Strategy)

**What to include in context:**
- Recent conversation turns (last 5 by default)
- Active tool results from current task
- Decision ledger (key decisions and outcomes)
- Error messages and their resolutions
- Files currently being modified

**What to exclude from context:**
- Old tool results from completed subtasks
- Verbose command outputs (summarize instead)
- Duplicate information
- Large files not relevant to current step

**Dynamic adjustment:**
- Monitor token usage with budget tracker
- Trigger summarization at 75% threshold
- Trigger compaction at 85% threshold
- Preserve critical context during compression
```

#### 3. Tool Selection Guidance

**Current tool descriptions are concise but could benefit from decision framework:**
```
## Tool Selection Strategy

**For exploration:**
- Start with `grep_search` or `ast_grep_search` to find relevant locations
- Use `list_files` to understand structure before diving into content
- Only call `read_file` after identifying specific files of interest

**For modifications:**
- Prefer `edit_file` for targeted changes (preserves context)
- Use `write_file` only for new files or complete rewrites
- For structural refactoring, use `ast_grep_search` with transform mode

**For execution:**
- Use `run_terminal_cmd` for build/test/validation
- Set reasonable timeouts (default: 60s, long-running: 300s)
- Parse output for errors before proceeding

**For network:**
- Only use `curl` for trusted HTTPS documentation
- Never fetch localhost or private IPs
- Report security_notice to user
```

#### 4. Multi-Turn Coherence

**Add explicit guidance on building context across turns:**
```
## Multi-Turn Coherence

**Track your progress:**
- Reference previous tool results without re-executing
- Build on decisions from earlier turns
- Maintain a mental model of:
  - Files you've examined
  - Changes you've made
  - Errors you've encountered and resolved
  - User preferences expressed

**When context approaches limits:**
- Summarize completed subtasks
- Preserve active work context
- Keep recent errors and solutions
- Maintain decision ledger entries

**Handling interruptions:**
- If user changes direction, acknowledge the shift
- Ask if previous work should be preserved or abandoned
- Update your mental model accordingly
```

## Implementation Recommendations

### 1. Enhanced System Prompt Template

Create a modular prompt system:

```rust
// vtcode-core/src/prompts/system.rs

pub struct SystemPromptBuilder {
    base_identity: String,
    response_framework: Option<String>,
    guidelines: Option<String>,
    context_strategy: Option<String>,
    tool_selection: Option<String>,
    multi_turn_coherence: Option<String>,
}

impl SystemPromptBuilder {
    pub fn new() -> Self {
        Self {
            base_identity: DEFAULT_BASE_IDENTITY.to_string(),
            response_framework: None,
            guidelines: None,
            context_strategy: None,
            tool_selection: None,
            multi_turn_coherence: None,
        }
    }
    
    pub fn with_response_framework(mut self) -> Self {
        self.response_framework = Some(RESPONSE_FRAMEWORK.to_string());
        self
    }
    
    pub fn build(&self, task_type: TaskType) -> String {
        // Compose sections based on task type
        match task_type {
            TaskType::Simple => self.build_lightweight(),
            TaskType::Standard => self.build_standard(),
            TaskType::Complex => self.build_specialized(),
        }
    }
}
```

### 2. Dynamic Context Curation

Implement per-turn context selection:

```rust
// vtcode-core/src/core/context_curator.rs

pub struct ContextCurator {
    token_budget: TokenBudgetManager,
    decision_ledger: Arc<RwLock<DecisionTracker>>,
    active_files: HashSet<String>,
    recent_errors: VecDeque<ErrorContext>,
}

impl ContextCurator {
    /// Curate context for the next model call
    pub async fn curate_context(&mut self, 
        conversation: &[Message],
        available_tools: &[ToolDefinition],
    ) -> CuratedContext {
        let budget = self.token_budget.remaining_tokens().await;
        
        let mut context = CuratedContext::new();
        
        // Priority 1: Recent conversation (always include)
        context.add_recent_messages(conversation, 5);
        
        // Priority 2: Active work context
        for file in &self.active_files {
            if let Some(content) = self.get_file_summary(file) {
                context.add_file_context(file, content);
            }
        }
        
        // Priority 3: Decision ledger (compact)
        let ledger = self.decision_ledger.read();
        context.add_ledger_summary(ledger.render_ledger_brief(12));
        
        // Priority 4: Recent errors and resolutions
        for error in self.recent_errors.iter().rev().take(3) {
            context.add_error_context(error);
        }
        
        // Priority 5: Tools (only relevant ones)
        let relevant_tools = self.select_relevant_tools(available_tools, &context);
        context.add_tools(relevant_tools);
        
        // Check budget and compress if needed
        if context.estimated_tokens() > budget {
            context = self.compress_context(context, budget);
        }
        
        context
    }
}
```

### 3. Adaptive Tool Descriptions

Provide context-aware tool descriptions:

```rust
pub fn get_tool_description(tool_name: &str, context: &ContextState) -> String {
    let base_desc = BASE_TOOL_DESCRIPTIONS.get(tool_name);
    
    // Add context-specific guidance
    match context.phase {
        Phase::Exploration => {
            // Emphasize search tools
            if matches!(tool_name, "grep_search" | "ast_grep_search") {
                format!("{}\n\n**Current Phase**: Use this to find relevant code before reading files.", base_desc)
            } else {
                base_desc.to_string()
            }
        },
        Phase::Implementation => {
            // Emphasize edit tools
            if matches!(tool_name, "edit_file" | "write_file") {
                format!("{}\n\n**Current Phase**: Make precise changes. Prefer edit_file for targeted modifications.", base_desc)
            } else {
                base_desc.to_string()
            }
        },
        Phase::Validation => {
            // Emphasize test/build tools
            if matches!(tool_name, "run_terminal_cmd") {
                format!("{}\n\n**Current Phase**: Validate changes with tests or builds.", base_desc)
            } else {
                base_desc.to_string()
            }
        },
    }
}
```

### 4. Configuration

Add context curation settings to `vtcode.toml`:

```toml
[context.curation]
# Enable dynamic context curation
enabled = true
# Maximum context tokens per turn
max_tokens_per_turn = 100000
# Number of recent messages to always include
preserve_recent_messages = 5
# Maximum tool descriptions to include
max_tool_descriptions = 10
# Include decision ledger summary
include_ledger = true
ledger_max_entries = 12
# Include recent errors
include_recent_errors = true
max_recent_errors = 3

[context.response_framework]
# Include explicit response framework in system prompt
enabled = true
# Include tool selection guidance
include_tool_selection = true
# Include multi-turn coherence guidance
include_multi_turn_guidance = true
```

## Best Practices Summary

### For System Prompts
1. ✅ **Be concise but complete** - 200-400 tokens is the sweet spot
2. ✅ **Provide response framework** - Not rigid steps, but general approach
3. ✅ **Include guidelines** - Principles that help model decide
4. ✅ **Avoid brittle rules** - No exhaustive if-else trees
5. ✅ **Keep it flexible** - Room for model to reason and adapt

### For Context Curation
1. ✅ **Curate every turn** - Don't include everything, select what's relevant
2. ✅ **Prioritize smartly** - Recent > Active > Historical
3. ✅ **Monitor budget** - Use token tracking to prevent overflow
4. ✅ **Compress intelligently** - Preserve critical context, summarize the rest
5. ✅ **Track coherence** - Maintain mental model across turns

### For Tool Design
1. ✅ **Clear purposes** - Each tool has distinct use case
2. ✅ **Minimal overlap** - Avoid capability redundancy
3. ✅ **Token guidance** - Built-in advice for efficient usage
4. ✅ **Metadata first** - Return lightweight refs before full content
5. ✅ **Auto-chunking** - Handle large outputs automatically

## Current VTCode Score

**System Prompt Calibration**: 8/10 ⭐⭐⭐⭐⭐⭐⭐⭐
- Excellent conciseness and clarity
- Good balance of guidance and flexibility
- Could benefit from more explicit response framework

**Context Curation**: 7/10 ⭐⭐⭐⭐⭐⭐⭐
- Token budget tracking implemented
- Decision ledger for key decisions
- Context compression available
- Could benefit from dynamic per-turn curation

**Tool Design**: 9/10 ⭐⭐⭐⭐⭐⭐⭐⭐⭐
- Excellent conciseness (80% reduction)
- Clear purposes with minimal overlap
- Token management guidance included
- Auto-chunking implemented

**Overall Context Engineering**: 8/10 ⭐⭐⭐⭐⭐⭐⭐⭐

VTCode already implements many context engineering best practices. The suggested enhancements focus on making the iterative curation process more explicit and providing stronger multi-turn coherence guidance.

## Next Steps

1. **Implement Response Framework** - Add explicit 5-step framework to system prompt
2. **Dynamic Context Curator** - Build per-turn context selection system
3. **Adaptive Tool Descriptions** - Context-aware tool guidance
4. **Multi-Turn Coherence** - Explicit guidance on building context across turns
5. **Testing & Validation** - Measure improvements in token efficiency and task completion
