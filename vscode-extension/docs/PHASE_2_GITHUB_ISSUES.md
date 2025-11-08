# Phase 2 GitHub Issues Template

Copy-paste these issue templates directly into GitHub. Update placeholders as needed.

---

## Week 3 - Command System Refactoring

### Issue 1: Create Command Infrastructure
```markdown
# Task 2.1.1: Create Command Infrastructure

## Description
Create the foundational interfaces and registry system for modular commands.

## Tasks
- [ ] Create `src/types/command.ts` with `ICommand` interface
- [ ] Create `src/commandRegistry.ts` with registration logic
- [ ] Add JSDoc documentation
- [ ] Create unit tests with >90% coverage

## Files Created
- `src/types/command.ts`
- `src/commandRegistry.ts`
- `src/types/command.test.ts`
- `src/commandRegistry.test.ts`

## Acceptance Criteria
- [ ] `ICommand` interface supports all command operations
- [ ] `CommandRegistry` can register and execute commands
- [ ] All types properly exported from `src/types/index.ts`
- [ ] Unit tests pass with >90% coverage
- [ ] No ESLint warnings

## Time Estimate
1-2 days

## Related
Subtask of Phase 2 - Architecture Refactoring

## Priority
🔴 Critical
```

---

### Issue 2: Extract Ask Command
```markdown
# Task 2.1.2a: Extract Ask Command

## Description
Refactor the `vtcode.ask` command into a modular `AskCommand` class.

## Tasks
- [ ] Create `src/commands/askCommand.ts`
- [ ] Implement `ICommand` interface
- [ ] Move logic from `extension.ts` to `AskCommand`
- [ ] Update `extension.ts` to use new command
- [ ] Create unit tests

## Files
- `src/commands/askCommand.ts` (NEW)
- `src/extension.ts` (MODIFIED)
- `src/commands/askCommand.test.ts` (NEW)

## Acceptance Criteria
- [ ] Command behaves identically to original implementation
- [ ] Command can be executed via VS Code command palette
- [ ] No regressions in existing functionality
- [ ] Unit tests pass

## Implementation Notes
```typescript
// Pattern to follow
export class AskCommand implements ICommand {
  readonly id = 'vtcode.ask'
  readonly title = 'VTCode: Ask Agent'
  
  constructor(private backend: VtcodeBackend) {}
  
  async execute(context: CommandContext): Promise<void> {
    // Move logic from extension.ts here
  }
  
  canExecute(context: CommandContext): boolean {
    return context.workspaceFolder !== undefined
  }
}
```

## Time Estimate
4-6 hours

## Related
Subtask of Task 2.1.2 - Extract Commands

## Priority
🟡 High
```

---

### Issue 3: Extract Ask Selection Command
```markdown
# Task 2.1.2b: Extract Ask Selection Command

## Description
Refactor the `vtcode.askSelection` command into a modular class.

## Tasks
- [ ] Create `src/commands/askSelectionCommand.ts`
- [ ] Implement `ICommand` interface
- [ ] Move logic from `extension.ts`
- [ ] Update `extension.ts` registration
- [ ] Create unit tests

## Files
- `src/commands/askSelectionCommand.ts` (NEW)
- `src/extension.ts` (MODIFIED)
- `src/commands/askSelectionCommand.test.ts` (NEW)

## Implementation Notes
Focus on code selection context. Verify selection API usage.

## Time Estimate
4-6 hours

## Related
Subtask of Task 2.1.2 - Extract Commands

## Priority
🟡 High
```

---

### Issue 4: Extract Analyze Command
```markdown
# Task 2.1.2c: Extract Analyze Command

## Description
Refactor the `vtcode.analyze` command into a modular class.

## Tasks
- [ ] Create `src/commands/analyzeCommand.ts`
- [ ] Implement `ICommand` interface
- [ ] Move logic from `extension.ts`
- [ ] Update `extension.ts` registration
- [ ] Create unit tests

## Files
- `src/commands/analyzeCommand.ts` (NEW)
- `src/extension.ts` (MODIFIED)
- `src/commands/analyzeCommand.test.ts` (NEW)

## Implementation Notes
This command triggers workspace analysis. Test with multiple project types.

## Time Estimate
4-6 hours

## Related
Subtask of Task 2.1.2 - Extract Commands

## Priority
🟡 High
```

---

