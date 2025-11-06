# VTCode Chat Sidebar Extension

A lightweight VS Code sidebar extension that replicates the core chat loop functionality from the main CLI vtcode system.

## Features

### Core Functionality

1. **Chat Interface** - Full conversation loop with user input, agent responses, and system messages
2. **PTY Terminal Integration** - Execute commands in a pseudo-terminal environment
3. **Tool Invocation** - Seamlessly invoke predefined tools for code analysis, file operations, and more
4. **Transcript Logging** - Complete conversation history with all interactions
5. **Human-in-the-Loop** - Real-time approval and intervention for agent actions

### Command Prefixes

The extension supports special command prefixes for enhanced control:

#### System Commands (`/`)

System-level commands for managing the extension and chat session:

-   `/clear` - Clear the conversation transcript
-   `/help` - Display help information
-   `/export` - Export transcript to JSON file
-   `/stats` - Show session statistics
-   `/config` - Display current vtcode configuration

#### Agent Commands (`@`)

Agent-directed commands for code-related tasks:

-   `@analyze` - Analyze selected code
-   `@explain` - Explain selected code in detail
-   `@refactor` - Suggest code refactorings
-   `@test` - Generate unit tests for code

#### Tool Commands (`#`)

Direct tool invocations:

-   `#run command="..."` - Execute a shell command
-   `#read path="..."` - Read file contents
-   `#write path="..." content="..."` - Write content to file

## Architecture

### Components

1. **ChatViewProvider** (`src/chatView.ts`)

    - Webview-based chat interface
    - Message handling and routing
    - State management
    - Tool approval flow

2. **VtcodeBackend** (`src/vtcodeBackend.ts`)

    - CLI integration layer
    - Process management
    - Response parsing
    - Tool execution

3. **UI Layer** (`media/chat-view.js` & `media/chat-view.css`)
    - Client-side chat interface
    - Message rendering
    - User input handling
    - Approval dialogs

### State Management

The extension maintains conversation state both in memory and in VS Code's webview state:

```typescript
interface TranscriptEntry {
    id: string;
    role: "user" | "assistant" | "system" | "tool";
    content: string;
    timestamp: number;
    metadata?: {
        toolCall?: ToolCall;
        toolResult?: ToolResult;
        reasoning?: string;
    };
}
```

### Message Flow

```
User Input → Command Router → Handler
                              ├─ System Command Handler
                              ├─ Agent Command Handler
                              ├─ Tool Command Handler
                              └─ Regular Message Handler
                                   ↓
                            VTCode Backend
                                   ↓
                            Response Parser
                                   ↓
                            UI Renderer
```

## Integration with Main CLI

The extension communicates with the vtcode CLI through process spawning:

```typescript
// Single prompt execution
const response = await backend.executePrompt({
    prompt: userMessage,
    config: { model: "gemini-2.5-flash-lite" },
});

// Streaming responses
for await (const chunk of backend.streamPrompt(request)) {
    // Handle chunk
}

// Tool execution
const result = await backend.executeTool(toolName, arguments);
```

## Configuration

Add to `settings.json`:

```json
{
    "vtcode.cli.path": "vtcode",
    "vtcode.chat.autoApproveTools": false,
    "vtcode.chat.maxHistoryLength": 100,
    "vtcode.chat.enableStreaming": true
}
```

## Extension Activation

Register the chat view provider in `extension.ts`:

```typescript
import { ChatViewProvider } from "./chatView";

export function activate(context: vscode.ExtensionContext) {
    const chatProvider = new ChatViewProvider(context, terminalManager);

    context.subscriptions.push(
        vscode.window.registerWebviewViewProvider(
            ChatViewProvider.viewType,
            chatProvider
        )
    );
}
```

## Package.json Configuration

Add to `contributes` section:

```json
{
    "viewsContainers": {
        "activitybar": [
            {
                "id": "vtcode-chat",
                "title": "VTCode Chat",
                "icon": "media/chat-icon.svg"
            }
        ]
    },
    "views": {
        "vtcode-chat": [
            {
                "type": "webview",
                "id": "vtcodeChat",
                "name": "Chat"
            }
        ]
    },
    "configuration": {
        "title": "VTCode Chat",
        "properties": {
            "vtcode.chat.autoApproveTools": {
                "type": "boolean",
                "default": false,
                "description": "Automatically approve tool executions"
            },
            "vtcode.chat.maxHistoryLength": {
                "type": "number",
                "default": 100,
                "description": "Maximum number of messages to keep in history"
            },
            "vtcode.chat.enableStreaming": {
                "type": "boolean",
                "default": true,
                "description": "Enable streaming responses from the agent"
            }
        }
    }
}
```

## Development

### Building

```bash
cd vscode-extension
npm install
npm run compile
```

### Testing

```bash
npm run test
```

### Debugging

1. Open VS Code extension folder
2. Press F5 to launch Extension Development Host
3. Open chat view from activity bar
4. Test commands and interactions

## Error Handling

The extension includes comprehensive error handling:

-   **CLI Not Found**: Shows error message with installation instructions
-   **Tool Execution Failures**: Displays error in transcript with details
-   **Network Issues**: Graceful degradation with retry logic
-   **Parse Errors**: Falls back to plain text display

## Performance Considerations

-   **Lazy Loading**: Components loaded on demand
-   **Message Virtualization**: Large transcripts handled efficiently
-   **Debounced Input**: Prevents excessive API calls
-   **Background Processing**: Long operations don't block UI

## Security

-   **Workspace Trust**: Respects VS Code workspace trust settings
-   **Tool Approval**: Required for file system operations
-   **Command Validation**: Input sanitization for shell commands
-   **Sandbox Integration**: Optional Anthropic sandbox runtime support

## Future Enhancements

-   [ ] Multi-agent support
-   [ ] Context window management
-   [ ] Token usage tracking
-   [ ] Custom tool registration
-   [ ] Conversation branching
-   [ ] Export to various formats
-   [ ] Voice input support
-   [ ] Inline diff viewer
-   [ ] Code snippet suggestions
-   [ ] Integration with VS Code Chat API

## Contributing

See [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

## License

Same as main vtcode project.
