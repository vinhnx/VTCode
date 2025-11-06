# VTCode Chat Sidebar - Architecture Diagrams

## System Architecture

```
┌───────────────────────────────────────────────────────────────────┐
│                        VS Code Window                              │
├───────────────────────────────────────────────────────────────────┤
│                                                                    │
│  ┌─────────────┐  ┌──────────────────────────────────────────┐  │
│  │  Activity   │  │        Editor Area                        │  │
│  │    Bar      │  │                                           │  │
│  │             │  │  ┌─────────────────────────────────────┐ │  │
│  │  ┌───────┐  │  │  │                                     │ │  │
│  │  │VTCode │◄─┼─►│  │      Code Editor                    │ │  │
│  │  │ Icon  │  │  │  │                                     │ │  │
│  │  └───┬───┘  │  │  └─────────────────────────────────────┘ │  │
│  │      │      │  │                                           │  │
│  └──────┼──────┘  └──────────────────────────────────────────┘  │
│         │                                                         │
│         ▼                                                         │
│  ┌─────────────────────────────────────────────────┐            │
│  │     VTCode Sidebar (Webview)                    │            │
│  ├─────────────────────────────────────────────────┤            │
│  │                                                  │            │
│  │  ┌────────────────────────────────────────┐    │            │
│  │  │      Chat Transcript                   │    │            │
│  │  │  ┌──────────────────────────────────┐  │    │            │
│  │  │  │ User: Can you help me?           │  │    │            │
│  │  │  └──────────────────────────────────┘  │    │            │
│  │  │  ┌──────────────────────────────────┐  │    │            │
│  │  │  │ Agent: Of course! What do you    │  │    │            │
│  │  │  │        need help with?           │  │    │            │
│  │  │  └──────────────────────────────────┘  │    │            │
│  │  │  ┌──────────────────────────────────┐  │    │            │
│  │  │  │ System: Tool approval requested  │  │    │            │
│  │  │  └──────────────────────────────────┘  │    │            │
│  │  └────────────────────────────────────────┘    │            │
│  │                                                  │            │
│  │  ┌────────────────────────────────────────┐    │            │
│  │  │      Thinking Indicator                │    │            │
│  │  │  ⚫⚫⚫ Agent is thinking...             │    │            │
│  │  └────────────────────────────────────────┘    │            │
│  │                                                  │            │
│  │  ┌────────────────────────────────────────┐    │            │
│  │  │      Tool Approval Panel               │    │            │
│  │  │  Tool: run_command                     │    │            │
│  │  │  Args: { command: "cargo test" }       │    │            │
│  │  │  [ ✓ Approve ]  [ ✗ Reject ]          │    │            │
│  │  └────────────────────────────────────────┘    │            │
│  │                                                  │            │
│  │  ┌────────────────────────────────────────┐    │            │
│  │  │      Input Area                        │    │            │
│  │  │  ┌──────────────────────────────────┐  │    │            │
│  │  │  │ Type your message...             │  │    │            │
│  │  │  │ (Use /, @, or # for commands)    │  │    │            │
│  │  │  └──────────────────────────────────┘  │    │            │
│  │  │  [Send] [Clear] [Cancel]               │    │            │
│  │  └────────────────────────────────────────┘    │            │
│  └─────────────────────────────────────────────────┘            │
│                                                                    │
└───────────────────────────────────────────────────────────────────┘
```

