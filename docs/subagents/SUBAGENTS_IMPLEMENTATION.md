# Sub-Agents Implementation Guide for VT Code

This guide applies Claude Code's sub-agent architecture to the VT Code project. Your existing agent system is already well-structured; this document explains how to leverage sub-agent capabilities and modernize your agent configuration.

## Overview

Your agents in `.claude/agents/` are already functioning as sub-agents. This guide clarifies:
1. How they align with Claude Code's sub-agent model
2. Recommended format updates for consistency
3. How to leverage sub-agent-specific features
4. Best practices for agent composition

## Current Architecture Alignment

Your system already implements key sub-agent concepts:

| Concept | Your Implementation | File |
|---------|-------------------|------|
| **Specialized expertise** | Agent-per-task (coder, tester, architect) | `.claude/agents/*.md` |
| **Separate context** | Each agent invoked independently | Orchestrated via hooks |
| **Custom prompts** | Detailed system prompts in frontmatter | Already in place |
| **Tool isolation** | Tools defined per agent | `tools:` field |
| **Model selection** | Model specified per agent | `model:` field (e.g., `sonnet`) |

## Recommended Format Updates

### Current Format (Your System)

Your agents use YAML frontmatter with custom fields:

```yaml
---
name: coder
description: Implementation specialist that writes code...
tools: Read, Write, Edit, Glob, Grep, Bash, Task
model: sonnet
extended_thinking: true
color: blue
---
```

### Aligned Format (Claude Code Standard)

To align with Claude Code's sub-agent system, ensure your agents match this structure:

```yaml
---
name: agent-name
description: Clear description of when this agent should be invoked. Use "MUST BE USED PROACTIVELY" or "Use immediately after" for automatic delegation hints.
tools: Tool1, Tool2, Tool3  # Comma-separated; omit to inherit all tools
model: sonnet               # sonnet, opus, haiku, or 'inherit'
permissionMode: default     # default, acceptEdits, bypassPermissions, plan, ignore
skills: skill1, skill2      # Optional; comma-separated
---
```

### VT Code Extended Fields

Keep your useful extensions:
- `extended_thinking: true` - For complex reasoning tasks
- `color: blue` - For UI distinction (optional)

**Updated `.claude/agents/coder.md` header:**

```yaml
---
name: coder
description: Implementation specialist. Use immediately after receiving a todo item to write code that fulfills requirements.
tools: Read, Write, Edit, Glob, Grep, Bash, Task
model: sonnet
extended_thinking: true
permissionMode: acceptEdits
---
```

## Key Agent Descriptions (for Auto-Delegation)

Update your agent descriptions to encourage automatic invocation by Claude Code. Use trigger phrases:

| Trigger Phrase | Use When | Agent |
|---|---|---|
| "Use immediately after" | Agent should run right after specific events | `coder`: after receiving todo |
| "Use proactively" | Agent should auto-run on certain conditions | `coding-standards-checker`: after code changes |
| "MUST BE USED" | Critical automatic invocation | `tester`: after all implementations |
| "Use when" | Conditional invocation | `debugger`: when encountering bugs |

### Updated Descriptions

**coder.md:**
```yaml
description: Implementation specialist. Use immediately after receiving a todo item to write code fulfilling requirements.
```

**coding-standards-checker.md:**
```yaml
description: Code quality verifier. Use proactively after any code changes to enforce standards and catch issues early.
```

**tester.md:**
```yaml
description: Functionality verification specialist. MUST BE USED after all code implementations to validate correctness.
```

**debugger.md:**
```yaml
description: Forensic debugging specialist. Use when encountering unexpected errors, test failures, or anomalies. Enforces read-only investigation mode.
```

## Model Optimization

Your agents currently specify `model: sonnet` directly. For flexibility:

### Recommended Approach

