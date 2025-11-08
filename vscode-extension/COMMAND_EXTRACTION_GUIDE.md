# Command Extraction Guide for Team

**Quick Reference for Extracting More Commands**

---

## Overview

All commands in the VTCode extension should follow the `ICommand` interface and use dependency injection. This guide shows you how to extract a command from `extension.ts` into its own modular file.

---

## Quick Start: 5 Step Process

### Step 1: Choose a Command
Pick a command from `extension.ts` that hasn't been extracted yet. Example: `openChat`

### Step 2: Create the Command Class
Create a new file: `src/commands/openChatCommand.ts`

```typescript
import * as vscode from "vscode";
import { ICommand, CommandContext } from "../types/command";

/**
 * Command: Open Chat View
 * [Description of what the command does]
 */
export class OpenChatCommand implements ICommand {
	// Required properties from ICommand
	readonly id = "vtcode.openChat";
	readonly title = "Open Chat";
	readonly description = "Open the VTCode chat view";
	readonly icon = "comment";

	// Dependencies (if needed)
	constructor(private chatViewProvider: ChatViewProvider) {}

	// Execute the command
	async execute(context: CommandContext): Promise<void> {
		try {
			// Command logic here
			await this.chatViewProvider.reveal();
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			void vscode.window.showErrorMessage(`Failed to open chat: ${message}`);
		}
	}

	// Check if command can run
	canExecute(context: CommandContext): boolean {
		return true; // Add guards as needed
	}
}
```

### Step 3: Create Tests
Create a test file: `src/commands/openChatCommand.test.ts`

```typescript
import { describe, it, expect, beforeEach, vi } from "vitest";
import * as vscode from "vscode";
import { OpenChatCommand } from "./openChatCommand";

describe("OpenChatCommand", () => {
	let command: OpenChatCommand;
	let mockProvider: any;

	beforeEach(() => {
		mockProvider = {
			reveal: vi.fn(),
		};
		command = new OpenChatCommand(mockProvider);
	});

	it("should have correct id and title", () => {
		expect(command.id).toBe("vtcode.openChat");
		expect(command.title).toBe("Open Chat");
	});

	it("should execute reveal on provider", async () => {
		await command.execute({
			workspaceFolder: undefined,
			activeTextEditor: undefined,
			selection: undefined,
			terminal: undefined,
			trusted: true,
		});
		expect(mockProvider.reveal).toHaveBeenCalled();
	});
});
```

### Step 4: Update Exports
Update `src/commands/index.ts`:

```typescript
export { OpenChatCommand } from "./openChatCommand";
```

### Step 5: Register in CommandRegistry
The command will be registered in `extension.ts` when the `CommandRegistry` is fully initialized.

---

## Implementation Patterns

### Pattern 1: Simple Command (No Dependencies)

```typescript
export class SimpleCommand implements ICommand {
	readonly id = "vtcode.example";
	readonly title = "Example";
	
	async execute(): Promise<void> {
		void vscode.window.showInformationMessage("Done!");
	}
	
	canExecute(): boolean {
		return true;
	}
}
```

### Pattern 2: Command with Dependencies

```typescript
export class DependentCommand implements ICommand {
	readonly id = "vtcode.example";
	readonly title = "Example";
	
	constructor(
		private backend: VtcodeBackend,
		private executeCommand: (args: string[]) => Promise<void>
	) {}
	
	async execute(context: CommandContext): Promise<void> {
		// Use injected dependencies
		await this.executeCommand(["analyze"]);
	}
	
	canExecute(context: CommandContext): boolean {
		return context.trusted === true;
	}
}
```

### Pattern 3: Command with Validation

```typescript
export class ValidatingCommand implements ICommand {
	readonly id = "vtcode.example";
	readonly title = "Example";
	
	async execute(context: CommandContext): Promise<void> {
		// Validate prerequisites
		if (!context.activeTextEditor) {
			void vscode.window.showWarningMessage("Please open an editor first");
			return;
		}
		
		// Execute logic
	}
	
	canExecute(context: CommandContext): boolean {
		return context.activeTextEditor !== undefined;
	}
}
```

---

## Common Commands to Extract

### High Priority (User-Facing)
1. **openChat** - Open chat view
2. **toggleHumanInTheLoop** - Toggle approval mode
3. **openDocumentation** - Open user docs
4. **openWalkthrough** - Show setup walkthrough
5. **launchAgentTerminal** - Launch terminal

### Medium Priority (Configuration)
1. **configureMcpProviders** - MCP configuration
2. **openDeepWiki** - Internal wiki
3. **openInstallGuide** - Installation help
4. **flushIdeContext** - Flush context snapshot