### Issue 5: Extract Remaining Commands
```markdown
# Task 2.1.2d: Extract Remaining Commands

## Description
Extract the following commands:
- updatePlanCommand
- configCommand
- trustCommand
- refreshCommand

## Tasks
- [ ] Create `src/commands/updatePlanCommand.ts`
- [ ] Create `src/commands/configCommand.ts`
- [ ] Create `src/commands/trustCommand.ts`
- [ ] Create `src/commands/refreshCommand.ts`
- [ ] Create corresponding test files
- [ ] Update `src/commands/index.ts` barrel export

## Files
- `src/commands/updatePlanCommand.ts` (NEW)
- `src/commands/configCommand.ts` (NEW)
- `src/commands/trustCommand.ts` (NEW)
- `src/commands/refreshCommand.ts` (NEW)
- `src/commands/index.ts` (NEW - barrel export)
- 4 test files (NEW)
- `src/extension.ts` (MODIFIED)

## Implementation Notes
Ensure all commands have consistent error handling and logging.

## Time Estimate
1 day

## Related
Subtask of Task 2.1.2 - Extract Commands

## Priority
🟡 High
```

---

### Issue 6: Refactor Extension.ts
```markdown
# Task 2.1.3: Refactor Extension.ts

## Description
Update `extension.ts` to use `CommandRegistry` instead of inline command registration.

## Tasks
- [ ] Import `CommandRegistry` and all commands
- [ ] Replace inline `vscode.commands.registerCommand` calls
- [ ] Use `registry.registerAll()` for batch registration
- [ ] Verify all commands still work
- [ ] Remove old command registration code

## Before/After
```typescript
// Before
export async function activate() {
  vscode.commands.registerCommand('vtcode.ask', async () => { ... })
  vscode.commands.registerCommand('vtcode.askSelection', async () => { ... })
  // ... 20+ more lines
}

// After
export async function activate() {
  const registry = new CommandRegistry(backend, config)
  registry.registerAll()
}
```

## Acceptance Criteria
- [ ] `extension.ts` reduced to <200 lines
- [ ] All commands work identically to before
- [ ] No console errors or warnings
- [ ] All existing tests still pass

## Time Estimate
1 day

## Related
End of Task 2.1 - Command System Refactoring

## Priority
🔴 Critical
```

---

## Week 4 - Participant System

### Issue 7: Create Participant Infrastructure
```markdown
# Task 2.2.1: Create Participant Infrastructure

## Description
Create the foundational interfaces and registry system for participants.

## Tasks
- [ ] Create `src/types/participant.ts` with `ChatParticipant` interface
- [ ] Create `src/participantRegistry.ts` with registration logic
- [ ] Add JSDoc documentation
- [ ] Create unit tests

## Files Created
- `src/types/participant.ts`
- `src/participantRegistry.ts`
- `src/types/participant.test.ts`
- `src/participantRegistry.test.ts`

## Acceptance Criteria
- [ ] Interface supports @ mention resolution
- [ ] Registry can manage multiple participants
- [ ] Context passing works correctly
- [ ] Unit tests pass with >90% coverage

## Time Estimate
1-2 days

## Related
Subtask of Phase 2 - Architecture Refactoring

## Priority
🔴 Critical
```

---

### Issue 8: Implement Code Participant
```markdown
# Task 2.2.2a: Implement Code Participant

## Description
Create `@code` participant for selected code context.

## Tasks
- [ ] Create `src/participants/codeParticipant.ts`
- [ ] Implement `ChatParticipant` interface
- [ ] Resolve code context from active editor
- [ ] Handle @ mention parsing
- [ ] Create unit tests

## Files
- `src/participants/codeParticipant.ts` (NEW)
- `src/participants/codeParticipant.test.ts` (NEW)

## Features
- Detects `@code` in user message
- Extracts selected code or entire file
- Includes file path and language info
- Preserves original message structure

## Implementation Notes
```typescript
class CodeParticipant implements ChatParticipant {
  readonly id = 'code'
  readonly displayName = '@code'
  
  canHandle(context: ParticipantContext): boolean {
    return context.activeFile !== undefined
  }
  
  async resolveReferenceContext(message, context) {
    if (message.includes('@code')) {
      const code = context.activeFile.selection 
        ? context.activeFile.getSelectedText()
        : context.activeFile.content
      return `${message}\n\nCode:\n\`\`\`${context.activeFile.language}\n${code}\n\`\`\``
    }
    return message
  }
}
```

## Time Estimate
4-6 hours

## Priority
🟡 High
```

---

### Issue 9: Implement Workspace Participant
```markdown
# Task 2.2.2b: Implement Workspace Participant

## Description
Create `@workspace` participant for project structure context.

## Tasks
- [ ] Create `src/participants/workspaceParticipant.ts`
- [ ] Implement `ChatParticipant` interface
- [ ] Extract workspace structure
- [ ] Include project metadata
- [ ] Create unit tests

## Files
- `src/participants/workspaceParticipant.ts` (NEW)
- `src/participants/workspaceParticipant.test.ts` (NEW)

## Features
- Detects `@workspace` in user message
- Provides directory structure
- Includes file counts and key files
- Shows workspace configuration

## Time Estimate
4-6 hours

## Priority
🟡 High
```

