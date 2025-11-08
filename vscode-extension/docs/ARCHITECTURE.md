# VTCode Extension Architecture

**Version**: 2.0 (Phase 2)  
**Date**: November 8, 2025  
**Status**: Active Development

---

## Overview

VTCode VSCode extension follows a modular, plugin-like architecture designed for extensibility, testability, and maintainability. The architecture separates concerns into distinct layers: Command System, Participant System, State Management, and Core Services.

---

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                        VS Code Extension Host                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌──────────────────┐    ┌──────────────────┐    ┌──────────────┐  │
│  │   Command Layer  │    │  Participant     │    │    State     │  │
│  │                  │    │     Layer        │    │   Layer      │  │
│  ├──────────────────┤    ├──────────────────┤    ├──────────────┤  │
│  │ • AskCommand     │    │ • Workspace      │    │ • Message    │  │
│  │ • AnalyzeCommand │    │ • Code           │    │ • Conversation│ │
│  │ • ConfigCommand  │    │ • Terminal       │    │ • Metadata   │  │
│  │ • ... (7 total)  │    │ • Git            │    │              │  │
│  └────────┬─────────┘    └────────┬─────────┘    └──────┬───────┘  │
│           │                       │                     │          │
│           └───────────────────────┼─────────────────────┘          │
│                                   │                                │
│  ┌────────────────────────────────▼──────────────────────────────┐  │
│  │                    Core Services Layer                         │  │
│  ├────────────────────────────────────────────────────────────────┤  │
│  │ • CommandRegistry                                              │  │
│  │ • ParticipantRegistry                                          │  │
│  │ • ConversationManager                                          │  │
│  │ • VtcodeBackend (CLI Integration)                            │  │
│  │ • ChatViewProvider (UI)                                      │  │
│  └────────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │                    Utility Layer                               │  │
│  ├────────────────────────────────────────────────────────────────┤  │
│  │ • vtcodeRunner (CLI execution)                               │  │
│  │ • Error handling                                             │  │
│  │ • Configuration management                                   │  │
│  └────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────┘
```

---

## Core Components

### 1. Command System

**Purpose**: Modular command execution with standardized interfaces

**Key Files**:
- [`src/types/command.ts`](src/types/command.ts:1) - ICommand interface
- [`src/commandRegistry.ts`](src/commandRegistry.ts:1) - Command registration and management
- [`src/commands/`](src/commands/) - Individual command implementations

**Architecture**:
```typescript
interface ICommand {
    readonly id: string
    readonly title: string
    readonly description?: string
    readonly icon?: string
    
    execute(context: CommandContext): Promise<void>
    canExecute(context: CommandContext): boolean | Promise<boolean>
}
```

**Benefits**:
- **Extensibility**: New commands added by implementing ICommand
- **Testability**: Each command independently testable
- **Maintainability**: Clear separation of command logic
- **Type Safety**: Full TypeScript strict mode compliance

**Usage Example**:
```typescript
const registry = new CommandRegistry()
registry.register(new AskCommand())
registry.register(new AnalyzeCommand())
await registry.executeCommand("vtcode.askAgent", context)
```

---

### 2. Participant System

**Purpose**: Context-aware conversation enhancement with @mention support

**Key Files**:
- [`src/types/participant.ts`](src/types/participant.ts:1) - ChatParticipant interface
- [`src/participantRegistry.ts`](src/participantRegistry.ts:1) - Participant management
- [`src/participants/`](src/participants/) - Built-in participants

**Architecture**:
```typescript
interface ChatParticipant {
    readonly id: string              // "@workspace", "@code", etc.
    readonly displayName: string
    readonly description?: string
    