| Agent | Model | Rationale |
|-------|-------|-----------|
| `init-explorer` | `haiku` | Fast codebase scanning |
| `architect` | `opus` | Complex specification design |
| `coder` | `sonnet` | Balanced reasoning + execution |
| `tester` | `sonnet` | Thorough test validation |
| `debugger` | `opus` | Deep investigation needed |
| `forensic` | `sonnet` | Structured investigation |
| `code-reviewer` | `sonnet` | Balanced analysis |

**Update recommendation:**

```yaml
# For agents needing best reasoning
model: opus

# For agents needing balanced performance
model: sonnet

# For fast, lightweight exploration
model: haiku

# To inherit parent conversation's model
model: inherit
```

## Skills Auto-Loading

Claude Code now supports auto-loading skills when an agent starts. This prevents context pollution while ensuring tools are available.

### Example: Add Skills to Agents

**coder.md:**
```yaml
---
name: coder
description: ...
tools: Read, Write, Edit, Glob, Grep, Bash, Task
model: sonnet
extended_thinking: true
skills: code-reviewer, code-review-skill
---
```

**architect.md:**
```yaml
---
name: architect
description: ...
model: opus
skills: doc-coauthoring, canvas-design
---
```

This way, skills are loaded only when needed, not in the main conversation.

## Permission Modes

Claude Code's sub-agents support granular permission control:

| Mode | Behavior | Use Case |
|------|----------|----------|
| `default` | Standard permissions | Most agents |
| `acceptEdits` | Can accept edit suggestions | `coder`, `refactorer` |
| `bypassPermissions` | Unrestricted (use rarely) | Trusted orchestrators only |
| `plan` | Plan-mode only (read-only research) | `init-explorer` during planning |
| `ignore` | Ignores permission requests | Investigation-only agents |

### VT Code Recommendations

```yaml
# .claude/agents/coder.md
permissionMode: acceptEdits

# .claude/agents/debugger.md
permissionMode: ignore  # Read-only enforcement

# .claude/agents/init-explorer.md
permissionMode: plan    # Planning mode only
```

## Agent Configuration Migration Path

### Phase 1: Update Headers (No Functional Change)
Update all agents to include the standard Claude Code fields:

```bash
# For each agent, add:
permissionMode: default
# Or appropriate mode for the agent
```

### Phase 2: Optimize Models
Review your model assignments and align with recommended strategy above.

### Phase 3: Add Skills
Identify which agents benefit from pre-loaded skills to reduce context bloat.

### Phase 4: Refine Descriptions
Update descriptions to include trigger phrases for better auto-delegation.

## Orchestration with Sub-Agents

Your current hook system (`.claude/config.json`) orchestrates agent workflows. This aligns perfectly with Claude Code's sub-agent model.

### Current Workflow Example

```
init-explorer → architect → bdd-agent → gherkin-to-test → 
codebase-analyst → test-creator → coder → coding-standards-checker → 
tester → bdd-test-runner
```

**This is already a sub-agent pipeline.** Hooks trigger sequential delegation.

### Recommended Enhancements

1. **Add explicit descriptions** so Claude Code can auto-delegate when appropriate
2. **Leverage permissionMode** to enforce read-only during investigation phases
3. **Pre-load skills** to reduce context overhead
4. **Use model: inherit** where consistency matters

## Tool Inheritance

### Current Behavior
Each agent specifies its tools explicitly (e.g., `tools: Read, Write, Edit...`)

### Recommended Best Practice

```yaml
# For general-purpose agents that need most tools
# Omit the tools field to inherit all tools:
---
name: architect
description: Greenfield specification designer
model: opus
# No tools field - inherits all tools

---

# For specialized read-only agents
# Explicitly list tools:
---
name: init-explorer
description: Codebase exploration specialist
tools: Glob, Grep, Read, Bash
model: haiku
permissionMode: plan
```

## Integration with Zed IDE & VS Code

If you deploy VT Code as a plugin, sub-agents improve integration:

