# Phase 1 - Quick Reference Card

## Status Indicator

### Quick Start

```typescript
import { StatusIndicator } from "./ui/statusIndicator";

const indicator = new StatusIndicator((text) => {
    statusEl.textContent = text;
});

indicator.setStatus("streaming");
indicator.updateTokens(250);
indicator.setModel("gpt-5");
```

### Status Values

```
"idle"      →  Ready
"thinking"  →  Thinking...
"streaming" →  Streaming response...
"executing" →  Executing tools...
"error"     →   Error occurred
```

### Methods

```typescript
setStatus(status: ChatStatus): void
setModel(modelName: string): void
updateTokens(tokensUsed: number): void
updateElapsedTime(elapsedMs: number): void
updateProgress(current: number, total: number, message?: string): void
reset(): void
```

### Output Examples

```
 Ready
 Thinking...
 Streaming response... | 1s | 250 tokens | gpt-4
 Executing tools (2/5) | 45s
  Error occurred
```

---

## Error Presentation Handler

### Quick Start

```typescript
import { ErrorPresentationHandler } from "./error/errorPresentation";

try {
    // operation
} catch (error) {
    const presentation = ErrorPresentationHandler.format(error);
    console.log(`${presentation.title}: ${presentation.message}`);

    // For chat display:
    const chatMessage = ErrorPresentationHandler.formatForChat(error);
    addMessage({ role: "error", content: chatMessage });
}
```

### Detected Error Types

| Pattern            | Title                   | Severity |
| ------------------ | ----------------------- | -------- |
| ECONNREFUSED       | Connection Failed       | error    |
| timeout            | Request Timeout         | warning  |
| ENOTFOUND          | Network Unreachable     | error    |
| token limit        | Token Limit Exceeded    | warning  |
| 429 / rate limit   | Rate Limited            | warning  |
| 401 / unauthorized | Authentication Failed   | error    |
| ENOENT             | File Not Found          | warning  |
| EACCES             | Permission Denied       | error    |
| JSON / parse       | Invalid Response Format | warning  |

### Methods

```typescript
format(error: Error | string): ErrorPresentation
formatForChat(error: Error | string): string
getContext(error: Error | string): Record<string, unknown>
```

### Output Example

```
**Connection Failed**

VT Code cannot connect to the backend service. The service may be starting
up or encountered an issue.

 **Suggestion:** Try again in a few moments. If the problem persists,
restart the extension.
```

---

## CSS Classes

### Markdown Elements

```html
<strong>Bold text</strong>
<em>Italic text</em>
<code>inline code</code>

<pre><code class="hljs language-typescript">
const x = 1;
</code></pre>

<table>
    <th>Header</th>
    <td>Cell</td>
</table>

<ul>
    <li>Item</li>
</ul>
<ol>
    <li>Item</li>
</ol>

<blockquote>Quote</blockquote>
<a href="#">Link</a>
```

### Code Block Actions

```html
<div class="code-block-wrapper">
    <div class="code-block-actions">
        <button class="code-copy-button">Copy</button>
    </div>
    <pre><code>...</code></pre>
</div>
```

---

## Integration Checklist

### Minimal Integration (1 hour)

- [ ] Import StatusIndicator in chatView.ts
- [ ] Initialize in resolveWebviewView
- [ ] Update status in handleUserMessage
- [ ] Test basic functionality

### Full Integration (3 hours)

- [ ] All of above plus:
- [ ] Import ErrorPresentationHandler
- [ ] Format error messages
- [ ] Add token tracking
- [ ] Update webview message handler
- [ ] Test error scenarios

### Advanced Integration (Optional)

- [ ] Progress indicators for tools
- [ ] Participant context display
- [ ] Advanced metrics
- [ ] Performance monitoring

---

## Common Code Snippets

### Initialize Status Indicator

```typescript
private statusIndicator: StatusIndicator | undefined;

// In resolveWebviewView:
this.statusIndicator = new StatusIndicator((text) => {
    this.view?.webview.postMessage({
        type: "updateStatus",
        text: text,
    });
});
```