---

### Issue 10: Implement Terminal & Git Participants
```markdown
# Task 2.2.2c: Implement Terminal and Git Participants

## Description
Create `@terminal` and `@git` participants for execution and version control context.

## Tasks
- [ ] Create `src/participants/terminalParticipant.ts`
- [ ] Create `src/participants/gitParticipant.ts`
- [ ] Implement both `ChatParticipant` interfaces
- [ ] Extract relevant context data
- [ ] Create unit tests

## Files
- `src/participants/terminalParticipant.ts` (NEW)
- `src/participants/gitParticipant.ts` (NEW)
- 2 test files (NEW)
- `src/participants/index.ts` (NEW - barrel export)

## Features
Terminal Participant:
- Captures recent terminal output
- Includes current working directory
- Shows last command execution result

Git Participant:
- Shows current branch
- Lists staged/unstaged changes
- Includes recent commit info

## Time Estimate
1 day (both participants)

## Priority
🟡 High
```

---

### Issue 11: ChatView Participant Integration
```markdown
# Task 2.2.3: ChatView Participant Integration

## Description
Update ChatView to support participant system with @ mentions.

## Tasks
- [ ] Import `ParticipantRegistry`
- [ ] Add @ mention autocomplete in chat input
- [ ] Display available participants
- [ ] Resolve participant context on message send
- [ ] Update chat UI to show selected participant
- [ ] Create integration tests

## Files Modified
- `src/chatView.ts`
- `media/chatView.html`
- `media/chat-view.css`

## Features
- @ mention dropdown in chat input
- Visual indicator of selected participant
- Participant icon/color in messages
- Seamless integration with existing UI

## Acceptance Criteria
- [ ] Users can @ mention participants
- [ ] Context resolves correctly
- [ ] No performance impact
- [ ] All tests pass

## Time Estimate
1-2 days

## Priority
🟡 High
```

---

## Week 5 - State Management

### Issue 12: Enhanced Message State
```markdown
# Task 2.3.1: Enhanced Message State

## Description
Create enhanced message types and storage for better conversation tracking.

## Tasks
- [ ] Create `src/types/message.ts` with enhanced types
- [ ] Create `src/state/messageStore.ts`
- [ ] Implement message lifecycle tracking
- [ ] Add error state handling
- [ ] Create unit tests

## Files Created
- `src/types/message.ts` (NEW)
- `src/state/messageStore.ts` (NEW)
- Test files (NEW)

## Message Type
```typescript
interface ChatMessage {
  readonly id: string
  readonly role: 'user' | 'assistant' | 'system'
  readonly content: string
  readonly timestamp: number
  readonly metadata?: {
    model?: string
    tokens?: { input: number; output: number }
    duration?: number
    participantId?: string
  }
  readonly state: 'pending' | 'complete' | 'error'
  readonly error?: { code: string; message: string }
}
```

## Acceptance Criteria
- [ ] All message types properly typed
- [ ] State transitions work correctly
- [ ] Error messages include context
- [ ] Unit tests pass

## Time Estimate
2-3 days

## Priority
🟡 High
```

---

### Issue 13: Conversation Manager
```markdown
# Task 2.3.2: Conversation Manager

## Description
Create conversation manager for message lifecycle and state management.

## Tasks
- [ ] Create `src/conversation/conversationManager.ts`
- [ ] Implement message addition/update operations
- [ ] Add message querying capabilities
- [ ] Create unit tests
- [ ] Verify integration with ChatView

## Files
- `src/conversation/conversationManager.ts` (NEW)
- `src/conversation/conversationManager.test.ts` (NEW)

## API
```typescript
class ConversationManager {
  async addMessage(message: ChatMessage): Promise<void>
  async updateMessage(id: string, updates: Partial<ChatMessage>): Promise<void>
  async getMessages(): Promise<ChatMessage[]>
  async getMessageById(id: string): Promise<ChatMessage | undefined>
  async clear(): Promise<void>
  onMessageAdded(callback: (msg: ChatMessage) => void): void
}
```

## Acceptance Criteria
- [ ] All operations work correctly
- [ ] Events fire at right times
- [ ] No memory leaks
- [ ] Unit tests pass

## Time Estimate
1-2 days

## Priority
🟡 High
```

---

## Week 6 - Testing & Documentation

### Issue 14: Unit Tests - Commands
```markdown
# Task 2.4.1a: Unit Tests for Commands

## Description
Create comprehensive unit tests for all extracted commands.

## Tasks
- [ ] Test each command's `execute()` method
- [ ] Test each command's `canExecute()` method
- [ ] Test error conditions
- [ ] Test with mock backend
- [ ] Achieve >90% coverage

## Files
- Tests for all 7 commands
- Mock fixtures if needed

## Test Coverage
- Happy path (command executes)
- Preconditions not met (canExecute returns false)
- Backend errors (error handling)
- Invalid input (edge cases)

## Time Estimate
1-2 days

## Priority
🟡 High
```

