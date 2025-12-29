# Phase 2 Quick Start Guide - Architecture Refactoring

**Duration**: 4 weeks | **Complexity**: Medium-High | **Priority**: High

---

## What is Phase 2?

Phase 2 transforms the extension from a monolithic structure to a modular, plugin-like architecture. This is the foundation for all future improvements.

### Before Phase 2

```typescript
// extension.ts - 500+ lines
export async function activate() {
  vscode.commands.registerCommand('vtcode.ask', async () => { ... })
  vscode.commands.registerCommand('vtcode.askSelection', async () => { ... })
  // ... 20+ more commands inline
}
```

### After Phase 2

```typescript
// extension.ts - <200 lines
export async function activate() {
  const registry = new CommandRegistry()
  registry.registerAll()
}

// src/commands/askCommand.ts - Clean, testable
export class AskCommand implements ICommand {
  async execute(context) { ... }
}
```

---

## Phase 2 Deliverables

### 3 Major Systems

1. **Command System** - Modular, testable commands
2. **Participant System** - Context-aware @ mentions
3. **State Management** - Better conversation tracking

### 4 Built-in Participants

-   `@workspace` - Full workspace context
-   `@code` - Selected code context
-   `@terminal` - Terminal output context
-   `@git` - Git branch/changes context

### 7 Modular Commands

-   Ask agent
-   Ask about selection
-   Analyze workspace
-   Update plan
-   Open configuration
-   Trust workspace
-   Refresh CLI availability

### Comprehensive Testing

-   30+ new unit tests
-   10+ integration tests
-   > 90% code coverage

---

## Getting Started (Choose Your Role)

### ‍ Project Manager

**Time**: 15 minutes

1. Read this guide (5 min)
2. Read `PHASE_2_IMPLEMENTATION_PLAN.md` - Overview (5 min)
3. Check timeline section (5 min)
4. Set up task tracking

**Action**: Create GitHub issues from task list below

---

### ‍ Developer (Individual Contributor)

**Time**: 2-4 hours total over 4 weeks (1 hour/week)

**Week 1**:

1. Read `PHASE_2_IMPLEMENTATION_PLAN.md` (20 min)
2. Start Task 2.1 - Command refactoring (focus: `askCommand.ts`)
3. Create PR with 1-2 commands extracted

**Week 2-3**:

1. Continue command extraction
2. Create participant infrastructure
3. Implement 1-2 participants

**Week 4**:

1. Write integration tests
2. Documentation updates
3. Final reviews

---

### Tech Lead / Architect

**Time**: 4-6 hours total

**Pre-work** (1 hour):

1. Review this document
2. Review `PHASE_2_IMPLEMENTATION_PLAN.md`
3. Understand refactoring approach

**Task Planning** (1 hour):

1. Create detailed GitHub issues
2. Define code review standards
3. Set up testing strategy

**Oversight** (2-4 hours over 4 weeks):

1. Weekly code reviews
2. Architectural guidance
3. Problem solving
4. Sign-off on key components

---

## Quick Task List

### Week 3 - Command System Refactoring (4-5 days)

**Task 2.1.1**: Command infrastructure

```bash
# Create files
touch src/types/command.ts
touch src/commandRegistry.ts

# Priority: Critical
# Owner: Lead developer
# Time: 1-2 days
```

**Task 2.1.2**: Extract commands

```bash
# Create modular commands
mkdir src/commands
touch src/commands/{askCommand,askSelectionCommand,analyzeCommand,...}.ts

# Priority: High
# Owner: Any developer
# Time: 3-4 days
# Tasks per developer: 1-2 commands each
```

**Task 2.1.3**: Refactor extension.ts

```bash
# Update main extension file
# Priority: High
# Owner: Lead developer
# Time: 1 day
```

---

### Week 4 - Participant System (4-5 days)

**Task 2.2.1**: Participant infrastructure

```bash
# Create files
touch src/types/participant.ts
touch src/participantRegistry.ts

# Priority: Critical
# Owner: Lead developer
# Time: 1-2 days
```

**Task 2.2.2**: Implement participants

```bash
# Create participant modules
mkdir src/participants
touch src/participants/{workspaceParticipant,codeParticipant,...}.ts

# Priority: High
# Owner: Any developer
# Time: 3-4 days
# Tasks per developer: 1-2 participants each
```

