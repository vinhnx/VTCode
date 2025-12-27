# TUI-Only Tool Permission Refactoring

## Overview

This document describes the refactoring of `prompt_user_for_tool()` to be TUI-only, removing all CLI-specific code and dependencies.

## Problem Statement

The original `prompt_user_for_tool()` function in `vtcode-core/src/tool_policy.rs` contained CLI-specific code with a guard to prevent execution in TUI mode. This created several issues:

1. The function could corrupt the terminal if accidentally called in TUI mode
2. It contained dialoguer (CLI) dependencies in the library crate
3. The architecture was not clean - TUI mode had to use a completely different code path
4. The library crate had mixed UI concerns

## Solution

The refactoring introduces a pluggable permission handler architecture:

### 1. PermissionPromptHandler Trait

```rust
pub trait PermissionPromptHandler: Send + Sync {
    fn prompt_tool_permission(&mut self, tool_name: &str) -> Result<ToolExecutionDecision>;
}
```

This trait allows different UI modes to provide their own implementation for prompting users about tool execution.

### 2. ToolPolicyManager Updates

The `ToolPolicyManager` now includes:

```rust
pub struct ToolPolicyManager {
    config_path: PathBuf,
    config: ToolPolicyConfig,
    permission_handler: Option<Box<dyn PermissionPromptHandler>>,
}

impl ToolPolicyManager {
    pub fn set_permission_handler(&mut self, handler: Box<dyn PermissionPromptHandler>);
    
    pub fn prompt_user_for_tool(&mut self, tool_name: &str) -> Result<ToolExecutionDecision>;
    
    // Updated to use handler
    pub async fn should_execute_tool(&mut self, tool_name: &str) -> Result<ToolExecutionDecision>;
}
```

### 3. Removed CLI-Specific Code

The original `prompt_user_for_tool()` function (lines 874-952) was completely removed, including:

- `VTCODE_TUI_MODE` environment variable check
- Interactive terminal detection
- `AnsiRenderer` usage
- `notify_attention()` calls
- `UserConfirmation::confirm_tool_usage()` calls
- `handle_tool_confirmation()` method

## Usage

### For TUI Mode

In the binary crate (src/agent/runloop/...), set up a TUI permission handler:

```rust
use vtcode_core::tool_policy::{ToolPolicyManager, PermissionPromptHandler};

// Create handler that integrates with TUI system
struct TuiPermissionHandler {
    handle: InlineHandle,
    session: UiSession,
    // ... other TUI-specific state
}

impl PermissionPromptHandler for TuiPermissionHandler {
    fn prompt_tool_permission(&mut self, tool_name: &str) -> Result<ToolExecutionDecision> {
        // Use TUI modal system via tool_routing.rs
        // This would call ensure_tool_permission() or similar
        todo!("Implement TUI-specific prompting")
    }
}

// In setup code
let mut policy_manager = ToolPolicyManager::new().await?;
policy_manager.set_permission_handler(Box::new(TuiPermissionHandler::new(...)));
```

### For CLI Mode

For CLI applications using the library:

```rust
use vtcode_core::tool_policy::{ToolPolicyManager, PermissionPromptHandler};
use vtcode_core::ui::user_confirmation::UserConfirmation;

struct CliPermissionHandler;

impl PermissionPromptHandler for CliPermissionHandler {
    fn prompt_tool_permission(&mut self, tool_name: &str) -> Result<ToolExecutionDecision> {
        let selection = UserConfirmation::confirm_tool_usage(tool_name, None)?;
        match selection {
            ToolConfirmationResult::Yes => Ok(ToolExecutionDecision::Allowed),
            ToolConfirmationResult::YesAutoAccept => Ok(ToolExecutionDecision::Allowed),
            ToolConfirmationResult::No => Ok(ToolExecutionDecision::Denied),
            ToolConfirmationResult::Feedback(msg) => {
                Ok(ToolExecutionDecision::DeniedWithFeedback(msg))
            }
        }
    }
}

// In setup code
let mut policy_manager = ToolPolicyManager::new().await?;
policy_manager.set_permission_handler(Box::new(CliPermissionHandler));
```

### For Headless/Non-Interactive Mode

```rust
use vtcode_core::tool_policy::{ToolPolicyManager, PermissionPromptHandler};

struct HeadlessPermissionHandler {
    default_decision: ToolExecutionDecision,
}

impl PermissionPromptHandler for HeadlessPermissionHandler {
    fn prompt_tool_permission(&mut self, _tool_name: &str) -> Result<ToolExecutionDecision> {
        Ok(self.default_decision.clone())
    }
}

// Auto-approve all tools in CI/CD
let mut policy_manager = ToolPolicyManager::new().await?;
policy_manager.set_permission_handler(Box::new(HeadlessPermissionHandler::new(
    ToolExecutionDecision::Allowed
)));
```

## Backward Compatibility

The refactoring maintains backward compatibility:

1. If no permission handler is set, `should_execute_tool()` returns `Allowed` for Prompt policies
2. This maintains the existing behavior where TUI mode permissions are handled externally
3. Existing code that doesn't set a handler continues to work

## Benefits

1. **TUI-Only**: No CLI code in the refactored function
2. **Clean Architecture**: UI prompting is now pluggable
3. **Library-Friendly**: The library can be used in any UI mode
4. **Testable**: Permission handlers can be easily mocked in tests
5. **Maintainable**: Clear separation of concerns between library and UI

## Files Modified

- `vtcode-core/src/tool_policy.rs`: Main refactoring location
- `vtcode-core/src/tool_policy_handlers.rs`: New file with example implementations (optional)

## Migration Guide

### For Library Users

If you're using `ToolPolicyManager` directly in your application:

1. If you were relying on CLI prompting, implement a `CliPermissionHandler`
2. If you're using TUI, implement a `TuiPermissionHandler`
3. If you're in headless mode, implement a `HeadlessPermissionHandler`
4. Call `set_permission_handler()` after creating the manager

### For Binary / Application Code

The main VT Code binary should:

1. Create a TUI permission handler that integrates with `tool_routing.rs`
2. Set the handler on the policy manager during session setup
3. The handler should call `ensure_tool_permission()` or similar TUI functions

## Testing

Create mock permission handlers for testing:

```rust
struct MockPermissionHandler {
    responses: Vec<ToolExecutionDecision>,
    call_count: usize,
}

impl PermissionPromptHandler for MockPermissionHandler {
    fn prompt_tool_permission(&mut self, _tool_name: &str) -> Result<ToolExecutionDecision> {
        let response = self.responses.get(self.call_count)
            .cloned()
            .unwrap_or(ToolExecutionDecision::Allowed);
        self.call_count += 1;
        Ok(response)
    }
}
```

## Future Enhancements

1. Async permission handlers (currently synchronous)
2. Permission handler that supports justification collection
3. Handler that integrates with ACP (Agent Client Protocol)
4. Persistent handler state across sessions