## Component Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Extension Host (Node.js)                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌────────────────────────────────────────────────────────┐    │
│  │              ChatViewProvider                          │    │
│  │  ┌──────────────────────────────────────────────────┐  │    │
│  │  │  Message Router                                  │  │    │
│  │  │  ├─ System Command Handler   (/clear, /help)    │  │    │
│  │  │  ├─ Agent Command Handler    (@analyze, @test)  │  │    │
│  │  │  └─ Tool Command Handler     (#run, #read)      │  │    │
│  │  └──────────────────────────────────────────────────┘  │    │
│  │  ┌──────────────────────────────────────────────────┐  │    │
│  │  │  State Manager                                   │  │    │
│  │  │  ├─ Transcript (TranscriptEntry[])              │  │    │
│  │  │  ├─ Pending Approvals (Map<id, resolve>)        │  │    │
│  │  │  └─ Message ID Counter                          │  │    │
│  │  └──────────────────────────────────────────────────┘  │    │
│  │  ┌──────────────────────────────────────────────────┐  │    │
│  │  │  Tool Approval Handler                           │  │    │
│  │  │  ├─ requestToolApproval()                        │  │    │
│  │  │  ├─ handleToolApproval()                         │  │    │
│  │  │  └─ executeTool()                                │  │    │
│  │  └──────────────────────────────────────────────────┘  │    │
│  └────────────────────────────────────────────────────────┘    │
│                             │                                    │
│                             ▼                                    │
│  ┌────────────────────────────────────────────────────────┐    │
│  │              VtcodeBackend                             │    │
│  │  ┌──────────────────────────────────────────────────┐  │    │
│  │  │  Process Manager                                 │  │    │
│  │  │  ├─ spawn() vtcode CLI                           │  │    │
│  │  │  ├─ Handle stdout/stderr                         │  │    │
│  │  │  └─ Cancellation support                         │  │    │
│  │  └──────────────────────────────────────────────────┘  │    │
│  │  ┌──────────────────────────────────────────────────┐  │    │
│  │  │  Response Parser                                 │  │    │
│  │  │  ├─ JSON parsing                                 │  │    │
│  │  │  ├─ Plain text fallback                          │  │    │
│  │  │  └─ Stream chunk handling                        │  │    │
│  │  └──────────────────────────────────────────────────┘  │    │
│  │  ┌──────────────────────────────────────────────────┐  │    │
│  │  │  Tool Execution                                  │  │    │
│  │  │  ├─ executeTool()                                │  │    │
│  │  │  ├─ getAvailableTools()                          │  │    │
│  │  │  └─ Result parsing                               │  │    │
│  │  └──────────────────────────────────────────────────┘  │    │
│  └────────────────────────────────────────────────────────┘    │
│                             │                                    │
│                             │ Child Process                      │
└─────────────────────────────┼────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                       vtcode CLI (Rust)                          │
├─────────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐          │
│  │ LLM Provider │  │Tool Registry │  │PTY Execution │          │
│  │ Integration  │  │              │  │              │          │
│  └──────────────┘  └──────────────┘  └──────────────┘          │
└─────────────────────────────────────────────────────────────────┘
```

## Message Flow Diagram

```
User Input Flow:
────────────────

┌──────┐
│ User │ Types message
└──┬───┘
   │ "Can you help me refactor this code?"
   ▼
┌────────────────┐
│ Webview (JS)   │ handleSend()
└───────┬────────┘
        │ postMessage({ type: "userMessage", text: "..." })
        ▼
┌────────────────────┐
│ ChatViewProvider   │ handleUserMessage()
└────────┬───────────┘
         │ Check prefix: /, @, #
         ├─ "/" → handleSystemCommand()
         ├─ "@" → handleAgentCommand()
         ├─ "#" → handleToolCommand()
         └─ else → processAgentResponse()
                    │
                    ▼
         ┌────────────────────┐
         │ VtcodeBackend      │ executePrompt()
         └──────┬─────────────┘
                │ spawn("vtcode", ["ask", "..."])
                ▼
         ┌────────────────────┐
         │ vtcode CLI         │ Process request
         └──────┬─────────────┘
                │ JSON/Text response
                ▼
         ┌────────────────────┐
         │ VtcodeBackend      │ parseVtcodeOutput()
         └──────┬─────────────┘
                │ VtcodeResponse
                ▼
┌────────────────────┐
│ ChatViewProvider   │ addToTranscript()
└────────┬───────────┘
         │ postMessage({ type: "addMessage", message: {...} })
         ▼
┌────────────────┐
│ Webview (JS)   │ createMessageElement()
└───────┬────────┘
        │ Render and append to DOM
        ▼
┌──────────────┐
│ Display to   │
│ User         │
└──────────────┘


Tool Execution Flow:
────────────────────

Agent Response contains tool_calls
        │
        ▼
┌────────────────────┐
│ ChatViewProvider   │ handleToolCalls()
└────────┬───────────┘
         │ For each tool call
         ▼
┌────────────────────┐
│ requestToolApproval│
└────────┬───────────┘
         │ Show approval UI
         │ postMessage({ type: "requestToolApproval", ... })
         ▼
┌────────────────┐
│ Webview (JS)   │ showToolApproval()
└───────┬────────┘
        │ User clicks Approve/Reject
        │ postMessage({ type: "toolApproval", approved: true/false })
        ▼
┌────────────────────┐
│ ChatViewProvider   │ handleToolApproval()
└────────┬───────────┘
         │ resolve(approved)
         ▼
┌────────────────────┐
│ executeTool()      │
└────────┬───────────┘
         │ If approved
         ▼
┌────────────────────┐
│ VtcodeBackend      │ executeTool(name, args)
└────────┬───────────┘
         │ spawn("vtcode", ["tool", "execute", ...])
         ▼
┌────────────────────┐
│ vtcode CLI         │ Execute tool
└────────┬───────────┘
         │ Tool result
         ▼
┌────────────────────┐
│ ChatViewProvider   │ addToTranscript(tool result)
└────────┬───────────┘
         │ Continue conversation
         ▼
```

## State Management

```
┌──────────────────────────────────────────────────────────────┐
│                    ChatViewProvider State                     │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  transcript: TranscriptEntry[]                                │
│  ├─ id: string                                                │
│  ├─ role: "user" | "assistant" | "system" | "tool"          │
│  ├─ content: string                                          │
│  ├─ timestamp: number                                        │
│  └─ metadata?: {                                             │
│       toolCall?: ToolCall                                    │
│       toolResult?: ToolResult                                │
│       reasoning?: string                                     │
│     }                                                         │
│                                                               │
│  pendingApprovals: Map<toolId, (approved: boolean) => void>  │
│                                                               │
│  messageIdCounter: number                                     │
│                                                               │
└──────────────────────────────────────────────────────────────┘
                            │
                            │ Persists to
                            ▼
┌──────────────────────────────────────────────────────────────┐
│                    Webview State (VS Code)                    │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  {                                                            │
│    messages: TranscriptEntry[]                                │
│  }                                                            │
│                                                               │
│  ➜ Survives webview reload                                   │
│  ➜ Persisted by VS Code                                      │
│  ➜ Retrieved via vscode.getState()                           │
│                                                               │
└──────────────────────────────────────────────────────────────┘
```

## Command Prefix Routing

```
User Input
    │
    ▼
  Starts with prefix?
    │
    ├─── "/" ────► System Commands
    │              ├─ /clear  → clearTranscript()
    │              ├─ /help   → getHelpText()
    │              ├─ /export → exportTranscript()
    │              ├─ /stats  → showStats()
    │              └─ /config → showConfig()
    │
    ├─── "@" ────► Agent Commands
    │              ├─ @analyze  → analyzeCode()
    │              ├─ @explain  → explainSelection()
    │              ├─ @refactor → refactorCode()
    │              └─ @test     → generateTests()
    │
    ├─── "#" ────► Tool Commands
    │              ├─ #run   → invokeTool("run_command")
    │              ├─ #read  → invokeTool("read_file")
    │              └─ #write → invokeTool("write_file")
    │
    └─── else ───► Regular Message
                   └─ processAgentResponse()
```

## Integration Points

```
┌──────────────────────────────────────────────────────────────┐
│                    VS Code Extension                          │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  extension.ts (activate)                                      │
│      │                                                        │
│      ├─► VtcodeTerminalManager (existing)                    │
│      │        Used for PTY command execution                 │
│      │                                                        │
│      ├─► createVtcodeBackend()                               │
│      │        Create CLI integration layer                   │
│      │                                                        │
│      └─► ChatViewProvider                                    │
│           ├─ Register webview view provider                  │
│           ├─ Register commands (clear, export)               │
│           └─ Connect to terminalManager                      │
│                                                               │
└──────────────────────────────────────────────────────────────┘
```

## Security Layers

```
┌──────────────────────────────────────────────────────────────┐
│                      Security Boundaries                      │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  Layer 1: Input Validation                                    │
│  ├─ HTML escaping in webview                                 │
│  ├─ Command argument validation                              │
│  └─ File path validation                                     │
│                                                               │
│  Layer 2: Process Isolation                                   │
│  ├─ vtcode CLI runs in separate process                     │
│  ├─ Sandboxed webview (no direct system access)             │
│  └─ Limited IPC via postMessage                              │
│                                                               │
│  Layer 3: Permission Controls                                 │
│  ├─ Tool approval required                                   │
│  ├─ Workspace trust respected                                │
│  └─ File system boundaries enforced                          │
│                                                               │
│  Layer 4: Resource Limits                                     │
│  ├─ Transcript length limits                                 │
│  ├─ Timeout for tool execution                               │
│  └─ Memory limits for transcripts                            │
│                                                               │
└──────────────────────────────────────────────────────────────┘
```

---

These diagrams provide a comprehensive visual understanding of the VTCode Chat Sidebar Extension architecture, data flow, and component interactions.