---

### Issue 15: Unit Tests - Participants
```markdown
# Task 2.4.1b: Unit Tests for Participants

## Description
Create comprehensive unit tests for all participants.

## Tasks
- [ ] Test each participant's `canHandle()` method
- [ ] Test context resolution
- [ ] Test @ mention parsing
- [ ] Test edge cases
- [ ] Achieve >90% coverage

## Files
- Tests for all 4 participants
- Mock context fixtures

## Test Coverage
- Context detection
- Message parsing
- Context injection
- Error handling

## Time Estimate
1 day

## Priority
🟡 High
```

---

### Issue 16: Integration Tests
```markdown
# Task 2.4.2: Integration Tests

## Description
Create end-to-end integration tests for Phase 2 systems.

## Tasks
- [ ] Test full command execution flow
- [ ] Test participant context resolution in chat
- [ ] Test message state transitions
- [ ] Test command + participant + state interaction
- [ ] Achieve >85% coverage

## Test Scenarios
1. User sends @ mention message
2. Participant resolves context
3. Message added to store
4. Message state updates
5. ChatView updates

## Acceptance Criteria
- [ ] All integration tests pass
- [ ] No flaky tests
- [ ] Reasonable execution time (<30s total)

## Time Estimate
1-2 days

## Priority
🟡 High
```

---

### Issue 17: Documentation Updates
```markdown
# Task 2.4.3: Documentation Updates

## Description
Update and create architecture documentation for Phase 2 systems.

## Tasks
- [ ] Update `docs/ARCHITECTURE.md`
- [ ] Create `docs/COMMAND_GUIDE.md`
- [ ] Create `docs/PARTICIPANT_GUIDE.md`
- [ ] Add code examples
- [ ] Update README with Phase 2 info

## Documentation Topics
- Architecture overview
- Command system design
- Participant system design
- State management flow
- Development guidelines
- Testing patterns

## Acceptance Criteria
- [ ] Documentation is clear and complete
- [ ] All code examples are tested
- [ ] Links are correct
- [ ] Reviewed by team

## Time Estimate
1 day

## Priority
🟡 High
```

---

## Meta-Tasks

### Issue 18: Phase 2 Progress Tracking
```markdown
# Phase 2: Architecture Refactoring - Progress Tracking

This is the parent issue for Phase 2 work. Check off subtasks as they complete.

## Week 3: Command System (Target: Nov 11-15)
- [ ] #1 - Command Infrastructure
- [ ] #2a - Ask Command
- [ ] #2b - Ask Selection Command
- [ ] #2c - Analyze Command
- [ ] #2d - Remaining Commands
- [ ] #6 - Refactor Extension.ts

## Week 4: Participant System (Target: Nov 18-22)
- [ ] #7 - Participant Infrastructure
- [ ] #8 - Code Participant
- [ ] #9 - Workspace Participant
- [ ] #10 - Terminal & Git Participants
- [ ] #11 - ChatView Integration

## Week 5: State Management (Target: Nov 25-29)
- [ ] #12 - Enhanced Message State
- [ ] #13 - Conversation Manager

## Week 6: Testing & Docs (Target: Dec 2-6)
- [ ] #14 - Command Tests
- [ ] #15 - Participant Tests
- [ ] #16 - Integration Tests
- [ ] #17 - Documentation

## Metrics
- Started: [Date]
- Completed: [Date]
- Burn down: [Track weekly]
- Test coverage: [Track weekly]

## Blockers
[Add any blockers here]

## Notes
[Add implementation notes]
```

---

## How to Use These Templates

1. **Copy** the issue text
2. **Paste** into GitHub new issue dialog
3. **Update** any placeholders (dates, names, etc.)
4. **Add labels**: `phase-2`, `refactor`, `architecture`
5. **Assign** to appropriate team member
6. **Link** to Phase 2 progress tracking issue

---

## Issue Labels to Create

```
phase-2              - Part of Phase 2
refactor            - Code refactoring
architecture        - Architecture/design
testing             - Testing related
documentation       - Documentation
command-system      - Command system work
participant-system  - Participant system work
state-management    - State management work
```

---

## Workflow

1. **Create** all Week 3 issues upfront
2. **Assign** as work begins
3. **Close** when done and reviewed
4. **Create** Week 4 issues mid-week
5. **Repeat** for subsequent weeks

---

**Copy these templates to GitHub to get started!**

Version: 1.0 | Created: November 8, 2025
