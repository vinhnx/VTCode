# Phase 2 Developer Guide

**Quick Reference for Phase 2 Implementation**

---

## Getting Started

### 1. Understand the Architecture

**Command System**: Modular, type-safe command handling

```typescript
// Command interface
interface ICommand {
  id: string                                    // Unique ID
  title: string                                 // Display name
  execute(context: CommandContext): Promise<void>  // Execute logic
  canExecute(context: CommandContext): boolean  // Can execute?
}

// Registry manages all commands
CommandRegistry {
  register(command)          // Add a command
  registerAll(context)       // Register with VS Code
  getCommand(id)            // Get by ID
}
```

**Participant System**: Context-aware conversation providers

```typescript
// Participant interface
interface ChatParticipant {
  id: string                                  // Unique ID (@workspace)
  displayName: string                         // UI display
  canHandle(context): boolean                 // Can provide value?
  resolveReferenceContext(message, context)   // Add context
}

// Registry manages all participants
ParticipantRegistry {
  register(participant)       // Add participant
  parseMentions(message)      // Find @mentions
  resolveParticipant(id, msg) // Get context for @mention
}
```

---

## Implementing a Command

### Step 1: Create the File

```bash
touch src/commands/myCommand.ts
touch src/commands/myCommand.test.ts
```

### Step 2: Implement the Interface

```typescript
import { ICommand, CommandContext } from "../types/command";
import { VtcodeBackend } from "../vtcodeBackend";

export class MyCommand implements ICommand {
    // Required properties
    readonly id = "vtcode.mycommand";
    readonly title = "My Command";
    readonly description = "What this does";
    readonly icon = "lightbulb"; // VS Code theme icon

    constructor(private backend: VtcodeBackend) {}

    // Required methods
    canExecute(context: CommandContext): boolean {
        // Return true if command can run now
        // e.g., return !!context.activeTextEditor
        return true;
    }

    async execute(context: CommandContext): Promise<void> {
        // Your command logic here
        try {
            // Do something
            vscode.window.showInformationMessage("Done!");
        } catch (error) {
            const msg = error instanceof Error ? error.message : String(error);
            vscode.window.showErrorMessage(`Error: ${msg}`);
        }
    }
}
```

### Step 3: Write Tests

```typescript
import { describe, it, expect, beforeEach, vi } from "vitest";
import { MyCommand } from "./myCommand";

describe("MyCommand", () => {
    let command: MyCommand;

    beforeEach(() => {
        command = new MyCommand(mockBackend);
    });

    it("should have correct metadata", () => {
        expect(command.id).toBe("vtcode.mycommand");
        expect(command.title).toBeDefined();
    });

    it("should execute successfully", async () => {
        const context = { trusted: true };
        await expect(command.execute(context)).resolves.not.toThrow();
    });
});
```

### Step 4: Export from Index

```typescript
// src/commands/index.ts
export { MyCommand } from "./myCommand";
```

### Step 5: Register in Extension

```typescript
// src/extension.ts
import { CommandRegistry } from "./commandRegistry";
import { MyCommand } from "./commands";

export function activate(context: vscode.ExtensionContext) {
    const registry = new CommandRegistry();

    // Register all commands
    registry.register(new MyCommand(backend));
    registry.registerAll(context);
}
```

---

## Implementing a Participant

### Step 1: Create the File

```bash
touch src/participants/myParticipant.ts
touch src/participants/myParticipant.test.ts
```

### Step 2: Implement the Interface

```typescript
import { ChatParticipant, ParticipantContext } from "../types/participant";

export class MyParticipant implements ChatParticipant {
    readonly id = "workspace";
    readonly displayName = "@workspace";
    readonly description = "Workspace context provider";
    readonly icon = "folder";

    canHandle(context: ParticipantContext): boolean {
        // Return true if this participant can provide value
        return !!context.workspace;
    }

    async resolveReferenceContext(
        message: string,
        context: ParticipantContext
    ): Promise<string> {
        // Return additional context to append to message
        if (!message.includes("@workspace")) {
            return "";
        }

        // Build context
        const workspaceName = context.workspace?.name || "Unknown";
        const fileCount = 42; // Get actual count

        return `
