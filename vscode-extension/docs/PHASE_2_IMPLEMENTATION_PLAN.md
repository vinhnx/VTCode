# Phase 2 Implementation Plan - Architecture Refactoring (Weeks 3-6)

**Status**: Ready to Start  
**Timeline**: 4 weeks  
**Priority**: High  
**Target**: Modularize commands, introduce participants, improve state management

---

## Overview

Phase 2 focuses on refactoring the extension architecture to improve maintainability, testability, and extensibility. The main goal is to move from a monolithic `extension.ts` to a modular, plugin-like system.

---

## Task Breakdown

### Week 3 - Task 2.1: Command System Refactoring

**Objective**: Extract commands from extension.ts into modular, reusable interfaces

#### 2.1.1 Create Command Infrastructure
```typescript
// src/types/command.ts
export interface ICommand {
  readonly id: string
  readonly title: string
  readonly description?: string
  readonly icon?: string
  
  execute(context: CommandContext): Promise<void>
  canExecute(context: CommandContext): boolean
}

export interface CommandContext {
  workspaceFolder?: vscode.WorkspaceFolder
  activeTextEditor?: vscode.TextEditor
  selection?: vscode.Selection
  terminal?: vscode.Terminal
}
```

**Files to Create**:
- `src/types/command.ts` - Command interface
- `src/commandRegistry.ts` - Command registration system

**Time**: 1-2 days
**Effort**: Medium
**Impact**: Enables modular commands

---

#### 2.1.2 Extract Individual Commands
Create separate files for each command:

```
src/commands/
 index.ts                 (barrel export)
 askCommand.ts            (ask agent a question)
 askSelectionCommand.ts   (ask about selected code)
 analyzeCommand.ts        (analyze workspace)
 taskTrackerCommand.ts     (update execution plan)
 configCommand.ts         (open configuration)
 trustCommand.ts          (trust workspace)
 refreshCommand.ts        (refresh CLI availability)
```

Each command follows the ICommand interface pattern.

**Time**: 3-4 days
**Effort**: High (but straightforward refactoring)
**Impact**: Better separation of concerns

---

#### 2.1.3 Update extension.ts
Refactor extension.ts to use CommandRegistry instead of inline command registration.

```typescript
// Before
vscode.commands.registerCommand('vtcode.ask', async () => { ... })

// After
const registry = new CommandRegistry()
registry.register(new AskCommand(backend))
registry.registerAll()
```

**Time**: 1 day
**Effort**: Medium
**Impact**: ~300 lines removed from extension.ts

---

### Week 4 - Task 2.2: Participant System

**Objective**: Implement a participant system for context-aware conversation

#### 2.2.1 Create Participant Infrastructure
```typescript
// src/types/participant.ts
export interface ChatParticipant {
  readonly id: string
  readonly displayName: string
  readonly description?: string
  readonly icon?: string
  
  canHandle(context: ParticipantContext): boolean
  resolveReferenceContext(
    message: string,
    context: ParticipantContext
  ): Promise<string>
}

export interface ParticipantContext {
  activeFile?: {
    path: string
    language: string
    selection?: vscode.Range
  }
  workspace?: vscode.WorkspaceFolder
  terminal?: { output: string; cwd: string }
  git?: { branch: string; changes: string }
}
```

**Files to Create**:
- `src/types/participant.ts` - Participant interface
- `src/participantRegistry.ts` - Participant management

**Time**: 1-2 days
**Effort**: Medium
**Impact**: Enables extensible context system

---

#### 2.2.2 Implement Built-in Participants
Create base participants (can be extended later):

```
src/participants/
 index.ts
 workspaceParticipant.ts    (@workspace context)
 codeParticipant.ts         (@code context)
 terminalParticipant.ts     (@terminal context)
 gitParticipant.ts          (@git context)
```

**Example**: CodeParticipant provides selected code context
```typescript
class CodeParticipant implements ChatParticipant {
  canHandle(context) {
    return context.activeFile?.language !== undefined
  }
  
  async resolveReferenceContext(message, context) {
    if (message.includes('@code')) {
      return `${message}\n\nCode context:\n${context.activeFile?.content}`
    }
    return message
  }
}
```

**Time**: 3-4 days
**Effort**: High
**Impact**: Enables context-aware responses

---

#### 2.2.3 Integrate with ChatView
Update ChatView to:
- Show available participants in UI
- Handle @ mentions
- Pass participant context to backend

**Time**: 1-2 days
**Effort**: Medium
**Impact**: User-visible feature

---

### Week 5 - Task 2.3: State Management Improvements

**Objective**: Improve message state and conversation flow

#### 2.3.1 Enhanced Message State
```typescript
// src/types/message.ts
export interface ChatMessage {
  readonly id: string
  readonly role: 'user' | 'assistant' | 'system'
  readonly content: string
  readonly timestamp: number
  readonly metadata?: {
    model?: string
    tokens?: { input: number; output: number }
    duration?: number
    participantId?: string
    toolCalls?: ToolCall[]
  }
  readonly state: 'pending' | 'complete' | 'error'
  readonly error?: { code: string; message: string }
}
```

**Files to Create**:
- `src/state/messageStore.ts` - In-memory message storage
- `src/state/conversationState.ts` - Conversation lifecycle management

**Time**: 2-3 days
**Effort**: Medium
**Impact**: Better message tracking

---