1. **VSCode Extension** (`.vscode-extension/`): Can request specific sub-agents
2. **Zed Extension** (`.zed-extension/`): Uses ACP protocol for agent delegation
3. **Language Server** Effects: LSP-like code intelligence calls sub-agents

Update your extension integrations to explicitly invoke sub-agents:

```typescript
// vscode-extension/src/client.ts
const response = await client.ask("Use the code-reviewer agent to check my changes", {
  subagent: "code-reviewer"
});
```

## Security Considerations

### Current Security Model
Your agents respect workspace boundaries through tool implementations.

### Enhanced with Sub-Agent Permissions

1. **permissionMode: ignore** - Blocks file modifications during investigation
2. **tools: [restricted list]** - Limits dangerous operations
3. **model: haiku** - Reduced token budget for untrusted tasks

### Example: Secure Debugging Agent

```yaml
---
name: secure-debugger
description: Investigation specialist with restricted permissions
tools: Read, Grep, Glob, Bash
model: haiku
permissionMode: ignore
extended_thinking: false  # Reduces complexity, improves safety
---
```

## MCP Tool Integration

VT Code supports MCP servers (`.mcp.json`). Sub-agents can inherit MCP tools:

```yaml
---
name: data-analyst
description: Data analysis specialist with MCP integration
# Omit tools field to inherit all MCP tools from main thread
model: sonnet
skills: spreadsheet-generator, xlsx
---
```

MCP tools are automatically available when `tools` field is omitted.

## Testing Sub-Agent Configurations

### Manual Testing

```bash
# Start Claude Code with explicit sub-agent
cd /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode
claude --agents '{
  "test-reviewer": {
    "description": "Test validation specialist",
    "tools": "Read, Grep, Bash",
    "model": "sonnet"
  }
}'
```

### Validation Checklist

For each agent, verify:
- [ ] `name` is unique and lowercase with hyphens
- [ ] `description` includes trigger phrase if auto-delegation desired
- [ ] `tools` list is appropriate for the agent's scope
- [ ] `model` choice matches complexity requirements
- [ ] `permissionMode` enforces intended constraints
- [ ] `extended_thinking` enables only when beneficial

## Recommended Agent Updates (Summary)

### High Priority
1. **coder.md** - Add `permissionMode: acceptEdits`
2. **debugger.md** - Change to `permissionMode: ignore`
3. **forensic.md** - Add `permissionMode: ignore`

### Medium Priority
4. Update all descriptions to include trigger phrases
5. Review and optimize model assignments
6. Test auto-delegation behavior

### Low Priority
7. Add `skills` fields where applicable
8. Refine tool lists based on actual needs

## FAQ

**Q: Should I convert my hooks to sub-agent invocations?**
A: No. Your hooks system is excellent for orchestration. Keep it. Sub-agent format is for clarity and auto-delegation hints.

**Q: Can I use `inherit` model everywhere?**
A: Yes, but only where consistency matters. Use explicit models for cost optimization (e.g., `haiku` for exploration).

**Q: How do I debug a sub-agent's behavior?**
A: Sub-agents operate in separate context. Review their output in the main conversation thread where they were invoked.

**Q: Should I create user-level (`~/.claude/agents/`) or project-level agents?**
A: Keep all VT Code agents in `.claude/agents/` (project-level). Share reusable agents via your plugin system if needed.

**Q: How do permissions work with my hooks?**
A: Hooks trigger agents, which then respect their `permissionMode`. Use `acceptEdits` for agents that modify code, `ignore` for read-only investigation.

## Resources

- [Claude Code Sub-Agents Docs](https://code.claude.com/docs/en/sub-agents)
- [Claude Code Plugins Reference](https://code.claude.com/docs/en/plugins-reference)
- [VT Code MCP Integration](docs/MCP_INTEGRATION_GUIDE.md)
- [Your Existing CLAUDE.md](./../CLAUDE.md)