Workspace Context:
- Name: ${workspaceName}
- Files: ${fileCount}
- Root: ${context.workspace?.uri.fsPath}
    `.trim();
    }
}
```

### Step 3: Write Tests

```typescript
describe("MyParticipant", () => {
    let participant: MyParticipant;

    beforeEach(() => {
        participant = new MyParticipant();
    });

    it("should have correct metadata", () => {
        expect(participant.id).toBe("workspace");
        expect(participant.displayName).toBe("@workspace");
    });

    it("should resolve context when @mentioned", async () => {
        const context = { workspace: { name: "MyProject" } };
        const result = await participant.resolveReferenceContext(
            "@workspace",
            context
        );
        expect(result).toContain("MyProject");
    });
});
```

### Step 4: Export and Register

```typescript
// src/participants/index.ts
export { MyParticipant } from "./myParticipant";

// src/extension.ts
import { ParticipantRegistry } from "./participantRegistry";
import { MyParticipant } from "./participants";

export function activate(context: vscode.ExtensionContext) {
    const registry = new ParticipantRegistry();
    registry.register(new MyParticipant());
    // Later: use registry.parseMentions(message)
}
```

---

## Testing Guidelines

### Unit Test Structure

```typescript
describe("ComponentName", () => {
    let component: ComponentName;
    let mock: any;

    beforeEach(() => {
        // Setup
        mock = {
            /* mock objects */
        };
        component = new ComponentName(mock);
    });

    it("should do X", () => {
        // Test
    });

    afterEach(() => {
        vi.clearAllMocks();
    });
});
```

### Common Test Patterns

**Testing canExecute()**

```typescript
it("should not execute without workspace", () => {
    const context = { workspace: undefined };
    expect(command.canExecute(context)).toBe(false);
});
```

**Testing async execute()**

```typescript
it("should execute successfully", async () => {
    const context = { workspace: mockWorkspace };
    await expect(command.execute(context)).resolves.not.toThrow();
    expect(mockBackend.doSomething).toHaveBeenCalled();
});
```

**Mocking VS Code API**

```typescript
import { vi } from "vitest";
import * as vscode from "vscode";

const showInputBox = vi
    .spyOn(vscode.window, "showInputBox")
    .mockResolvedValue("user input");
```

---

## CommandContext Reference

```typescript
interface CommandContext {
    workspaceFolder?: vscode.WorkspaceFolder; // Current workspace
    activeTextEditor?: vscode.TextEditor; // Active editor
    selection?: vscode.Selection; // Text selection
    terminal?: vscode.Terminal; // Active terminal
    trusted: boolean; // Workspace trusted?
}
```

**Usage Examples**:

```typescript
// Check if we have selected text
if (context.activeTextEditor && !context.selection?.isEmpty) {
    const text = context.activeTextEditor.document.getText(context.selection);
}

// Check workspace trust
if (!context.trusted) {
    throw new Error("Workspace not trusted");
}

// Get workspace folder
const folder = context.workspaceFolder?.uri.fsPath;
```

---

## ParticipantContext Reference

```typescript
interface ParticipantContext {
    activeFile?: {
        path: string;
        language: string;
        content?: string;
        selection?: vscode.Range;
    };
    workspace?: vscode.WorkspaceFolder;
    terminal?: {
        output: string;
        cwd: string;
    };
    git?: {
        branch: string;
        changes: string[];
        status?: string;
    };
}
```

**Usage Examples**:

```typescript
// Get selected code
if (context.activeFile?.selection) {
    const range = context.activeFile.selection;
    // Use range for context
}

// Get git branch
if (context.git?.branch) {
    return `Working on branch: ${context.git.branch}`;
}
```

---

## Common Patterns

### Pattern 1: Show Input Box