**Task 2.2.3**: ChatView integration

```bash
# Update ChatView for participant support
# Priority: Medium
# Owner: UI specialist
# Time: 1-2 days
```

---

### Week 5 - State Management (3-4 days)

**Task 2.3.1**: Message state

```bash
# Create files
touch src/types/message.ts
touch src/state/{messageStore,conversationState}.ts

# Priority: High
# Owner: Lead developer
# Time: 2-3 days
```

**Task 2.3.2**: Conversation manager

```bash
# Create conversation management
mkdir src/conversation
touch src/conversation/conversationManager.ts

# Priority: High
# Owner: Any developer
# Time: 1-2 days
```

---

### Week 6 - Testing & Documentation (4-5 days)

**Task 2.4.1**: Unit tests

```bash
# Add tests
mkdir src/test/unit/{commands,participants,state}
# Create test files for each module

# Priority: High
# Owner: QA + developers
# Time: 2-3 days
```

**Task 2.4.2**: Integration tests

```bash
# Add integration tests
mkdir src/test/integration
# Create end-to-end flow tests

# Priority: Medium
# Owner: QA
# Time: 1-2 days
```

**Task 2.4.3**: Documentation

```bash
# Create guides
touch docs/{COMMAND_GUIDE,PARTICIPANT_GUIDE}.md
# Update ARCHITECTURE.md

# Priority: Medium
# Owner: Anyone
# Time: 1 day
```

---

## Technical Implementation Details

### Command System Pattern

```typescript
// Step 1: Define interface
interface ICommand {
    readonly id: string;
    readonly title: string;
    execute(context: CommandContext): Promise<void>;
    canExecute(context: CommandContext): boolean;
}

// Step 2: Create command
class AskCommand implements ICommand {
    readonly id = "vtcode.ask";
    readonly title = "VT Code: Ask Agent";

    async execute(context) {
        // Implementation
    }

    canExecute(context) {
        return context.workspaceFolder !== undefined;
    }
}

// Step 3: Register via registry
const registry = new CommandRegistry(backend, config);
registry.register(new AskCommand(backend));
registry.registerAll(); // VS Code registration
```

### Participant System Pattern

```typescript
// Step 1: Define interface
interface ChatParticipant {
    readonly id: string;
    readonly displayName: string;
    canHandle(context: ParticipantContext): boolean;
    resolveReferenceContext(message: string, context): Promise<string>;
}

// Step 2: Create participant
class CodeParticipant implements ChatParticipant {
    readonly id = "code";
    readonly displayName = "@code";

    canHandle(context) {
        return context.activeFile !== undefined;
    }

    async resolveReferenceContext(message, context) {
        if (message.includes("@code")) {
            return `${message}\nCode:\n${context.activeFile.content}`;
        }
        return message;
    }
}

// Step 3: Register via registry
const participantRegistry = new ParticipantRegistry();
participantRegistry.register(new CodeParticipant());
```

### State Management Pattern

```typescript
// Step 1: Enhanced message type
interface ChatMessage {
  id: string
  role: 'user' | 'assistant'
  content: string
  metadata?: { model?: string; tokens?: number }
  state: 'pending' | 'complete' | 'error'
}

// Step 2: Message store
class MessageStore {
  private messages: ChatMessage[] = []

  addMessage(msg: ChatMessage): void
  updateMessage(id: string, updates: Partial<ChatMessage>): void
  getMessages(): ChatMessage[]
}

// Step 3: Conversation manager
class ConversationManager {
  private store = new MessageStore()

  async addMessage(msg): Promise<void> { ... }
  async updateMessage(id, updates): Promise<void> { ... }
}
```

---

## Testing Strategy

### Unit Tests (Target: 30+ tests)

```typescript
// src/test/unit/commands/askCommand.test.ts
describe("AskCommand", () => {
    it("should execute with valid context", async () => {
        const cmd = new AskCommand(mockBackend);
        await cmd.execute(mockContext);
        expect(backend.ask).toHaveBeenCalled();
    });

    it("should not execute without workspace", () => {
        const cmd = new AskCommand(mockBackend);
        expect(cmd.canExecute(emptyContext)).toBe(false);
    });
});
```