### Lower Priority (UI Management)
1. **refreshQuickActions** - Refresh UI
2. **verifyWorkspaceTrust** - Verify trust status

---

## Dependency Injection Patterns

### How to Identify Dependencies

Look at what the command uses in `extension.ts`:

```typescript
// In extension.ts
const exampleCommand = vscode.commands.registerCommand(
	"vtcode.example",
	async () => {
		await ideContextBridge.flush();  // ← Dependency
		const result = await chatView.something();  // ← Dependency
		vscode.window.showMessage(...);  // ← VS Code API (OK to use directly)
	}
);
```

Create constructor parameter for injected dependencies:

```typescript
constructor(
	private ideContextBridge: IdealContextBridge,
	private chatView: ChatViewProvider
) {}
```

### Available Injections (Common)

```typescript
constructor(
	private backend: VtcodeBackend,
	private executeCommand: (args: string[], opts?: any) => Promise<void>,
	private getWorkspaceTrusted: () => boolean,
	private getOutputChannel: () => vscode.OutputChannel,
	private chatViewProvider: ChatViewProvider,
	private ideContextBridge?: IdealContextBridge
) {}
```

---

## Testing Checklist

For each new command:

- [ ] Command class exists
- [ ] Implements `ICommand` interface
- [ ] Has `id`, `title`, `description`
- [ ] Has `execute()` method
- [ ] Has `canExecute()` method
- [ ] Has JSDoc comments
- [ ] Test file exists
- [ ] Tests pass
- [ ] Export in `commands/index.ts`

---

## Error Handling Pattern

All commands should handle errors consistently:

```typescript
async execute(context: CommandContext): Promise<void> {
	try {
		// Command logic
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		void vscode.window.showErrorMessage(
			`Failed to [action]: ${message}`
		);
		// Optionally log to output channel
	}
}
```

---

## Validation Patterns

Use `canExecute()` for prerequisites:

```typescript
canExecute(context: CommandContext): boolean {
	// Check workspace trust
	if (!context.trusted) {
		return false;
	}
	
	// Check editor
	if (!context.activeTextEditor) {
		return false;
	}
	
	// Check workspace folder
	if (!context.workspaceFolder) {
		return false;
	}
	
	return true;
}
```

---

## Example: Complete Command Implementation

### File: `src/commands/openDocumentationCommand.ts`

```typescript
import * as vscode from "vscode";
import { ICommand, CommandContext } from "../types/command";

/**
 * Command: Open VTCode Documentation
 * Opens the main VTCode documentation in the default browser
 */
export class OpenDocumentationCommand implements ICommand {
	readonly id = "vtcode.openDocumentation";
	readonly title = "Open Documentation";
	readonly description = "Open VTCode documentation website";
	readonly icon = "book";

	async execute(): Promise<void> {
		const docsUrl = "https://vtcode.dev/docs";
		try {
			await vscode.env.openExternal(vscode.Uri.parse(docsUrl));
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			void vscode.window.showErrorMessage(
				`Failed to open documentation: ${message}`
			);
		}
	}

	canExecute(): boolean {
		return true;
	}
}
```

### File: `src/commands/openDocumentationCommand.test.ts`

```typescript
import { describe, it, expect, beforeEach, vi } from "vitest";
import * as vscode from "vscode";
import { OpenDocumentationCommand } from "./openDocumentationCommand";

describe("OpenDocumentationCommand", () => {
	let command: OpenDocumentationCommand;

	beforeEach(() => {
		command = new OpenDocumentationCommand();
	});

	it("should have correct id", () => {
		expect(command.id).toBe("vtcode.openDocumentation");
	});

	it("should always be executable", () => {
		const context = {
			trusted: false,
			activeTextEditor: undefined,
		};
		expect(command.canExecute(context)).toBe(true);
	});

	it("should open docs URL", async () => {
		const openExternal = vi.spyOn(vscode.env, "openExternal");
		await command.execute({
			trusted: true,
		});
		expect(openExternal).toHaveBeenCalled();
	});
});
```

---

## Next Steps

1. Pick a command from the list above
2. Follow the 5-step process
3. Test your command class
4. Update exports
5. Create a PR with your command

---

## Questions?

- Check `askSelectionCommand.ts` for a complete example
- Review `ICommand` interface in `types/command.ts`
- Look at existing tests in `*.test.ts` files

---

## Speed Reference

- **Average time per command**: 15-30 minutes
- **Simple commands** (just open URL): 10 minutes
- **Complex commands** (with dependencies): 30-45 minutes
- **Testing**: 10-15 minutes per command

---

**Good luck! Start with simple commands first, then move to complex ones.**