```typescript
const input = await vscode.window.showInputBox({
    prompt: "Your question?",
    placeHolder: "Example...",
    ignoreFocusOut: true,
});

if (!input?.trim()) {
    return; // User cancelled
}
```

### Pattern 2: Show Error Message

```typescript
try {
    // Do something
} catch (error) {
    const msg = error instanceof Error ? error.message : String(error);
    void vscode.window.showErrorMessage(`Failed: ${msg}`);
}
```

### Pattern 3: Check File Extension

```typescript
const fileName = context.activeTextEditor?.document.fileName;
if (fileName?.endsWith(".rs")) {
    // Handle Rust file
}
```

### Pattern 4: Get Workspace Root

```typescript
const root = context.workspaceFolder?.uri.fsPath;
const path = vscode.workspace.asRelativePath(uri);
```

---

## File Organization

### Suggested Structure

```
src/
 types/              # Interfaces and types
    command.ts
    participant.ts
    index.ts
 commands/           # Command implementations
    askCommand.ts
    myCommand.ts
    index.ts
 participants/       # Participant implementations
    workspaceParticipant.ts
    myParticipant.ts
    index.ts
 commandRegistry.ts  # Command registry
 participantRegistry.ts # Participant registry
 extension.ts        # Main extension file
```

---

## Debugging Tips

### Enable Debug Output

```typescript
const outputChannel = vscode.window.createOutputChannel("VT Code Debug");
outputChannel.appendLine(`[debug] Command executed: ${context}`);
```

### Log Command Execution

```typescript
async execute(context: CommandContext): Promise<void> {
  console.log(`Executing ${this.id}`, context);
  try {
    // Do work
  } catch (error) {
    console.error(`Error in ${this.id}:`, error);
    throw error;
  }
}
```

### Test Registries

```typescript
// Check what's registered
const allCommands = registry.getAllCommands();
console.log(
    "Registered commands:",
    allCommands.map((c) => c.id)
);

const applicable = registry.getApplicableParticipants(context);
console.log(
    "Applicable participants:",
    applicable.map((p) => p.id)
);
```

---

## Checklist for New Command

-   [ ] File created: `src/commands/myCommand.ts`
-   [ ] Class implements `ICommand` interface
-   [ ] All required properties defined (id, title, execute, canExecute)
-   [ ] execute() handles errors gracefully
-   [ ] canExecute() checks prerequisites
-   [ ] Test file created: `src/commands/myCommand.test.ts`
-   [ ] Tests cover happy path and error cases
-   [ ] Exported in `src/commands/index.ts`
-   [ ] Registered in `extension.ts`
-   [ ] No breaking changes
-   [ ] JSDoc comments added
-   [ ] Tests pass: `npm test`
-   [ ] No linting errors: `npm run lint`

---

## Checklist for New Participant

-   [ ] File created: `src/participants/myParticipant.ts`
-   [ ] Class implements `ChatParticipant` interface
-   [ ] All required properties defined (id, displayName, canHandle, resolveReferenceContext)
-   [ ] canHandle() returns boolean
-   [ ] resolveReferenceContext() returns string promise
-   [ ] Test file created
-   [ ] Tests cover multiple scenarios
-   [ ] Exported in `src/participants/index.ts`
-   [ ] Registered in `extension.ts`
-   [ ] No breaking changes
-   [ ] JSDoc comments added
-   [ ] Tests pass: `npm test`
-   [ ] No linting errors: `npm run lint`

---

## Quick Command Reference

```bash
# Run tests
npm test

# Run specific test
npm test -- myCommand.test.ts

# Lint
npm run lint

# Format
npm run format

# Build
npm run compile

# Run extension
npm run watch
```

---

## Resources

-   [VS Code Extension API](https://code.visualstudio.com/api)
-   [Phase 2 Implementation Plan](./PHASE_2_IMPLEMENTATION_PLAN.md)
-   [Phase 2 Quick Start](./PHASE_2_QUICK_START.md)
-   [AskCommand Example](../src/commands/askCommand.ts)

---

**Version**: 1.0
**Status**: Active
**Last Updated**: November 8, 2025
