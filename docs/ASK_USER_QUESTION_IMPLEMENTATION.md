# ask_user_question Tool Implementation

## Overview

Implemented a new interactive HITL (Human-in-the-Loop) tool `ask_user_question` that presents users with a tabbed list interface for making structured choices during agent execution.

## Feature Summary

-   **Tool Name**: `ask_user_question`
-   **UI**: Tabbed wizard modal with list selection (using existing ratatui TUI infrastructure)
-   **Integration**: Unified runloop + Plan Mode support
-   **Returns**: JSON with selected `{tab_id, choice_id}` or `{cancelled: true}`

## Implementation Details

### Core Components

1. **Tool Definition**

    - Location: `vtcode-core/src/tools/ask_user_question.rs`
    - Schema: Takes `question`, `tabs[]` with `tab_id`, `title`, `choices[]`
    - Registered in: `vtcode-core/src/tools/registry/builtins.rs`

2. **TUI Modal Extension**

    - Extended `WizardModal` with new `TabbedList` mode
    - Location: `vtcode-core/src/ui/tui/types.rs`, `vtcode-core/src/ui/tui/session/modal.rs`
    - Behavior: Tab switching without step completion; Enter submits immediately
    - Search support: Typing/backspace/delete/tab/paste integrated

3. **Execution Handler**

    - Location: `src/agent/runloop/unified/ask_user_question.rs`
    - Opens TUI modal via `InlineCommand::ShowWizardModal` with `TabbedList` mode
    - Blocks on `InlineSession` events until user submits/cancels
    - Returns structured JSON response

4. **Tool Interception**

    - Location: `src/agent/runloop/unified/tool_pipeline.rs`
    - Intercepts `ASK_USER_QUESTION` tool calls before execution
    - Routes to `execute_ask_user_question_tool()` for interactive UI handling

5. **Plan Mode Support**
    - System prompt: `src/agent/runloop/unified/incremental_system_prompt.rs`
    - UI message: `src/agent/runloop/unified/turn/session/slash_commands/handlers.rs`
    - Explicitly allows using `ask_user_question` for clarifications during planning
    - Tool registry already treats it as non-mutating (compatible with read-only mode)

## Stabilization Fixes

### 1. Reasoning-Only Output Handling

-   **Problem**: Model responses with only reasoning (no content) weren't counted as emitted tokens
-   **Fix**: Added fallback in `src/agent/runloop/unified/ui_interaction.rs` to render reasoning and set `emitted_tokens=true`
-   **Test**: `agent::runloop::unified::ui_interaction::tests::renders_reasoning_when_no_content`

### 2. TaskRunState Constructor Signature

-   **Problem**: `TaskRunState::new()` gained a 4th parameter `max_context_tokens` but example wasn't updated
-   **Fix**: Updated `vtcode-core/examples/verify_optimization.rs` to pass 4 args
-   **Impact**: Build failure in examples directory

### 3. Tab Navigation in List Modals

-   **Problem**: Tab key was intercepted by search handler for autocompletion instead of advancing selection
-   **Fix**: Removed Tab case from search key handler in `vtcode-core/src/ui/tui/session/modal.rs`
-   **Test**: `ui::tui::session::modal::tests::list_modal_tab_moves_forward`

### 4. Command Safety Test Expectation

-   **Problem**: `cat_is_always_safe` test expected `Unknown` but `cat` is now in safe tools list (returns `Allow`)
-   **Fix**: Updated test assertion to expect `Allow` in `vtcode-core/src/command_safety/safe_command_registry.rs`
-   **Rationale**: Test name and comment suggest cat should be safe; behavior matches implementation

## Testing Status

### Passing Tests (Verified)

-   [x] TUI wizard tabbed-list mode: tab switching, Enter submit
-   [x] Reasoning-only rendering fallback
-   [x] Command safety: cat_is_always_safe (updated expectation)

### Known Test Issues (Pre-existing)

-   Many vtcode-core tests take long to compile/run (full suite ~1800+ tests)
-   Full suite runs sometimes hang or get killed (timeout/SIGTERM)
-   Strategy: targeted subset testing to avoid blocking workflow

## Usage Example

```json
{
    "tool": "ask_user_question",
    "input": {
        "question": "Which language would you prefer?",
        "tabs": [
            {
                "tab_id": "backend",
                "title": "Backend",
                "choices": [
                    { "choice_id": "rust", "label": "Rust" },
                    { "choice_id": "go", "label": "Go" }
                ]
            },
            {
                "tab_id": "frontend",
                "title": "Frontend",
                "choices": [
                    { "choice_id": "react", "label": "React" },
                    { "choice_id": "vue", "label": "Vue" }
                ]
            }
        ]
    }
}
```

**Response (submit)**:

```json
{ "tab_id": "backend", "choice_id": "rust" }
```

**Response (cancel)**:

```json
{ "cancelled": true }
```

## Files Modified

### New Files

-   `vtcode-core/src/tools/ask_user_question.rs`
-   `src/agent/runloop/unified/ask_user_question.rs`

### Modified Files

-   `vtcode-config/src/constants.rs` - Added ASK_USER_QUESTION constant
-   `vtcode-core/src/tools/mod.rs` - Exported ask_user_question module
-   `vtcode-core/src/tools/registry/builtins.rs` - Registered tool
-   `vtcode-core/src/ui/tui/types.rs` - Added InlineListSelection::AskUserChoice, WizardModalMode::TabbedList
-   `vtcode-core/src/ui/tui/session/*.rs` - Wizard modal event handling, search integration
-   `src/agent/runloop/unified/tool_pipeline.rs` - Tool interception for ask_user_question
-   `src/agent/runloop/unified/incremental_system_prompt.rs` - Plan Mode prompt update
-   `src/agent/runloop/unified/turn/session/slash_commands/handlers.rs` - Plan Mode UI message
-   `src/agent/runloop/unified/ui_interaction.rs` - Reasoning-only fallback
-   `vtcode-core/examples/verify_optimization.rs` - TaskRunState signature fix
-   `vtcode-core/src/command_safety/safe_command_registry.rs` - Test expectation fix

## Next Steps

-   [ ] Full test suite validation (pending long compile times)
-   [ ] Manual integration testing with real agent workflows
-   [ ] Documentation updates (if not already covered in AGENTS.md)
