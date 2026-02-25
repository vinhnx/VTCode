# Phase 1 Integration Guide

This document provides step-by-step instructions for integrating the new Phase 1 components into the existing ChatViewProvider.

---

## Integration Tasks

### Task 1: Import New Components

In `src/chatView.ts`, add these imports at the top:

```typescript
import { StatusIndicator } from "./ui/statusIndicator";
import { ErrorPresentationHandler } from "./error/errorPresentation";
```

---

### Task 2: Add Status Indicator to ChatViewProvider

In the `ChatViewProvider` class:

```typescript
export class ChatViewProvider implements vscode.WebviewViewProvider {
    public static readonly viewId = "vtcodeChatView";

    private view: vscode.WebviewView | undefined;
    private readonly messages: ChatMessage[] = [];
    private workspaceTrusted = vscode.workspace.isTrusted;
    private lastHumanInLoopSetting: boolean | undefined;
    
    // ADD THIS:
    private statusIndicator: StatusIndicator | undefined;

    // ... rest of constructor
}
```

---

### Task 3: Initialize Status Indicator in resolveWebviewView

```typescript
public resolveWebviewView(
    view: vscode.WebviewView,
    _context: vscode.WebviewViewResolveContext,
    _token: vscode.CancellationToken
): void {
    this.output.appendLine("[chatView] resolveWebviewView called");
    this.view = view;
    view.webview.options = {
        enableScripts: true,
        localResourceRoots: [
            vscode.Uri.joinPath(this.extensionUri, "media"),
        ],
    };

    this.output.appendLine("[chatView] Setting webview HTML");
    view.webview.html = this.getHtml(view.webview);
    this.output.appendLine("[chatView] Webview HTML set successfully");

    // ADD THIS:
    // Initialize status indicator
    this.statusIndicator = new StatusIndicator((text) => {
        view.webview.postMessage({
            type: "updateStatus",
            text: text,
        });
    });

    view.webview.onDidReceiveMessage(async (message: WebviewMessage) => {
        // ... existing message handling
    });
}
```

---

### Task 4: Update Status During Message Handling

In the `handleUserMessage` method, update status indicators:

```typescript
private async handleUserMessage(content: string): Promise<void> {
    this.addMessage({ role: "user", content, timestamp: Date.now() });
    
    // ADD THIS:
    this.statusIndicator?.setStatus("thinking");

    try {
        const response = await this.backend.chat(content, this.messages);

        // ADD THIS:
        this.statusIndicator?.setStatus("streaming");

        for await (const chunk of response) {
            // Handle streaming
            // ADD THIS in streaming loop:
            if (chunk.type === "text_chunk") {
                // Update token count if available
                if (chunk.tokenCount !== undefined) {
                    this.statusIndicator?.updateTokens(chunk.tokenCount);
                }
            }
        }

        // ADD THIS:
        this.statusIndicator?.setStatus("idle");
    } catch (error) {
        // ADD THIS:
        this.statusIndicator?.setStatus("error");
        
        // Format error using new handler
        const errorPresentation = ErrorPresentationHandler.format(error);
        const formattedMessage = ErrorPresentationHandler.formatForChat(error);
        
        this.addMessage({
            role: "error",
            content: formattedMessage,
            timestamp: Date.now(),
        });

        this.output.appendLine(`[chatView] Error: ${errorPresentation.title}`);
        this.output.appendLine(`[chatView] ${errorPresentation.message}`);
    }
}
```

---

### Task 5: Add Tool Execution Status Tracking

When tools are being executed:

```typescript
private async executeTool(toolCall: VtcodeToolCall): Promise<void> {
    // ADD THIS:
    this.statusIndicator?.setStatus("executing");
    this.statusIndicator?.updateProgress(0, toolCall.parameters.length, "Executing tool");

    try {
        // Tool execution logic
        // ADD THIS in loop:
        let index = 0;
        for (const result of results) {
            index++;
            this.statusIndicator?.updateProgress(
                index,
                toolCall.parameters.length,
                `Executing tool (${toolCall.name})`
            );
        }

        // ADD THIS:
        this.statusIndicator?.setStatus("streaming");
    } catch (error) {
        // ADD THIS:
        this.statusIndicator?.setStatus("error");
        const formatted = ErrorPresentationHandler.formatForChat(error);
        this.addMessage({
            role: "error",
            content: formatted,
            timestamp: Date.now(),
        });
    }
}
```

---

### Task 6: Update HTML to Receive Status Messages

In the webview script (`media/chat-view.js`), add this message handler:

```javascript
// Listen for status updates from backend
vscode.postMessage({ type: "ready" });

window.addEventListener("message", (event) => {
    const message = event.data;

    if (message.type === "updateStatus") {
        const statusEl = document.getElementById("status");
        if (statusEl) {
            statusEl.textContent = message.text;
            // Apply status class for styling
            const statusMatch = message.text.match(/^([ ])/);
            if (statusMatch) {
                statusEl.className = "chat-status";
                if (message.text.includes("Ready")) {
                    statusEl.classList.add("idle");
                } else if (message.text.includes("Error")) {
                    statusEl.classList.add("error");
                }
            }
        }
    }
    // ... other message types
});
```