#### 2.3.2 Conversation Manager
```typescript
// src/conversation/conversationManager.ts
class ConversationManager {
  async addMessage(message: ChatMessage): Promise<void>
  async updateMessage(id: string, updates: Partial<ChatMessage>): Promise<void>
  async getMessages(): Promise<ChatMessage[]>
  async clear(): Promise<void>
}
```

**Time**: 1-2 days
**Effort**: Medium
**Impact**: Enables conversation persistence (Phase 3)

---

### Week 6 - Task 2.4: Testing & Integration

**Objective**: Ensure all refactored code works together

#### 2.4.1 Unit Tests
- Command tests (one per command)
- Participant tests (one per participant)
- Registry tests
- State management tests

**Target**: >90% coverage

**Time**: 2-3 days
**Effort**: Medium
**Impact**: Confidence in refactoring

---

#### 2.4.2 Integration Tests
- Full command flow (from UI to backend)
- Participant context resolution
- State transitions

**Time**: 1-2 days
**Effort**: Medium
**Impact**: End-to-end validation

---

#### 2.4.3 Documentation Updates
- Update ARCHITECTURE.md
- Add command development guide
- Add participant development guide

**Time**: 1 day
**Effort**: Low
**Impact**: Onboarding for future contributors

---

## File Structure After Phase 2

```
vscode-extension/
 src/
    types/
       command.ts            NEW
       participant.ts        NEW
       message.ts            NEW
       index.ts
   
    commands/                 NEW
       askCommand.ts
       askSelectionCommand.ts
       analyzeCommand.ts
       taskTrackerCommand.ts
       configCommand.ts
       trustCommand.ts
       refreshCommand.ts
       index.ts
   
    commandRegistry.ts        NEW
   
    participants/             NEW
       workspaceParticipant.ts
       codeParticipant.ts
       terminalParticipant.ts
       gitParticipant.ts
       index.ts
   
    participantRegistry.ts    NEW
   
    state/                    NEW
       messageStore.ts
       conversationState.ts
       index.ts
   
    conversation/             NEW
       conversationManager.ts
       index.ts
   
    extension.ts             (REFACTORED - ~300 lines removed)
    chatView.ts              (Enhanced with participants)
    vtcodeBackend.ts
    ... other existing files

 src/test/
    unit/
       commands/
          askCommand.test.ts
          ...
       participants/
          codeParticipant.test.ts
          ...
       state/
          messageStore.test.ts
       ...
    integration/
        ... (new integration tests)

 docs/
     ARCHITECTURE.md          (UPDATED)
     COMMAND_GUIDE.md          NEW
     PARTICIPANT_GUIDE.md      NEW
     ...
```

---

## Success Criteria

### Code Quality
- [ ] Extension.ts reduced to <200 lines
- [ ] All commands follow ICommand interface
- [ ] All participants follow ChatParticipant interface
- [ ] >90% test coverage
- [ ] No ESLint warnings
- [ ] TypeScript strict mode

### Functionality
- [ ] All existing commands work
- [ ] All participants resolve context correctly
- [ ] State management works end-to-end
- [ ] No performance regression

### Documentation
- [ ] Architecture updated
- [ ] Command development guide complete
- [ ] Participant development guide complete
- [ ] Examples for each system

### User Experience
- [ ] No breaking changes
- [ ] All existing features still work
- [ ] New @ participant mention system visible to users
- [ ] No performance impact

---

## Risk Mitigation

### Risk: Breaking existing functionality
**Mitigation**: 
- Comprehensive refactoring tests
- Feature parity verification
- Gradual rollout strategy

### Risk: Performance regression
**Mitigation**:
- Performance benchmarks in tests
- Memory profiling
- Registry lookup optimization

### Risk: Incomplete refactoring
**Mitigation**:
- Regular reviews at task boundaries
- Weekly progress meetings
- Clear rollback plan

---

## Dependencies & Blockers

None - Phase 2 can proceed independently.

---

## Resources Required

- 1 Senior Developer (refactoring lead)
- 1 QA/Test Engineer (integration tests)
- Code review support from team

---

## Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| Extension.ts size | <200 lines | Line count |
| Test coverage | >90% | Coverage report |
| Command count | 7+ modular | Number of modules |
| Participants | 4+ built-in | Participant registry |
| No breaking changes | 100% | Feature test results |
| Performance | Same/better | Benchmark comparison |

---

## Timeline

```
Week 3:  Command System Refactoring
 Mon-Tue: Command infrastructure
 Wed-Fri: Extract commands
 Time: 4-5 days

Week 4:  Participant System
 Mon-Tue: Participant infrastructure
 Wed-Fri: Build base participants
 Time: 4-5 days

Week 5:  State Management
 Mon: Enhanced message state
 Tue-Wed: Conversation manager
 Time: 2-3 days

Week 6:  Testing & Documentation
 Mon-Wed: Unit & integration tests
 Thu-Fri: Documentation & reviews
 Time: 3-4 days
```

**Total Effort**: ~20-25 developer days

---

## Next Phase Preview

**Phase 3**: Chat Improvements (Weeks 7-9)
- Conversation persistence
- Tool approval UI
- Streaming improvements
- MCP enhancements

**Phase 4**: Polish & Release (Weeks 10-12)
- Performance optimization
- Security hardening
- Marketplace preparation

---

## References

- Phase 1 status: `PHASE_1_STATUS.md`
- VS Copilot Chat: https://github.com/microsoft/vscode-copilot-chat

---

**Version**: 1.0  
**Created**: November 8, 2025  
**Status**: Ready for Review
