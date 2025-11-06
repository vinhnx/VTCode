# MCP Integration Guide for VTCode Chat Extension

## Overview

This guide shows how to integrate Model Context Protocol (MCP) tools into the VTCode Chat Extension, enabling enhanced functionality through external tool providers.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                  VTCode Chat Extension                       │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ChatViewProvider                                            │
│        │                                                     │
│        ├─► VtcodeBackend (CLI Integration)                  │
│        │        ├─► Tool Execution                          │
│        │        └─► MCP Tool Invocation                     │
│        │                                                     │
│        └─► McpToolManager                                   │
│                 ├─► Provider Discovery                      │
│                 ├─► Tool Registry                           │
│                 └─► Tool Execution                          │
│                                                              │
└─────────────────────────────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────┐
│              MCP Providers (External)                        │
├─────────────────────────────────────────────────────────────┤
│  • GitHub MCP                                               │
│  • Filesystem MCP                                           │
│  • Database MCP                                             │
│  • Custom MCP Servers                                       │
└─────────────────────────────────────────────────────────────┘
```

## Configuration

### vtcode.toml Configuration

Add MCP providers to your `vtcode.toml`:

```toml
[mcp]
enabled = true
max_concurrent_connections = 5
request_timeout_seconds = 30
max_retries = 3

[[mcp.providers]]
name = "github"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
enabled = true
env = { GITHUB_TOKEN = "env:GITHUB_TOKEN" }

