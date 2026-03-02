# Skill Tool Usage Guide

## Quick Start

The skills system provides three core tools for discovery, activation, and resource access:

1. `list_skills`
2. `load_skill`
3. `load_skill_resource`

## Tool Workflow

### 1. Discover Skills

Use `list_skills` to discover available skills:

```
You: list_skills

Agent: Found 15 skills:
- explore (agent_skill): Fast read-only code exploration
- code-reviewer (agent_skill): Review code quality and bugs
- debugger (agent_skill): Debug issues in your code
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

Load a skill before using it:

```
You: load_skill "theme-factory"

Agent:
# Theme Factory Skill

[Full SKILL.md contents]

**Activation Status:** Associated tools activated and added to context.
**Resources:** theme-factory/scripts/apply.py, theme-factory/references/color-palettes.json
```

### 4. Access Skill Resources

Load specific files referenced by the skill:

```
You: load_skill_resource skill_name="theme-factory" resource_path="references/color-palettes.json"

Agent:
{
  "skill_name": "theme-factory",
  "resource_path": "references/color-palettes.json",
  "content": "[color palette definitions]"
}
```

## Tool Reference

### list_skills

List all available skills (agent skills and system utilities).

**Parameters:**
- `query` (optional): Filter by skill name (case-insensitive)
- `variety` (optional): Filter by type: `agent_skill`, `system_utility`, or `built_in`

### load_skill

Load skill instructions and activate its associated tools.

**Parameters:**
- `name` (required): Name of the skill to load

### load_skill_resource

Access specific resources from a loaded skill.

**Parameters:**
- `skill_name` (required): Name of the skill
- `resource_path` (required): Relative path to resource (e.g., `scripts/helper.py`)

## Best Practices

1. Discover first (`list_skills`) before loading.
2. Load only the skills needed for the current task.
3. Use `load_skill_resource` for targeted files instead of loading everything.
4. Verify resource paths from `load_skill` output.

## Troubleshooting

### Skill Not Found

- Check spelling: `list_skills query="skill-name"`
- Skills must be in `~/.vtcode/skills/` or project `.agents/skills/` (legacy `.vtcode/skills/` supported)

### Resource Not Found

- Verify resource path from `load_skill` output
- Paths are relative to skill directory
- Check file existence under `scripts/`, `references/`, or `assets/`

### Tool Not Appearing

- Load the skill first: `load_skill "skill-name"`
- Skill tools remain dormant until loaded
