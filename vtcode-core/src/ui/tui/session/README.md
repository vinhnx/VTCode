# Session.rs Module Architecture

## Module Organization

```
session.rs (main coordinator)
 palette.rs       - File & prompt palette management
 editing.rs       - Text editing & cursor movement
 messages.rs      - Transcript message operations
 reflow.rs        - Text wrapping & layout
 state.rs         - Lifecycle & state management
 events.rs        - Event handling (existing)
 command.rs       - Command execution (existing)
 render.rs        - UI rendering (existing)
 input.rs         - Input display (existing)
 header.rs        - Header rendering (existing)
 modal.rs         - Modal dialogs (existing)
 scroll.rs        - Scroll manager (existing)
 transcript.rs    - Transcript cache (existing)
 input_manager.rs - Input state (existing)
 styling.rs       - Style helpers (existing)
 ... (other supporting modules)
```

## Refactored Modules (New)

### palette.rs

-   Manages file and prompt palette lifecycle
-   Handles palette triggers and key events
-   Inserts references into input

### editing.rs

-   All text insertion and deletion
-   Cursor movement (char, word, line)
-   History navigation (currently disabled)

### messages.rs

-   Message line operations (push, append, replace)
-   Tool code fence handling
-   Message styling and prefixes

### reflow.rs

-   Transcript line wrapping
-   Tool and PTY output formatting
-   Diff line padding
-   Text justification for agent messages

### state.rs

-   Session initialization
-   Exit and redraw management
-   Modal management
-   Scroll operations
-   Cache invalidation

## Design Principles

1. **Single Responsibility**: Each module has one clear purpose
2. **Cohesion**: Related functionality grouped together
3. **Low Coupling**: Minimal dependencies between modules
4. **Information Hiding**: Internal methods use `pub(super)`
5. **Zero-Cost**: No runtime overhead from refactoring

## Benefits

-   **Reduced complexity**: Smaller, focused files
-   **Better navigation**: Clear where to find functionality
-   **Easier testing**: Isolated concerns
-   **Maintainability**: Changes localized to relevant modules
-   **Onboarding**: New developers can understand modules incrementally