    canHandle(context: ParticipantContext): boolean
    resolveReferenceContext(message: string, context: ParticipantContext): Promise<string>
}
```

**Built-in Participants**:
- **@workspace**: Workspace-wide context, file statistics
- **@code**: Code-specific analysis, syntax information
- **@terminal**: Terminal context, command history
- **@git**: Git repository context, branch information

**Usage Example**:
```typescript
// User types: "@workspace what files are in this project?"
const enhancedMessage = await participantRegistry.resolveContext(
    "what files are in this project?",
    participantContext
)
// Result includes workspace statistics and file listings
```

---

### 3. State Management

**Purpose**: Enhanced message tracking and conversation lifecycle management

**Key Files**:
- [`src/types/message.ts`](src/types/message.ts:1) - Message and conversation interfaces
- [`src/conversation/conversationManager.ts`](src/conversation/conversationManager.ts:1) - Conversation state management

**Architecture**:
```typescript
interface ChatMessage {
    readonly id: string
    readonly role: 'user' | 'assistant' | 'system' | 'tool' | 'error'
    readonly content: string
    readonly timestamp: number
    readonly metadata?: {
        model?: string
        tokens?: { input: number; output: number }
        participantId?: string
        toolCalls?: ToolCall[]
    }
    readonly state: 'pending' | 'complete' | 'error'
}

interface Conversation {
    readonly id: string
    readonly messages: ChatMessage[]
    readonly metadata: ConversationMetadata
}
```

**Features**:
- **Message Lifecycle**: Pending → Complete/Error states
- **Token Tracking**: Input/output token counting
- **Participant Tracking**: Which participants contributed context
- **Conversation History**: Automatic conversation management
- **Export/Import**: JSON serialization for persistence

---

### 4. Core Services

#### CommandRegistry
- **Manages command lifecycle**: Registration, execution, error handling
- **Provides discovery**: List available commands, get command metadata
- **Handles availability**: Check if commands can execute in current context

#### ParticipantRegistry
- **Resolves @mentions**: Processes messages for participant references
- **Combines contexts**: Merges multiple participant contexts
- **Provides suggestions**: Autocomplete for @mentions

#### ConversationManager
- **Manages conversation state**: Current conversation, message history
- **Tracks metadata**: Tokens, participants, timestamps
- **Provides statistics**: Message counts, token usage
- **Handles persistence**: Save/load conversations (future enhancement)

#### VtcodeBackend
- **CLI integration**: Spawns VTCode CLI processes
- **Streaming support**: Real-time response streaming
- **Tool execution**: Handles tool calls and approvals
- **Error handling**: Normalizes CLI errors

#### ChatViewProvider
- **Webview management**: Chat UI in VS Code
- **Message rendering**: Displays messages with markdown support
- **Tool approval UI**: Human-in-the-loop tool approval
- **Status indicators**: Real-time status updates

---

## Data Flow

### Command Execution Flow
```
1. User triggers command (keyboard, menu, command palette)
2. CommandRegistry receives execution request
3. Registry validates command exists and canExecute()
4. Command.execute() runs with context
5. Command uses vtcodeRunner for CLI execution
6. Result displayed to user via VS Code notifications
7. Errors caught and displayed as error messages
```

### Participant Context Flow
```
1. User sends message with @mention (e.g., "@workspace list files")
2. ChatViewProvider receives message
3. buildParticipantContext() gathers IDE state
4. participantRegistry.resolveContext() processes @mentions
5. Each participant adds context if mentioned
6. Enhanced message sent to backend with context
7. Response streamed back to user
```

### Conversation Management Flow
```
1. New conversation created on extension activation
2. Messages added with createMessage()/createPendingMessage()
3. Metadata updated (tokens, participants, title)
4. Message history maintained for context building
5. Conversation can be cleared/exported
6. Future: Persistence to disk
```

---

## Extension Points

### Adding New Commands
```typescript
// 1. Create command class
class MyCommand implements ICommand {
    readonly id = "vtcode.myCommand"
    readonly title = "My Command"
    
    async execute(context: CommandContext): Promise<void> {
        // Implementation
    }
}

// 2. Register in extension.ts
commandRegistry.register(new MyCommand())
```

### Adding New Participants
```typescript
// 1. Create participant class
class MyParticipant extends BaseParticipant {
    readonly id = "myParticipant"
    readonly displayName = "My Participant"
    
    canHandle(context: ParticipantContext): boolean {
        // Check if participant can provide context
    }
    