[[mcp.providers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/workspace"]
enabled = true

[[mcp.providers]]
name = "postgres"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-postgres"]
enabled = true
env = { DATABASE_URL = "env:DATABASE_URL" }
```

## Implementation Steps

### 1. MCP Tool Manager (mcpTools.ts)

The `McpToolManager` class handles:

-   Loading providers from configuration
-   Discovering available tools
-   Executing tool invocations
-   Managing provider lifecycle

Key methods:

```typescript
// Load providers from vtcode.toml
await mcpManager.loadProviders(workspaceRoot);

// Get available tools
const tools = mcpManager.getAvailableTools();

// Invoke a tool
const result = await mcpManager.invokeTool({
    provider: "github",
    tool: "create_issue",
    arguments: {
        repo: "user/repo",
        title: "Bug report",
        body: "Description...",
    },
});
```

### 2. Enhanced Backend Integration

Update `vtcodeBackend.ts` to support MCP tools:

```typescript
import { McpToolManager } from "./mcpTools";

export class VtcodeBackend {
    private mcpManager?: McpToolManager;

    constructor(
        vtcodePath: string,
        workspaceRoot: string | undefined,
        outputChannel: vscode.OutputChannel,
        mcpManager?: McpToolManager
    ) {
        this.vtcodePath = vtcodePath;
        this.workspaceRoot = workspaceRoot;
        this.outputChannel = outputChannel;
        this.mcpManager = mcpManager;
    }

    async executeTool(
        toolName: string,
        args: Record<string, unknown>
    ): Promise<unknown> {
        // Check if it's an MCP tool
        if (toolName.includes("/") && this.mcpManager) {
            const [provider, tool] = toolName.split("/", 2);
            const result = await this.mcpManager.invokeTool({
                provider,
                tool,
                arguments: args,
            });

            if (!result.success) {
                throw new Error(result.error || "MCP tool failed");
            }

            return result.result;
        }

        // Fall back to vtcode CLI tools
        return this.invokeVtcodeTool(toolName, args);
    }
}
```

### 3. Chat View Commands

Add MCP commands to the chat interface:

#### System Commands

```
/mcp list         - List all available MCP tools
/mcp providers    - Show active MCP providers
/mcp reload       - Reload MCP configuration
```

#### Tool Invocations

```
#github/create_issue repo="user/repo" title="Bug" body="Description"
#filesystem/read_file path="/path/to/file"
#postgres/query sql="SELECT * FROM users"
```

### 4. Extension Activation

Update `extension.ts` to initialize MCP:

```typescript
import { createMcpToolManager } from "./mcpTools";
import { ChatViewProvider } from "./chatView";

export async function activate(context: vscode.ExtensionContext) {
    // ... existing code ...

    if (terminalManager) {
        const outputChannel = vscode.window.createOutputChannel("VTCode");

        // Create MCP tool manager
        const mcpManager = await createMcpToolManager(outputChannel);

        if (mcpManager) {
            const tools = mcpManager.getAvailableTools();
            outputChannel.appendLine(`[MCP] Loaded ${tools.length} tools`);
        }

        // Create backend with MCP support
        const backend = new VtcodeBackend(
            vtcodePath,
            workspaceRoot,
            outputChannel,
            mcpManager
        );

        // Create chat provider
        const chatProvider = new ChatViewProvider(
            context,
            terminalManager,
            backend
        );

        context.subscriptions.push(
            vscode.window.registerWebviewViewProvider(
                ChatViewProvider.viewType,
                chatProvider
            )
        );
    }
}
```

## MCP Tool Examples

### GitHub Integration

```typescript
// List repositories
const repos = await mcpManager.invokeTool({
    provider: "github",
    tool: "list_repos",
    arguments: { org: "organization" },
});

// Create issue
const issue = await mcpManager.invokeTool({
    provider: "github",
    tool: "create_issue",
    arguments: {
        repo: "user/repo",
        title: "Feature request",
        body: "Please add this feature...",
        labels: ["enhancement"],
    },
});

// Get pull requests
const prs = await mcpManager.invokeTool({
    provider: "github",
    tool: "list_pull_requests",
    arguments: {
        repo: "user/repo",
        state: "open",
    },
});
```

### Filesystem Integration

```typescript
// Read file
const content = await mcpManager.invokeTool({
    provider: "filesystem",
    tool: "read_file",
    arguments: { path: "./src/main.rs" },
});

// Write file
await mcpManager.invokeTool({
    provider: "filesystem",
    tool: "write_file",
    arguments: {
        path: "./output.txt",
        content: "Hello, World!",
    },
});

// List directory
const files = await mcpManager.invokeTool({
    provider: "filesystem",
    tool: "list_directory",
    arguments: { path: "./src" },
});
```

### Database Integration

```typescript
// Execute query
const results = await mcpManager.invokeTool({
    provider: "postgres",
    tool: "query",
    arguments: {
        sql: "SELECT * FROM users WHERE active = true",
    },
});

// Get schema
const schema = await mcpManager.invokeTool({
    provider: "postgres",
    tool: "describe_table",
    arguments: { table: "users" },
});
```

## Security Considerations

### Tool Approval Flow

Always request user approval for MCP tool invocations:

```typescript
// Before executing MCP tool
const approved = await this.requestToolApproval({
    id: `mcp_${Date.now()}`,
    name: `${provider}/${tool}`,
    arguments: args,
});

if (!approved) {
    throw new Error("Tool execution denied by user");
}

// Execute tool
const result = await mcpManager.invokeTool(invocation);
```

### Permission Scoping

Configure tool permissions in `vtcode.toml`:

```toml
[tools.policy]
require_approval = true
allow_list = [
    "github/*",
    "filesystem/read_file",
    "filesystem/list_directory"
]
deny_list = [
    "filesystem/delete_file",
    "postgres/execute_update"
]
```

## Error Handling

Implement robust error handling for MCP operations:

```typescript
try {
    const result = await mcpManager.invokeTool(invocation);

    if (!result.success) {
        // Handle tool-specific error
        this.sendSystemMessage(`Tool failed: ${result.error}`, "error");
        return;
    }

    // Process successful result
    this.displayToolResult(result.result);
} catch (error) {
    // Handle system-level error
    this.sendSystemMessage(
        `MCP error: ${error instanceof Error ? error.message : String(error)}`,
        "error"
    );
}
```

## Testing

### Unit Tests

```typescript
import { McpToolManager } from "../src/mcpTools";

describe("McpToolManager", () => {
    it("should load providers from config", async () => {
        const manager = new McpToolManager(outputChannel);
        await manager.loadProviders("./test/fixtures");

        const providers = manager.getAvailableTools();
        expect(providers.length).toBeGreaterThan(0);
    });

    it("should execute tool successfully", async () => {
        const result = await manager.invokeTool({
            provider: "test",
            tool: "echo",
            arguments: { message: "Hello" },
        });

        expect(result.success).toBe(true);
        expect(result.result).toBe("Hello");
    });
});
```

### Integration Tests

```typescript
describe("Chat with MCP", () => {
    it("should list MCP tools", async () => {
        await chatProvider.handleUserMessage("/mcp list");

        const lastMessage = getLastTranscriptEntry();
        expect(lastMessage.content).toContain("Available MCP Tools");
    });

    it("should invoke MCP tool via chat", async () => {
        await chatProvider.handleUserMessage('#github/list_repos org="test"');

        const lastMessage = getLastTranscriptEntry();
        expect(lastMessage.role).toBe("tool");
    });
});
```

## Performance Optimization

### Tool Caching

Cache frequently accessed tool results:

```typescript
private toolCache = new Map<string, { result: unknown; timestamp: number }>();
private CACHE_TTL = 60000; // 1 minute

async invokeTool(invocation: McpToolInvocation): Promise<McpToolResult> {
    const cacheKey = `${invocation.provider}/${invocation.tool}/${
        JSON.stringify(invocation.arguments)
    }`;

    const cached = this.toolCache.get(cacheKey);
    if (cached && Date.now() - cached.timestamp < this.CACHE_TTL) {
        return {
            success: true,
            result: cached.result,
            executionTimeMs: 0
        };
    }

    const result = await this.executeToolViaProvider(...);

    if (result.success) {
        this.toolCache.set(cacheKey, {
            result: result.result,
            timestamp: Date.now()
        });
    }

    return result;
}
```

### Concurrent Execution

Execute independent tools in parallel:

```typescript
async executeMultipleTools(
    invocations: McpToolInvocation[]
): Promise<McpToolResult[]> {
    const promises = invocations.map(inv =>
        this.invokeTool(inv)
    );

    return Promise.all(promises);
}
```

## Monitoring & Observability

### Metrics Collection

```typescript
interface McpMetrics {
    totalInvocations: number;
    successfulInvocations: number;
    failedInvocations: number;
    averageExecutionTimeMs: number;
    toolUsage: Map<string, number>;
}

class McpToolManager {
    private metrics: McpMetrics = {
        totalInvocations: 0,
        successfulInvocations: 0,
        failedInvocations: 0,
        averageExecutionTimeMs: 0,
        toolUsage: new Map()
    };

    async invokeTool(invocation: McpToolInvocation): Promise<McpToolResult> {
        this.metrics.totalInvocations++;

        const toolKey = `${invocation.provider}/${invocation.tool}`;
        this.metrics.toolUsage.set(
            toolKey,
            (this.metrics.toolUsage.get(toolKey) || 0) + 1
        );

        const result = await this.executeToolViaProvider(...);

        if (result.success) {
            this.metrics.successfulInvocations++;
        } else {
            this.metrics.failedInvocations++;
        }

        // Update average execution time
        const total = this.metrics.averageExecutionTimeMs *
                     (this.metrics.totalInvocations - 1);
        this.metrics.averageExecutionTimeMs =
            (total + result.executionTimeMs) / this.metrics.totalInvocations;

        return result;
    }
}
```

## Troubleshooting

### Common Issues

**Problem**: MCP tools not discovered
**Solution**: Check provider configuration and ensure commands are in PATH

**Problem**: Tool invocation times out
**Solution**: Increase `request_timeout_seconds` in configuration

**Problem**: Permission denied errors
**Solution**: Verify environment variables and API tokens are set correctly

## Future Enhancements

-   [ ] MCP tool auto-discovery from marketplace
-   [ ] Visual tool configuration editor
-   [ ] Tool composition and chaining
-   [ ] Streaming tool outputs
-   [ ] Tool result caching strategies
-   [ ] Multi-provider tool orchestration

## Resources

-   [MCP Specification](https://modelcontextprotocol.io)
-   [MCP Server Implementations](https://github.com/modelcontextprotocol)
-   [VTCode MCP Documentation](../docs/mcp_enhancements.md)

---

This integration enables the VTCode Chat Extension to leverage the full power of the Model Context Protocol ecosystem, providing seamless access to external tools and services.