---

### Task 7: Add Model and Participant Info

Update the status indicator to show model information:

```typescript
// In resolveWebviewView or when backend info changes:
private updateStatusIndicatorInfo(): void {
    const config = this.backend.getConfig(); // Add this method to VtcodeBackend if needed
    
    if (config?.model) {
        this.statusIndicator?.setModel(config.model);
    }
}
```

---

### Task 8: Update CSS Integration

Ensure the CSS file is properly linked in `getHtml()`:

```typescript
private getHtml(webview: vscode.Webview): string {
    const scriptUri = webview.asWebviewUri(
        vscode.Uri.joinPath(this.extensionUri, "media", "chat-view.js")
    );
    const styleUri = webview.asWebviewUri(
        vscode.Uri.joinPath(this.extensionUri, "media", "chat-view.css")
    );

    // CSS is already referenced, just ensure it's the enhanced version
    // The new CSS classes will automatically apply when message content includes
    // proper HTML elements (code blocks, tables, etc.)

    const nonce = this.createNonce();

    return `<!DOCTYPE html>
    <!-- ... HTML template with new CSS active ... -->`;
}
```

---

### Task 9: Test Integration

Create a test file to verify integration:

```typescript
// tests/integration/statusIntegration.test.ts
import * as assert from "assert";
import * as vscode from "vscode";
import { ChatViewProvider } from "../../src/chatView";

describe("ChatViewProvider - Status Integration", () => {
    it("should initialize status indicator", async () => {
        // Create provider instance
        // Verify statusIndicator is created
        // Check initial state
        assert.ok(true); // Placeholder
    });

    it("should update status during message handling", async () => {
        // Send message
        // Track status changes
        // Verify progression: thinking -> streaming -> idle
        assert.ok(true); // Placeholder
    });

    it("should show error status on failure", async () => {
        // Trigger error
        // Verify status shows error
        // Verify error message formatted correctly
        assert.ok(true); // Placeholder
    });
});
```

---

## Incremental Integration Steps

### Step 1: Basic Status Indicator
- Add StatusIndicator import and initialization
- Wire up basic status updates (thinking → streaming → idle)
- Test with simple messages

### Step 2: Error Handling Integration
- Add ErrorPresentationHandler usage
- Format error messages
- Display errors in chat

### Step 3: Metrics Display
- Add token counting
- Show elapsed time
- Display model name

### Step 4: CSS Enhancements
- Verify code blocks render correctly
- Test markdown formatting
- Check dark/light theme compatibility

### Step 5: Advanced Features
- Add progress indicators for tool execution
- Implement participant display
- Add more detailed metrics

---

## Common Issues & Solutions

### Issue: Status Not Displaying
**Solution**: Ensure the status element exists in HTML and message handler is wired correctly.

```typescript
// Check element exists
const statusEl = document.getElementById("status");
if (!statusEl) {
    console.error("Status element not found in DOM");
}
```

### Issue: Errors Not Formatted
**Solution**: Import ErrorPresentationHandler and use formatForChat method.

```typescript
const formatted = ErrorPresentationHandler.formatForChat(error);
// Ensure this formatted string is being added to messages
```

### Issue: CSS Not Applied
**Solution**: Clear browser cache and restart extension.

```bash
# Reload extension
cmd+shift+P > Developer: Reload Window
```

### Issue: TypeScript Errors
**Solution**: Ensure all imports are correct and types are imported.

```typescript
import { StatusIndicator, type ChatStatus } from "./ui/statusIndicator";
import { ErrorPresentationHandler, type ErrorPresentation } from "./error/errorPresentation";
```

---

## Performance Considerations

### Avoid
- Frequent status updates (> 10 per second)
- Large error messages in logs
- Unbounded message history

### Optimize
- Batch status updates using debouncing
- Sample metrics (e.g., every 500ms)
- Implement message cleanup for old conversations

---

## Verification Checklist

- [ ] StatusIndicator imported and initialized
- [ ] Status updates during thinking/streaming/executing
- [ ] Error messages formatted using ErrorPresentationHandler
- [ ] CSS enhanced for markdown, code blocks, tables
- [ ] Status animations display correctly
- [ ] No console errors or warnings
- [ ] All types are properly imported
- [ ] Unit tests pass
- [ ] Integration tests pass
- [ ] Manual testing completed

---

## Related Documentation

- [../../docs/vscode-extension-improve-docs/VSCODE_EXTENSION_MIGRATION_ROADMAP.md](../../docs/vscode-extension-improve-docs/VSCODE_EXTENSION_MIGRATION_ROADMAP.md) - 12-week roadmap
