# Skill Tool Usage Guide

## Quick Start

The skill management system provides four core tools for discovering, loading, and executing skills.

## Tool Workflow

### 1. Discover Skills

First, use `list_skills` to discover what's available:

```
You: list_skills

Agent: Found 15 skills:
- explore (agent_skill): Fast read-only code exploration
- code-reviewer (agent_skill): Review code quality and bugs
- debugger (agent_skill): Debug issues in your code
- pdf-generator (system_utility): Generate PDF reports
- theme-factory (system_utility): Apply theme styling
- [... more skills ...]
```

### 2. Filter Skills

Filter by name or variety:

```
You: list_skills with query="theme"

Agent: Found 2 matching skills:
- theme-factory: Apply theme styling to documents
- theme-core: Core theming utilities
```

### 3. Load Skill Instructions

When you want to use a skill, load it to see full instructions:

```
You: load_skill "theme-factory"

Agent: 
# Theme Factory Skill

[Full SKILL.md contents]

**Activation Status:** Associated tools activated and added to context.
**Resources:** theme-factory/scripts/apply.py, theme-factory/references/color-palettes.json
```

### 4. Access Skill Resources

If the skill instructions reference specific files, load them:

```
You: load_skill_resource skill_name="theme-factory" resource_path="references/color-palettes.json"

Agent:
{
  "skill_name": "theme-factory",
  "resource_path": "references/color-palettes.json",
  "content": "[color palette definitions]"
}
```

### 5. Spawn Subagents

For complex tasks, spawn a specialized subagent:

```
You: spawn_subagent prompt="Review this code for security issues" subagent_type="code-reviewer"

Agent: Subagent 'code-reviewer' completed (id: agent-uuid, 1250ms, 3 turns)
Output:
## Security Review Results
- SQL Injection vulnerability in login.ts:42
- Missing input validation in form handler
- [... more findings ...]
```

## Tool Reference

### list_skills

List all available skills (agent skills and system utilities).

**Parameters:**
- `query` (optional): Filter by skill name (case-insensitive)
- `variety` (optional): Filter by type: `agent_skill`, `system_utility`, or `built_in`

**Response:**
```json
{
  "count": 15,
  "groups": {
    "AgentSkill": [
      {"name": "explore", "description": "...", "variety": "AgentSkill", "status": "active"},
      ...
    ],
    "SystemUtility": [
      {"name": "pdf-generator", "description": "...", "variety": "SystemUtility", "status": "dormant"},
      ...
    ]
  },
  "filter_applied": true
}
```

### load_skill

Load skill instructions and activate its associated tools.

**Parameters:**
- `name` (required): Name of the skill to load

**Response:**
```json
{
  "name": "theme-factory",
  "variety": "AgentSkill",
  "instructions": "[Full SKILL.md content]",
  "instructions_status": "These instructions are now [ACTIVE] and will persist in your system prompt for the remainder of this session.",
  "activation_status": "Associated tools activated and added to context.",
  "resources": [
    "scripts/apply.py",
    "references/color-palettes.json",
    "assets/themes.json"
  ],
  "path": "/path/to/.agents/skills/theme-factory",
  "description": "Apply theme styling to documents and UIs"
}
```

### load_skill_resource

Access specific resources from a loaded skill.

**Parameters:**
- `skill_name` (required): Name of the skill
- `resource_path` (required): Relative path to resource (e.g., `scripts/helper.py`)

**Response:**
```json
{
  "skill_name": "theme-factory",
  "resource_path": "scripts/apply.py",
  "content": "[File contents]"
}
```

**Note:** Resource paths must exist within the skill directory and are validated for security.

### spawn_subagent

Spawn a specialized subagent to handle a task with isolated context.

**Parameters:**
- `prompt` (required): Task description for the subagent
- `subagent_type` (optional): Specific subagent type
  - `explore`: Fast read-only code search (haiku model)
  - `plan`: Research and planning (sonnet model)
  - `general`: Full capabilities (sonnet model)
  - `code-reviewer`: Code quality review
  - `debugger`: Debugging and troubleshooting
- `resume` (optional): Agent ID to resume a previous conversation
- `thoroughness` (optional): Search depth for explore tasks
  - `quick`: Surface-level search
  - `medium`: Standard search (default)
  - `very_thorough`: Deep comprehensive search
- `timeout_seconds` (optional): Execution timeout (default: 300)
- `parent_context` (optional): Context from parent agent

**Response:**
```json
{
  "agent_id": "agent-uuid",
  "subagent_name": "code-reviewer",
  "success": true,
  "duration_ms": 1250,
  "turn_count": 3,
  "output": "[Subagent execution output]",
  "error": null
}
```

## When to Use Each Tool

| Task | Tool | Example |
|------|------|---------|
| Find available features | `list_skills` | "What skills are available?" |
| Activate a skill | `load_skill` | "Load the code-reviewer skill" |
| Get skill details | `load_skill_resource` | "Read the theme palette definitions" |
| Delegate focused work | `spawn_subagent` | "Review this code for security" |

## Best Practices

### 1. Always Discover First
Start by listing skills to understand what's available:
```
list_skills → load_skill → use skill → load_skill_resource if needed
```

### 2. Use Subagents for Isolation
Spawn subagents when you need:
- Focused expertise (code-reviewer, debugger)
- Preserve main conversation context (subagents run in isolated context)
- Parallel exploration (multiple subagents)

Note: Subagents must be enabled in `vtcode.toml` (`[subagents] enabled = true`).

### 3. Progressive Loading
Load skills as needed, not all at once:
- `list_skills` first to see descriptions
- `load_skill` only when ready to use
- `load_skill_resource` for specific references

### 4. Understand Tool Activation
When you `load_skill`, associated tools become active:
- They're added to the tool definitions
- They persist for the session
- They're saved in snapshots for resume

### 5. Subagent Context
Subagents start with a clean context. Provide parent context when spawning for better results:
```
spawn_subagent 
  prompt="Review the authentication module"
  parent_context="We're migrating to OAuth 2.0"
  subagent_type="code-reviewer"
```

## Session Resume

Skills and subagents are session-aware:

- **Skill State**: Active skills are saved in snapshots and restored on resume
- **Subagent Context**: Subagent results are captured in the parent conversation (not shared beforehand)
- **Tool Definitions**: Active skill tools are re-registered on session resume

This ensures your skill workflow continues seamlessly across sessions.

## Troubleshooting

### Skill Not Found
- Check spelling: `list_skills query="skill-name"`
- The skill might be dormant; use `load_skill` to activate
- Skills must be in `~/.vtcode/skills/` or project `.agents/skills/` (legacy `.vtcode/skills/` supported)

### Resource Not Found
- Verify resource path with `load_skill` output
- Paths are relative to skill directory
- Check that the file exists in `skill_name/scripts/`, `skill_name/references/`, etc.

### Subagent Timeout
- Increase timeout: `spawn_subagent ... timeout_seconds=600`
- Reduce thoroughness: `thoroughness="quick"`
- Check agent logs for errors

### Tool Not Appearing
- Load the skill first: `load_skill "skill-name"`
- Tools are dormant until skill is loaded
- Verify skill has associated tools in SKILL.md metadata

## Integration with Agent Workflow

Skill tools integrate with the main agent:
- LLM sees all available skills via `list_skills`
- Agent can autonomously load skills as needed
- Subagents operate with isolated context and require explicit parent_context
- Skill state is preserved across sessions