    async resolveReferenceContext(message: string, context: ParticipantContext): Promise<string> {
        // Add participant-specific context
    }
}

// 2. Register in extension.ts
participantRegistry.register(new MyParticipant())
```

---

## Testing Strategy

### Unit Tests
- **Commands**: Test execute(), canExecute(), error handling
- **Participants**: Test context resolution, @mention handling
- **Registries**: Test registration, lookup, error cases
- **Utilities**: Test CLI execution, error formatting

### Integration Tests
- **Command flow**: End-to-end command execution
- **Participant flow**: @mention resolution with multiple participants
- **Conversation flow**: Message lifecycle, metadata updates
- **Error scenarios**: CLI failures, network issues, user cancellations

### Test Files
- [`src/commands/__tests__/*.test.ts`](src/commands/__tests__/) - Command unit tests
- [`src/participants/__tests__/*.test.ts`](src/participants/__tests__/) - Participant unit tests
- [`src/conversation/__tests__/*.test.ts`](src/conversation/__tests__/) - Conversation tests
- [`src/__tests__/integration/*.test.ts`](src/__tests__/integration/) - Integration tests

---

## Performance Considerations

### Command System
- **Lazy loading**: Commands instantiated on first use
- **Registry caching**: Command lookups cached after registration
- **Error handling**: Fast failure for unavailable commands

### Participant System
- **Parallel resolution**: Multiple participants resolve concurrently
- **Context caching**: IDE state cached to avoid repeated queries
- **Timeout handling**: Participant resolution with timeouts

### State Management
- **In-memory storage**: Messages stored in memory for performance
- **History limits**: Maximum message count to prevent memory leaks
- **Lazy metadata updates**: Metadata updated incrementally

---

## Security Considerations

### Command Execution
- **Workspace trust**: Commands require trusted workspace
- **CLI validation**: Command paths validated before execution
- **User approval**: Destructive operations require explicit approval

### Participant Context
- **Path validation**: File paths checked against workspace
- **Content limits**: Large files truncated to prevent token overflow
- **Sensitive data**: No sensitive data included in context

### Tool Execution
- **Human-in-the-loop**: All tool calls require user approval
- **Command validation**: Shell commands validated before execution
- **Timeout protection**: Long-running commands automatically terminated

---

## Future Enhancements

### Phase 3 (Weeks 7-9)
- **Conversation persistence**: Save/load conversations from disk
- **Tool approval UI**: Enhanced UI for tool approval
- **Streaming improvements**: Better streaming performance
- **MCP enhancements**: Improved MCP integration

### Phase 4 (Weeks 10-12)
- **Performance optimization**: Faster startup, reduced memory usage
- **Security hardening**: Additional security checks
- **Marketplace readiness**: Compliance with VS Code marketplace requirements

---

## Development Guidelines

### Code Style
- **TypeScript strict mode**: All code must pass strict type checking
- **Error handling**: Use try/catch with proper error messages
- **Documentation**: All public APIs must have JSDoc comments
- **Testing**: All new features require unit tests (>90% coverage)

### File Organization
- **Commands**: `src/commands/` - One file per command
- **Participants**: `src/participants/` - One file per participant
- **Types**: `src/types/` - Shared interfaces and types
- **Tests**: Mirror source structure in `__tests__/` directories

### Naming Conventions
- **Commands**: `VerbNounCommand` (e.g., `AskCommand`, `AnalyzeCommand`)
- **Participants**: `NounParticipant` (e.g., `WorkspaceParticipant`)
- **Interfaces**: `ICommand`, `ChatParticipant` (prefix with I for interfaces)
- **Files**: `kebab-case.ts` for files, `PascalCase` for classes

---

## References

- [VS Code Extension API](https://code.visualstudio.com/api)
- [VS Code Chat API](https://code.visualstudio.com/api/references/vscode-api#chat)
- [Model Context Protocol](https://modelcontextprotocol.io/)
- [VTCode CLI Documentation](https://github.com/vinhnx/vtcode)

---

**Document Version**: 2.0  
**Last Updated**: November 8, 2025  
**Maintained By**: VTCode Development Team