### Handle Streaming with Status

```typescript
this.statusIndicator?.setStatus("streaming");

for await (const chunk of response) {
    if (chunk.tokenCount !== undefined) {
        this.statusIndicator?.updateTokens(chunk.tokenCount);
    }
}

this.statusIndicator?.setStatus("idle");
```

### Format Error in Chat

```typescript
catch (error) {
    this.statusIndicator?.setStatus("error");

    const formatted = ErrorPresentationHandler.formatForChat(error);
    this.addMessage({
        role: "error",
        content: formatted,
        timestamp: Date.now(),
    });
}
```

### Update Status in Webview

```typescript
window.addEventListener("message", (event) => {
    const message = event.data;
    if (message.type === "updateStatus") {
        const el = document.getElementById("status");
        if (el) {
            el.textContent = message.text;
            el.classList.toggle("error", message.text.includes("Error"));
        }
    }
});
```

---

## Testing

### Run Tests

```bash
# StatusIndicator tests
npm test -- --grep "StatusIndicator"

# ErrorPresentation tests
npm test -- --grep "ErrorPresentation"

# All Phase 1 tests
npm test -- --grep "Phase1|StatusIndicator|ErrorPresentation"

# With coverage
npm test -- --coverage
```

### Expected Results

```
 StatusIndicator (10 tests)
 ErrorPresentationHandler (12 tests)
 Coverage: >92%
 All passing
```

---

## Troubleshooting

### Status Not Showing

```typescript
// Verify element exists
const el = document.getElementById("status");
console.assert(el, "Status element not found");

// Verify message handler registered
// Check webview.postMessage calls
```

### Errors Not Formatted

```typescript
// Verify handler imported
import { ErrorPresentationHandler } from "./error/errorPresentation";

// Use formatForChat for chat display
const text = ErrorPresentationHandler.formatForChat(error);
```

### CSS Not Applied

```bash
# Reload extension
cmd+shift+P > Developer: Reload Window

# Clear cache
rm -rf out/ && npm run compile
```

### TypeScript Errors

```typescript
// Verify imports
import { StatusIndicator, type ChatStatus } from "./ui/statusIndicator";
import {
    ErrorPresentationHandler,
    type ErrorPresentation,
} from "./error/errorPresentation";
```

---

## Performance Tips

### Do

- Update status every 100-500ms
- Batch updates when possible
- Use debouncing for frequent updates
- Sample metrics instead of every event

### Avoid

- Status updates > 10 per second
- Large error messages in logs
- Unbounded message history
- Synchronous file I/O in callbacks

---

## Files Reference

| File                      | Purpose                    | Location   |
| ------------------------- | -------------------------- | ---------- |
| statusIndicator.ts        | Status indicator component | src/ui/    |
| statusIndicator.test.ts   | Status indicator tests     | src/ui/    |
| errorPresentation.ts      | Error formatting           | src/error/ |
| errorPresentation.test.ts | Error handler tests        | src/error/ |
| chat-view.css             | Enhanced styles            | media/     |
| PHASE_1_INTEGRATION.md    | Integration guide          | docs/      |

---

## Key Exports

```typescript
// statusIndicator.ts
export class StatusIndicator
export type ChatStatus = "idle" | "thinking" | "streaming" | "executing" | "error"
export interface StatusIndicatorState
export interface StatusIndicatorState.progress
export interface StatusIndicatorState.indicators

// errorPresentation.ts
export class ErrorPresentationHandler
export interface ErrorPresentation
export const ERROR_MESSAGES
```

---

## Links & Resources

- **Integration Guide**: `docs/PHASE_1_INTEGRATION.md`
- **Phase Status**: `PHASE_1_STATUS.md`
- **Full Roadmap**: `docs/vscode-extension-improve-docs/VSCODE_EXTENSION_MIGRATION_ROADMAP.md`

---

**Quick Reference v1.0** | November 8, 2025