### Integration Tests (Target: 10+ tests)

```typescript
// src/test/integration/commandFlow.test.ts
describe("Command Flow", () => {
    it("should execute full ask command flow", async () => {
        const registry = new CommandRegistry(backend, config);
        registry.registerAll();

        // Simulate user action
        await vscode.commands.executeCommand("vtcode.ask");

        // Verify result
        expect(chatView.hasMessages()).toBe(true);
    });
});
```

---

## Code Review Checklist

**Every PR should have**:

-   [ ] Matches ICommand or ChatParticipant interface
-   [ ] 3+ test cases (happy path + edge cases)
-   [ ] JSDoc comments on public APIs
-   [ ] No breaking changes to existing functionality
-   [ ] No console warnings/errors
-   [ ] <10ms execution time for simple operations

---

## Common Pitfalls to Avoid

**Don't**: Hardcode VS Code APIs inside commands
**Do**: Inject dependencies through constructor

**Don't**: Make participants do too much
**Do**: Keep participants focused on one context type

**Don't**: Skip error handling
**Do**: Use try-catch with proper error messages

**Don't**: Forget to update extension.ts
**Do**: Remove old code immediately after refactoring

---

## Weekly Standup Template

**Every Friday**:

```
-   Completed: [Task name]
-  In progress: [Task name]
-  Blocked: [Issue/Person needed]
-  Progress: X/Y tasks complete (X%)
-  Next week: [Plan for next week]
```

---

## Success Indicators

### End of Week 3

-   All commands extracted into modular files
-   CommandRegistry working
-   extension.ts significantly smaller
-   All command tests passing

### End of Week 4

-   ParticipantRegistry working
-   4 participants implemented
-   ChatView shows @ mentions
-   Participant tests passing

### End of Week 5

-   MessageStore working
-   ConversationManager implemented
-   State transitions working
-   State management tests passing

### End of Week 6

-   All unit tests passing (>90% coverage)
-   All integration tests passing
-   Documentation updated
-   Ready for Phase 3

---

## Getting Help

### Questions?

1. Check `PHASE_2_IMPLEMENTATION_PLAN.md` (detailed specs)
2. Review existing code patterns
3. Ask in team chat/meeting
4. Check references section

### Stuck?

1. Create a GitHub issue with context
2. Reference this document
3. Include: what you tried, error message, code snippet
4. Request review from tech lead

### Performance issues?

1. Run performance benchmarks
2. Profile with Chrome DevTools
3. Check memory usage
4. Report to tech lead

---

## Reference Files

| Document                       | Purpose            | Read Time              |
| ------------------------------ | ------------------ | ---------------------- |
| PHASE_2_IMPLEMENTATION_PLAN.md | Detailed specs     | 30 min                 |
| ARCHITECTURE.md                | System overview    | 20 min                 |
| COMMAND_GUIDE.md               | Command system     | 15 min (after Phase 2) |
| PARTICIPANT_GUIDE.md           | Participant system | 15 min (after Phase 2) |

---

## Key Files to Know

**Main files you'll work with**:

-   `src/extension.ts` - Main entry point (will shrink)
-   `src/chatView.ts` - Chat UI integration
-   `src/vtcodeBackend.ts` - Backend connection
-   `src/types/command.ts` - Command interface (NEW)
-   `src/commands/*.ts` - Individual commands (NEW)
-   `src/types/participant.ts` - Participant interface (NEW)
-   `src/participants/*.ts` - Individual participants (NEW)

---

## Timeline at a Glance

```
Nov 11-15 (W3): Command System       25%
Nov 18-22 (W4): Participant System   25%
Nov 25-29 (W5): State Management     15%
Dec 02-06 (W6): Testing & Docs       25%
```

---

## Next After Phase 2?

Once Phase 2 is complete:

**Phase 3** (Weeks 7-9): Chat Improvements

-   Conversation persistence
-   Tool approval UI redesign
-   Streaming enhancements
-   MCP integration

**Phase 4** (Weeks 10-12): Polish & Release

-   Performance optimization
-   Security hardening
-   Marketplace preparation
-   User documentation

---

**Ready to start?** Pick a task from the Quick Task List above and create a GitHub issue!

---

**Version**: 1.0
**Created**: November 8, 2025
**Status**: Ready to Begin
