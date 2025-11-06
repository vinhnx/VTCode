# Quick Setup Guide - VTCode Enhanced Chat Extension

## Prerequisites

-   Node.js 18+ and npm
-   Visual Studio Code 1.80+
-   TypeScript 5.0+

## Installation Steps

### 1. Install Dependencies

```bash
cd vscode-extension
npm install
```

### 2. Configure MCP Providers

Create or update `vtcode.toml` in workspace root:

```toml
[mcp]
enabled = true

[[mcp.providers]]
name = "context7"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-context7"]
enabled = true

[context7]
enabled = true
max_tokens = 5000
cache_results = true
cache_ttl_seconds = 3600
auto_fetch_docs = true
```

### 3. Update Extension Entry Point

In `src/extension.ts`:

```typescript
import * as vscode from "vscode";
import { EnhancedChatViewProvider } from "./enhancedChatView";
import { createContext7Integration } from "./context7Integration";
import { createMcpToolManager } from "./mcpTools";
import { VtcodeTerminalManager } from "./agentTerminal";

export async function activate(context: vscode.ExtensionContext) {
    const outputChannel = vscode.window.createOutputChannel("VTCode Chat");
    const terminalManager = new VtcodeTerminalManager(context);

    try {
        // Create MCP tool manager
        const mcpManager = await createMcpToolManager(outputChannel);

        // Create Context7 integration
        const context7 = await createContext7Integration(
            mcpManager,
            outputChannel
        );

        // Create enhanced chat provider
        const chatProvider = new EnhancedChatViewProvider(
            context,
            terminalManager,
            outputChannel
        );

        // Register webview provider
        context.subscriptions.push(
            vscode.window.registerWebviewViewProvider(
                EnhancedChatViewProvider.viewType,
                chatProvider,
                { webviewOptions: { retainContextWhenHidden: true } }
            )
        );

        // Register commands
        context.subscriptions.push(
            vscode.commands.registerCommand("vtcode.chat.clear", () => {
                vscode.commands.executeCommand(
                    "workbench.action.webview.reloadWebviewAction"
                );
            })
        );

        outputChannel.appendLine("[Extension] Activated successfully");
    } catch (error) {
        outputChannel.appendLine(`[Extension] Activation failed: ${error}`);
        vscode.window.showErrorMessage(`VTCode activation failed: ${error}`);
    }
}

export function deactivate() {}
```

### 4. Update Package.json

Add view container and contribution points:

```json
{
    "contributes": {
        "viewsContainers": {
            "activitybar": [
                {
                    "id": "vtcode-enhanced",
                    "title": "VTCode Enhanced",
                    "icon": "$(comment-discussion)"
                }
            ]
        },
        "views": {
            "vtcode-enhanced": [
                {
                    "type": "webview",
                    "id": "vtcodeEnhancedChat",
                    "name": "Chat",
                    "contextualTitle": "VTCode Enhanced Chat"
                }
            ]
        },
        "commands": [
            {
                "command": "vtcode.chat.clear",
                "title": "VTCode: Clear Chat",
                "icon": "$(clear-all)"
            }
        ],
        "configuration": {
            "title": "VTCode Enhanced Chat",
            "properties": {
                "vtcode.chat.maxHistoryLength": {
                    "type": "number",
                    "default": 500,
                    "description": "Maximum messages in chat history"
                },
                "vtcode.chat.showTimestamps": {
                    "type": "boolean",
                    "default": true,
                    "description": "Show message timestamps"
                },
                "vtcode.context7.enabled": {
                    "type": "boolean",
                    "default": true,
                    "description": "Enable Context7 MCP integration"
                },
                "vtcode.context7.autoFetchDocs": {
                    "type": "boolean",
                    "default": true,
                    "description": "Automatically fetch relevant documentation"
                }
            }
        }
    }
}
```

### 5. Build and Run

```bash
# Compile TypeScript
npm run compile

# Or watch mode
npm run watch
```

Press `F5` in VS Code to launch Extension Development Host.

## Usage

### Basic Chat

1. Open VTCode Enhanced Chat from sidebar
2. Type your message in the input field
3. Press Enter to send
4. View responses in transcript

### Commands

```
/help                    # Show all commands
/search <query>          # Search messages
/export markdown         # Export transcript
/stats                   # Show statistics
```

### Context7 Integration

Context7 automatically:

-   Detects libraries in your queries
-   Fetches relevant documentation
-   Enhances responses with context

Manual usage:

```
Ask about vscode API    # Auto-fetches VSCode docs
import * as vscode      # Detects and enhances context
```

### Export Transcript

1. Click 📥 Export button in toolbar
2. Choose format (JSON, Markdown, Text, HTML)
3. Save to desired location

## Troubleshooting

### Extension Not Loading

-   Check Output panel → VTCode Chat
-   Verify dependencies installed
-   Rebuild with `npm run compile`

### Context7 Not Working

-   Verify MCP provider in vtcode.toml
-   Check network connectivity
-   View logs in Output panel

### Messages Not Appearing

-   Check browser console (Ctrl+Shift+I)
-   Verify webview loaded
-   Check postMessage calls

## Testing

```bash
# Run unit tests
npm test

# Run with coverage
npm run test:coverage

# Lint code
npm run lint

# Format code
npm run format
```

## Development

### File Structure

```
src/
├── enhancedChatView.ts      # Main chat provider
├── context7Integration.ts   # Context7 MCP integration
├── mcpTools.ts              # MCP tool manager
├── mcpChatAdapter.ts        # MCP adapter
└── extension.ts             # Entry point

media/
├── enhanced-chat.js         # Client-side logic
└── enhanced-chat.css        # Styling
```

### Adding New Features

1. Update `enhancedChatView.ts` for server-side logic
2. Update `enhanced-chat.js` for client-side UI
3. Update `enhanced-chat.css` for styling
4. Add tests
5. Update documentation

### Debug Tips

-   Use `outputChannel.appendLine()` for logging
-   Check webview console for client errors
-   Use VS Code debugger (F5)
-   Enable verbose logging in vtcode.toml

## Next Steps

1. ✅ Test basic chat functionality
2. ✅ Verify Context7 integration
3. ✅ Test export features
4. ✅ Customize theme/styling
5. ✅ Add custom commands
6. ✅ Deploy to production

## Support

-   GitHub Issues: https://github.com/vinhnx/vtcode/issues
-   Documentation: See COMPLETE_IMPLEMENTATION_GUIDE.md
-   Discord: [Your Discord Link]

---

**Quick Reference Card**

**Keyboard Shortcuts:**

-   `Enter` - Send message
-   `Shift+Enter` - New line
-   `Ctrl+K` - Clear input
-   `Ctrl+L` - Clear transcript

**Command Prefixes:**

-   `/` - System commands
-   `@` - Agent commands
-   `#` - Tool commands

**Toolbar:**

-   🔍 Search
-   🔎 Filter
-   📥 Export
-   📦 Archive
-   🗑️ Clear
-   📊 Stats

Ready to code! 🚀
